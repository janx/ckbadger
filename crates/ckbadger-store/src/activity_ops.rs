//! Activity query operations.

use crate::keys;
use crate::store::*;
use crate::types::*;

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

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
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "list_activities expects 32-byte lock_hash, got {} bytes",
                lock_hash.len()
            );
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let prefix = &lock_hash[..32];

        // For cursor: seek to the cursor key and skip that exact row.
        // For no cursor: start from the lock_hash prefix (newest first due to desc key).
        let start_key = match cursor {
            Some((block_num, tx_idx)) => {
                keys::encode_activity_seek_after_key(lock_hash, block_num, tx_idx)
            }
            None => prefix.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_activities(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate activities in list_activities: {}", e)
            })?;
            if !key.starts_with(prefix) {
                break;
            }
            if key.len() == keys::ACTIVITY_KEY_SIZE {
                let (_, block_num, tx_idx, block_hash_from_key, tx_hash_from_key) =
                    keys::decode_activity_key(&key);
                let entry: ActivityEntry = bincode::deserialize(&value)?;
                if entry.tx_hash != tx_hash_from_key {
                    anyhow::bail!(
                        "activity key/value tx_hash mismatch in list_activities: lock_hash=0x{}, block_num={}, tx_idx={}, key_tx_hash=0x{}, value_tx_hash=0x{}",
                        bytes_to_hex(prefix),
                        block_num,
                        tx_idx,
                        bytes_to_hex(&tx_hash_from_key),
                        bytes_to_hex(&entry.tx_hash)
                    );
                }
                if entry.block_hash != block_hash_from_key {
                    anyhow::bail!(
                        "activity key/value block_hash mismatch in list_activities: lock_hash=0x{}, block_num={}, tx_idx={}, key_block_hash=0x{}, value_block_hash=0x{}",
                        bytes_to_hex(prefix),
                        block_num,
                        tx_idx,
                        bytes_to_hex(&block_hash_from_key),
                        bytes_to_hex(&entry.block_hash)
                    );
                }
                if entry.block_number != block_num || entry.tx_index != tx_idx {
                    anyhow::bail!(
                        "activity key/value location mismatch in list_activities: lock_hash=0x{}, key_block_num={}, value_block_num={}, key_tx_idx={}, value_tx_idx={}",
                        bytes_to_hex(prefix),
                        block_num,
                        entry.block_number,
                        tx_idx,
                        entry.tx_index
                    );
                }
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
            Some(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::TempDir;

    fn make_activity_with_hash(tx_hash: &[u8], block_num: i64, tx_idx: i32) -> ActivityEntry {
        ActivityEntry {
            tx_hash: tx_hash.to_vec(),
            block_hash: vec![0x40 | (block_num as u8); 32],
            block_number: block_num,
            tx_index: tx_idx,
            timestamp: 1_700_000_000 + block_num,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![],
            peers: vec![],
        }
    }

    fn make_activity(block_num: i64, tx_idx: i32) -> ActivityEntry {
        make_activity_with_hash(&[block_num as u8; 32], block_num, tx_idx)
    }

    #[test]
    fn test_list_activities_limit_zero_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let lock = [0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_activity(&lock, 100, 0, &make_activity(100, 0));
        batch.commit().unwrap();

        let rows = store.list_activities(&lock, 0, None, None).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_list_activities_unknown_filter_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let lock = [0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_activity(&lock, 100, 0, &make_activity(100, 0));
        batch.commit().unwrap();

        let rows = store.list_activities(&lock, 10, None, Some("tok")).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_list_activities_keeps_two_rows_same_position_different_tx_hash() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let lock = [0xAB; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_activity(&lock, 100, 1, &make_activity_with_hash(&[0x10; 32], 100, 1));
        batch.put_activity(&lock, 100, 1, &make_activity_with_hash(&[0x20; 32], 100, 1));
        batch.put_activity(&lock, 99, 3, &make_activity_with_hash(&[0x30; 32], 99, 3));
        batch.commit().unwrap();

        let rows = store.list_activities(&lock, 10, None, None).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, 100);
        assert_eq!(rows[0].1, 1);
        assert_eq!(rows[0].2.tx_hash, vec![0x10; 32]);
        assert_eq!(rows[1].0, 100);
        assert_eq!(rows[1].1, 1);
        assert_eq!(rows[1].2.tx_hash, vec![0x20; 32]);
        assert_eq!(rows[2].0, 99);
        assert_eq!(rows[2].1, 3);

        let next = store
            .list_activities(&lock, 10, Some((100, 1)), None)
            .unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].0, 99);
        assert_eq!(next[0].1, 3);
        assert_eq!(next[0].2.tx_hash, vec![0x30; 32]);
    }
}
