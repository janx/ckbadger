//! Activity query operations.

use crate::keys;
use crate::store::*;
use crate::types::*;

impl CkbadgerStore {
    /// List activities for an address (lock_hash), newest first.
    ///
    /// Optionally start after the given `(block_num, tx_idx)` cursor.
    /// Returns `(block_num, tx_idx, entry)` tuples for cursor construction.
    pub fn list_activities(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, ActivityEntry)>> {
        let prefix = &lock_hash[..32];

        // For cursor: start from the key just after the cursor position.
        // For no cursor: start from the lock_hash prefix (newest first due to desc key).
        let start_key = match cursor {
            Some((block_num, tx_idx)) => {
                // tx_idx + 1 moves past the cursor entry in the descending order
                keys::encode_activity_key(lock_hash, block_num, tx_idx + 1)
            }
            None => prefix.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_activities(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(prefix) {
                break;
            }
            if key.len() == 44 {
                let (_, block_num, tx_idx) = keys::decode_activity_key(&key);
                let entry: ActivityEntry = bincode::deserialize(&value)?;
                results.push((block_num, tx_idx, entry));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }
}
