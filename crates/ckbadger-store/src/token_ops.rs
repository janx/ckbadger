//! Token operations.

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::TokenInfo;

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
}
