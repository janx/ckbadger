//! Spore-specific store operations.

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{ClusterDailyDelta, ObjectEntry, SporeDailyDelta, SporeTypeIndex};

#[cfg(test)]
use crate::batch::StoreBatch;

pub(crate) type SporeBatchEntry = (Vec<u8>, Option<ObjectEntry>);
pub(crate) type SporeOutpointLookup = (Vec<u8>, i16, Vec<u8>);

use crate::bytes_to_hex;

impl CkbadgerStore {
    pub fn get_spore(&self, id: &[u8]) -> anyhow::Result<Option<ObjectEntry>> {
        match self.get_cf(self.cf_spore_data(), id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// Batch-fetch multiple spore/DOB entries by ID in a single RocksDB multi_get.
    pub fn get_spores_batch(&self, ids: &[Vec<u8>]) -> anyhow::Result<Vec<SporeBatchEntry>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let cf = self.cf_spore_data();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            ids.iter().map(|id| (cf, id.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);
        let mut result = Vec::with_capacity(ids.len());
        for (id, value_result) in ids.iter().zip(values) {
            let entry = match value_result {
                Ok(Some(value)) => Some(bincode::deserialize::<ObjectEntry>(&value).map_err(
                    |e| {
                        anyhow::anyhow!(
                            "failed to deserialize spore entry in get_spores_batch: spore_id=0x{}, error={}",
                            bytes_to_hex(id),
                            e
                        )
                    },
                )?),
                Ok(None) => None,
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_spores_batch: spore_id=0x{}, error={}",
                        bytes_to_hex(id),
                        e
                    ));
                }
            };
            result.push((id.clone(), entry));
        }
        Ok(result)
    }

    pub fn put_spore_direct(&self, id: &[u8], entry: &ObjectEntry) -> anyhow::Result<()> {
        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_spore_data(), id, &value)
    }

    /// List all spores.
    pub fn list_spores(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, ObjectEntry)>> {
        let iter = self.iterator_cf(self.cf_spore_data(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate spore_data in list_spores: {}", e)
            })?;
            let entry: ObjectEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize spore entry in list_spores: spore_id=0x{}, error={}",
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

    /// List spores belonging to a specific cluster using the secondary index.
    pub fn list_spores_by_cluster(
        &self,
        cluster_id: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(Vec<u8>, ObjectEntry)>> {
        let iter = self.prefix_iterator_cf(self.cf_spore_by_cluster(), cluster_id);
        let mut spore_ids: Vec<Vec<u8>> = Vec::new();

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate spore_by_cluster in list_spores_by_cluster: {}",
                    e
                )
            })?;
            if !key.starts_with(cluster_id) {
                break;
            }
            // Key: cluster_id(32B) + spore_id(32B) = 64 bytes
            if key.len() == 64 {
                spore_ids.push(key[32..64].to_vec());
                if spore_ids.len() >= limit {
                    break;
                }
            }
        }

        let entries = self.get_spores_batch(&spore_ids)?;
        Ok(entries
            .into_iter()
            .filter_map(|(spore_id, entry)| entry.map(|entry| (spore_id, entry)))
            .collect())
    }

    /// Count spores in a cluster using the secondary index.
    pub fn count_spores_in_cluster(&self, cluster_id: &[u8]) -> anyhow::Result<i64> {
        let iter = self.prefix_iterator_cf(self.cf_spore_by_cluster(), cluster_id);
        let mut count: i64 = 0;

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate spore_by_cluster in count_spores_in_cluster: {}",
                    e
                )
            })?;
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
        match self.get_cf(self.cf_stats_spore(), &key)? {
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
        self.put_cf(self.cf_stats_spore(), &key, &value)
    }

    pub fn get_spore_id_by_outpoint(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_spore_outpoint_key(tx_hash, output_index);
        match self.get_cf(self.cf_stats_spore(), &key)? {
            Some(value) if value.len() >= 32 => Ok(Some(value[..32].to_vec())),
            _ => Ok(None),
        }
    }

    pub fn get_spore_ids_by_outpoints_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<Vec<SporeOutpointLookup>> {
        let cf = self.cf_stats_spore();
        let keys: Vec<[u8; keys::SPORE_OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_spore_outpoint_key(tx_hash, *idx))
            .collect();
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

        let mut results = Vec::new();
        for (i, value_result) in values.into_iter().enumerate() {
            let (tx_hash, idx) = outpoints[i];
            match value_result {
                Ok(Some(value)) => {
                    if value.len() < 32 {
                        return Err(anyhow::anyhow!(
                            "invalid spore outpoint value length in get_spore_ids_by_outpoints_batch: tx_hash=0x{}, output_index={}, value_len={}",
                            bytes_to_hex(tx_hash),
                            idx,
                            value.len()
                        ));
                    }
                    results.push((tx_hash.to_vec(), idx, value[..32].to_vec()));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_spore_ids_by_outpoints_batch: tx_hash=0x{}, output_index={}, error={}",
                        bytes_to_hex(tx_hash),
                        idx,
                        e
                    ));
                }
            }
        }
        Ok(results)
    }

    /// List all historical spore outpoints recorded for a spore ID.
    /// Uses the reverse index (spore_id → outpoints) for O(log N) prefix scan.
    pub fn list_spore_outpoints_by_spore_id(
        &self,
        spore_id: &[u8],
    ) -> anyhow::Result<Vec<(Vec<u8>, i16)>> {
        let prefix = keys::encode_spore_outpoint_by_id_prefix(spore_id);
        let iter = self.prefix_iterator_cf(self.cf_stats_spore(), &prefix);
        let mut outpoints = Vec::new();

        for item in iter {
            let (key, _value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_spore in list_spore_outpoints_by_spore_id: {}",
                    e
                )
            })?;
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
        match self.get_cf(self.cf_stats_spore(), &key)? {
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
        self.put_cf(self.cf_stats_spore(), &key, &value)
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
            self.cf_stats_spore(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_spore in list_cluster_daily_deltas_in_range: {}",
                    e
                )
            })?;
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
            let delta: ClusterDailyDelta = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize cluster daily delta in list_cluster_daily_deltas_in_range: cluster_id=0x{}, date={}, error={}",
                    bytes_to_hex(cluster_id),
                    date,
                    e
                )
            })?;
            results.push((date, delta));
        }

        Ok(results)
    }

    pub fn get_spore_daily_delta(
        &self,
        spore_id: &[u8],
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<SporeDailyDelta>> {
        let key = keys::encode_spore_daily_key(spore_id, date_yyyymmdd);
        match self.get_cf(self.cf_stats_spore(), &key)? {
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
        self.put_cf(self.cf_stats_spore(), &key, &value)
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
            self.cf_stats_spore(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_spore in list_spore_daily_deltas_in_range: {}",
                    e
                )
            })?;
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
            let delta: SporeDailyDelta = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize spore daily delta in list_spore_daily_deltas_in_range: spore_id=0x{}, date={}, error={}",
                    bytes_to_hex(spore_id),
                    date,
                    e
                )
            })?;
            results.push((date, delta));
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ObjectExtra;
    use crate::types::{ClusterDailyDelta, SporeDailyDelta, SporeTypeIndex};
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
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
        let results = store.get_spore_ids_by_outpoints_batch(&outpoints).unwrap();
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
    fn test_get_spore_ids_by_outpoints_batch_fails_on_short_value() {
        let (_dir, store) = test_store();
        let tx_a = [0xA1u8; 32];
        let key = keys::encode_spore_outpoint_key(&tx_a, 1);
        store
            .put_cf(store.cf_stats_spore(), &key, &[0x11; 31])
            .unwrap();

        let outpoints: Vec<(&[u8], i16)> = vec![(&tx_a, 1)];
        let err = store
            .get_spore_ids_by_outpoints_batch(&outpoints)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid spore outpoint value length in get_spore_ids_by_outpoints_batch"));
    }

    #[test]
    fn test_list_spores_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let spore_id = [0x11u8; 32];
        store
            .put_cf(store.cf_spore_data(), &spore_id, b"invalid-spore-payload")
            .unwrap();

        let err = store.list_spores(10).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize spore entry in list_spores"));
    }

    #[test]
    fn test_list_cluster_daily_deltas_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let cluster_id = [0x44u8; 32];
        let key = keys::encode_cluster_daily_key(&cluster_id, 20260219);
        store
            .put_cf(store.cf_stats_spore(), &key, b"invalid-cluster-daily")
            .unwrap();

        let err = store.list_cluster_daily_deltas(&cluster_id).unwrap_err();
        assert!(err.to_string().contains(
            "failed to deserialize cluster daily delta in list_cluster_daily_deltas_in_range"
        ));
    }

    #[test]
    fn test_list_spore_daily_deltas_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        let spore_id = [0x55u8; 32];
        let key = keys::encode_spore_daily_key(&spore_id, 20260219);
        store
            .put_cf(store.cf_stats_spore(), &key, b"invalid-spore-daily")
            .unwrap();

        let err = store.list_spore_daily_deltas(&spore_id).unwrap_err();
        assert!(err.to_string().contains(
            "failed to deserialize spore daily delta in list_spore_daily_deltas_in_range"
        ));
    }

    #[test]
    fn test_list_spores_by_cluster_returns_entries() {
        let (_dir, store) = test_store();
        let cluster_id = [0x44u8; 32];
        let spore_a = [0xA1u8; 32];
        let spore_b = [0xB2u8; 32];
        let other_cluster = [0x55u8; 32];
        let spore_other = [0xC3u8; 32];

        let make_entry = |created_at_block: i64| ObjectEntry {
            standard: crate::types::ObjectStandard::Spore,
            collection_id: Some(cluster_id.to_vec()),
            token_id: None,
            owner_lock_hash: Some(vec![0x11; 32]),
            name: Some("spore".to_string()),
            description: None,
            is_live: true,
            created_at_block,
            created_at_tx: vec![0x22; 32],
            extra: ObjectExtra::Spore {
                content_type: "text/plain".to_string(),
                content_length: 5,
                media_profile: Default::default(),
            },
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_spore(&spore_a, &make_entry(10));
        batch.put_spore(&spore_b, &make_entry(20));
        batch.put_spore(
            &spore_other,
            &ObjectEntry {
                collection_id: Some(other_cluster.to_vec()),
                ..make_entry(30)
            },
        );
        batch.put_spore_by_cluster(&cluster_id, &spore_a);
        batch.put_spore_by_cluster(&cluster_id, &spore_b);
        batch.put_spore_by_cluster(&other_cluster, &spore_other);
        batch.commit().unwrap();

        let rows = store.list_spores_by_cluster(&cluster_id, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, spore_a.to_vec());
        assert_eq!(rows[1].0, spore_b.to_vec());
    }

    #[test]
    fn test_list_spores_by_cluster_fails_on_invalid_spore_payload() {
        let (_dir, store) = test_store();
        let cluster_id = [0x66u8; 32];
        let spore_id = [0x77u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_spore_by_cluster(&cluster_id, &spore_id);
        batch.commit().unwrap();
        store
            .put_cf(store.cf_spore_data(), &spore_id, b"invalid-spore-payload")
            .unwrap();

        let err = store.list_spores_by_cluster(&cluster_id, 10).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize spore entry in get_spores_batch"));
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
                    live_used_capacity_delta: 600,
                },
            )
            .unwrap();
        store
            .put_spore_daily_delta(
                &spore_id,
                20260219,
                &SporeDailyDelta {
                    live_capacity_delta: 100,
                    live_used_capacity_delta: 61,
                },
            )
            .unwrap();

        let cluster = store
            .get_cluster_daily_delta(&cluster_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(cluster.live_capacity_delta, 1000);
        assert_eq!(cluster.live_used_capacity_delta, 600);

        let spore = store
            .get_spore_daily_delta(&spore_id, 20260219)
            .unwrap()
            .unwrap();
        assert_eq!(spore.live_capacity_delta, 100);
        assert_eq!(spore.live_used_capacity_delta, 61);

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
