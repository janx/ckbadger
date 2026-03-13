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

fn bundle_owner_to_activity_entry(
    bundle: &TxActivityBundle,
    owner: &OwnerActivityDelta,
) -> ActivityEntry {
    ActivityEntry {
        tx_hash: bundle.tx_hash.clone(),
        block_hash: bundle.block_hash.clone(),
        block_number: bundle.block_number,
        tx_index: bundle.tx_index,
        timestamp: bundle.timestamp,
        ckb_delta: owner.ckb_delta,
        used_delta: owner.used_delta,
        is_cellbase: bundle.is_cellbase,
        has_type_script: owner.has_type_script,
        asset_changes: owner.asset_changes.clone(),
        script_calls: owner.script_calls.clone(),
        peers: owner.peers.clone(),
    }
}

fn resolve_owner_activity_entry(
    bundle: &TxActivityBundle,
    lock_hash: &[u8],
) -> anyhow::Result<ActivityEntry> {
    let mut matched_owner: Option<&OwnerActivityDelta> = None;
    for owner in &bundle.owners {
        if owner.lock_hash != lock_hash {
            continue;
        }
        if matched_owner.replace(owner).is_some() {
            anyhow::bail!(
                "duplicate owner lock_hash in tx activity bundle: block_num={}, tx_idx={}, tx_hash=0x{}, lock_hash=0x{}",
                bundle.block_number,
                bundle.tx_index,
                bytes_to_hex(&bundle.tx_hash),
                bytes_to_hex(lock_hash)
            );
        }
    }

    let owner = matched_owner.ok_or_else(|| {
        anyhow::anyhow!(
            "addr_txs points to tx activity bundle without matching owner: block_num={}, tx_idx={}, tx_hash=0x{}, lock_hash=0x{}",
            bundle.block_number,
            bundle.tx_index,
            bytes_to_hex(&bundle.tx_hash),
            bytes_to_hex(lock_hash)
        )
    })?;
    Ok(bundle_owner_to_activity_entry(bundle, owner))
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

    pub fn get_latest_activities(&self) -> anyhow::Result<Vec<LatestActivityItem>> {
        const LATEST_ACTIVITY_LIMIT: usize = 64;

        let mut results = Vec::with_capacity(LATEST_ACTIVITY_LIMIT);
        let mut cursor = None;

        loop {
            let bundles = self.list_tx_activity_bundles_recent(LATEST_ACTIVITY_LIMIT, cursor)?;
            if bundles.is_empty() {
                break;
            }

            let bundles_len = bundles.len();
            let mut last_seen = None;
            for bundle in bundles {
                last_seen = Some((bundle.block_number, bundle.tx_index));
                if bundle.is_cellbase {
                    continue;
                }

                for owner in &bundle.owners {
                    results.push(LatestActivityItem {
                        lock_hash: owner.lock_hash.clone(),
                        lock_code_hash: owner.lock_code_hash.clone(),
                        lock_hash_type: owner.lock_hash_type,
                        lock_args: owner.lock_args.clone(),
                        entry: bundle_owner_to_activity_entry(&bundle, owner),
                    });
                    if results.len() >= LATEST_ACTIVITY_LIMIT {
                        return Ok(results);
                    }
                }
            }

            if bundles_len < LATEST_ACTIVITY_LIMIT {
                break;
            }
            let Some(next_cursor) = last_seen else {
                break;
            };
            cursor = Some(next_cursor);
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

        let scan_limit = limit.max(128);
        let mut results = Vec::with_capacity(limit);
        let mut scan_cursor = cursor;
        let activity_cf = self.cf_activities();

        loop {
            let rows = self.list_addr_txs_recent(lock_hash, scan_limit, scan_cursor)?;
            if rows.is_empty() {
                break;
            }

            let bundle_keys: Vec<Vec<u8>> = rows
                .iter()
                .map(|(block_num, tx_idx, tx_hash)| {
                    keys::encode_tx_activity_bundle_key(*block_num, *tx_idx, tx_hash)
                })
                .collect();
            let bundle_refs: Vec<(&rocksdb::ColumnFamily, &[u8])> = bundle_keys
                .iter()
                .map(|key| (activity_cf, key.as_slice()))
                .collect();
            let bundle_values = self.multi_get_cf(bundle_refs);

            let mut last_seen = None;
            for ((block_num, tx_idx, tx_hash), value_result) in rows.iter().zip(bundle_values) {
                last_seen = Some((*block_num, *tx_idx));
                let value = match value_result {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        anyhow::bail!(
                            "missing tx activity bundle for addr_txs entry: lock_hash=0x{}, block_num={}, tx_idx={}, tx_hash=0x{}",
                            bytes_to_hex(lock_hash),
                            block_num,
                            tx_idx,
                            bytes_to_hex(tx_hash)
                        );
                    }
                    Err(e) => {
                        anyhow::bail!(
                            "rocksdb multi_get failed in list_activities: lock_hash=0x{}, block_num={}, tx_idx={}, tx_hash=0x{}, error={}",
                            bytes_to_hex(lock_hash),
                            block_num,
                            tx_idx,
                            bytes_to_hex(tx_hash),
                            e
                        );
                    }
                };
                let bundle: TxActivityBundle = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize tx activity bundle in list_activities: lock_hash=0x{}, block_num={}, tx_idx={}, tx_hash=0x{}, error={}",
                        bytes_to_hex(lock_hash),
                        block_num,
                        tx_idx,
                        bytes_to_hex(tx_hash),
                        e
                    )
                })?;
                validate_tx_activity_bundle_identity(&bundle, *block_num, *tx_idx, tx_hash)?;
                let entry = resolve_owner_activity_entry(&bundle, lock_hash)?;
                if Self::matches_activity_filter(&entry, filter) {
                    results.push((*block_num, *tx_idx, entry));
                    if results.len() >= limit {
                        return Ok(results);
                    }
                }
            }

            if rows.len() < scan_limit {
                break;
            }
            let Some(next_cursor) = last_seen else {
                break;
            };
            scan_cursor = Some(next_cursor);
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

    fn make_bundle_for_owner(
        tx_hash: &[u8],
        block_num: i64,
        tx_idx: i32,
        lock_hash: &[u8],
    ) -> TxActivityBundle {
        TxActivityBundle {
            tx_hash: tx_hash.to_vec(),
            block_hash: vec![0x40 | (block_num as u8); 32],
            block_number: block_num,
            tx_index: tx_idx,
            timestamp: 1_700_000_000 + block_num,
            is_cellbase: false,
            owners: vec![OwnerActivityDelta {
                lock_hash: lock_hash.to_vec(),
                lock_code_hash: vec![0x11; 32],
                lock_hash_type: 1,
                lock_args: vec![0x22; 20],
                ckb_delta: 0,
                used_delta: 0,
                has_type_script: false,
                involved_script_code_hashes: vec![vec![0x33; 32]],
                asset_changes: vec![],
                script_calls: None,
                peers: vec![],
            }],
        }
    }

    #[test]
    fn test_list_activities_limit_zero_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        let bundle = make_bundle_for_owner(&[0x10; 32], 100, 0, &lock);
        batch.put_tx_activity_bundle(&bundle);
        batch.put_addr_tx(&lock, 100, 0, &bundle.tx_hash);
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
        let bundle = make_bundle_for_owner(&[0x10; 32], 100, 0, &lock);
        batch.put_tx_activity_bundle(&bundle);
        batch.put_addr_tx(&lock, 100, 0, &bundle.tx_hash);
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
        let first = make_bundle_for_owner(&[0x10; 32], 100, 1, &lock);
        let second = make_bundle_for_owner(&[0x20; 32], 100, 1, &lock);
        let third = make_bundle_for_owner(&[0x30; 32], 99, 3, &lock);
        batch.put_tx_activity_bundle(&first);
        batch.put_tx_activity_bundle(&second);
        batch.put_tx_activity_bundle(&third);
        batch.put_addr_tx(&lock, 100, 1, &first.tx_hash);
        batch.put_addr_tx(&lock, 100, 1, &second.tx_hash);
        batch.put_addr_tx(&lock, 99, 3, &third.tx_hash);
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

    #[test]
    fn test_get_latest_activities_reads_from_bundles() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock_hash = [0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        let mut bundle = make_bundle(200, 1, 1);
        bundle.owners[0].lock_hash = lock_hash.to_vec();
        bundle.owners[0].lock_code_hash = vec![0x11; 32];
        bundle.owners[0].lock_hash_type = 1;
        bundle.owners[0].lock_args = vec![0x22; 20];
        batch.put_tx_activity_bundle(&bundle);
        batch.commit().unwrap();

        let latest = store.get_latest_activities().unwrap();
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].lock_hash, lock_hash);
        assert_eq!(latest[0].entry.block_number, 200);
        assert_eq!(latest[0].entry.tx_hash, bundle.tx_hash);
    }

    #[test]
    fn test_get_latest_activities_skips_cellbase_bundles() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        let mut cellbase = make_bundle(200, 0, 1);
        cellbase.is_cellbase = true;
        let non_cellbase = make_bundle(199, 1, 1);
        batch.put_tx_activity_bundle(&cellbase);
        batch.put_tx_activity_bundle(&non_cellbase);
        batch.commit().unwrap();

        let latest = store.get_latest_activities().unwrap();
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].entry.block_number, 199);
        assert_eq!(latest[0].entry.tx_hash, non_cellbase.tx_hash);
    }
}
