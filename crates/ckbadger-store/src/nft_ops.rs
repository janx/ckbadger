//! NFT (Spore, mNFT, DotBit) operations.

use crate::batch::StoreBatch;
use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{
    ClusterDailyDelta, NftCollectionAggregate, NftEntry, SporeDailyDelta, SporeEntry,
    SporeTypeIndex,
};

impl CkbadgerStore {
    pub fn get_spore(&self, id: &[u8]) -> anyhow::Result<Option<SporeEntry>> {
        match self.get_cf(self.cf_spore_data(), id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_spore_direct(&self, id: &[u8], entry: &SporeEntry) -> anyhow::Result<()> {
        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_spore_data(), id, &value)
    }

    /// List all spores.
    pub fn list_spores(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, SporeEntry)>> {
        let iter = self.iterator_cf(self.cf_spore_data(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(entry) = bincode::deserialize::<SporeEntry>(&value) {
                results.push((key.to_vec(), entry));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// List spores belonging to a specific cluster using the secondary index.
    pub fn list_spores_by_cluster(
        &self,
        cluster_id: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(Vec<u8>, SporeEntry)>> {
        let iter = self.prefix_iterator_cf(self.cf_spore_by_cluster(), cluster_id);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(cluster_id) {
                break;
            }
            // Key: cluster_id(32B) + spore_id(32B) = 64 bytes
            if key.len() == 64 {
                let spore_id = key[32..64].to_vec();
                if let Ok(Some(entry)) = self.get_spore(&spore_id) {
                    results.push((spore_id, entry));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    /// Count spores in a cluster using the secondary index.
    pub fn count_spores_in_cluster(&self, cluster_id: &[u8]) -> anyhow::Result<i64> {
        let iter = self.prefix_iterator_cf(self.cf_spore_by_cluster(), cluster_id);
        let mut count: i64 = 0;

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(cluster_id) {
                break;
            }
            count += 1;
        }
        Ok(count)
    }

    pub fn get_spore_type_index(
        &self,
        type_script_hash: &[u8],
    ) -> anyhow::Result<Option<SporeTypeIndex>> {
        let key = keys::encode_spore_type_index_key(type_script_hash);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_spore_type_index_direct(
        &self,
        type_script_hash: &[u8],
        index: &SporeTypeIndex,
    ) -> anyhow::Result<()> {
        let key = keys::encode_spore_type_index_key(type_script_hash);
        let value = bincode::serialize(index)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn get_cluster_daily_delta(
        &self,
        cluster_id: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<ClusterDailyDelta>> {
        let key = keys::encode_cluster_daily_key(cluster_id, date_yyyymmdd);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_cluster_daily_delta(
        &self,
        cluster_id: &[u8],
        date_yyyymmdd: u32,
        delta: &ClusterDailyDelta,
    ) -> anyhow::Result<()> {
        let key = keys::encode_cluster_daily_key(cluster_id, date_yyyymmdd);
        let value = bincode::serialize(delta)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn list_cluster_daily_deltas(
        &self,
        cluster_id: &[u8],
    ) -> anyhow::Result<Vec<(u32, ClusterDailyDelta)>> {
        let prefix = keys::encode_cluster_daily_prefix(cluster_id);
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::CLUSTER_DAILY_KEY_SIZE {
                continue;
            }
            let (_, date) = keys::decode_cluster_daily_key(&key);
            if let Ok(delta) = bincode::deserialize::<ClusterDailyDelta>(&value) {
                results.push((date, delta));
            }
        }

        Ok(results)
    }

    pub fn get_spore_daily_delta(
        &self,
        spore_id: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<SporeDailyDelta>> {
        let key = keys::encode_spore_daily_key(spore_id, date_yyyymmdd);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_spore_daily_delta(
        &self,
        spore_id: &[u8],
        date_yyyymmdd: u32,
        delta: &SporeDailyDelta,
    ) -> anyhow::Result<()> {
        let key = keys::encode_spore_daily_key(spore_id, date_yyyymmdd);
        let value = bincode::serialize(delta)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn list_spore_daily_deltas(
        &self,
        spore_id: &[u8],
    ) -> anyhow::Result<Vec<(u32, SporeDailyDelta)>> {
        let prefix = keys::encode_spore_daily_prefix(spore_id);
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::SPORE_DAILY_KEY_SIZE {
                continue;
            }
            let (_, date) = keys::decode_spore_daily_key(&key);
            if let Ok(delta) = bincode::deserialize::<SporeDailyDelta>(&value) {
                results.push((date, delta));
            }
        }

        Ok(results)
    }

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

    /// Backfill the spore-by-cluster secondary index from existing spore data.
    #[allow(clippy::manual_is_multiple_of)]
    /// Gated by a marker key in sync_meta to ensure it only runs once.
    pub fn migrate_spore_by_cluster_index(&self) -> anyhow::Result<u64> {
        let marker = b"migration:spore_by_cluster";
        if self.get_cf(self.cf_sync_meta(), marker)?.is_some() {
            return Ok(0); // Already migrated
        }

        let spores = self.list_spores(1_000_000)?;
        let mut count = 0u64;
        let mut batch = StoreBatch::new(self);

        for (spore_id, entry) in &spores {
            if entry.standard.is_cluster() {
                continue;
            }
            if let Some(ref cluster_id) = entry.collection_id {
                if cluster_id.len() >= 32 && spore_id.len() >= 32 {
                    batch.put_spore_by_cluster(cluster_id, spore_id);
                    count += 1;

                    // Commit in chunks to avoid huge batches
                    if count % 10_000 == 0 {
                        batch.commit()?;
                        batch = StoreBatch::new(self);
                    }
                }
            }
        }

        // Write migration marker
        batch.put_sync_meta(marker, b"done");
        batch.commit()?;

        Ok(count)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ClusterDailyDelta, NftStandard, SporeDailyDelta, SporeTypeIndex};
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
    fn test_spore_type_index_roundtrip() {
        let (_dir, store) = test_store();
        let type_script_hash = [0x11u8; 32];
        let index = SporeTypeIndex {
            spore_id: vec![0x22; 32],
            cluster_id: Some(vec![0x33; 32]),
        };

        store
            .put_spore_type_index_direct(&type_script_hash, &index)
            .unwrap();

        let loaded = store
            .get_spore_type_index(&type_script_hash)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.spore_id, index.spore_id);
        assert_eq!(loaded.cluster_id, index.cluster_id);
    }

    #[test]
    fn test_cluster_and_spore_daily_delta_roundtrip() {
        let (_dir, store) = test_store();
        let cluster_id = [0x44u8; 32];
        let spore_id = [0x55u8; 32];

        store
            .put_cluster_daily_delta(
                &cluster_id,
                20260219,
                &ClusterDailyDelta {
                    live_capacity_delta: 1000,
                    live_occupied_capacity_delta: 600,
                },
            )
            .unwrap();
        store
            .put_spore_daily_delta(
                &spore_id,
                20260219,
                &SporeDailyDelta {
                    live_capacity_delta: 100,
                    live_occupied_capacity_delta: 61,
                },
            )
            .unwrap();

        let cluster = store
            .get_cluster_daily_delta(&cluster_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(cluster.live_capacity_delta, 1000);
        assert_eq!(cluster.live_occupied_capacity_delta, 600);

        let spore = store
            .get_spore_daily_delta(&spore_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(spore.live_capacity_delta, 100);
        assert_eq!(spore.live_occupied_capacity_delta, 61);

        let cluster_list = store.list_cluster_daily_deltas(&cluster_id).unwrap();
        assert_eq!(cluster_list.len(), 1);
        assert_eq!(cluster_list[0].0, 20260219);

        let spore_list = store.list_spore_daily_deltas(&spore_id).unwrap();
        assert_eq!(spore_list.len(), 1);
        assert_eq!(spore_list[0].0, 20260219);
    }
}
