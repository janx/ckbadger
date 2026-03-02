//! Activity query operations.

use crate::keys;
use crate::store::*;
use crate::types::*;
use rocksdb::IteratorMode;

impl CkbadgerStore {
    /// List activities for an address (lock_hash), newest first.
    ///
    /// Optionally start after the given `(block_num, tx_idx)` cursor.
    /// An optional `filter` narrows results: "ckb", "token", "nft", "dao".
    /// Returns `(block_num, tx_idx, entry)` tuples for cursor construction.
    pub fn list_activities(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
        filter: Option<&str>,
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
                if Self::matches_activity_filter(&entry, filter) {
                    results.push((block_num, tx_idx, entry));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    /// List daily stats for an address in a date range (inclusive).
    /// Returns (date_yyyymmdd, stats) tuples in ascending date order.
    pub fn list_addr_daily_stats(
        &self,
        lock_hash: &[u8],
        from_date: u32,
        to_date: u32,
    ) -> anyhow::Result<Vec<(u32, AddressDailyStats)>> {
        let start_key = keys::encode_addr_daily_stats_key(lock_hash, from_date);
        let prefix = &lock_hash[..32];

        let iter = self.iterator_cf(
            self.cf_addr_daily_stats(),
            IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(prefix) {
                break;
            }
            if key.len() == keys::ADDR_DAILY_STATS_KEY_SIZE {
                let (_, date) = keys::decode_addr_daily_stats_key(&key);
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
