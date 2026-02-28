//! Token operations.

use std::collections::{HashMap, HashSet};

use rocksdb::{IteratorMode, WriteBatch};

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{
    decode_consumed_cell_info, AssetAction, LiveCellInfo, TokenActivityEntry,
    TokenActivityTransfer, TokenDailyDelta, TokenInfo, TokenTransferRecord,
};

const TOKEN_REBUILD_BATCH_SIZE: usize = 20_000;

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TokenStateRebuildResult {
    pub token_holders_cleared: u64,
    pub token_transfer_stats_cleared: u64,
    pub token_hourly_stats_cleared: u64,
    pub tokens_written: u64,
    pub tokens_deleted: u64,
    pub token_holders_written: u64,
    pub token_transfer_stats_written: u64,
    pub token_hourly_stats_written: u64,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TokenDailyRebuildResult {
    pub token_daily_cleared: u64,
    pub token_daily_written: u64,
    pub live_cells_scanned: u64,
    pub consumed_cells_scanned: u64,
}

#[derive(Debug, Clone)]
pub struct TokenDailyValidationError {
    pub type_hash: Vec<u8>,
    pub date_yyyymmdd: u32,
    pub live_capacity: i128,
    pub live_occupied_capacity: i128,
    pub capacity_delta: i128,
    pub occupied_delta: i128,
}

#[derive(Debug, Default)]
struct LiveTokenAgg {
    total_supply: i128,
    first_seen_block: Option<i64>,
    holder_balances: HashMap<Vec<u8>, i128>,
}

#[derive(Debug, Default)]
struct TransferTokenAgg {
    transfers_count: i64,
    first_seen_block: Option<i64>,
    hourly_counts: HashMap<i64, i64>,
}

fn flush_rebuild_batch(
    store: &CkbadgerStore,
    write_batch: &mut WriteBatch,
    pending_writes: &mut usize,
    force: bool,
) -> anyhow::Result<()> {
    if *pending_writes >= TOKEN_REBUILD_BATCH_SIZE || (force && !write_batch.is_empty()) {
        store.write_batch(std::mem::take(write_batch))?;
        *write_batch = WriteBatch::default();
        *pending_writes = 0;
    }
    Ok(())
}

fn update_first_seen(first_seen: &mut Option<i64>, candidate: i64) {
    if first_seen.is_none_or(|current| candidate < current) {
        *first_seen = Some(candidate);
    }
}

fn effective_occupied_capacity(
    info: &LiveCellInfo,
    outpoint_key: &[u8],
    source: &str,
) -> anyhow::Result<i64> {
    if info.occupied_capacity > 0 {
        if info.occupied_capacity > info.capacity {
            anyhow::bail!(
                "token daily rebuild found occupied capacity exceeding capacity in {} cell: outpoint=0x{}, occupied_capacity={}, capacity={}",
                source,
                bytes_to_hex(outpoint_key),
                info.occupied_capacity,
                info.capacity
            );
        }
        return Ok(info.occupied_capacity);
    }

    let lock_script_size = 32i64
        .checked_add(1)
        .and_then(|x| x.checked_add(i64::try_from(info.lock_args.len()).ok()?))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "token daily rebuild lock script size overflow in {} cell: outpoint=0x{}",
                source,
                bytes_to_hex(outpoint_key)
            )
        })?;

    let type_script_size = if info.type_script_hash.is_some() {
        let type_args = info.type_args.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "token daily rebuild missing type_args for typed {} cell: outpoint=0x{}",
                source,
                bytes_to_hex(outpoint_key)
            )
        })?;
        32i64
            .checked_add(1)
            .and_then(|x| x.checked_add(i64::try_from(type_args.len()).ok()?))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "token daily rebuild type script size overflow in {} cell: outpoint=0x{}",
                    source,
                    bytes_to_hex(outpoint_key)
                )
            })?
    } else {
        0
    };

    if info.data_size < 0 {
        anyhow::bail!(
            "token daily rebuild found negative data_size in {} cell: outpoint=0x{}, data_size={}",
            source,
            bytes_to_hex(outpoint_key),
            info.data_size
        );
    }
    let data_size = i64::from(info.data_size);
    let occupied = (8i64)
        .checked_add(lock_script_size)
        .and_then(|x| x.checked_add(type_script_size))
        .and_then(|x| x.checked_add(data_size))
        .and_then(|x| x.checked_mul(100_000_000))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "token daily rebuild occupied capacity overflow in {} cell: outpoint=0x{}",
                source,
                bytes_to_hex(outpoint_key)
            )
        })?;

    if occupied <= 0 || occupied > info.capacity {
        anyhow::bail!(
            "token daily rebuild computed invalid occupied capacity in {} cell: outpoint=0x{}, occupied_capacity={}, capacity={}",
            source,
            bytes_to_hex(outpoint_key),
            occupied,
            info.capacity
        );
    }

    Ok(occupied)
}

impl CkbadgerStore {
    pub fn get_token(&self, type_hash: &[u8]) -> anyhow::Result<Option<TokenInfo>> {
        match self.get_cf(self.cf_tokens(), type_hash)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// Batch-fetch multiple tokens by type_script_hash in a single RocksDB multi_get.
    pub fn get_tokens_batch(&self, type_hashes: &[Vec<u8>]) -> Vec<(Vec<u8>, Option<TokenInfo>)> {
        if type_hashes.is_empty() {
            return Vec::new();
        }
        let cf = self.cf_tokens();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            type_hashes.iter().map(|h| (cf, h.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);
        type_hashes
            .iter()
            .zip(values)
            .map(|(hash, result)| {
                let info = result
                    .ok()
                    .flatten()
                    .and_then(|v| bincode::deserialize::<TokenInfo>(&v).ok());
                (hash.clone(), info)
            })
            .collect()
    }

    pub fn put_token_direct(&self, type_hash: &[u8], info: &TokenInfo) -> anyhow::Result<()> {
        let value = bincode::serialize(info)?;
        self.put_cf(self.cf_tokens(), type_hash, &value)
    }

    /// List all tokens.
    pub fn list_tokens(&self) -> anyhow::Result<Vec<(Vec<u8>, TokenInfo)>> {
        let iter = self.iterator_cf(self.cf_tokens(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<TokenInfo>(&value) {
                results.push((key.to_vec(), info));
            }
        }
        Ok(results)
    }

    /// Get token holder balance.
    pub fn get_token_holder_balance(
        &self,
        type_hash: &[u8],
        lock_hash: &[u8],
    ) -> anyhow::Result<Option<i128>> {
        let key = keys::encode_token_holder_key(type_hash, lock_hash);
        match self.get_cf(self.cf_token_holders(), &key)? {
            Some(value) if value.len() == 16 => {
                Ok(Some(i128::from_le_bytes(value[..16].try_into().unwrap())))
            }
            _ => Ok(None),
        }
    }

    /// Get total transfer count for a token from the stats CF.
    pub fn get_token_transfers_count(&self, type_hash: &[u8]) -> anyhow::Result<i64> {
        let key = keys::encode_token_transfers_key(type_hash);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) if value.len() == 8 => {
                Ok(i64::from_le_bytes(value[..8].try_into().unwrap()))
            }
            _ => Ok(0),
        }
    }

    /// Get 24h transfer count for a token by summing recent hourly buckets.
    pub fn get_token_24h_transfers(&self, type_hash: &[u8], now_ms: i64) -> anyhow::Result<i64> {
        let current_hour = now_ms / 3_600_000;
        let cutoff_hour = current_hour - 24;
        let prefix = keys::encode_token_hourly_prefix(type_hash);
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut total: i64 = 0;

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            // Key: prefix(1B) + type_hash(32B) + hour_bucket(8B) = 41 bytes
            if key.len() == 41 && value.len() == 8 {
                let hour = i64::from_be_bytes(key[33..41].try_into().unwrap());
                if hour > cutoff_hour {
                    total += i64::from_le_bytes(value[..8].try_into().unwrap());
                }
            }
        }
        Ok(total)
    }

    pub fn get_token_daily_delta(
        &self,
        type_hash: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<TokenDailyDelta>> {
        let key = keys::encode_token_daily_key(type_hash, date_yyyymmdd);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_token_daily_delta(
        &self,
        type_hash: &[u8],
        date_yyyymmdd: u32,
        delta: &TokenDailyDelta,
    ) -> anyhow::Result<()> {
        let key = keys::encode_token_daily_key(type_hash, date_yyyymmdd);
        let value = bincode::serialize(delta)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn list_token_daily_deltas(
        &self,
        type_hash: &[u8],
    ) -> anyhow::Result<Vec<(u32, TokenDailyDelta)>> {
        self.list_token_daily_deltas_in_range(type_hash, None, None)
    }

    pub fn list_token_daily_deltas_in_range(
        &self,
        type_hash: &[u8],
        from_date_yyyymmdd: Option<u32>,
        to_date_yyyymmdd: Option<u32>,
    ) -> anyhow::Result<Vec<(u32, TokenDailyDelta)>> {
        let prefix = keys::encode_token_daily_prefix(type_hash);
        let start_key =
            keys::encode_token_daily_key(type_hash, from_date_yyyymmdd.unwrap_or(u32::MIN));
        let iter = self.iterator_cf(
            self.cf_stats(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::TOKEN_DAILY_KEY_SIZE {
                continue;
            }
            let (_, date) = keys::decode_token_daily_key(&key);
            if let Some(to_date) = to_date_yyyymmdd {
                if date > to_date {
                    break;
                }
            }
            if let Ok(delta) = bincode::deserialize::<TokenDailyDelta>(&value) {
                results.push((date, delta));
            }
        }

        Ok(results)
    }

    /// Return the first invalid token daily accumulation (if any).
    ///
    /// Validity checks are applied in per-token date order:
    /// - running live capacity must be >= 0
    /// - running live occupied capacity must be >= 0
    /// - running live occupied capacity must be <= running live capacity
    pub fn find_first_invalid_token_daily_delta(
        &self,
    ) -> anyhow::Result<Option<TokenDailyValidationError>> {
        let start = [keys::STATS_PREFIX_TOKEN_DAILY];
        let iter = self.iterator_cf(
            self.cf_stats(),
            IteratorMode::From(&start, rocksdb::Direction::Forward),
        );

        let mut current_type_hash: Option<Vec<u8>> = None;
        let mut live_capacity: i128 = 0;
        let mut live_occupied_capacity: i128 = 0;

        for item in iter.flatten() {
            let (key, value) = item;
            if key.first().copied() != Some(keys::STATS_PREFIX_TOKEN_DAILY) {
                break;
            }
            if key.len() != keys::TOKEN_DAILY_KEY_SIZE {
                anyhow::bail!(
                    "invalid token daily key length while validating token daily deltas: key_len={}, key=0x{}",
                    key.len(),
                    bytes_to_hex(&key)
                );
            }

            let (type_hash, date_yyyymmdd) = keys::decode_token_daily_key(&key);
            let delta: TokenDailyDelta = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize token daily delta while validating: type_hash=0x{}, date={}, error={}",
                    bytes_to_hex(&type_hash),
                    date_yyyymmdd,
                    e
                )
            })?;

            if current_type_hash.as_ref() != Some(&type_hash) {
                current_type_hash = Some(type_hash.clone());
                live_capacity = 0;
                live_occupied_capacity = 0;
            }

            live_capacity = live_capacity
                .checked_add(delta.live_capacity_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily validation overflow on capacity: type_hash=0x{}, date={}, current={}, delta={}",
                        bytes_to_hex(&type_hash),
                        date_yyyymmdd,
                        live_capacity,
                        delta.live_capacity_delta
                    )
                })?;
            live_occupied_capacity = live_occupied_capacity
                .checked_add(delta.live_occupied_capacity_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily validation overflow on occupied capacity: type_hash=0x{}, date={}, current={}, delta={}",
                        bytes_to_hex(&type_hash),
                        date_yyyymmdd,
                        live_occupied_capacity,
                        delta.live_occupied_capacity_delta
                    )
                })?;

            if live_capacity < 0
                || live_occupied_capacity < 0
                || live_occupied_capacity > live_capacity
            {
                return Ok(Some(TokenDailyValidationError {
                    type_hash,
                    date_yyyymmdd,
                    live_capacity,
                    live_occupied_capacity,
                    capacity_delta: delta.live_capacity_delta,
                    occupied_delta: delta.live_occupied_capacity_delta,
                }));
            }
        }

        Ok(None)
    }

    /// Rebuild token daily deltas from canonical cell sets (`live_cells` + `consumed_cells`).
    ///
    /// For each token cell:
    /// - add `(capacity, occupied)` at `created_at_block` day
    /// - if consumed, subtract `(capacity, occupied)` at `consumed_at_block` day
    #[allow(clippy::too_many_lines)]
    pub fn rebuild_token_daily_deltas_from_cells(&self) -> anyhow::Result<TokenDailyRebuildResult> {
        let mut result = TokenDailyRebuildResult::default();
        let mut block_date_cache: HashMap<i64, u32> = HashMap::new();
        let mut daily: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();

        let mut resolve_date = |block_number: i64| -> anyhow::Result<u32> {
            if let Some(date) = block_date_cache.get(&block_number).copied() {
                return Ok(date);
            }
            let header = self.get_block_header(block_number)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "missing block header while rebuilding token daily deltas: block_number={}",
                    block_number
                )
            })?;
            let date = keys::timestamp_ms_to_date(header.timestamp);
            block_date_cache.insert(block_number, date);
            Ok(date)
        };

        // 1) Aggregate live cells: +created
        let iter = self.iterator_cf(self.cf_live_cells(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            result.live_cells_scanned += 1;

            if key.len() != keys::OUTPOINT_KEY_SIZE {
                continue;
            }

            let info: LiveCellInfo = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize live cell while rebuilding token daily deltas: outpoint=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            let Some(type_hash) = info.type_script_hash.as_ref() else {
                continue;
            };
            let date = resolve_date(info.created_at_block)?;
            let occupied = effective_occupied_capacity(&info, &key, "live")?;

            let entry = daily.entry((type_hash.clone(), date)).or_insert((0, 0));
            entry.0 = entry
                .0
                .checked_add(i128::from(info.capacity))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily rebuild capacity overflow from live cell: type_hash=0x{}, date={}, current={}, delta={}",
                        bytes_to_hex(type_hash),
                        date,
                        entry.0,
                        info.capacity
                    )
                })?;
            entry.1 = entry
                .1
                .checked_add(i128::from(occupied))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily rebuild occupied overflow from live cell: type_hash=0x{}, date={}, current={}, delta={}",
                        bytes_to_hex(type_hash),
                        date,
                        entry.1,
                        occupied
                    )
                })?;
        }

        // 2) Aggregate consumed cells: +created, -consumed
        let iter = self.iterator_cf(self.cf_consumed_cells(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            result.consumed_cells_scanned += 1;

            if key.len() != keys::OUTPOINT_KEY_SIZE {
                continue;
            }

            let consumed = decode_consumed_cell_info(&value).ok_or_else(|| {
                anyhow::anyhow!(
                    "failed to decode consumed cell while rebuilding token daily deltas: outpoint=0x{}",
                    bytes_to_hex(&key)
                )
            })?;
            let info = consumed.cell;
            let Some(type_hash) = info.type_script_hash.as_ref() else {
                continue;
            };

            let created_date = resolve_date(info.created_at_block)?;
            let consumed_date = resolve_date(consumed.consumed_at_block)?;
            let occupied = effective_occupied_capacity(&info, &key, "consumed")?;

            let created = daily
                .entry((type_hash.clone(), created_date))
                .or_insert((0, 0));
            created.0 = created
                .0
                .checked_add(i128::from(info.capacity))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily rebuild capacity overflow on consumed-created edge: type_hash=0x{}, date={}, current={}, delta={}",
                        bytes_to_hex(type_hash),
                        created_date,
                        created.0,
                        info.capacity
                    )
                })?;
            created.1 = created
                .1
                .checked_add(i128::from(occupied))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily rebuild occupied overflow on consumed-created edge: type_hash=0x{}, date={}, current={}, delta={}",
                        bytes_to_hex(type_hash),
                        created_date,
                        created.1,
                        occupied
                    )
                })?;

            let consumed_entry = daily
                .entry((type_hash.clone(), consumed_date))
                .or_insert((0, 0));
            consumed_entry.0 = consumed_entry
                .0
                .checked_sub(i128::from(info.capacity))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily rebuild capacity overflow on consumed edge: type_hash=0x{}, date={}, current={}, delta=-{}",
                        bytes_to_hex(type_hash),
                        consumed_date,
                        consumed_entry.0,
                        info.capacity
                    )
                })?;
            consumed_entry.1 = consumed_entry
                .1
                .checked_sub(i128::from(occupied))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily rebuild occupied overflow on consumed edge: type_hash=0x{}, date={}, current={}, delta=-{}",
                        bytes_to_hex(type_hash),
                        consumed_date,
                        consumed_entry.1,
                        occupied
                    )
                })?;
        }

        // Keep only non-zero daily entries, then validate per-token running totals.
        let mut entries: Vec<(Vec<u8>, u32, i128, i128)> = daily
            .into_iter()
            .filter_map(|((type_hash, date), (capacity_delta, occupied_delta))| {
                if capacity_delta == 0 && occupied_delta == 0 {
                    None
                } else {
                    Some((type_hash, date, capacity_delta, occupied_delta))
                }
            })
            .collect();
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        let mut current_type_hash: Option<Vec<u8>> = None;
        let mut live_capacity: i128 = 0;
        let mut live_occupied_capacity: i128 = 0;
        for (type_hash, date_yyyymmdd, capacity_delta, occupied_delta) in &entries {
            if current_type_hash.as_ref() != Some(type_hash) {
                current_type_hash = Some(type_hash.clone());
                live_capacity = 0;
                live_occupied_capacity = 0;
            }

            live_capacity = live_capacity.checked_add(*capacity_delta).ok_or_else(|| {
                anyhow::anyhow!(
                    "token daily rebuild validation overflow on capacity: type_hash=0x{}, date={}, current={}, delta={}",
                    bytes_to_hex(type_hash),
                    date_yyyymmdd,
                    live_capacity,
                    capacity_delta
                )
            })?;
            live_occupied_capacity = live_occupied_capacity
                .checked_add(*occupied_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily rebuild validation overflow on occupied capacity: type_hash=0x{}, date={}, current={}, delta={}",
                        bytes_to_hex(type_hash),
                        date_yyyymmdd,
                        live_occupied_capacity,
                        occupied_delta
                    )
                })?;

            if live_capacity < 0
                || live_occupied_capacity < 0
                || live_occupied_capacity > live_capacity
            {
                anyhow::bail!(
                    "token daily rebuild produced invalid accumulation: type_hash=0x{}, date={}, live_capacity={}, live_occupied_capacity={}, delta_capacity={}, delta_occupied={}",
                    bytes_to_hex(type_hash),
                    date_yyyymmdd,
                    live_capacity,
                    live_occupied_capacity,
                    capacity_delta,
                    occupied_delta
                );
            }
        }

        // 3) Clear all existing token_daily entries.
        let mut write_batch = WriteBatch::default();
        let start = [keys::STATS_PREFIX_TOKEN_DAILY];
        let iter = self.iterator_cf(
            self.cf_stats(),
            IteratorMode::From(&start, rocksdb::Direction::Forward),
        );
        for item in iter.flatten() {
            let (key, _) = item;
            if key.first().copied() != Some(keys::STATS_PREFIX_TOKEN_DAILY) {
                break;
            }
            write_batch.delete_cf(self.cf_stats(), &key);
            result.token_daily_cleared += 1;
            if result
                .token_daily_cleared
                .is_multiple_of(TOKEN_REBUILD_BATCH_SIZE as u64)
            {
                self.write_batch(std::mem::take(&mut write_batch))?;
                write_batch = WriteBatch::default();
            }
        }
        if !write_batch.is_empty() {
            self.write_batch(write_batch)?;
        }

        // 4) Write rebuilt entries.
        let mut write_batch = WriteBatch::default();
        for (type_hash, date_yyyymmdd, capacity_delta, occupied_delta) in entries {
            let key = keys::encode_token_daily_key(&type_hash, date_yyyymmdd);
            let value = bincode::serialize(&TokenDailyDelta {
                live_capacity_delta: capacity_delta,
                live_occupied_capacity_delta: occupied_delta,
            })?;
            write_batch.put_cf(self.cf_stats(), key, value);
            result.token_daily_written += 1;
            if result
                .token_daily_written
                .is_multiple_of(TOKEN_REBUILD_BATCH_SIZE as u64)
            {
                self.write_batch(std::mem::take(&mut write_batch))?;
                write_batch = WriteBatch::default();
            }
        }
        if !write_batch.is_empty() {
            self.write_batch(write_batch)?;
        }

        Ok(result)
    }

    /// Scan ALL hourly transfer entries in one pass and group by type_hash.
    /// Returns a map of type_hash → 24h transfer count.
    /// Much faster than calling `get_token_24h_transfers` per-token (N+1).
    pub fn scan_all_token_24h_transfers(
        &self,
        now_ms: i64,
    ) -> anyhow::Result<HashMap<Vec<u8>, i64>> {
        let current_hour = now_ms / 3_600_000;
        let cutoff_hour = current_hour - 24;

        // Scan all entries with the TOKEN_HOURLY prefix (0x0A)
        let prefix = [keys::STATS_PREFIX_TOKEN_HOURLY];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut result: HashMap<Vec<u8>, i64> = HashMap::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if key.first() != Some(&keys::STATS_PREFIX_TOKEN_HOURLY) {
                break;
            }
            // Key: prefix(1B) + type_hash(32B) + hour_bucket(8B) = 41 bytes
            if key.len() == 41 && value.len() == 8 {
                let hour = i64::from_be_bytes(key[33..41].try_into().unwrap());
                if hour > cutoff_hour {
                    let type_hash = key[1..33].to_vec();
                    let count = i64::from_le_bytes(value[..8].try_into().unwrap());
                    *result.entry(type_hash).or_insert(0) += count;
                }
            }
        }

        Ok(result)
    }

    /// Scan ALL spore hourly transfer entries in one pass and group by cluster_id.
    /// Returns a map of cluster_id → 24h transfer count.
    pub fn scan_all_spore_24h_transfers(
        &self,
        now_ms: i64,
    ) -> anyhow::Result<HashMap<Vec<u8>, i64>> {
        let current_hour = now_ms / 3_600_000;
        let cutoff_hour = current_hour - 24;

        let prefix = [keys::STATS_PREFIX_SPORE_HOURLY];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut result: HashMap<Vec<u8>, i64> = HashMap::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if key.first() != Some(&keys::STATS_PREFIX_SPORE_HOURLY) {
                break;
            }
            if key.len() == 41 && value.len() == 8 {
                let hour = i64::from_be_bytes(key[33..41].try_into().unwrap());
                if hour > cutoff_hour {
                    let cluster_id = key[1..33].to_vec();
                    let count = i64::from_le_bytes(value[..8].try_into().unwrap());
                    *result.entry(cluster_id).or_insert(0) += count;
                }
            }
        }

        Ok(result)
    }

    /// Scan ALL NFT hourly transfer entries in one pass and group by collection_id.
    /// Returns a map of collection_id → 24h transfer count.
    pub fn scan_all_nft_24h_transfers(&self, now_ms: i64) -> anyhow::Result<HashMap<Vec<u8>, i64>> {
        let current_hour = now_ms / 3_600_000;
        let cutoff_hour = current_hour - 24;

        let prefix = [keys::STATS_PREFIX_NFT_HOURLY];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut result: HashMap<Vec<u8>, i64> = HashMap::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if key.first() != Some(&keys::STATS_PREFIX_NFT_HOURLY) {
                break;
            }
            if key.len() == 41 && value.len() == 8 {
                let hour = i64::from_be_bytes(key[33..41].try_into().unwrap());
                if hour > cutoff_hour {
                    let collection_id = key[1..33].to_vec();
                    let count = i64::from_le_bytes(value[..8].try_into().unwrap());
                    *result.entry(collection_id).or_insert(0) += count;
                }
            }
        }

        Ok(result)
    }

    /// Delete hourly buckets older than the cutoff hour for a given token.
    pub fn cleanup_old_hourly_buckets(
        &self,
        type_hash: &[u8],
        cutoff_hour: i64,
    ) -> anyhow::Result<u64> {
        let prefix = keys::encode_token_hourly_prefix(type_hash);
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut deleted = 0u64;

        for item in iter.flatten() {
            let (key, _value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() == 41 {
                let hour = i64::from_be_bytes(key[33..41].try_into().unwrap());
                if hour < cutoff_hour {
                    self.delete_cf(self.cf_stats(), &key)?;
                    deleted += 1;
                }
            }
        }
        Ok(deleted)
    }

    /// Migrate transfer stats into TokenInfo.transfers_count for all tokens.
    /// Reads from the stats CF and writes back into the tokens CF.
    pub fn migrate_token_transfer_stats(&self) -> anyhow::Result<u64> {
        let tokens = self.list_tokens()?;
        let mut migrated = 0u64;

        for (type_hash, mut info) in tokens {
            let count = self.get_token_transfers_count(&type_hash)?;
            if info.transfers_count != count {
                info.transfers_count = count;
                self.put_token_direct(&type_hash, &info)?;
                migrated += 1;
            }
        }

        Ok(migrated)
    }

    /// List transfers for a token, newest first.
    ///
    /// Optionally start after the given `(block_num, tx_idx)` cursor.
    /// Returns `(block_num, tx_idx, record)` tuples for cursor construction.
    pub fn list_token_transfers(
        &self,
        type_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, TokenTransferRecord)>> {
        let prefix = &type_hash[..32];

        // For cursor: start from the key just after the cursor position.
        // For no cursor: start from the type_hash prefix (newest first due to desc key).
        let start_key = match cursor {
            Some((block_num, tx_idx)) => {
                keys::encode_token_transfer_key(type_hash, block_num, tx_idx + 1)
            }
            None => prefix.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_token_transfers(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(prefix) {
                break;
            }
            if key.len() == 44 {
                let (block_num, tx_idx) = keys::decode_token_transfer_key(&key);
                let record: TokenTransferRecord = bincode::deserialize(&value)?;
                results.push((block_num, tx_idx, record));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// List token activities grouped by transaction.
    ///
    /// Iterates `cf_token_transfers()` and groups consecutive records sharing the same
    /// `tx_hash` into a single `TokenActivityEntry`.  Returns `(block_num, entry_idx, entry)`
    /// where `entry_idx` is the *last* record's key index within the group — suitable as
    /// the cursor for the next page.
    pub fn list_token_activities(
        &self,
        type_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, TokenActivityEntry)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let prefix = &type_hash[..32];

        let start_key = match cursor {
            Some((block_num, tx_idx)) => {
                keys::encode_token_transfer_key(type_hash, block_num, tx_idx + 1)
            }
            None => prefix.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_token_transfers(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results: Vec<(i64, i32, TokenActivityEntry)> = Vec::new();

        // Current group state
        let mut current_tx_hash: Option<Vec<u8>> = None;
        let mut current_block_number: i64 = 0;
        let mut current_timestamp_ms: i64 = 0;
        let mut current_transfers: Vec<TokenActivityTransfer> = Vec::new();
        let mut current_last_idx: i32 = 0;

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(prefix) {
                break;
            }
            if key.len() != 44 {
                continue;
            }

            let (block_num, tx_idx) = keys::decode_token_transfer_key(&key);
            let record: TokenTransferRecord = bincode::deserialize(&value)?;

            if let Some(ref prev_tx_hash) = current_tx_hash {
                if record.tx_hash != *prev_tx_hash {
                    // Finalize the previous group
                    let entry = Self::finalize_activity_group(
                        current_tx_hash.take().unwrap(),
                        current_block_number,
                        current_timestamp_ms,
                        std::mem::take(&mut current_transfers),
                    );
                    results.push((current_block_number, current_last_idx, entry));

                    if results.len() >= limit {
                        // Don't start a new group; we have enough
                        return Ok(results);
                    }
                }
            }

            // Add to current group (or start new group)
            if current_tx_hash.is_none() {
                current_tx_hash = Some(record.tx_hash.clone());
                current_block_number = block_num;
                current_timestamp_ms = record.timestamp;
            }

            current_transfers.push(TokenActivityTransfer {
                from_lock_hash: record.from_lock_hash,
                to_lock_hash: record.to_lock_hash,
                amount: record.amount,
                is_mint: record.is_mint,
                is_burn: record.is_burn,
            });
            current_last_idx = tx_idx;
        }

        // Finalize the last group
        if let Some(tx_hash) = current_tx_hash.take() {
            let entry = Self::finalize_activity_group(
                tx_hash,
                current_block_number,
                current_timestamp_ms,
                std::mem::take(&mut current_transfers),
            );
            results.push((current_block_number, current_last_idx, entry));
        }

        Ok(results)
    }

    fn finalize_activity_group(
        tx_hash: Vec<u8>,
        block_number: i64,
        timestamp_ms: i64,
        transfers: Vec<TokenActivityTransfer>,
    ) -> TokenActivityEntry {
        let mut actions = Vec::new();
        let mut has_mint = false;
        let mut has_burn = false;
        let mut has_transfer = false;
        for t in &transfers {
            if t.is_mint && !has_mint {
                actions.push(AssetAction::Mint);
                has_mint = true;
            } else if t.is_burn && !has_burn {
                actions.push(AssetAction::Burn);
                has_burn = true;
            } else if !t.is_mint && !t.is_burn && !has_transfer {
                actions.push(AssetAction::Transfer);
                has_transfer = true;
            }
        }
        TokenActivityEntry {
            tx_hash,
            block_number,
            timestamp_ms,
            actions,
            transfers,
        }
    }

    /// Count holders for a token (prefix scan by type_hash).
    ///
    /// Counts entries with balance > 0 without collecting them.
    pub fn count_token_holders(&self, type_hash: &[u8]) -> anyhow::Result<i64> {
        let iter = self.prefix_iterator_cf(self.cf_token_holders(), type_hash);
        let mut count: i64 = 0;

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(type_hash) {
                break;
            }
            if key.len() == 64 && value.len() == 16 {
                let balance = i128::from_le_bytes(value[..16].try_into().unwrap());
                if balance > 0 {
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// List holders for a token (prefix scan by type_hash).
    ///
    /// Returns `(lock_hash, balance)` pairs, limited to `limit` results.
    pub fn list_token_holders(
        &self,
        type_hash: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(Vec<u8>, i128)>> {
        let iter = self.prefix_iterator_cf(self.cf_token_holders(), type_hash);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(type_hash) {
                break;
            }
            // Key: type_hash(32) + lock_hash(32) = 64
            if key.len() == 64 && value.len() == 16 {
                let lock_hash = key[32..64].to_vec();
                let balance = i128::from_le_bytes(value[..16].try_into().unwrap());
                results.push((lock_hash, balance));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// Rebuild token state after rollback.
    ///
    /// Sources of truth:
    /// - `live_cells` for holder balances and outstanding total supply
    /// - `token_transfers` for transfer counters/hourly buckets
    pub fn rebuild_token_state_from_transfers(&self) -> anyhow::Result<TokenStateRebuildResult> {
        let mut result = TokenStateRebuildResult::default();

        let mut existing_tokens: HashMap<Vec<u8>, TokenInfo> = HashMap::new();
        let iter = self.iterator_cf(self.cf_tokens(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            let info: TokenInfo = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize token metadata during rebuild: type_hash=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            existing_tokens.insert(key.to_vec(), info);
        }

        // 1) Aggregate live UDT balances and current total supply from live_cells.
        let mut live_aggs: HashMap<Vec<u8>, LiveTokenAgg> = HashMap::new();
        let iter = self.iterator_cf(self.cf_live_cells(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() != keys::OUTPOINT_KEY_SIZE {
                continue;
            }
            let info: LiveCellInfo = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize live cell during token rebuild: outpoint=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            let Some(type_hash) = info.type_script_hash.as_ref() else {
                continue;
            };
            let Some(amount_u128) = info.udt_amount else {
                continue;
            };
            if amount_u128 == 0 {
                continue;
            }
            let amount = i128::try_from(amount_u128).map_err(|_| {
                anyhow::anyhow!(
                    "token rebuild amount exceeds i128 in live cell: type_hash=0x{}, outpoint=0x{}, amount={}",
                    bytes_to_hex(type_hash),
                    bytes_to_hex(&key),
                    amount_u128
                )
            })?;

            let agg = live_aggs.entry(type_hash.clone()).or_default();
            agg.total_supply = agg.total_supply.checked_add(amount).ok_or_else(|| {
                anyhow::anyhow!(
                    "token rebuild supply overflow from live cells: type_hash=0x{}, current={}, delta={}",
                    bytes_to_hex(type_hash),
                    agg.total_supply,
                    amount
                )
            })?;
            update_first_seen(&mut agg.first_seen_block, info.created_at_block);

            let holder = agg
                .holder_balances
                .entry(info.lock_script_hash.clone())
                .or_insert(0);
            *holder = holder.checked_add(amount).ok_or_else(|| {
                anyhow::anyhow!(
                    "token rebuild holder balance overflow from live cells: type_hash=0x{}, lock_hash=0x{}, current={}, delta={}",
                    bytes_to_hex(type_hash),
                    bytes_to_hex(&info.lock_script_hash),
                    *holder,
                    amount
                )
            })?;
        }

        // 2) Aggregate transfer counters from token_transfers.
        let mut transfer_aggs: HashMap<Vec<u8>, TransferTokenAgg> = HashMap::new();
        let iter = self.iterator_cf(self.cf_token_transfers(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() != 44 {
                anyhow::bail!(
                    "invalid token_transfer key length during rebuild: key_len={}, key=0x{}",
                    key.len(),
                    bytes_to_hex(&key)
                );
            }
            let type_hash = key[..32].to_vec();
            let (block_number, tx_idx) = keys::decode_token_transfer_key(&key);
            let record: TokenTransferRecord = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize token transfer during rebuild: type_hash=0x{}, block_number={}, tx_idx={}, error={}",
                    bytes_to_hex(&type_hash),
                    block_number,
                    tx_idx,
                    e
                )
            })?;

            let agg = transfer_aggs.entry(type_hash).or_default();
            agg.transfers_count = agg.transfers_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "token rebuild transfers_count overflow: block_number={}, tx_idx={}, current_count={}",
                    block_number,
                    tx_idx,
                    agg.transfers_count
                )
            })?;
            update_first_seen(&mut agg.first_seen_block, block_number);

            let hour_bucket = record.timestamp / 3_600_000;
            let hourly = agg.hourly_counts.entry(hour_bucket).or_insert(0);
            *hourly = hourly.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "token rebuild hourly count overflow: hour_bucket={}, block_number={}, tx_idx={}, current={}",
                    hour_bucket,
                    block_number,
                    tx_idx,
                    *hourly
                )
            })?;
        }

        // 3) Clear token_holders.
        let mut clear_batch = WriteBatch::default();
        let iter = self.iterator_cf(self.cf_token_holders(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            clear_batch.delete_cf(self.cf_token_holders(), &key);
            result.token_holders_cleared += 1;
            if result
                .token_holders_cleared
                .is_multiple_of(TOKEN_REBUILD_BATCH_SIZE as u64)
            {
                self.write_batch(std::mem::take(&mut clear_batch))?;
                clear_batch = WriteBatch::default();
            }
        }
        if !clear_batch.is_empty() {
            self.write_batch(clear_batch)?;
        }

        // 4) Clear token stats rollups (TOKEN_TRANSFERS and TOKEN_HOURLY).
        let mut clear_batch = WriteBatch::default();
        let iter = self.iterator_cf(self.cf_stats(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            match key.first().copied() {
                Some(keys::STATS_PREFIX_TOKEN_TRANSFERS) => {
                    clear_batch.delete_cf(self.cf_stats(), &key);
                    result.token_transfer_stats_cleared += 1;
                }
                Some(keys::STATS_PREFIX_TOKEN_HOURLY) => {
                    clear_batch.delete_cf(self.cf_stats(), &key);
                    result.token_hourly_stats_cleared += 1;
                }
                _ => continue,
            }
            let cleared_total =
                result.token_transfer_stats_cleared + result.token_hourly_stats_cleared;
            if cleared_total.is_multiple_of(TOKEN_REBUILD_BATCH_SIZE as u64) {
                self.write_batch(std::mem::take(&mut clear_batch))?;
                clear_batch = WriteBatch::default();
            }
        }
        if !clear_batch.is_empty() {
            self.write_batch(clear_batch)?;
        }

        // 5) Rebuild tokens and token_holders.
        let mut type_hashes: HashSet<Vec<u8>> = HashSet::new();
        type_hashes.extend(existing_tokens.keys().cloned());
        type_hashes.extend(live_aggs.keys().cloned());
        type_hashes.extend(transfer_aggs.keys().cloned());
        let mut type_hashes: Vec<Vec<u8>> = type_hashes.into_iter().collect();
        type_hashes.sort_unstable();

        let mut write_batch = WriteBatch::default();
        let mut pending_writes: usize = 0;
        for type_hash in type_hashes {
            let live = live_aggs.remove(&type_hash);
            let transfer = transfer_aggs.remove(&type_hash);

            // Stale token: no live state and no transfer history left after rollback.
            if live.is_none() && transfer.is_none() {
                if existing_tokens.remove(&type_hash).is_some() {
                    write_batch.delete_cf(self.cf_tokens(), &type_hash);
                    pending_writes += 1;
                    result.tokens_deleted += 1;
                    flush_rebuild_batch(self, &mut write_batch, &mut pending_writes, false)?;
                }
                continue;
            }

            let mut info = existing_tokens.remove(&type_hash).ok_or_else(|| {
                anyhow::anyhow!(
                    "token rebuild missing token metadata for state key: type_hash=0x{}",
                    bytes_to_hex(&type_hash)
                )
            })?;

            let (total_supply, holders_count, live_first_seen, holder_balances) = match live {
                Some(live) => {
                    let holders_count =
                        i64::try_from(live.holder_balances.len()).map_err(|_| {
                            anyhow::anyhow!(
                                "token rebuild holders_count overflow: type_hash=0x{}, holders={}",
                                bytes_to_hex(&type_hash),
                                live.holder_balances.len()
                            )
                        })?;
                    (
                        live.total_supply,
                        holders_count,
                        live.first_seen_block,
                        live.holder_balances,
                    )
                }
                None => (0, 0, None, HashMap::new()),
            };

            let (transfers_count, transfer_first_seen, hourly_counts) = match transfer {
                Some(transfer) => (
                    transfer.transfers_count,
                    transfer.first_seen_block,
                    transfer.hourly_counts,
                ),
                None => (0, None, HashMap::new()),
            };

            let mut first_seen = None;
            if info.first_seen_block > 0 {
                update_first_seen(&mut first_seen, info.first_seen_block);
            }
            if let Some(block) = live_first_seen {
                update_first_seen(&mut first_seen, block);
            }
            if let Some(block) = transfer_first_seen {
                update_first_seen(&mut first_seen, block);
            }

            info.total_supply = Some(total_supply);
            info.holders_count = holders_count;
            info.transfers_count = transfers_count;
            info.first_seen_block = first_seen.unwrap_or(0);

            let token_value = bincode::serialize(&info)?;
            write_batch.put_cf(self.cf_tokens(), &type_hash, &token_value);
            pending_writes += 1;
            result.tokens_written += 1;
            flush_rebuild_batch(self, &mut write_batch, &mut pending_writes, false)?;

            if transfers_count > 0 {
                let transfer_stats_key = keys::encode_token_transfers_key(&type_hash);
                write_batch.put_cf(
                    self.cf_stats(),
                    &transfer_stats_key,
                    transfers_count.to_le_bytes(),
                );
                pending_writes += 1;
                result.token_transfer_stats_written += 1;
                flush_rebuild_batch(self, &mut write_batch, &mut pending_writes, false)?;
            }

            for (hour_bucket, count) in hourly_counts {
                let hourly_key = keys::encode_token_hourly_key(&type_hash, hour_bucket);
                write_batch.put_cf(self.cf_stats(), &hourly_key, count.to_le_bytes());
                pending_writes += 1;
                result.token_hourly_stats_written += 1;
                flush_rebuild_batch(self, &mut write_batch, &mut pending_writes, false)?;
            }

            for (lock_hash, balance) in holder_balances {
                if balance <= 0 {
                    anyhow::bail!(
                        "token rebuild found non-positive live holder balance: type_hash=0x{}, lock_hash=0x{}, balance={}",
                        bytes_to_hex(&type_hash),
                        bytes_to_hex(&lock_hash),
                        balance
                    );
                }
                let holder_key = keys::encode_token_holder_key(&type_hash, &lock_hash);
                write_batch.put_cf(self.cf_token_holders(), holder_key, balance.to_le_bytes());
                pending_writes += 1;
                result.token_holders_written += 1;
                flush_rebuild_batch(self, &mut write_batch, &mut pending_writes, false)?;
            }
        }
        flush_rebuild_batch(self, &mut write_batch, &mut pending_writes, true)?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use crate::types::CachedBlockHeader;
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_get_token_transfers_count_default_zero() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        assert_eq!(store.get_token_transfers_count(&type_hash).unwrap(), 0);
    }

    #[test]
    fn test_put_and_get_token_transfers_count() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_transfers_count(&type_hash, 42);
        batch.commit().unwrap();

        assert_eq!(store.get_token_transfers_count(&type_hash).unwrap(), 42);
    }

    #[test]
    fn test_token_transfers_count_accumulates() {
        let (_dir, store) = test_store();
        let type_hash = [0x02u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_transfers_count(&type_hash, 10);
        batch.commit().unwrap();

        // Read-modify-write (as the indexer does)
        let current = store.get_token_transfers_count(&type_hash).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_token_transfers_count(&type_hash, current + 5);
        batch.commit().unwrap();

        assert_eq!(store.get_token_transfers_count(&type_hash).unwrap(), 15);
    }

    #[test]
    fn test_get_token_24h_transfers_empty() {
        let (_dir, store) = test_store();
        let type_hash = [0x03u8; 32];
        let now_ms = 1_700_000_000_000i64; // some timestamp
        assert_eq!(
            store.get_token_24h_transfers(&type_hash, now_ms).unwrap(),
            0
        );
    }

    #[test]
    fn test_get_token_24h_transfers_sums_recent_buckets() {
        let (_dir, store) = test_store();
        let type_hash = [0x04u8; 32];
        let now_ms = 1_700_000_000_000i64;
        let current_hour = now_ms / 3_600_000;

        // Write 3 hourly buckets: current hour, 12h ago, 23h ago
        let mut batch = StoreBatch::new(&store);
        batch.put_token_hourly_transfer(&type_hash, current_hour, 10);
        batch.put_token_hourly_transfer(&type_hash, current_hour - 12, 20);
        batch.put_token_hourly_transfer(&type_hash, current_hour - 23, 30);
        batch.commit().unwrap();

        // All 3 are within 24h (cutoff_hour = current_hour - 24)
        assert_eq!(
            store.get_token_24h_transfers(&type_hash, now_ms).unwrap(),
            60
        );
    }

    #[test]
    fn test_get_token_24h_transfers_excludes_old_buckets() {
        let (_dir, store) = test_store();
        let type_hash = [0x05u8; 32];
        let now_ms = 1_700_000_000_000i64;
        let current_hour = now_ms / 3_600_000;

        let mut batch = StoreBatch::new(&store);
        batch.put_token_hourly_transfer(&type_hash, current_hour, 10);
        // Exactly at cutoff (== cutoff_hour) — should be excluded (only > cutoff)
        batch.put_token_hourly_transfer(&type_hash, current_hour - 24, 20);
        // Older than 24h
        batch.put_token_hourly_transfer(&type_hash, current_hour - 48, 30);
        batch.commit().unwrap();

        assert_eq!(
            store.get_token_24h_transfers(&type_hash, now_ms).unwrap(),
            10
        );
    }

    #[test]
    fn test_token_daily_delta_roundtrip_and_list() {
        let (_dir, store) = test_store();
        let type_hash = [0x06u8; 32];

        store
            .put_token_daily_delta(
                &type_hash,
                20240115,
                &TokenDailyDelta {
                    live_capacity_delta: 1_000_000_000_000,
                    live_occupied_capacity_delta: 610_000_000_000,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_hash,
                20240116,
                &TokenDailyDelta {
                    live_capacity_delta: -200_000_000_000,
                    live_occupied_capacity_delta: -150_000_000_000,
                },
            )
            .unwrap();

        let day1 = store
            .get_token_daily_delta(&type_hash, 20240115)
            .unwrap()
            .unwrap();
        assert_eq!(day1.live_capacity_delta, 1_000_000_000_000);
        assert_eq!(day1.live_occupied_capacity_delta, 610_000_000_000);

        let listed = store.list_token_daily_deltas(&type_hash).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].0, 20240115);
        assert_eq!(listed[1].0, 20240116);

        let ranged = store
            .list_token_daily_deltas_in_range(&type_hash, Some(20240116), Some(20240116))
            .unwrap();
        assert_eq!(ranged.len(), 1);
        assert_eq!(ranged[0].0, 20240116);
    }

    fn make_header(hash_byte: u8, timestamp: i64) -> CachedBlockHeader {
        CachedBlockHeader {
            block_number: 0,
            hash: vec![hash_byte; 32],
            timestamp,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        }
    }

    fn make_token_cell(
        lock_byte: u8,
        type_hash: &[u8],
        created_at_block: i64,
        capacity: i64,
        occupied_capacity: i64,
    ) -> LiveCellInfo {
        LiveCellInfo {
            capacity,
            created_at_block,
            lock_script_hash: vec![lock_byte; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.to_vec()),
            type_code_hash: Some(vec![0x11; 32]),
            type_args: Some(vec![0x22; 32]),
            data_size: 16,
            occupied_capacity,
            udt_amount: Some(1),
        }
    }

    #[test]
    fn test_find_first_invalid_token_daily_delta_none_for_valid_data() {
        let (_dir, store) = test_store();
        let type_hash = [0x11u8; 32];

        store
            .put_token_daily_delta(
                &type_hash,
                20240101,
                &TokenDailyDelta {
                    live_capacity_delta: 1_000,
                    live_occupied_capacity_delta: 600,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_hash,
                20240102,
                &TokenDailyDelta {
                    live_capacity_delta: -200,
                    live_occupied_capacity_delta: -100,
                },
            )
            .unwrap();

        assert!(store
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_find_first_invalid_token_daily_delta_reports_first_violation() {
        let (_dir, store) = test_store();
        let type_good = [0x01u8; 32];
        let type_bad = [0x02u8; 32];

        store
            .put_token_daily_delta(
                &type_good,
                20240101,
                &TokenDailyDelta {
                    live_capacity_delta: 500,
                    live_occupied_capacity_delta: 300,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_bad,
                20240101,
                &TokenDailyDelta {
                    live_capacity_delta: 100,
                    live_occupied_capacity_delta: 120,
                },
            )
            .unwrap();

        let invalid = store
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .expect("expected invalid token daily delta");
        assert_eq!(invalid.type_hash, type_bad.to_vec());
        assert_eq!(invalid.date_yyyymmdd, 20240101);
        assert_eq!(invalid.live_capacity, 100);
        assert_eq!(invalid.live_occupied_capacity, 120);
        assert_eq!(invalid.capacity_delta, 100);
        assert_eq!(invalid.occupied_delta, 120);
    }

    #[test]
    fn test_rebuild_token_daily_deltas_from_cells_rebuilds_from_live_and_consumed() {
        let (_dir, store) = test_store();
        let type_hash = vec![0xAB; 32];
        let day1_ts = 1_704_067_200_000i64; // 2024-01-01T00:00:00Z
        let day2_ts = 1_704_153_600_000i64; // 2024-01-02T00:00:00Z
        let day1 = keys::timestamp_ms_to_date(day1_ts);
        let day2 = keys::timestamp_ms_to_date(day2_ts);

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &make_header(0x01, day1_ts));
        batch.put_block_header(2, &make_header(0x02, day2_ts));
        batch.put_cell(
            &[0x11; 32],
            0,
            &make_token_cell(0xAA, &type_hash, 1, 1_000, 600),
        );
        batch.put_consumed_cell(
            &[0x22; 32],
            0,
            &make_token_cell(0xBB, &type_hash, 1, 400, 300),
            2,
        );
        batch.commit().unwrap();

        // Seed stale/invalid token daily rows to prove rebuild clears and rewrites them.
        store
            .put_token_daily_delta(
                &type_hash,
                day1,
                &TokenDailyDelta {
                    live_capacity_delta: -1,
                    live_occupied_capacity_delta: 2,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_hash,
                day2,
                &TokenDailyDelta {
                    live_capacity_delta: 123,
                    live_occupied_capacity_delta: 456,
                },
            )
            .unwrap();

        let rebuild = store.rebuild_token_daily_deltas_from_cells().unwrap();
        assert_eq!(rebuild.live_cells_scanned, 1);
        assert_eq!(rebuild.consumed_cells_scanned, 1);
        assert_eq!(rebuild.token_daily_cleared, 2);
        assert_eq!(rebuild.token_daily_written, 2);

        let d1 = store
            .get_token_daily_delta(&type_hash, day1)
            .unwrap()
            .expect("missing day1 delta");
        let d2 = store
            .get_token_daily_delta(&type_hash, day2)
            .unwrap()
            .expect("missing day2 delta");
        assert_eq!(d1.live_capacity_delta, 1_400);
        assert_eq!(d1.live_occupied_capacity_delta, 900);
        assert_eq!(d2.live_capacity_delta, -400);
        assert_eq!(d2.live_occupied_capacity_delta, -300);

        assert!(store
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_cleanup_old_hourly_buckets() {
        let (_dir, store) = test_store();
        let type_hash = [0x06u8; 32];
        let current_hour = 500_000i64;

        let mut batch = StoreBatch::new(&store);
        batch.put_token_hourly_transfer(&type_hash, current_hour, 10);
        batch.put_token_hourly_transfer(&type_hash, current_hour - 24, 20);
        batch.put_token_hourly_transfer(&type_hash, current_hour - 100, 30);
        batch.put_token_hourly_transfer(&type_hash, current_hour - 200, 40);
        batch.commit().unwrap();

        // Cleanup buckets older than 48h
        let cutoff = current_hour - 48;
        let deleted = store
            .cleanup_old_hourly_buckets(&type_hash, cutoff)
            .unwrap();
        assert_eq!(deleted, 2); // -100 and -200 are < cutoff

        // Verify remaining buckets
        let now_ms = current_hour * 3_600_000;
        // current_hour and current_hour-24 should remain
        assert_eq!(
            store.get_token_24h_transfers(&type_hash, now_ms).unwrap(),
            10 // only current_hour is within 24h window (current_hour - 24 == cutoff, excluded)
        );
    }

    #[test]
    fn test_scan_all_token_24h_transfers_empty() {
        let (_dir, store) = test_store();
        let now_ms = 1_700_000_000_000i64;
        let result = store.scan_all_token_24h_transfers(now_ms).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_scan_all_token_24h_transfers_multiple_tokens() {
        let (_dir, store) = test_store();
        let hash_a = [0x0Au8; 32];
        let hash_b = [0x0Bu8; 32];
        let now_ms = 1_700_000_000_000i64;
        let current_hour = now_ms / 3_600_000;

        let mut batch = StoreBatch::new(&store);
        // Token A: 2 recent buckets
        batch.put_token_hourly_transfer(&hash_a, current_hour, 10);
        batch.put_token_hourly_transfer(&hash_a, current_hour - 5, 20);
        // Token B: 1 recent bucket
        batch.put_token_hourly_transfer(&hash_b, current_hour - 1, 15);
        batch.commit().unwrap();

        let result = store.scan_all_token_24h_transfers(now_ms).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(*result.get(hash_a.as_slice()).unwrap(), 30);
        assert_eq!(*result.get(hash_b.as_slice()).unwrap(), 15);
    }

    #[test]
    fn test_scan_all_token_24h_transfers_excludes_old() {
        let (_dir, store) = test_store();
        let hash_a = [0x0Au8; 32];
        let now_ms = 1_700_000_000_000i64;
        let current_hour = now_ms / 3_600_000;

        let mut batch = StoreBatch::new(&store);
        batch.put_token_hourly_transfer(&hash_a, current_hour, 10);
        // Exactly at cutoff — excluded
        batch.put_token_hourly_transfer(&hash_a, current_hour - 24, 20);
        // Old — excluded
        batch.put_token_hourly_transfer(&hash_a, current_hour - 48, 30);
        batch.commit().unwrap();

        let result = store.scan_all_token_24h_transfers(now_ms).unwrap();
        assert_eq!(*result.get(hash_a.as_slice()).unwrap(), 10);
    }

    #[test]
    fn test_scan_all_matches_per_token() {
        let (_dir, store) = test_store();
        let hash_a = [0x0Au8; 32];
        let now_ms = 1_700_000_000_000i64;
        let current_hour = now_ms / 3_600_000;

        let mut batch = StoreBatch::new(&store);
        batch.put_token_hourly_transfer(&hash_a, current_hour, 10);
        batch.put_token_hourly_transfer(&hash_a, current_hour - 12, 20);
        batch.commit().unwrap();

        // Compare scan result with per-token result
        let scan_result = store.scan_all_token_24h_transfers(now_ms).unwrap();
        let per_token = store.get_token_24h_transfers(&hash_a, now_ms).unwrap();
        assert_eq!(*scan_result.get(hash_a.as_slice()).unwrap(), per_token);
    }

    #[test]
    fn test_different_tokens_independent() {
        let (_dir, store) = test_store();
        let hash_a = [0x0Au8; 32];
        let hash_b = [0x0Bu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_transfers_count(&hash_a, 100);
        batch.put_token_transfers_count(&hash_b, 200);
        batch.put_token_hourly_transfer(&hash_a, 1000, 5);
        batch.put_token_hourly_transfer(&hash_b, 1000, 15);
        batch.commit().unwrap();

        assert_eq!(store.get_token_transfers_count(&hash_a).unwrap(), 100);
        assert_eq!(store.get_token_transfers_count(&hash_b).unwrap(), 200);

        let now_ms = 1000 * 3_600_000;
        assert_eq!(store.get_token_24h_transfers(&hash_a, now_ms).unwrap(), 5);
        assert_eq!(store.get_token_24h_transfers(&hash_b, now_ms).unwrap(), 15);
    }

    // ---- Spore hourly transfers ----

    #[test]
    fn test_scan_all_spore_24h_transfers_empty() {
        let (_dir, store) = test_store();
        let now_ms = 1_700_000_000_000i64;
        let result = store.scan_all_spore_24h_transfers(now_ms).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_scan_all_spore_24h_transfers_multiple_clusters() {
        let (_dir, store) = test_store();
        let cluster_a = [0x0Au8; 32];
        let cluster_b = [0x0Bu8; 32];
        let now_ms = 1_700_000_000_000i64;
        let current_hour = now_ms / 3_600_000;

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_hourly_transfer(&cluster_a, current_hour, 10);
        batch.put_spore_hourly_transfer(&cluster_a, current_hour - 5, 20);
        batch.put_spore_hourly_transfer(&cluster_b, current_hour - 1, 15);
        batch.commit().unwrap();

        let result = store.scan_all_spore_24h_transfers(now_ms).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(*result.get(cluster_a.as_slice()).unwrap(), 30);
        assert_eq!(*result.get(cluster_b.as_slice()).unwrap(), 15);
    }

    #[test]
    fn test_scan_all_spore_24h_transfers_excludes_old() {
        let (_dir, store) = test_store();
        let cluster_a = [0x0Au8; 32];
        let now_ms = 1_700_000_000_000i64;
        let current_hour = now_ms / 3_600_000;

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_hourly_transfer(&cluster_a, current_hour, 10);
        batch.put_spore_hourly_transfer(&cluster_a, current_hour - 24, 20); // at cutoff, excluded
        batch.put_spore_hourly_transfer(&cluster_a, current_hour - 48, 30); // old, excluded
        batch.commit().unwrap();

        let result = store.scan_all_spore_24h_transfers(now_ms).unwrap();
        assert_eq!(*result.get(cluster_a.as_slice()).unwrap(), 10);
    }

    // ---- NFT hourly transfers ----

    #[test]
    fn test_scan_all_nft_24h_transfers_empty() {
        let (_dir, store) = test_store();
        let now_ms = 1_700_000_000_000i64;
        let result = store.scan_all_nft_24h_transfers(now_ms).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_scan_all_nft_24h_transfers_multiple_collections() {
        let (_dir, store) = test_store();
        let coll_a = [0x0Au8; 32];
        let coll_b = [0x0Bu8; 32];
        let now_ms = 1_700_000_000_000i64;
        let current_hour = now_ms / 3_600_000;

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_hourly_transfer(&coll_a, current_hour, 10);
        batch.put_nft_hourly_transfer(&coll_a, current_hour - 5, 20);
        batch.put_nft_hourly_transfer(&coll_b, current_hour - 1, 15);
        batch.commit().unwrap();

        let result = store.scan_all_nft_24h_transfers(now_ms).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(*result.get(coll_a.as_slice()).unwrap(), 30);
        assert_eq!(*result.get(coll_b.as_slice()).unwrap(), 15);
    }

    #[test]
    fn test_scan_all_nft_24h_transfers_excludes_old() {
        let (_dir, store) = test_store();
        let coll_a = [0x0Au8; 32];
        let now_ms = 1_700_000_000_000i64;
        let current_hour = now_ms / 3_600_000;

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_hourly_transfer(&coll_a, current_hour, 10);
        batch.put_nft_hourly_transfer(&coll_a, current_hour - 24, 20); // at cutoff, excluded
        batch.put_nft_hourly_transfer(&coll_a, current_hour - 48, 30); // old, excluded
        batch.commit().unwrap();

        let result = store.scan_all_nft_24h_transfers(now_ms).unwrap();
        assert_eq!(*result.get(coll_a.as_slice()).unwrap(), 10);
    }

    // ---- count_token_holders ----

    #[test]
    fn test_count_token_holders_empty() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        assert_eq!(store.count_token_holders(&type_hash).unwrap(), 0);
    }

    #[test]
    fn test_count_token_holders_excludes_zero_balance() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let lock_a = [0x0Au8; 32];
        let lock_b = [0x0Bu8; 32];
        let lock_c = [0x0Cu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_holder(&type_hash, &lock_a, 100);
        batch.put_token_holder(&type_hash, &lock_b, 0); // zero balance
        batch.put_token_holder(&type_hash, &lock_c, 50);
        batch.commit().unwrap();

        assert_eq!(store.count_token_holders(&type_hash).unwrap(), 2);
    }

    #[test]
    fn test_count_token_holders_different_tokens() {
        let (_dir, store) = test_store();
        let type_a = [0x01u8; 32];
        let type_b = [0x02u8; 32];
        let lock_a = [0x0Au8; 32];
        let lock_b = [0x0Bu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_holder(&type_a, &lock_a, 100);
        batch.put_token_holder(&type_a, &lock_b, 200);
        batch.put_token_holder(&type_b, &lock_a, 50);
        batch.commit().unwrap();

        assert_eq!(store.count_token_holders(&type_a).unwrap(), 2);
        assert_eq!(store.count_token_holders(&type_b).unwrap(), 1);
    }

    // ---- list_token_activities ----

    fn make_transfer_record(
        tx_hash: &[u8],
        block_number: i64,
        from: Option<&[u8]>,
        to: &[u8],
        amount: u128,
        is_mint: bool,
        is_burn: bool,
    ) -> TokenTransferRecord {
        TokenTransferRecord {
            tx_hash: tx_hash.to_vec(),
            block_number,
            from_lock_hash: from.map(|f| f.to_vec()),
            to_lock_hash: to.to_vec(),
            amount,
            is_mint,
            is_burn,
            timestamp: 1_700_000_000_000,
        }
    }

    #[test]
    fn test_list_token_activities_empty() {
        let (_dir, store) = test_store();
        let type_hash = [0xA1u8; 32];
        let result = store.list_token_activities(&type_hash, 10, None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_list_token_activities_groups_by_tx_hash() {
        let (_dir, store) = test_store();
        let type_hash = [0xA2u8; 32];
        let tx_hash = [0x01u8; 32];
        let lock_a = [0x0Au8; 32];
        let lock_b = [0x0Bu8; 32];
        let lock_c = [0x0Cu8; 32];

        // 3 records in same block with same tx_hash → should group into 1 activity
        let mut batch = StoreBatch::new(&store);
        batch.put_token_transfer(
            &type_hash,
            100,
            0,
            &make_transfer_record(&tx_hash, 100, None, &lock_a, 1000, true, false),
        );
        batch.put_token_transfer(
            &type_hash,
            100,
            1,
            &make_transfer_record(&tx_hash, 100, Some(&lock_a), &lock_b, 500, false, false),
        );
        batch.put_token_transfer(
            &type_hash,
            100,
            2,
            &make_transfer_record(&tx_hash, 100, Some(&lock_a), &lock_c, 200, false, false),
        );
        batch.commit().unwrap();

        let result = store.list_token_activities(&type_hash, 10, None).unwrap();
        assert_eq!(result.len(), 1);

        let (block_num, last_idx, entry) = &result[0];
        assert_eq!(*block_num, 100);
        assert_eq!(*last_idx, 2);
        assert_eq!(entry.tx_hash, tx_hash.to_vec());
        assert_eq!(entry.transfers.len(), 3);
        assert!(entry.transfers[0].is_mint);
        assert!(!entry.transfers[1].is_mint);
    }

    #[test]
    fn test_list_token_activities_pagination() {
        let (_dir, store) = test_store();
        let type_hash = [0xA3u8; 32];
        let tx_1 = [0x01u8; 32];
        let tx_2 = [0x02u8; 32];
        let tx_3 = [0x03u8; 32];
        let lock_a = [0x0Au8; 32];

        // 3 separate transactions across blocks (keys sorted desc by block_num)
        let mut batch = StoreBatch::new(&store);
        batch.put_token_transfer(
            &type_hash,
            300,
            0,
            &make_transfer_record(&tx_1, 300, None, &lock_a, 100, true, false),
        );
        batch.put_token_transfer(
            &type_hash,
            200,
            0,
            &make_transfer_record(&tx_2, 200, None, &lock_a, 200, true, false),
        );
        batch.put_token_transfer(
            &type_hash,
            100,
            0,
            &make_transfer_record(&tx_3, 100, None, &lock_a, 300, true, false),
        );
        batch.commit().unwrap();

        // Page 1: limit=2
        let page1 = store.list_token_activities(&type_hash, 2, None).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].2.tx_hash, tx_1.to_vec()); // block 300 first (desc)
        assert_eq!(page1[1].2.tx_hash, tx_2.to_vec()); // block 200

        // Page 2: use cursor from last entry
        let cursor = (page1[1].0, page1[1].1);
        let page2 = store
            .list_token_activities(&type_hash, 2, Some(cursor))
            .unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].2.tx_hash, tx_3.to_vec()); // block 100
    }

    #[test]
    fn test_list_token_activities_mixed_actions() {
        let (_dir, store) = test_store();
        let type_hash = [0xA4u8; 32];
        let tx_hash = [0x01u8; 32];
        let lock_a = [0x0Au8; 32];
        let lock_b = [0x0Bu8; 32];

        // mint + transfer in same tx
        let mut batch = StoreBatch::new(&store);
        batch.put_token_transfer(
            &type_hash,
            100,
            0,
            &make_transfer_record(&tx_hash, 100, None, &lock_a, 1000, true, false),
        );
        batch.put_token_transfer(
            &type_hash,
            100,
            1,
            &make_transfer_record(&tx_hash, 100, Some(&lock_a), &lock_b, 500, false, false),
        );
        batch.commit().unwrap();

        let result = store.list_token_activities(&type_hash, 10, None).unwrap();
        assert_eq!(result.len(), 1);
        let actions = &result[0].2.actions;
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0], AssetAction::Mint));
        assert!(matches!(actions[1], AssetAction::Transfer));
    }

    #[test]
    fn test_list_token_activities_dedup_actions() {
        let (_dir, store) = test_store();
        let type_hash = [0xA5u8; 32];
        let tx_hash = [0x01u8; 32];
        let lock_a = [0x0Au8; 32];
        let lock_b = [0x0Bu8; 32];
        let lock_c = [0x0Cu8; 32];

        // Multiple plain transfers → single Transfer action
        let mut batch = StoreBatch::new(&store);
        batch.put_token_transfer(
            &type_hash,
            100,
            0,
            &make_transfer_record(&tx_hash, 100, Some(&lock_a), &lock_b, 500, false, false),
        );
        batch.put_token_transfer(
            &type_hash,
            100,
            1,
            &make_transfer_record(&tx_hash, 100, Some(&lock_b), &lock_c, 300, false, false),
        );
        batch.commit().unwrap();

        let result = store.list_token_activities(&type_hash, 10, None).unwrap();
        assert_eq!(result.len(), 1);
        let actions = &result[0].2.actions;
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AssetAction::Transfer));
    }
}
