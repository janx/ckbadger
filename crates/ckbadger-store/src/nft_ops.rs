//! Generic NFT operations (cross-standard infrastructure).

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{
    AssetAction, NftCollectionActivityEntry, NftCollectionAggregate, NftDailyDelta, NftEntry,
    NftTypeIndex,
};

impl CkbadgerStore {
    pub fn get_nft(&self, id: &[u8]) -> anyhow::Result<Option<NftEntry>> {
        match self.get_cf(self.cf_nft_data(), id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_nft_direct(&self, id: &[u8], entry: &NftEntry) -> anyhow::Result<()> {
        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_nft_data(), id, &value)
    }

    /// List all NFTs.
    pub fn list_nfts(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, NftEntry)>> {
        let iter = self.iterator_cf(self.cf_nft_data(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(entry) = bincode::deserialize::<NftEntry>(&value) {
                results.push((key.to_vec(), entry));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// Get pre-aggregated data for an NFT collection.
    pub fn get_nft_collection_aggregate(
        &self,
        collection_id: &[u8],
    ) -> anyhow::Result<Option<NftCollectionAggregate>> {
        match self.get_cf(self.cf_nft_collection_agg(), collection_id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// List all NFT collection aggregates. Scans the small `nft_collection_agg` CF.
    pub fn list_nft_collection_aggregates(
        &self,
    ) -> anyhow::Result<Vec<(Vec<u8>, NftCollectionAggregate)>> {
        let iter = self.iterator_cf(self.cf_nft_collection_agg(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(agg) = bincode::deserialize::<NftCollectionAggregate>(&value) {
                results.push((key.to_vec(), agg));
            }
        }
        Ok(results)
    }

    pub fn get_nft_type_index(
        &self,
        type_script_hash: &[u8],
    ) -> anyhow::Result<Option<NftTypeIndex>> {
        let key = keys::encode_nft_type_index_key(type_script_hash);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_nft_type_index_direct(
        &self,
        type_script_hash: &[u8],
        index: &NftTypeIndex,
    ) -> anyhow::Result<()> {
        let key = keys::encode_nft_type_index_key(type_script_hash);
        let value = bincode::serialize(index)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn get_nft_daily_delta(
        &self,
        collection_id: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<NftDailyDelta>> {
        let key = keys::encode_nft_daily_key(collection_id, date_yyyymmdd);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_nft_daily_delta(
        &self,
        collection_id: &[u8],
        date_yyyymmdd: u32,
        delta: &NftDailyDelta,
    ) -> anyhow::Result<()> {
        let key = keys::encode_nft_daily_key(collection_id, date_yyyymmdd);
        let value = bincode::serialize(delta)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn list_nft_daily_deltas(
        &self,
        collection_id: &[u8],
    ) -> anyhow::Result<Vec<(u32, NftDailyDelta)>> {
        self.list_nft_daily_deltas_in_range(collection_id, None, None)
    }

    pub fn list_nft_daily_deltas_in_range(
        &self,
        collection_id: &[u8],
        from_date_yyyymmdd: Option<u32>,
        to_date_yyyymmdd: Option<u32>,
    ) -> anyhow::Result<Vec<(u32, NftDailyDelta)>> {
        let prefix = keys::encode_nft_daily_prefix(collection_id);
        let start_key =
            keys::encode_nft_daily_key(collection_id, from_date_yyyymmdd.unwrap_or(u32::MIN));
        let iter = self.iterator_cf(
            self.cf_stats(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::NFT_DAILY_KEY_SIZE {
                continue;
            }
            let (_, date) = keys::decode_nft_daily_key(&key);
            if let Some(to_date) = to_date_yyyymmdd {
                if date > to_date {
                    break;
                }
            }
            if let Ok(delta) = bincode::deserialize::<NftDailyDelta>(&value) {
                results.push((date, delta));
            }
        }

        Ok(results)
    }

    /// List NFT IDs in a collection via the `nft_by_collection` secondary index.
    ///
    /// Pagination is keyset-based by `nft_id` lexicographic order.
    /// - `cursor = None` starts from the beginning.
    /// - `cursor = Some(id)` starts AFTER that id.
    pub fn list_nft_ids_by_collection(
        &self,
        collection_id: &[u8],
        cursor: Option<&[u8]>,
        limit: usize,
    ) -> anyhow::Result<Vec<Vec<u8>>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let prefix = keys::encode_nft_by_collection_prefix(collection_id);
        let start_nft_id = cursor.unwrap_or(&[]);
        let start_key = keys::encode_nft_by_collection_key(collection_id, start_nft_id);

        let iter = self.iterator_cf(
            self.cf_nft_by_collection(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(&prefix) {
                break;
            }

            if cursor.is_some() && key.as_ref() == start_key.as_slice() {
                continue;
            }

            let Some((_, nft_id)) = keys::decode_nft_by_collection_key(&key) else {
                anyhow::bail!("invalid nft_by_collection key length: {}", key.len());
            };
            if nft_id.is_empty() {
                anyhow::bail!("invalid empty nft_id in nft_by_collection key");
            }

            results.push(nft_id);
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }

    /// List pre-computed activities for an NFT collection, newest first.
    ///
    /// Returns `(block_number, tx_index, entry)` tuples. Simple prefix scan
    /// on `CF_NFT_COLLECTION_ACTIVITIES` with early termination at `limit`.
    pub fn list_nft_collection_activities(
        &self,
        collection_id: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
        action_filter: Option<&str>,
    ) -> anyhow::Result<Vec<(i64, i32, NftCollectionActivityEntry)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let prefix = keys::encode_nft_collection_activity_prefix(collection_id);
        let start_key = if let Some((cursor_block, cursor_tx_idx)) = cursor {
            keys::encode_nft_collection_activity_key(collection_id, cursor_block, cursor_tx_idx)
        } else {
            // Start from the beginning of this collection (newest first)
            let mut k = [0u8; keys::NFT_COLLECTION_ACTIVITY_KEY_SIZE];
            k[..32].copy_from_slice(&prefix);
            // block_desc = 0 means block_num = i64::MAX (start of descending range)
            k
        };

        let iter = self.iterator_cf(
            self.cf_nft_collection_activities(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        let action_filter_parsed = action_filter.map(|s| match s {
            "mint" => AssetAction::Mint,
            "transfer" => AssetAction::Transfer,
            "burn" => AssetAction::Burn,
            _ => AssetAction::Mint, // unreachable if caller validates
        });

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::NFT_COLLECTION_ACTIVITY_KEY_SIZE {
                continue;
            }

            // Skip the cursor row itself
            if cursor.is_some() && key.as_ref() == start_key.as_slice() {
                continue;
            }

            let (_, block_num, tx_idx) = keys::decode_nft_collection_activity_key(&key);
            let entry: NftCollectionActivityEntry = bincode::deserialize(&value)?;

            // Apply action filter
            if let Some(ref filter) = action_filter_parsed {
                let matches = entry
                    .actions
                    .iter()
                    .any(|a| std::mem::discriminant(a) == std::mem::discriminant(filter));
                if !matches {
                    continue;
                }
            }

            results.push((block_num, tx_idx, entry));
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }

    /// Count total activities for a collection (prefix scan, no deserialization).
    pub fn count_nft_collection_activities(&self, collection_id: &[u8]) -> anyhow::Result<i64> {
        let prefix = keys::encode_nft_collection_activity_prefix(collection_id);
        let iter = self.iterator_cf(
            self.cf_nft_collection_activities(),
            rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward),
        );

        let mut count: i64 = 0;
        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            count += 1;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use crate::types::{NftDailyDelta, NftStandard, NftTypeIndex};
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_get_nft_collection_aggregate_missing() {
        let (_dir, store) = test_store();
        let id = [0x01u8; 32];
        assert!(store.get_nft_collection_aggregate(&id).unwrap().is_none());
    }

    #[test]
    fn test_put_and_get_nft_collection_aggregate() {
        let (_dir, store) = test_store();
        let id = [0x01u8; 32];
        let agg = NftCollectionAggregate {
            name: Some("Test mNFT Class".to_string()),
            standard: NftStandard::MnftClass,
            total_count: 50,
            live_count: 42,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_collection_aggregate(&id, &agg);
        batch.commit().unwrap();

        let result = store.get_nft_collection_aggregate(&id).unwrap().unwrap();
        assert_eq!(result.name.as_deref(), Some("Test mNFT Class"));
        assert_eq!(result.standard, NftStandard::MnftClass);
        assert_eq!(result.total_count, 50);
        assert_eq!(result.live_count, 42);
    }

    #[test]
    fn test_list_nft_collection_aggregates() {
        let (_dir, store) = test_store();
        let id_a = [0x01u8; 32];
        let id_b = [0x02u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_collection_aggregate(
            &id_a,
            &NftCollectionAggregate {
                name: Some("Class A".to_string()),
                standard: NftStandard::MnftClass,
                total_count: 10,
                live_count: 8,
            },
        );
        batch.put_nft_collection_aggregate(
            &id_b,
            &NftCollectionAggregate {
                name: Some("DotBit".to_string()),
                standard: NftStandard::DotBit,
                total_count: 100,
                live_count: 90,
            },
        );
        batch.commit().unwrap();

        let results = store.list_nft_collection_aggregates().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_nft_collection_aggregate_default() {
        let agg = NftCollectionAggregate::default();
        assert!(agg.name.is_none());
        assert_eq!(agg.standard, NftStandard::MnftClass);
        assert_eq!(agg.total_count, 0);
        assert_eq!(agg.live_count, 0);
    }

    #[test]
    fn test_nft_type_index_and_nft_daily_delta_roundtrip() {
        let (_dir, store) = test_store();
        let type_script_hash = [0x66u8; 32];
        let collection_id = [0x77u8; 24];

        store
            .put_nft_type_index_direct(
                &type_script_hash,
                &NftTypeIndex {
                    collection_id: collection_id.to_vec(),
                },
            )
            .unwrap();
        let loaded_index = store
            .get_nft_type_index(&type_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_index.collection_id, collection_id.to_vec());

        store
            .put_nft_daily_delta(
                &collection_id,
                20260219,
                &NftDailyDelta {
                    live_capacity_delta: 500,
                    live_occupied_capacity_delta: 320,
                },
            )
            .unwrap();
        let loaded_daily = store
            .get_nft_daily_delta(&collection_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_daily.live_capacity_delta, 500);
        assert_eq!(loaded_daily.live_occupied_capacity_delta, 320);

        let list = store.list_nft_daily_deltas(&collection_id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, 20260219);

        let ranged = store
            .list_nft_daily_deltas_in_range(&collection_id, Some(20260219), Some(20260219))
            .unwrap();
        assert_eq!(ranged.len(), 1);
        assert_eq!(ranged[0].0, 20260219);
    }

    #[test]
    fn test_list_nft_ids_by_collection_pagination() {
        let (_dir, store) = test_store();
        let collection_id = [0x88u8; 24];
        let nft_a = [0x01u8; 20];
        let nft_b = [0x02u8; 20];
        let nft_c = [0x03u8; 20];

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_by_collection(&collection_id, &nft_b);
        batch.put_nft_by_collection(&collection_id, &nft_c);
        batch.put_nft_by_collection(&collection_id, &nft_a);
        batch.commit().unwrap();

        let first = store
            .list_nft_ids_by_collection(&collection_id, None, 2)
            .unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], nft_a.to_vec());
        assert_eq!(first[1], nft_b.to_vec());

        let second = store
            .list_nft_ids_by_collection(&collection_id, Some(&first[1]), 2)
            .unwrap();
        assert_eq!(second, vec![nft_c.to_vec()]);
    }

    // ---- NFT collection activities ----

    use crate::types::{AssetAction, NftCollectionActivityEntry};

    fn make_activity(
        tx_hash: &[u8],
        ts_ms: i64,
        actions: Vec<AssetAction>,
    ) -> NftCollectionActivityEntry {
        NftCollectionActivityEntry {
            tx_hash: tx_hash.to_vec(),
            timestamp_ms: ts_ms,
            actions,
        }
    }

    #[test]
    fn test_list_nft_collection_activities_empty() {
        let (_dir, store) = test_store();
        let cid = [0x01u8; 32];
        let results = store
            .list_nft_collection_activities(&cid, 10, None, None)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_list_nft_collection_activities_basic_pagination() {
        let (_dir, store) = test_store();
        let cid = [0x01u8; 32];

        let mut batch = StoreBatch::new(&store);
        // Insert 5 activities at different blocks (newest first due to descending key)
        for block in 100..105 {
            let tx_hash = [block as u8; 32];
            batch.put_nft_collection_activity(
                &cid,
                block,
                0,
                &make_activity(&tx_hash, block * 1000, vec![AssetAction::Mint]),
            );
        }
        batch.commit().unwrap();

        // Request limit=3
        let page1 = store
            .list_nft_collection_activities(&cid, 3, None, None)
            .unwrap();
        assert_eq!(page1.len(), 3);
        // Should be newest first: 104, 103, 102
        assert_eq!(page1[0].0, 104);
        assert_eq!(page1[1].0, 103);
        assert_eq!(page1[2].0, 102);

        // Page 2 using cursor
        let cursor = (page1[2].0, page1[2].1);
        let page2 = store
            .list_nft_collection_activities(&cid, 3, Some(cursor), None)
            .unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].0, 101);
        assert_eq!(page2[1].0, 100);
    }

    #[test]
    fn test_list_nft_collection_activities_action_filter() {
        let (_dir, store) = test_store();
        let cid = [0x02u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_collection_activity(
            &cid,
            100,
            0,
            &make_activity(&[1u8; 32], 100000, vec![AssetAction::Mint]),
        );
        batch.put_nft_collection_activity(
            &cid,
            200,
            0,
            &make_activity(&[2u8; 32], 200000, vec![AssetAction::Transfer]),
        );
        batch.put_nft_collection_activity(
            &cid,
            300,
            0,
            &make_activity(&[3u8; 32], 300000, vec![AssetAction::Burn]),
        );
        batch.commit().unwrap();

        let mints = store
            .list_nft_collection_activities(&cid, 10, None, Some("mint"))
            .unwrap();
        assert_eq!(mints.len(), 1);
        assert_eq!(mints[0].0, 100);

        let transfers = store
            .list_nft_collection_activities(&cid, 10, None, Some("transfer"))
            .unwrap();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].0, 200);

        let burns = store
            .list_nft_collection_activities(&cid, 10, None, Some("burn"))
            .unwrap();
        assert_eq!(burns.len(), 1);
        assert_eq!(burns[0].0, 300);
    }

    #[test]
    fn test_list_nft_collection_activities_multi_action_per_tx() {
        let (_dir, store) = test_store();
        let cid = [0x03u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_collection_activity(
            &cid,
            500,
            0,
            &make_activity(
                &[5u8; 32],
                500000,
                vec![AssetAction::Mint, AssetAction::Burn],
            ),
        );
        batch.commit().unwrap();

        // Should match both mint and burn filters
        let mints = store
            .list_nft_collection_activities(&cid, 10, None, Some("mint"))
            .unwrap();
        assert_eq!(mints.len(), 1);

        let burns = store
            .list_nft_collection_activities(&cid, 10, None, Some("burn"))
            .unwrap();
        assert_eq!(burns.len(), 1);

        // Transfer filter should not match
        let transfers = store
            .list_nft_collection_activities(&cid, 10, None, Some("transfer"))
            .unwrap();
        assert!(transfers.is_empty());
    }

    #[test]
    fn test_list_nft_collection_activities_isolation_between_collections() {
        let (_dir, store) = test_store();
        let cid_a = [0x0Au8; 32];
        let cid_b = [0x0Bu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_nft_collection_activity(
            &cid_a,
            100,
            0,
            &make_activity(&[1u8; 32], 100000, vec![AssetAction::Mint]),
        );
        batch.put_nft_collection_activity(
            &cid_b,
            200,
            0,
            &make_activity(&[2u8; 32], 200000, vec![AssetAction::Mint]),
        );
        batch.commit().unwrap();

        let results_a = store
            .list_nft_collection_activities(&cid_a, 10, None, None)
            .unwrap();
        assert_eq!(results_a.len(), 1);
        assert_eq!(results_a[0].0, 100);

        let results_b = store
            .list_nft_collection_activities(&cid_b, 10, None, None)
            .unwrap();
        assert_eq!(results_b.len(), 1);
        assert_eq!(results_b[0].0, 200);
    }

    #[test]
    fn test_count_nft_collection_activities() {
        let (_dir, store) = test_store();
        let cid = [0x0Cu8; 32];
        let cid_empty = [0x0Du8; 32];

        let mut batch = StoreBatch::new(&store);
        for block in 100..105 {
            batch.put_nft_collection_activity(
                &cid,
                block,
                0,
                &make_activity(&[block as u8; 32], block * 1000, vec![AssetAction::Mint]),
            );
        }
        batch.commit().unwrap();

        assert_eq!(store.count_nft_collection_activities(&cid).unwrap(), 5);
        assert_eq!(
            store.count_nft_collection_activities(&cid_empty).unwrap(),
            0
        );
    }
}
