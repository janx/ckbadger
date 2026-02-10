//! Activity operations.

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::ActivityEntry;

impl CkbadgerStore {
    pub fn get_activity(
        &self,
        block_num: i64,
        activity_idx: i32,
    ) -> anyhow::Result<Option<ActivityEntry>> {
        let key = keys::encode_activity_key(block_num, activity_idx);
        match self.get_cf(self.cf_activities(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// List activities for a block.
    pub fn list_block_activities(
        &self,
        block_num: i64,
    ) -> anyhow::Result<Vec<(i32, ActivityEntry)>> {
        let prefix = keys::encode_block_num(block_num);
        let iter = self.prefix_iterator_cf(self.cf_activities(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() == 12 {
                let idx = keys::decode_tx_idx(&key[8..12]);
                let entry: ActivityEntry = bincode::deserialize(&value)?;
                results.push((idx, entry));
            }
        }
        Ok(results)
    }

    /// List activities for an address.
    pub fn list_activities_by_addr(
        &self,
        lock_hash: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<ActivityEntry>> {
        let iter = self.prefix_iterator_cf(self.cf_activities_by_addr(), lock_hash);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(lock_hash) {
                break;
            }
            // Key: lock_hash(32) + block_num(8) + idx(4) = 44
            if key.len() == 44 {
                let block_num = keys::decode_block_num(&key[32..40]);
                let idx = keys::decode_tx_idx(&key[40..44]);
                if let Some(entry) = self.get_activity(block_num, idx)? {
                    results.push(entry);
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }
}
