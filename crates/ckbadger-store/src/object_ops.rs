//! Object operations: mNFT-specific ops + cross-standard collection activity queries.

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{
    AssetAction, MnftCollectionAggregate, MnftDailyDelta, MnftTypeIndex,
    ObjectCollectionActivityEntry, ObjectEntry,
};

pub(crate) type MnftBatchEntry = (Vec<u8>, Option<ObjectEntry>);

use crate::bytes_to_hex;

impl CkbadgerStore {
    pub fn get_mnft(&self, id: &[u8]) -> anyhow::Result<Option<ObjectEntry>> {
        match self.get_cf(self.cf_mnft_data(), id)? {
            Some(value) => Ok(Some(postcard::from_bytes(&value)?)),
            None => Ok(None),
        }
    }

    /// Batch-fetch multiple object entries by ID in a single RocksDB multi_get.
    pub fn get_mnfts_batch(&self, ids: &[Vec<u8>]) -> anyhow::Result<Vec<MnftBatchEntry>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let cf = self.cf_mnft_data();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            ids.iter().map(|id| (cf, id.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);
        let mut result = Vec::with_capacity(ids.len());
        for (id, value_result) in ids.iter().zip(values) {
            let entry = match value_result {
                Ok(Some(value)) => {
                    Some(postcard::from_bytes::<ObjectEntry>(&value).map_err(|e| {
                        anyhow::anyhow!(
                            "failed to deserialize object entry in get_objects_batch: object_id=0x{}, error={}",
                            bytes_to_hex(id),
                            e
                        )
                    })?)
                }
                Ok(None) => None,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_objects_batch: object_id=0x{}, error={}",
                        bytes_to_hex(id),
                        e
                    ));
                }
            };
            result.push((id.clone(), entry));
        }
        Ok(result)
    }

    /// List all objects.
    pub fn list_mnfts(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, ObjectEntry)>> {
        let iter = self.iterator_cf(self.cf_mnft_data(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate object_data in list_objects: {}", e)
            })?;
            let entry: ObjectEntry = postcard::from_bytes(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize object entry in list_objects: object_id=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push((key.to_vec(), entry));
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    /// Get pre-aggregated data for an object collection.
    pub fn get_mnft_collection_aggregate(
        &self,
        collection_id: &[u8],
    ) -> anyhow::Result<Option<MnftCollectionAggregate>> {
        match self.get_cf(self.cf_mnft_collection_agg(), collection_id)? {
            Some(value) => Ok(Some(postcard::from_bytes(&value)?)),
            None => Ok(None),
        }
    }

    /// List all object collection aggregates. Scans the small `object_collection_agg` CF.
    pub fn list_mnft_collection_aggregates(
        &self,
    ) -> anyhow::Result<Vec<(Vec<u8>, MnftCollectionAggregate)>> {
        let iter = self.iterator_cf(self.cf_mnft_collection_agg(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate object_collection_agg in list_object_collection_aggregates: {}",
                    e
                )
            })?;
            let agg: MnftCollectionAggregate = postcard::from_bytes(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize object collection aggregate in list_object_collection_aggregates: collection_id=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push((key.to_vec(), agg));
        }
        Ok(results)
    }

    /// Get the live object count for a specific owner in a collection.
    pub fn get_mnft_collection_owner_count(
        &self,
        collection_id: &[u8],
        lock_hash: &[u8],
    ) -> anyhow::Result<i64> {
        if collection_id.is_empty() || collection_id.len() > 32 {
            anyhow::bail!(
                "invalid collection_id length in get_object_collection_owner_count: expected 1..=32, got {}",
                collection_id.len()
            );
        }
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "invalid lock_hash length in get_object_collection_owner_count: expected 32, got {}",
                lock_hash.len()
            );
        }

        let key = keys::encode_nft_collection_owner_key(collection_id, lock_hash);
        match self.get_cf(self.cf_stats_mnft(), &key)? {
            Some(value) if value.len() == 8 => {
                Ok(i64::from_le_bytes(value[..8].try_into().unwrap()))
            }
            _ => Ok(0),
        }
    }

    /// List all owners and live object counts for a collection.
    pub fn list_mnft_collection_owner_counts(
        &self,
        collection_id: &[u8],
    ) -> anyhow::Result<Vec<(Vec<u8>, i64)>> {
        if collection_id.is_empty() || collection_id.len() > 32 {
            anyhow::bail!(
                "invalid collection_id length in list_object_collection_owner_counts: expected 1..=32, got {}",
                collection_id.len()
            );
        }

        let prefix = keys::encode_nft_collection_owner_prefix(collection_id);
        let iter = self.prefix_iterator_cf(self.cf_stats_mnft(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_object in list_object_collection_owner_counts: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::NFT_COLLECTION_OWNER_KEY_SIZE {
                anyhow::bail!(
                    "invalid object collection owner key length: expected {}, got {}",
                    keys::NFT_COLLECTION_OWNER_KEY_SIZE,
                    key.len()
                );
            }
            if value.len() != 8 {
                anyhow::bail!(
                    "invalid object collection owner value length: expected 8, got {}",
                    value.len()
                );
            }

            let count = i64::from_le_bytes(value[..8].try_into().unwrap());
            if count <= 0 {
                continue;
            }
            results.push((key[33..65].to_vec(), count));
        }

        Ok(results)
    }

    pub fn get_mnft_type_index(
        &self,
        type_script_hash: &[u8],
    ) -> anyhow::Result<Option<MnftTypeIndex>> {
        let key = keys::encode_nft_type_index_key(type_script_hash);
        match self.get_cf(self.cf_stats_mnft(), &key)? {
            Some(value) => Ok(Some(postcard::from_bytes(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_mnft_type_index_direct(
        &self,
        type_script_hash: &[u8],
        index: &MnftTypeIndex,
    ) -> anyhow::Result<()> {
        let key = keys::encode_nft_type_index_key(type_script_hash);
        let value = postcard::to_allocvec(index)?;
        self.put_cf(self.cf_stats_mnft(), &key, &value)
    }

    pub fn get_mnft_daily_delta(
        &self,
        collection_id: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<MnftDailyDelta>> {
        let key = keys::encode_nft_daily_key(collection_id, date_yyyymmdd);
        match self.get_cf(self.cf_stats_mnft(), &key)? {
            Some(value) => Ok(Some(postcard::from_bytes(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_mnft_daily_delta(
        &self,
        collection_id: &[u8],
        date_yyyymmdd: u32,
        delta: &MnftDailyDelta,
    ) -> anyhow::Result<()> {
        let key = keys::encode_nft_daily_key(collection_id, date_yyyymmdd);
        let value = postcard::to_allocvec(delta)?;
        self.put_cf(self.cf_stats_mnft(), &key, &value)
    }

    pub fn list_mnft_daily_deltas(
        &self,
        collection_id: &[u8],
    ) -> anyhow::Result<Vec<(u32, MnftDailyDelta)>> {
        self.list_mnft_daily_deltas_in_range(collection_id, None, None)
    }

    pub fn list_mnft_daily_deltas_in_range(
        &self,
        collection_id: &[u8],
        from_date_yyyymmdd: Option<u32>,
        to_date_yyyymmdd: Option<u32>,
    ) -> anyhow::Result<Vec<(u32, MnftDailyDelta)>> {
        let prefix = keys::encode_nft_daily_prefix(collection_id);
        let start_key =
            keys::encode_nft_daily_key(collection_id, from_date_yyyymmdd.unwrap_or(u32::MIN));
        let iter = self.iterator_cf(
            self.cf_stats_mnft(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_object in list_object_daily_deltas_in_range: {}",
                    e
                )
            })?;
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
            let delta: MnftDailyDelta = postcard::from_bytes(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize object daily delta in list_object_daily_deltas_in_range: collection_id=0x{}, date={}, error={}",
                    bytes_to_hex(collection_id),
                    date,
                    e
                )
            })?;
            results.push((date, delta));
        }

        Ok(results)
    }

    /// List object IDs in a collection via the `object_by_collection` secondary index.
    ///
    /// Pagination is keyset-based by `object_id` lexicographic order.
    /// - `cursor = None` starts from the beginning.
    /// - `cursor = Some(id)` starts AFTER that id.
    pub fn list_mnft_ids_by_collection(
        &self,
        collection_id: &[u8],
        cursor: Option<&[u8]>,
        limit: usize,
    ) -> anyhow::Result<Vec<Vec<u8>>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let prefix = keys::encode_nft_by_collection_prefix(collection_id);
        let start_object_id = cursor.unwrap_or(&[]);
        let start_key = keys::encode_nft_by_collection_key(collection_id, start_object_id);

        let iter = self.iterator_cf(
            self.cf_mnft_by_collection(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate object_by_collection in list_object_ids_by_collection: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }

            if cursor.is_some() && key.as_ref() == start_key.as_slice() {
                continue;
            }

            let Some((_, object_id)) = keys::decode_nft_by_collection_key(&key) else {
                anyhow::bail!("invalid object_by_collection key length: {}", key.len());
            };
            if object_id.is_empty() {
                anyhow::bail!("invalid empty object_id in object_by_collection key");
            }

            results.push(object_id);
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }

    /// List pre-computed activities for an object collection, newest first.
    ///
    /// Returns `(block_number, tx_index, entry)` tuples. Simple prefix scan
    /// on `CF_OBJECT_COLLECTION_ACTIVITIES` with early termination at `limit`.
    pub fn list_object_collection_activities(
        &self,
        collection_id: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
        action_filter: Option<&str>,
    ) -> anyhow::Result<Vec<(i64, i32, ObjectCollectionActivityEntry)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let prefix = keys::encode_nft_collection_activity_prefix(collection_id);
        let start_key = if let Some((cursor_block, cursor_tx_idx)) = cursor {
            keys::encode_nft_collection_activity_seek_after_key(
                collection_id,
                cursor_block,
                cursor_tx_idx,
            )
        } else {
            let mut k = [0u8; keys::NFT_COLLECTION_ACTIVITY_KEY_SIZE];
            k[..32].copy_from_slice(&prefix);
            k
        };

        let iter = self.iterator_cf(
            self.cf_object_collection_activities(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        let action_filter_parsed = match action_filter {
            Some(s) => Some(match s {
                "mint" => AssetAction::Mint,
                "transfer" => AssetAction::Transfer,
                "burn" => AssetAction::Burn,
                "recycle" => AssetAction::Recycle,
                "renew" => AssetAction::Renew,
                "update" => AssetAction::Update,
                other => anyhow::bail!(
                    "unsupported action filter in list_object_collection_activities: {:?}",
                    other
                ),
            }),
            None => None,
        };

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate object_collection_activities in list_object_collection_activities: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::NFT_COLLECTION_ACTIVITY_KEY_SIZE {
                continue;
            }

            let (_, block_num, tx_idx, block_hash_from_key, tx_hash_from_key) =
                keys::decode_nft_collection_activity_key(&key);
            let entry: ObjectCollectionActivityEntry = postcard::from_bytes(&value)?;
            if entry.tx_hash != tx_hash_from_key {
                anyhow::bail!(
                    "object_collection_activities key/value tx_hash mismatch in list_object_collection_activities: collection_id=0x{}, block_num={}, tx_idx={}, key_tx_hash=0x{}, value_tx_hash=0x{}",
                    bytes_to_hex(&prefix),
                    block_num,
                    tx_idx,
                    bytes_to_hex(&tx_hash_from_key),
                    bytes_to_hex(&entry.tx_hash)
                );
            }
            if entry.block_hash != block_hash_from_key {
                anyhow::bail!(
                    "object_collection_activities key/value block_hash mismatch in list_object_collection_activities: collection_id=0x{}, block_num={}, tx_idx={}, key_block_hash=0x{}, value_block_hash=0x{}",
                    bytes_to_hex(&prefix),
                    block_num,
                    tx_idx,
                    bytes_to_hex(&block_hash_from_key),
                    bytes_to_hex(&entry.block_hash)
                );
            }

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
    pub fn count_object_collection_activities(&self, collection_id: &[u8]) -> anyhow::Result<i64> {
        let prefix = keys::encode_nft_collection_activity_prefix(collection_id);
        let iter = self.iterator_cf(
            self.cf_object_collection_activities(),
            rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward),
        );

        let mut count: i64 = 0;
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate object_collection_activities in count_object_collection_activities: {}",
                    e
                )
            })?;
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
    use crate::types::{MnftDailyDelta, MnftTypeIndex, ObjectStandard};
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        (dir, store)
    }

    fn test_domain_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_get_object_collection_aggregate_missing() {
        let (_dir, store) = test_store();
        let id = [0x01u8; 32];
        assert!(store.get_mnft_collection_aggregate(&id).unwrap().is_none());
    }

    #[test]
    fn test_put_and_get_mnft_collection_aggregate() {
        let (_dir, store) = test_store();
        let id = [0x01u8; 32];
        let agg = MnftCollectionAggregate {
            name: Some("Test mNFT Class".to_string()),
            standard: ObjectStandard::MnftClass,
            total_count: 50,
            live_count: 42,
            holders_count: 11,
            activities_count: 37,
            ..Default::default()
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_collection_aggregate(&id, &agg);
        batch.commit().unwrap();

        let result = store.get_mnft_collection_aggregate(&id).unwrap().unwrap();
        assert_eq!(result.name.as_deref(), Some("Test mNFT Class"));
        assert_eq!(result.standard, ObjectStandard::MnftClass);
        assert_eq!(result.total_count, 50);
        assert_eq!(result.live_count, 42);
        assert_eq!(result.holders_count, 11);
        assert_eq!(result.activities_count, 37);
    }

    #[test]
    fn test_list_mnft_collection_aggregates() {
        let (_dir, store) = test_store();
        let id_a = [0x01u8; 32];
        let id_b = [0x02u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_collection_aggregate(
            &id_a,
            &MnftCollectionAggregate {
                name: Some("Class A".to_string()),
                standard: ObjectStandard::MnftClass,
                total_count: 10,
                live_count: 8,
                holders_count: 3,
                activities_count: 15,
                ..Default::default()
            },
        );
        batch.put_mnft_collection_aggregate(
            &id_b,
            &MnftCollectionAggregate {
                name: Some("Spore Cluster".to_string()),
                standard: ObjectStandard::SporeCluster,
                total_count: 100,
                live_count: 90,
                holders_count: 50,
                activities_count: 800,
                ..Default::default()
            },
        );
        batch.commit().unwrap();

        let results = store.list_mnft_collection_aggregates().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_object_collection_aggregate_default() {
        let agg = MnftCollectionAggregate::default();
        assert!(agg.name.is_none());
        assert_eq!(agg.standard, ObjectStandard::Spore);
        assert_eq!(agg.total_count, 0);
        assert_eq!(agg.live_count, 0);
        assert_eq!(agg.holders_count, 0);
        assert_eq!(agg.activities_count, 0);
    }

    #[test]
    fn test_object_collection_owner_count_roundtrip() {
        let (_dir, store) = test_store();
        let collection_id = [0x31u8; 32];
        let owner_a = [0x41u8; 32];
        let owner_b = [0x42u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_collection_owner_count(&collection_id, &owner_a, 2);
        batch.put_mnft_collection_owner_count(&collection_id, &owner_b, 1);
        batch.commit().unwrap();

        let count_a = store
            .get_mnft_collection_owner_count(&collection_id, &owner_a)
            .unwrap();
        let count_b = store
            .get_mnft_collection_owner_count(&collection_id, &owner_b)
            .unwrap();
        assert_eq!(count_a, 2);
        assert_eq!(count_b, 1);

        let mut rows = store
            .list_mnft_collection_owner_counts(&collection_id)
            .unwrap();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], (owner_a.to_vec(), 2));
        assert_eq!(rows[1], (owner_b.to_vec(), 1));
    }

    #[test]
    fn test_object_collection_owner_count_length_validation() {
        let (_dir, store) = test_store();
        let err = store
            .get_mnft_collection_owner_count(&[0x11; 33], &[0x22; 32])
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid collection_id length in get_object_collection_owner_count"));

        let err = store.list_mnft_collection_owner_counts(&[]).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid collection_id length in list_object_collection_owner_counts"));
    }

    #[test]
    fn test_list_objects_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        store
            .put_cf(store.cf_mnft_data(), &[0x11; 32], b"invalid-object-payload")
            .unwrap();

        let err = store.list_mnfts(10).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize object entry in list_objects"));
    }

    #[test]
    fn test_get_objects_batch_reads_multiple_entries() {
        let (_dir, store) = test_store();
        let object_a = [0x11u8; 32];
        let object_b = [0x22u8; 32];

        let make_entry = |owner: &[u8]| ObjectEntry {
            standard: ObjectStandard::MnftToken,
            collection_id: Some(vec![0x01; 32]),
            token_id: None,
            owner_lock_hash: Some(owner.to_vec()),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![],
            extra: crate::types::ObjectExtra::MnftToken {
                token_index: 0,
                characteristic: vec![],
                configure: 0,
                state: 0,
            },
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft(&object_a, &make_entry(&[0xA1; 32]));
        batch.put_mnft(&object_b, &make_entry(&[0xA2; 32]));
        batch.commit().unwrap();

        let fetched = store
            .get_mnfts_batch(&[object_a.to_vec(), object_b.to_vec(), vec![0x33; 32]])
            .unwrap();
        assert_eq!(fetched.len(), 3);
        assert_eq!(fetched[0].0, object_a.to_vec());
        assert_eq!(fetched[1].0, object_b.to_vec());
        assert_eq!(
            fetched[0]
                .1
                .as_ref()
                .unwrap()
                .owner_lock_hash
                .as_ref()
                .unwrap(),
            &[0xA1; 32]
        );
        assert_eq!(
            fetched[1]
                .1
                .as_ref()
                .unwrap()
                .owner_lock_hash
                .as_ref()
                .unwrap(),
            &[0xA2; 32]
        );
        assert!(fetched[2].1.is_none());
    }

    #[test]
    fn test_get_objects_batch_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let object_id = [0x44u8; 32];
        store
            .put_cf(store.cf_mnft_data(), &object_id, b"invalid-object-payload")
            .unwrap();

        let err = store.get_mnfts_batch(&[object_id.to_vec()]).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize object entry in get_objects_batch"));
    }

    #[test]
    fn test_list_object_collection_aggregates_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        store
            .put_cf(
                store.cf_mnft_collection_agg(),
                &[0x22; 32],
                b"invalid-object-collection-agg",
            )
            .unwrap();

        let err = store.list_mnft_collection_aggregates().unwrap_err();
        assert!(err.to_string().contains(
            "failed to deserialize object collection aggregate in list_object_collection_aggregates"
        ));
    }

    #[test]
    fn test_object_type_index_and_object_daily_delta_roundtrip() {
        let (_dir, store) = test_store();
        let type_script_hash = [0x66u8; 32];
        let collection_id = [0x77u8; 24];

        store
            .put_mnft_type_index_direct(
                &type_script_hash,
                &MnftTypeIndex {
                    collection_id: collection_id.to_vec(),
                },
            )
            .unwrap();
        let loaded_index = store
            .get_mnft_type_index(&type_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_index.collection_id, collection_id.to_vec());

        store
            .put_mnft_daily_delta(
                &collection_id,
                20260219,
                &MnftDailyDelta {
                    owned_capacity_delta: 500,
                    owned_knowledge_delta: 320,
                },
            )
            .unwrap();
        let loaded_daily = store
            .get_mnft_daily_delta(&collection_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_daily.owned_capacity_delta, 500);
        assert_eq!(loaded_daily.owned_knowledge_delta, 320);

        let list = store.list_mnft_daily_deltas(&collection_id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, 20260219);

        let ranged = store
            .list_mnft_daily_deltas_in_range(&collection_id, Some(20260219), Some(20260219))
            .unwrap();
        assert_eq!(ranged.len(), 1);
        assert_eq!(ranged[0].0, 20260219);
    }

    #[test]
    fn test_list_object_daily_deltas_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let collection_id = [0x77u8; 24];
        let key = keys::encode_nft_daily_key(&collection_id, 20260219);
        store.put_cf(store.cf_stats_mnft(), &key, &[0xFF]).unwrap();

        let err = store.list_mnft_daily_deltas(&collection_id).unwrap_err();
        assert!(err.to_string().contains(
            "failed to deserialize object daily delta in list_object_daily_deltas_in_range"
        ));
    }

    #[test]
    fn test_list_object_ids_by_collection_pagination() {
        let (_dir, store) = test_store();
        let collection_id = [0x88u8; 24];
        let object_a = [0x01u8; 20];
        let object_b = [0x02u8; 20];
        let object_c = [0x03u8; 20];

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_by_collection(&collection_id, &object_b);
        batch.put_mnft_by_collection(&collection_id, &object_c);
        batch.put_mnft_by_collection(&collection_id, &object_a);
        batch.commit().unwrap();

        let first = store
            .list_mnft_ids_by_collection(&collection_id, None, 2)
            .unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], object_a.to_vec());
        assert_eq!(first[1], object_b.to_vec());

        let second = store
            .list_mnft_ids_by_collection(&collection_id, Some(&first[1]), 2)
            .unwrap();
        assert_eq!(second, vec![object_c.to_vec()]);
    }

    // ---- Object collection activities ----

    use crate::types::{AssetAction, ObjectCollectionActivityEntry};

    fn make_activity(
        tx_hash: &[u8],
        ts_ms: i64,
        actions: Vec<AssetAction>,
    ) -> ObjectCollectionActivityEntry {
        ObjectCollectionActivityEntry {
            tx_hash: tx_hash.to_vec(),
            block_hash: vec![0x71; 32],
            timestamp_ms: ts_ms,
            actions,
        }
    }

    #[test]
    fn test_list_object_collection_activities_empty() {
        let (_dir, store) = test_domain_store();
        let cid = [0x01u8; 32];
        let results = store
            .list_object_collection_activities(&cid, 10, None, None)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_list_object_collection_activities_basic_pagination() {
        let (_dir, store) = test_domain_store();
        let cid = [0x01u8; 32];

        let mut batch = StoreBatch::new(&store);
        // Insert 5 activities at different blocks (newest first due to descending key)
        for block in 100..105 {
            let tx_hash = [block as u8; 32];
            batch.put_object_collection_activity(
                &cid,
                block,
                0,
                &make_activity(&tx_hash, block * 1000, vec![AssetAction::Mint]),
            );
        }
        batch.commit().unwrap();

        // Request limit=3
        let page1 = store
            .list_object_collection_activities(&cid, 3, None, None)
            .unwrap();
        assert_eq!(page1.len(), 3);
        // Should be newest first: 104, 103, 102
        assert_eq!(page1[0].0, 104);
        assert_eq!(page1[1].0, 103);
        assert_eq!(page1[2].0, 102);

        // Page 2 using cursor
        let cursor = (page1[2].0, page1[2].1);
        let page2 = store
            .list_object_collection_activities(&cid, 3, Some(cursor), None)
            .unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].0, 101);
        assert_eq!(page2[1].0, 100);
    }

    #[test]
    fn test_list_object_collection_activities_action_filter() {
        let (_dir, store) = test_domain_store();
        let cid = [0x02u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_object_collection_activity(
            &cid,
            100,
            0,
            &make_activity(&[1u8; 32], 100000, vec![AssetAction::Mint]),
        );
        batch.put_object_collection_activity(
            &cid,
            200,
            0,
            &make_activity(&[2u8; 32], 200000, vec![AssetAction::Transfer]),
        );
        batch.put_object_collection_activity(
            &cid,
            300,
            0,
            &make_activity(&[3u8; 32], 300000, vec![AssetAction::Burn]),
        );
        batch.commit().unwrap();

        let mints = store
            .list_object_collection_activities(&cid, 10, None, Some("mint"))
            .unwrap();
        assert_eq!(mints.len(), 1);
        assert_eq!(mints[0].0, 100);

        let transfers = store
            .list_object_collection_activities(&cid, 10, None, Some("transfer"))
            .unwrap();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].0, 200);

        let burns = store
            .list_object_collection_activities(&cid, 10, None, Some("burn"))
            .unwrap();
        assert_eq!(burns.len(), 1);
        assert_eq!(burns[0].0, 300);
    }

    #[test]
    fn test_list_object_collection_activities_multi_action_per_tx() {
        let (_dir, store) = test_domain_store();
        let cid = [0x03u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_object_collection_activity(
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
            .list_object_collection_activities(&cid, 10, None, Some("mint"))
            .unwrap();
        assert_eq!(mints.len(), 1);

        let burns = store
            .list_object_collection_activities(&cid, 10, None, Some("burn"))
            .unwrap();
        assert_eq!(burns.len(), 1);

        // Transfer filter should not match
        let transfers = store
            .list_object_collection_activities(&cid, 10, None, Some("transfer"))
            .unwrap();
        assert!(transfers.is_empty());
    }

    #[test]
    fn test_list_object_collection_activities_keeps_two_rows_same_position_different_tx_hash() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let cid = [0x04u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_object_collection_activity(
            &cid,
            100,
            1,
            &make_activity(&[0x10; 32], 100_000, vec![AssetAction::Mint]),
        );
        batch.put_object_collection_activity(
            &cid,
            100,
            1,
            &make_activity(&[0x20; 32], 100_001, vec![AssetAction::Transfer]),
        );
        batch.put_object_collection_activity(
            &cid,
            99,
            0,
            &make_activity(&[0x30; 32], 99_000, vec![AssetAction::Burn]),
        );
        batch.commit().unwrap();

        let rows = store
            .list_object_collection_activities(&cid, 10, None, None)
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, 100);
        assert_eq!(rows[0].1, 1);
        assert_eq!(rows[0].2.tx_hash, vec![0x10; 32]);
        assert_eq!(rows[1].0, 100);
        assert_eq!(rows[1].1, 1);
        assert_eq!(rows[1].2.tx_hash, vec![0x20; 32]);
        assert_eq!(rows[2].0, 99);
        assert_eq!(rows[2].1, 0);
        assert_eq!(rows[2].2.tx_hash, vec![0x30; 32]);

        let next = store
            .list_object_collection_activities(&cid, 10, Some((100, 1)), None)
            .unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].0, 99);
        assert_eq!(next[0].1, 0);
        assert_eq!(next[0].2.tx_hash, vec![0x30; 32]);
    }

    #[test]
    fn test_list_object_collection_activities_isolation_between_collections() {
        let (_dir, store) = test_domain_store();
        let cid_a = [0x0Au8; 32];
        let cid_b = [0x0Bu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_object_collection_activity(
            &cid_a,
            100,
            0,
            &make_activity(&[1u8; 32], 100000, vec![AssetAction::Mint]),
        );
        batch.put_object_collection_activity(
            &cid_b,
            200,
            0,
            &make_activity(&[2u8; 32], 200000, vec![AssetAction::Mint]),
        );
        batch.commit().unwrap();

        let results_a = store
            .list_object_collection_activities(&cid_a, 10, None, None)
            .unwrap();
        assert_eq!(results_a.len(), 1);
        assert_eq!(results_a[0].0, 100);

        let results_b = store
            .list_object_collection_activities(&cid_b, 10, None, None)
            .unwrap();
        assert_eq!(results_b.len(), 1);
        assert_eq!(results_b[0].0, 200);
    }

    #[test]
    fn test_count_object_collection_activities() {
        let (_dir, store) = test_domain_store();
        let cid = [0x0Cu8; 32];
        let cid_empty = [0x0Du8; 32];

        let mut batch = StoreBatch::new(&store);
        for block in 100..105 {
            batch.put_object_collection_activity(
                &cid,
                block,
                0,
                &make_activity(&[block as u8; 32], block * 1000, vec![AssetAction::Mint]),
            );
        }
        batch.commit().unwrap();

        assert_eq!(store.count_object_collection_activities(&cid).unwrap(), 5);
        assert_eq!(
            store
                .count_object_collection_activities(&cid_empty)
                .unwrap(),
            0
        );
    }
}
