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

fn validate_tx_activity_bundle_identity(
    bundle: &TxActivityBundle,
    block_num: i64,
    tx_idx: i32,
    tx_hash_from_key: &[u8],
) -> anyhow::Result<()> {
    if bundle.block_number != block_num || bundle.tx_index != tx_idx {
        anyhow::bail!(
            "tx activity bundle key/value location mismatch: key_block_num={}, value_block_num={}, key_tx_idx={}, value_tx_idx={}",
            block_num,
            bundle.block_number,
            tx_idx,
            bundle.tx_index
        );
    }
    if bundle.tx_hash != tx_hash_from_key {
        anyhow::bail!(
            "tx activity bundle key/value tx_hash mismatch: block_num={}, tx_idx={}, key_tx_hash=0x{}, value_tx_hash=0x{}",
            block_num,
            tx_idx,
            bytes_to_hex(tx_hash_from_key),
            bytes_to_hex(&bundle.tx_hash)
        );
    }
    Ok(())
}

impl CkbadgerStore {
    pub fn get_tx_activity_bundle(
        &self,
        block_num: i64,
        tx_idx: i32,
        tx_hash: &[u8],
    ) -> anyhow::Result<Option<TxActivityBundle>> {
        let key = keys::encode_tx_activity_bundle_key(block_num, tx_idx, tx_hash);
        match self.get_cf(self.cf_activities(), &key)? {
            Some(value) => {
                let bundle: TxActivityBundle = bincode::deserialize(&value)?;
                validate_tx_activity_bundle_identity(&bundle, block_num, tx_idx, tx_hash)?;
                Ok(Some(bundle))
            }
            None => Ok(None),
        }
    }

    pub fn list_tx_activity_bundles_recent(
        &self,
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<TxActivityBundle>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let start_key = cursor.map(|(block_num, tx_idx)| {
            keys::encode_tx_activity_bundle_seek_after_key(block_num, tx_idx)
        });

        let iter = match start_key.as_ref() {
            Some(key) => self.iterator_cf(
                self.cf_activities(),
                rocksdb::IteratorMode::From(key, rocksdb::Direction::Forward),
            ),
            None => self.iterator_cf(self.cf_activities(), rocksdb::IteratorMode::Start),
        };

        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate tx activity bundles in list_tx_activity_bundles_recent: {}",
                    e
                )
            })?;
            if key.len() != keys::TX_ACTIVITY_BUNDLE_KEY_SIZE {
                continue;
            }

            let (block_num, tx_idx, tx_hash_from_key) = keys::decode_tx_activity_bundle_key(&key);
            let bundle: TxActivityBundle = bincode::deserialize(&value)?;
            validate_tx_activity_bundle_identity(&bundle, block_num, tx_idx, &tx_hash_from_key)?;

            results.push(bundle);
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }

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
            Some("ckb") => entry.asset_changes.is_empty() && !entry.has_type_script,
            Some("token") => entry
                .asset_changes
                .iter()
                .any(|c| matches!(c, AssetChange::Token { .. })),
            Some("object") | Some("nft") => entry
                .asset_changes
                .iter()
                .any(|c| matches!(c, AssetChange::Object { .. })),
            Some("dao") => entry.asset_changes.iter().any(|c| {
                matches!(
                    c,
                    AssetChange::DaoDeposit { .. }
                        | AssetChange::DaoWithdrawRequest { .. }
                        | AssetChange::DaoWithdrawComplete { .. }
                )
            }),
            Some("script_call") => entry
                .script_calls
                .as_ref()
                .is_some_and(|calls| !calls.is_empty()),
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
            used_delta: 0,
            is_cellbase: false,
            has_type_script: false,
            asset_changes: vec![],
            script_calls: None,
            peers: vec![],
        }
    }

    fn make_activity(block_num: i64, tx_idx: i32) -> ActivityEntry {
        make_activity_with_hash(&[block_num as u8; 32], block_num, tx_idx)
    }

    fn make_bundle(block_num: i64, tx_idx: i32, owner_count: usize) -> TxActivityBundle {
        TxActivityBundle {
            tx_hash: vec![block_num as u8; 32],
            block_hash: vec![0x40 | (block_num as u8); 32],
            block_number: block_num,
            tx_index: tx_idx,
            timestamp: 1_700_000_000 + block_num,
            is_cellbase: false,
            owners: (0..owner_count)
                .map(|i| OwnerActivityDelta {
                    lock_hash: vec![i as u8; 32],
                    lock_code_hash: vec![0x11; 32],
                    lock_hash_type: 1,
                    lock_args: vec![0x22; 20],
                    ckb_delta: i as i128,
                    used_delta: 0,
                    has_type_script: false,
                    involved_script_code_hashes: vec![vec![0x33; 32]],
                    asset_changes: vec![],
                    script_calls: None,
                    peers: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn test_list_activities_limit_zero_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
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
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
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
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
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

    #[test]
    fn test_list_tx_activity_bundles_recent_orders_newest_first() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_tx_activity_bundle(&make_bundle(300, 0, 1));
        batch.put_tx_activity_bundle(&make_bundle(200, 1, 1));
        batch.put_tx_activity_bundle(&make_bundle(100, 2, 1));
        batch.commit().unwrap();

        let rows = store.list_tx_activity_bundles_recent(10, None).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].block_number, 300);
        assert_eq!(rows[1].block_number, 200);
        assert_eq!(rows[2].block_number, 100);
    }
}
