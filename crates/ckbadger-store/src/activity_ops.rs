//! Activity query operations.

use crate::keys;
use crate::store::*;
use crate::types::*;
use rocksdb::IteratorMode;

impl CkbadgerStore {
    /// List activities for an address (lock_hash), newest first.
    ///
    /// Optionally start after the given `(block_num, tx_idx, seq)` cursor.
    /// An optional `filter` narrows results: "ckb", "token", "nft", "dao".
    /// Returns `(block_num, tx_idx, seq, entry)` tuples for cursor construction.
    pub fn list_activities(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32, i16)>,
        filter: Option<&str>,
    ) -> anyhow::Result<Vec<(i64, i32, i16, ActivityEntry)>> {
        let prefix = &lock_hash[..32];

        // For cursor: start from the key just after the cursor position.
        // For no cursor: start from the lock_hash prefix (newest first due to desc key).
        let start_key = match cursor {
            Some((block_num, tx_idx, seq)) => {
                // seq + 1 moves past the cursor entry in the descending (inverted) key order
                keys::encode_addr_activity_key(lock_hash, block_num, tx_idx, seq + 1)
            }
            None => prefix.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_addr_activities(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        // Step 1: Collect activity IDs from the thin index
        let over_fetch = limit * 4; // over-fetch to allow for filter rejection
        let mut index_keys: Vec<(i64, i32, i16, [u8; keys::ACTIVITY_ID_SIZE])> = Vec::new();
        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(prefix) {
                break;
            }
            if key.len() == 46 {
                let (_, block_num, tx_idx, seq) = keys::decode_addr_activity_key(&key);
                let activity_id = keys::encode_activity_id(block_num, tx_idx, seq);
                index_keys.push((block_num, tx_idx, seq, activity_id));
                if index_keys.len() >= over_fetch {
                    break;
                }
            }
        }

        if index_keys.is_empty() {
            return Ok(Vec::new());
        }

        // Step 2: Batch-get full entries from append store
        let cf = self.cf_activities();
        let cf_keys: Vec<_> = index_keys
            .iter()
            .map(|(_, _, _, id)| (cf, id.as_slice()))
            .collect();
        let values = self.append_multi_get_cf(cf_keys);

        let mut results = Vec::new();
        for ((block_num, tx_idx, seq, _), value_result) in index_keys.into_iter().zip(values) {
            if let Ok(Some(value)) = value_result {
                let entry: ActivityEntry = bincode::deserialize(&value)?;
                if Self::matches_activity_filter(&entry, filter) {
                    results.push((block_num, tx_idx, seq, entry));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    /// Get daily stats for an address for a single date.
    pub fn get_addr_daily_stats(
        &self,
        lock_hash: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<AddressDailyStats>> {
        let key = keys::encode_addr_daily_stats_stats_key(lock_hash, date_yyyymmdd);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// List daily stats for an address in a date range (inclusive).
    /// Returns (date_yyyymmdd, stats) tuples in ascending date order.
    pub fn list_addr_daily_stats(
        &self,
        lock_hash: &[u8],
        from_date: u32,
        to_date: u32,
    ) -> anyhow::Result<Vec<(u32, AddressDailyStats)>> {
        let start_key = keys::encode_addr_daily_stats_stats_key(lock_hash, from_date);
        let prefix = keys::encode_addr_daily_stats_stats_prefix(lock_hash);

        let iter = self.iterator_cf(
            self.cf_stats(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            // Key: prefix(1) + lock_hash(32) + date(4) = 37B
            if key.len() == 1 + keys::ADDR_DAILY_STATS_KEY_SIZE {
                let (_, date) = keys::decode_addr_daily_stats_key(&key[1..]);
                if date > to_date {
                    break;
                }
                let stats: AddressDailyStats = bincode::deserialize(&value)?;
                results.push((date, stats));
            }
        }
        Ok(results)
    }

    fn matches_activity_filter(entry: &ActivityEntry, filter: Option<&str>) -> bool {
        match filter {
            None | Some("all") => true,
            Some("ckb") => entry.asset_changes.is_empty(),
            Some("token") => entry
                .asset_changes
                .iter()
                .any(|c| matches!(c, AssetChange::Token { .. })),
            Some("nft") => entry
                .asset_changes
                .iter()
                .any(|c| matches!(c, AssetChange::Nft { .. } | AssetChange::Dob { .. })),
            Some("dao") => entry.asset_changes.iter().any(|c| {
                matches!(
                    c,
                    AssetChange::DaoDeposit { .. }
                        | AssetChange::DaoWithdrawRequest { .. }
                        | AssetChange::DaoWithdrawComplete { .. }
                )
            }),
            Some(_) => true,
        }
    }
}
