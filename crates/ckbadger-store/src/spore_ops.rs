//! Spore/DOB-specific store operations.

use crate::batch::StoreBatch;
use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{ClusterDailyDelta, SporeDailyDelta, SporeEntry, SporeTypeIndex};

impl CkbadgerStore {
    pub fn get_spore(&self, id: &[u8]) -> anyhow::Result<Option<SporeEntry>> {
        match self.get_cf(self.cf_nft_item_meta(), id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// Batch-fetch multiple spore/DOB entries by ID in a single RocksDB multi_get.
    pub fn get_spores_batch(&self, ids: &[Vec<u8>]) -> Vec<(Vec<u8>, Option<SporeEntry>)> {
        if ids.is_empty() {
            return Vec::new();
        }
        let cf = self.cf_nft_item_meta();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            ids.iter().map(|id| (cf, id.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);
        ids.iter()
            .zip(values)
            .map(|(id, result)| {
                let entry = result
                    .ok()
                    .flatten()
                    .and_then(|v| bincode::deserialize::<SporeEntry>(&v).ok());
                (id.clone(), entry)
            })
            .collect()
    }

    pub fn put_spore_direct(&self, id: &[u8], entry: &SporeEntry) -> anyhow::Result<()> {
        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_nft_item_meta(), id, &value)
    }

    /// List all spores.
    pub fn list_spores(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, SporeEntry)>> {
        let iter = self.iterator_cf(self.cf_nft_item_meta(), rocksdb::IteratorMode::Start);
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
        let iter = self.prefix_iterator_cf(self.cf_nft_item_by_collection(), cluster_id);
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
        let iter = self.prefix_iterator_cf(self.cf_nft_item_by_collection(), cluster_id);
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

    pub fn get_spore_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_spore_outpoint_key(tx_hash, output_index);
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) if value.len() >= 32 => Ok(Some(value[..32].to_vec())),
            _ => Ok(None),
        }
    }

    pub fn get_spore_ids_by_outpoints_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> Vec<(Vec<u8>, i16, Vec<u8>)> {
        let cf = self.cf_stats();
        let keys: Vec<[u8; keys::SPORE_OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_spore_outpoint_key(tx_hash, *idx))
            .collect();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

        let mut results = Vec::new();
        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if value.len() >= 32 {
                    let (tx_hash, idx) = outpoints[i];
                    results.push((tx_hash.to_vec(), idx, value[..32].to_vec()));
                }
            }
        }
        results
    }

    /// List all historical spore outpoints recorded for a spore ID.
    /// Uses the reverse index (spore_id → outpoints) for O(log N) prefix scan.
    pub fn list_spore_outpoints_by_spore_id(
        &self,
        spore_id: &[u8],
    ) -> anyhow::Result<Vec<(Vec<u8>, i16)>> {
        let prefix = keys::encode_spore_outpoint_by_id_prefix(spore_id);
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut outpoints = Vec::new();

        for item in iter.flatten() {
            let (key, _value) = item;
            if key.len() != keys::SPORE_OUTPOINT_BY_ID_KEY_SIZE
                || key[0] != keys::STATS_PREFIX_SPORE_OUTPOINT_BY_ID
                || &key[1..33] != spore_id
            {
                break;
            }
            outpoints.push(keys::decode_spore_outpoint_by_id_key(&key));
        }

        Ok(outpoints)
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
        self.list_cluster_daily_deltas_in_range(cluster_id, None, None)
    }

    pub fn list_cluster_daily_deltas_in_range(
        &self,
        cluster_id: &[u8],
        from_date_yyyymmdd: Option<u32>,
        to_date_yyyymmdd: Option<u32>,
    ) -> anyhow::Result<Vec<(u32, ClusterDailyDelta)>> {
        let prefix = keys::encode_cluster_daily_prefix(cluster_id);
        let start_key =
            keys::encode_cluster_daily_key(cluster_id, from_date_yyyymmdd.unwrap_or(u32::MIN));
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
            if key.len() != keys::CLUSTER_DAILY_KEY_SIZE {
                continue;
            }
            let (_, date) = keys::decode_cluster_daily_key(&key);
            if let Some(to_date) = to_date_yyyymmdd {
                if date > to_date {
                    break;
                }
            }
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
        self.list_spore_daily_deltas_in_range(spore_id, None, None)
    }

    pub fn list_spore_daily_deltas_in_range(
        &self,
        spore_id: &[u8],
        from_date_yyyymmdd: Option<u32>,
        to_date_yyyymmdd: Option<u32>,
    ) -> anyhow::Result<Vec<(u32, SporeDailyDelta)>> {
        let prefix = keys::encode_spore_daily_prefix(spore_id);
        let start_key =
            keys::encode_spore_daily_key(spore_id, from_date_yyyymmdd.unwrap_or(u32::MIN));
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
            if key.len() != keys::SPORE_DAILY_KEY_SIZE {
                continue;
            }
            let (_, date) = keys::decode_spore_daily_key(&key);
            if let Some(to_date) = to_date_yyyymmdd {
                if date > to_date {
                    break;
                }
            }
            if let Ok(delta) = bincode::deserialize::<SporeDailyDelta>(&value) {
                results.push((date, delta));
            }
        }

        Ok(results)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ClusterDailyDelta, SporeDailyDelta, SporeTypeIndex};
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        (dir, store)
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
    fn test_spore_outpoint_roundtrip_and_batch_lookup() {
        let (_dir, store) = test_store();
        let tx_a = [0xA1u8; 32];
        let tx_b = [0xB2u8; 32];
        let spore_a = [0x11u8; 32];
        let spore_b = [0x22u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_outpoint(&tx_a, 1, &spore_a);
        batch.put_spore_outpoint(&tx_b, 2, &spore_b);
        batch.commit().unwrap();

        let single = store.get_spore_id_by_outpoint(&tx_a, 1).unwrap().unwrap();
        assert_eq!(single, spore_a.to_vec());
        assert!(store.get_spore_id_by_outpoint(&tx_a, 9).unwrap().is_none());

        let outpoints: Vec<(&[u8], i16)> = vec![(&tx_a, 1), (&tx_b, 2), (&tx_a, 9)];
        let results = store.get_spore_ids_by_outpoints_batch(&outpoints);
        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .any(|(tx, idx, id)| tx == tx_a.as_slice() && *idx == 1 && id == spore_a.as_slice()));
        assert!(results
            .iter()
            .any(|(tx, idx, id)| tx == tx_b.as_slice() && *idx == 2 && id == spore_b.as_slice()));

        let mut spore_outpoints = store.list_spore_outpoints_by_spore_id(&spore_a).unwrap();
        spore_outpoints.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        assert_eq!(spore_outpoints.len(), 1);
        assert_eq!(spore_outpoints[0], (tx_a.to_vec(), 1));
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

        let cluster_ranged = store
            .list_cluster_daily_deltas_in_range(&cluster_id, Some(20260219), Some(20260219))
            .unwrap();
        assert_eq!(cluster_ranged.len(), 1);
        assert_eq!(cluster_ranged[0].0, 20260219);

        let spore_ranged = store
            .list_spore_daily_deltas_in_range(&spore_id, Some(20260219), Some(20260219))
            .unwrap();
        assert_eq!(spore_ranged.len(), 1);
        assert_eq!(spore_ranged[0].0, 20260219);
    }
}
