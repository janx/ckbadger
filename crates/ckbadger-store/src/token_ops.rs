//! Token operations.

use std::collections::{HashMap, HashSet};

use rocksdb::{IteratorMode, WriteBatch};

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{LiveCellInfo, TokenDailyDelta, TokenInfo, TokenTransferRecord};

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

impl CkbadgerStore {
    pub fn get_token(&self, type_hash: &[u8]) -> anyhow::Result<Option<TokenInfo>> {
        match self.get_cf(self.cf_tokens(), type_hash)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
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
}
