//! Token operations.

use std::collections::HashMap;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{TokenInfo, TokenTransferRecord};

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
}
