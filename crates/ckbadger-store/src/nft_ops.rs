//! Generic NFT operations (cross-standard infrastructure).

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{NftCollectionAggregate, NftDailyDelta, NftEntry, NftTypeIndex};

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
}
