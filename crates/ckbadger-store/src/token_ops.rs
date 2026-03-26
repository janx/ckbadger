//! Token operations.

use std::collections::HashMap;

use rocksdb::IteratorMode;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{
    AssetAction, TokenActivityEntry, TokenActivityTransfer, TokenDailyDelta, TokenInfo,
    TokenTransferRecord,
};

use crate::bytes_to_hex;

#[derive(Debug, Clone)]
pub struct TokenDailyValidationError {
    pub type_hash: Vec<u8>,
    pub date_yyyymmdd: u32,
    pub owned_capacity: i128,
    pub owned_knowledge: i128,
    pub capacity_delta: i128,
    pub used_delta: i128,
}

impl CkbadgerStore {
    pub fn get_token(&self, type_hash: &[u8]) -> anyhow::Result<Option<TokenInfo>> {
        match self.get_cf(self.cf_tokens(), type_hash)? {
            Some(value) => Ok(Some(postcard::from_bytes(&value)?)),
            None => Ok(None),
        }
    }

    /// Batch-fetch multiple tokens by type_script_hash in a single RocksDB multi_get.
    pub fn get_tokens_batch(
        &self,
        type_hashes: &[Vec<u8>],
    ) -> anyhow::Result<Vec<(Vec<u8>, Option<TokenInfo>)>> {
        if type_hashes.is_empty() {
            return Ok(Vec::new());
        }
        let cf = self.cf_tokens();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            type_hashes.iter().map(|h| (cf, h.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);
        let mut result = Vec::with_capacity(type_hashes.len());
        for (hash, value_result) in type_hashes.iter().zip(values) {
            let info = match value_result {
                Ok(Some(value)) => Some(postcard::from_bytes::<TokenInfo>(&value).map_err(
                    |e| {
                        anyhow::anyhow!(
                            "failed to deserialize token info in get_tokens_batch: type_hash=0x{}, error={}",
                            bytes_to_hex(hash),
                            e
                        )
                    },
                )?),
                Ok(None) => None,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_tokens_batch: type_hash=0x{}, error={}",
                        bytes_to_hex(hash),
                        e
                    ));
                }
            };
            result.push((hash.clone(), info));
        }
        Ok(result)
    }

    pub fn put_token_direct(&self, type_hash: &[u8], info: &TokenInfo) -> anyhow::Result<()> {
        let value = postcard::to_allocvec(info)?;
        self.put_cf(self.cf_tokens(), type_hash, &value)
    }

    /// List all tokens.
    pub fn list_tokens(&self) -> anyhow::Result<Vec<(Vec<u8>, TokenInfo)>> {
        let iter = self.iterator_cf(self.cf_tokens(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item
                .map_err(|e| anyhow::anyhow!("failed to iterate tokens in list_tokens: {}", e))?;
            let info: TokenInfo = postcard::from_bytes(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize token info in list_tokens: type_hash=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push((key.to_vec(), info));
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
            Some(value) => anyhow::bail!(
                "token_holders: corrupt value length {} (expected 16) for type_hash=0x{}, lock_hash=0x{}",
                value.len(),
                bytes_to_hex(type_hash),
                bytes_to_hex(lock_hash)
            ),
            None => Ok(None),
        }
    }

    /// Get total transfer count for a token from the stats CF.
    pub fn get_token_transfers_count(&self, type_hash: &[u8]) -> anyhow::Result<i64> {
        let key = keys::encode_token_transfers_key(type_hash);
        match self.get_cf(self.cf_stats_token(), &key)? {
            Some(value) if value.len() == 8 => {
                Ok(i64::from_le_bytes(value[..8].try_into().unwrap()))
            }
            Some(value) => anyhow::bail!(
                "stats_token: corrupt transfers_count value length {} (expected 8) for type_hash=0x{}",
                value.len(),
                bytes_to_hex(type_hash)
            ),
            None => Ok(0),
        }
    }

    /// Get 24h transfer count for a token by summing recent hourly buckets.
    pub fn get_token_24h_transfers(&self, type_hash: &[u8], now_ms: i64) -> anyhow::Result<i64> {
        let current_hour = now_ms / 3_600_000;
        let cutoff_hour = current_hour - 24;
        let prefix = keys::encode_token_hourly_prefix(type_hash);
        let iter = self.prefix_iterator_cf(self.cf_stats_token(), &prefix);
        let mut total: i64 = 0;

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_token in get_token_24h_transfers: {}",
                    e
                )
            })?;
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
        match self.get_cf(self.cf_stats_token(), &key)? {
            Some(value) => Ok(Some(postcard::from_bytes(&value)?)),
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
        let value = postcard::to_allocvec(delta)?;
        self.put_cf(self.cf_stats_token(), &key, &value)
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
            self.cf_stats_token(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_token in list_token_daily_deltas_in_range: {}",
                    e
                )
            })?;
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
            let delta: TokenDailyDelta = postcard::from_bytes(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize token daily delta in list_token_daily_deltas_in_range: type_hash=0x{}, date={}, error={}",
                    bytes_to_hex(type_hash),
                    date,
                    e
                )
            })?;
            results.push((date, delta));
        }

        Ok(results)
    }

    /// Return the first invalid token daily accumulation (if any).
    ///
    /// Validity checks are applied in per-token date order:
    /// - running live capacity must be >= 0
    /// - running live used capacity must be >= 0
    /// - running live used capacity must be <= running live capacity
    pub fn find_first_invalid_token_daily_delta(
        &self,
    ) -> anyhow::Result<Option<TokenDailyValidationError>> {
        let start = [keys::STATS_PREFIX_TOKEN_DAILY];
        let iter = self.iterator_cf(
            self.cf_stats_token(),
            IteratorMode::From(&start, rocksdb::Direction::Forward),
        );

        let mut current_type_hash: Option<Vec<u8>> = None;
        let mut owned_capacity: i128 = 0;
        let mut owned_knowledge: i128 = 0;

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_token in find_first_invalid_token_daily_delta: {}",
                    e
                )
            })?;
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
            let delta: TokenDailyDelta = postcard::from_bytes(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize token daily delta while validating: type_hash=0x{}, date={}, error={}",
                    bytes_to_hex(&type_hash),
                    date_yyyymmdd,
                    e
                )
            })?;

            if current_type_hash.as_ref() != Some(&type_hash) {
                current_type_hash = Some(type_hash.clone());
                owned_capacity = 0;
                owned_knowledge = 0;
            }

            owned_capacity = owned_capacity
                .checked_add(delta.owned_capacity_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily validation overflow on capacity: type_hash=0x{}, date={}, current={}, delta={}",
                        bytes_to_hex(&type_hash),
                        date_yyyymmdd,
                        owned_capacity,
                        delta.owned_capacity_delta
                    )
                })?;
            owned_knowledge = owned_knowledge
                .checked_add(delta.owned_knowledge_delta)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "token daily validation overflow on used capacity: type_hash=0x{}, date={}, current={}, delta={}",
                        bytes_to_hex(&type_hash),
                        date_yyyymmdd,
                        owned_knowledge,
                        delta.owned_knowledge_delta
                    )
                })?;

            if owned_capacity < 0 || owned_knowledge < 0 || owned_knowledge > owned_capacity {
                return Ok(Some(TokenDailyValidationError {
                    type_hash,
                    date_yyyymmdd,
                    owned_capacity,
                    owned_knowledge,
                    capacity_delta: delta.owned_capacity_delta,
                    used_delta: delta.owned_knowledge_delta,
                }));
            }
        }

        Ok(None)
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
        let iter = self.prefix_iterator_cf(self.cf_stats_token(), &prefix);
        let mut result: HashMap<Vec<u8>, i64> = HashMap::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_token in scan_all_token_24h_transfers: {}",
                    e
                )
            })?;
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
        let iter = self.prefix_iterator_cf(self.cf_stats_spore(), &prefix);
        let mut result: HashMap<Vec<u8>, i64> = HashMap::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_spore in scan_all_spore_24h_transfers: {}",
                    e
                )
            })?;
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
        let iter = self.prefix_iterator_cf(self.cf_stats_mnft(), &prefix);
        let mut result: HashMap<Vec<u8>, i64> = HashMap::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_object in scan_all_nft_24h_transfers: {}",
                    e
                )
            })?;
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
        let iter = self.prefix_iterator_cf(self.cf_stats_token(), &prefix);
        let mut deleted = 0u64;

        for item in iter {
            let (key, _value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_token hourly buckets in cleanup_old_hourly_buckets: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() == 41 {
                let hour = i64::from_be_bytes(key[33..41].try_into().unwrap());
                if hour < cutoff_hour {
                    self.delete_cf(self.cf_stats_token(), &key)?;
                    deleted += 1;
                }
            }
        }
        Ok(deleted)
    }

    /// Delete NFT hourly buckets older than the cutoff hour for a collection.
    pub fn cleanup_old_nft_hourly_buckets(
        &self,
        collection_id: &[u8],
        cutoff_hour: i64,
    ) -> anyhow::Result<u64> {
        if collection_id.is_empty() || collection_id.len() > 32 {
            anyhow::bail!(
                "cleanup_old_nft_hourly_buckets expects 1..=32 byte collection_id, got {} bytes",
                collection_id.len()
            );
        }
        let prefix = keys::encode_nft_hourly_prefix(collection_id);
        let iter = self.prefix_iterator_cf(self.cf_stats_mnft(), &prefix);
        let mut deleted = 0u64;

        for item in iter {
            let (key, _value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate nft hourly buckets in cleanup_old_nft_hourly_buckets: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() == 41 {
                let hour = i64::from_be_bytes(key[33..41].try_into().unwrap());
                if hour < cutoff_hour {
                    self.delete_cf(self.cf_stats_mnft(), &key)?;
                    deleted += 1;
                }
            }
        }
        Ok(deleted)
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
        if type_hash.len() != 32 {
            anyhow::bail!(
                "list_token_transfers expects 32-byte type_hash, got {} bytes",
                type_hash.len()
            );
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let prefix = &type_hash[..32];

        // For cursor: seek to the cursor key and skip that exact row.
        // For no cursor: start from the type_hash prefix (newest first due to desc key).
        let start_key = match cursor {
            Some((block_num, tx_idx)) => {
                keys::encode_token_transfer_key(type_hash, block_num, tx_idx)
            }
            None => prefix.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_token_transfers(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate token_transfers in list_token_transfers: {}",
                    e
                )
            })?;
            if !key.starts_with(prefix) {
                break;
            }
            if key.len() == 44 {
                if cursor.is_some() && key.as_ref() == start_key.as_slice() {
                    continue;
                }
                let (block_num, tx_idx) = keys::decode_token_transfer_key(&key);
                let record: TokenTransferRecord = postcard::from_bytes(&value)?;
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
        if type_hash.len() != 32 {
            anyhow::bail!(
                "list_token_activities expects 32-byte type_hash, got {} bytes",
                type_hash.len()
            );
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let prefix = &type_hash[..32];

        let start_key = match cursor {
            Some((block_num, tx_idx)) => {
                keys::encode_token_transfer_key(type_hash, block_num, tx_idx)
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

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate token_transfers in list_token_activities: {}",
                    e
                )
            })?;
            if !key.starts_with(prefix) {
                break;
            }
            if key.len() != 44 {
                continue;
            }
            if cursor.is_some() && key.as_ref() == start_key.as_slice() {
                continue;
            }

            let (block_num, tx_idx) = keys::decode_token_transfer_key(&key);
            let record: TokenTransferRecord = postcard::from_bytes(&value)?;

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

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate token_holders in count_token_holders: {}",
                    e
                )
            })?;
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

    /// Aggregate live holder count and total live supply for a token from `token_holders`.
    ///
    /// This derives correctness-critical read data from the holder source of truth
    /// instead of trusting any cached aggregate embedded in `TokenInfo`.
    pub fn aggregate_token_holder_stats(&self, type_hash: &[u8]) -> anyhow::Result<(i64, i128)> {
        let iter = self.prefix_iterator_cf(self.cf_token_holders(), type_hash);
        let mut holders_count: i64 = 0;
        let mut total_supply: i128 = 0;

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate token_holders in aggregate_token_holder_stats: {}",
                    e
                )
            })?;
            if !key.starts_with(type_hash) {
                break;
            }
            if key.len() != 64 || value.len() != 16 {
                continue;
            }

            let lock_hash = &key[32..64];
            let balance = i128::from_le_bytes(value[..16].try_into().unwrap());
            if balance < 0 {
                anyhow::bail!(
                    "negative token holder balance in aggregate_token_holder_stats: type_hash=0x{}, lock_hash=0x{}, balance={}",
                    bytes_to_hex(type_hash),
                    bytes_to_hex(lock_hash),
                    balance
                );
            }
            if balance == 0 {
                continue;
            }

            holders_count = holders_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!(
                    "token holders_count overflow in aggregate_token_holder_stats: type_hash=0x{}",
                    bytes_to_hex(type_hash)
                )
            })?;
            total_supply = total_supply.checked_add(balance).ok_or_else(|| {
                anyhow::anyhow!(
                    "token total_supply overflow in aggregate_token_holder_stats: type_hash=0x{}, lock_hash=0x{}, current_total={}, balance={}",
                    bytes_to_hex(type_hash),
                    bytes_to_hex(lock_hash),
                    total_supply,
                    balance
                )
            })?;
        }

        Ok((holders_count, total_supply))
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

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate token_holders in list_token_holders: {}",
                    e
                )
            })?;
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

    /// List holders for a token ordered by balance DESC, lock_hash ASC.
    ///
    /// Optionally start after the given `(balance, lock_hash)` cursor.
    pub fn list_token_holders_by_balance(
        &self,
        type_hash: &[u8],
        limit: usize,
        cursor: Option<(i128, Vec<u8>)>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i128)>> {
        if type_hash.len() != 32 {
            anyhow::bail!(
                "list_token_holders_by_balance expects 32-byte type_hash, got {} bytes",
                type_hash.len()
            );
        }
        if let Some((_, ref lock_hash)) = cursor {
            if lock_hash.len() != 32 {
                anyhow::bail!(
                    "list_token_holders_by_balance cursor expects 32-byte lock_hash, got {} bytes",
                    lock_hash.len()
                );
            }
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let start_key = match cursor {
            Some((balance, lock_hash)) => {
                keys::encode_token_holder_balance_seek_after_key(type_hash, balance, &lock_hash)
            }
            None => type_hash.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_token_holders_by_balance(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate token_holders_by_balance in list_token_holders_by_balance: {}",
                    e
                )
            })?;
            if !key.starts_with(type_hash) {
                break;
            }
            if key.len() == keys::TOKEN_HOLDER_BALANCE_KEY_SIZE {
                if !value.is_empty() {
                    anyhow::bail!(
                        "token_holders_by_balance value must be empty in list_token_holders_by_balance: type_hash=0x{}, value_len={}",
                        bytes_to_hex(type_hash),
                        value.len()
                    );
                }
                let (_, balance, lock_hash) = keys::decode_token_holder_balance_key(&key);
                results.push((lock_hash, balance));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// List tokens held by an address ordered by balance DESC, type_hash ASC.
    ///
    /// Optionally start after the given `(balance, type_hash)` cursor.
    pub fn list_address_tokens_by_balance(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i128, Vec<u8>)>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i128)>> {
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "list_address_tokens_by_balance expects 32-byte lock_hash, got {} bytes",
                lock_hash.len()
            );
        }
        if let Some((_, ref type_hash)) = cursor {
            if type_hash.len() != 32 {
                anyhow::bail!(
                    "list_address_tokens_by_balance cursor expects 32-byte type_hash, got {} bytes",
                    type_hash.len()
                );
            }
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let start_key = match cursor {
            Some((balance, type_hash)) => {
                keys::encode_addr_token_balance_seek_after_key(lock_hash, balance, &type_hash)
            }
            None => lock_hash.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_addr_tokens_by_balance(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate addr_tokens_by_balance in list_address_tokens_by_balance: {}",
                    e
                )
            })?;
            if !key.starts_with(lock_hash) {
                break;
            }
            if key.len() == keys::ADDR_TOKEN_BALANCE_KEY_SIZE {
                if !value.is_empty() {
                    anyhow::bail!(
                        "addr_tokens_by_balance value must be empty in list_address_tokens_by_balance: lock_hash=0x{}, value_len={}",
                        bytes_to_hex(lock_hash),
                        value.len()
                    );
                }
                let (_, balance, type_hash) = keys::decode_addr_token_balance_key(&key);
                results.push((type_hash, balance));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
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
    fn test_get_tokens_batch_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let type_hash = vec![0xAA; 32];
        store
            .put_cf(store.cf_tokens(), &type_hash, b"invalid-token-payload")
            .unwrap();

        let err = store
            .get_tokens_batch(std::slice::from_ref(&type_hash))
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize token info in get_tokens_batch"));
    }

    #[test]
    fn test_list_tokens_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let type_hash = vec![0xAB; 32];
        store
            .put_cf(store.cf_tokens(), &type_hash, b"invalid-token-payload")
            .unwrap();

        let err = store.list_tokens().unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize token info in list_tokens"));
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
                    owned_capacity_delta: 1_000_000_000_000,
                    owned_knowledge_delta: 610_000_000_000,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_hash,
                20240116,
                &TokenDailyDelta {
                    owned_capacity_delta: -200_000_000_000,
                    owned_knowledge_delta: -150_000_000_000,
                },
            )
            .unwrap();

        let day1 = store
            .get_token_daily_delta(&type_hash, 20240115)
            .unwrap()
            .unwrap();
        assert_eq!(day1.owned_capacity_delta, 1_000_000_000_000);
        assert_eq!(day1.owned_knowledge_delta, 610_000_000_000);

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
    fn test_list_token_daily_deltas_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let type_hash = [0x06u8; 32];
        let key = keys::encode_token_daily_key(&type_hash, 20240115);
        store.put_cf(store.cf_stats_token(), &key, &[0xFF]).unwrap();

        let err = store.list_token_daily_deltas(&type_hash).unwrap_err();
        assert!(err.to_string().contains(
            "failed to deserialize token daily delta in list_token_daily_deltas_in_range"
        ));
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
                    owned_capacity_delta: 1_000,
                    owned_knowledge_delta: 600,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_hash,
                20240102,
                &TokenDailyDelta {
                    owned_capacity_delta: -200,
                    owned_knowledge_delta: -100,
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
                    owned_capacity_delta: 500,
                    owned_knowledge_delta: 300,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_bad,
                20240101,
                &TokenDailyDelta {
                    owned_capacity_delta: 100,
                    owned_knowledge_delta: 120,
                },
            )
            .unwrap();

        let invalid = store
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .expect("expected invalid token daily delta");
        assert_eq!(invalid.type_hash, type_bad.to_vec());
        assert_eq!(invalid.date_yyyymmdd, 20240101);
        assert_eq!(invalid.owned_capacity, 100);
        assert_eq!(invalid.owned_knowledge, 120);
        assert_eq!(invalid.capacity_delta, 100);
        assert_eq!(invalid.used_delta, 120);
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
    fn test_cleanup_old_nft_hourly_buckets() {
        let (_dir, store) = test_store();
        let collection_id = [0x16u8; 32];
        let current_hour = 510_000i64;

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_hourly_transfer(&collection_id, current_hour, 10);
        batch.put_mnft_hourly_transfer(&collection_id, current_hour - 24, 20);
        batch.put_mnft_hourly_transfer(&collection_id, current_hour - 100, 30);
        batch.commit().unwrap();

        let cutoff = current_hour - 48;
        let deleted = store
            .cleanup_old_nft_hourly_buckets(&collection_id, cutoff)
            .unwrap();
        assert_eq!(deleted, 1);

        let keep_key = keys::encode_nft_hourly_key(&collection_id, current_hour);
        let keep_key2 = keys::encode_nft_hourly_key(&collection_id, current_hour - 24);
        let deleted_key = keys::encode_nft_hourly_key(&collection_id, current_hour - 100);
        assert!(store.get_stats_key(&keep_key).unwrap().is_some());
        assert!(store.get_stats_key(&keep_key2).unwrap().is_some());
        assert!(store.get_stats_key(&deleted_key).unwrap().is_none());
    }

    #[test]
    fn test_cleanup_old_nft_hourly_buckets_rejects_empty_collection_id() {
        let (_dir, store) = test_store();
        let err = store.cleanup_old_nft_hourly_buckets(&[], 100).unwrap_err();
        assert!(err
            .to_string()
            .contains("cleanup_old_nft_hourly_buckets expects 1..=32 byte collection_id"));
    }

    #[test]
    fn test_cleanup_old_nft_hourly_buckets_rejects_oversized_collection_id() {
        let (_dir, store) = test_store();
        let err = store
            .cleanup_old_nft_hourly_buckets(&[0x16u8; 33], 100)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("cleanup_old_nft_hourly_buckets expects 1..=32 byte collection_id"));
    }

    #[test]
    fn test_cleanup_old_nft_hourly_buckets_accepts_24_byte_collection_id() {
        let (_dir, store) = test_store();
        let collection_id = [0x16u8; 24]; // mNFT class_id: 20B issuer + 4B class index
        let current_hour = 510_000i64;

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_hourly_transfer(&collection_id, current_hour, 10);
        batch.put_mnft_hourly_transfer(&collection_id, current_hour - 100, 30);
        batch.commit().unwrap();

        let cutoff = current_hour - 48;
        let deleted = store
            .cleanup_old_nft_hourly_buckets(&collection_id, cutoff)
            .unwrap();
        assert_eq!(deleted, 1); // current_hour - 100 < cutoff
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
        batch.put_mnft_hourly_transfer(&coll_a, current_hour, 10);
        batch.put_mnft_hourly_transfer(&coll_a, current_hour - 5, 20);
        batch.put_mnft_hourly_transfer(&coll_b, current_hour - 1, 15);
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
        batch.put_mnft_hourly_transfer(&coll_a, current_hour, 10);
        batch.put_mnft_hourly_transfer(&coll_a, current_hour - 24, 20); // at cutoff, excluded
        batch.put_mnft_hourly_transfer(&coll_a, current_hour - 48, 30); // old, excluded
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

    #[test]
    fn test_aggregate_token_holder_stats_sums_positive_balances() {
        let (_dir, store) = test_store();
        let type_hash = [0x21u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_holder(&type_hash, &[0x01; 32], 100);
        batch.put_token_holder(&type_hash, &[0x02; 32], 250);
        batch.put_token_holder(&type_hash, &[0x03; 32], 0);
        batch.commit().unwrap();

        let (holders_count, total_supply) = store.aggregate_token_holder_stats(&type_hash).unwrap();
        assert_eq!(holders_count, 2);
        assert_eq!(total_supply, 350);
    }

    #[test]
    fn test_list_token_holders_by_balance_keeps_equal_balances_with_cursor() {
        let (_dir, store) = test_store();
        let type_hash = [0x11u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_holder_by_balance(&type_hash, &[0x01; 32], 100);
        batch.put_token_holder_by_balance(&type_hash, &[0x02; 32], 100);
        batch.put_token_holder_by_balance(&type_hash, &[0x03; 32], 50);
        batch.commit().unwrap();

        let first = store
            .list_token_holders_by_balance(&type_hash, 1, None)
            .unwrap();
        assert_eq!(first, vec![(vec![0x01; 32], 100)]);

        let second = store
            .list_token_holders_by_balance(&type_hash, 1, Some((100, vec![0x01; 32])))
            .unwrap();
        assert_eq!(second, vec![(vec![0x02; 32], 100)]);
    }

    #[test]
    fn test_list_address_tokens_by_balance_advances_cursor() {
        let (_dir, store) = test_store();
        let lock_hash = [0x22u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_token_by_balance(&lock_hash, &[0x11; 32], 200);
        batch.put_addr_token_by_balance(&lock_hash, &[0x22; 32], 100);
        batch.commit().unwrap();

        let first = store
            .list_address_tokens_by_balance(&lock_hash, 1, None)
            .unwrap();
        assert_eq!(first, vec![(vec![0x11; 32], 200)]);

        let second = store
            .list_address_tokens_by_balance(&lock_hash, 1, Some((200, vec![0x11; 32])))
            .unwrap();
        assert_eq!(second, vec![(vec![0x22; 32], 100)]);
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
        // With descending tx_idx encoding, within a block entries are iterated
        // from highest tx_idx to lowest, so last_idx (cursor) is the lowest.
        assert_eq!(*last_idx, 0);
        assert_eq!(entry.tx_hash, tx_hash.to_vec());
        assert_eq!(entry.transfers.len(), 3);
        // With descending tx_idx, iteration order is tx_idx=2, 1, 0.
        // tx_idx=0 was the mint, so it appears last in the transfers vec.
        assert!(!entry.transfers[0].is_mint);
        assert!(entry.transfers[2].is_mint);
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
    fn test_list_token_transfers_cursor_i32_max_does_not_overflow() {
        let (_dir, store) = test_store();
        let type_hash = [0xA6u8; 32];
        let tx_hash = [0x01u8; 32];
        let lock = [0x0Au8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_token_transfer(
            &type_hash,
            100,
            0,
            &make_transfer_record(&tx_hash, 100, None, &lock, 100, true, false),
        );
        batch.commit().unwrap();

        // Verify i32::MAX as cursor tx_idx does not cause overflow.
        // With descending tx_idx encoding, i32::MAX lands at the start of
        // block 100's entries (before tx_idx=0), so the entry IS returned.
        let page = store
            .list_token_transfers(&type_hash, 10, Some((100, i32::MAX)))
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].0, 100); // block_num
        assert_eq!(page[0].1, 0); // tx_idx
    }

    #[test]
    fn test_list_token_activities_rejects_non_32_byte_type_hash() {
        let (_dir, store) = test_store();

        let err = store
            .list_token_activities(&[0x01; 31], 10, None)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("list_token_activities expects 32-byte type_hash"));
    }

    #[test]
    fn test_list_token_transfers_rejects_non_32_byte_type_hash() {
        let (_dir, store) = test_store();

        let err = store
            .list_token_transfers(&[0x01; 31], 10, None)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("list_token_transfers expects 32-byte type_hash"));
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
        assert!(actions.iter().any(|a| matches!(a, AssetAction::Mint)));
        assert!(actions.iter().any(|a| matches!(a, AssetAction::Transfer)));
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
