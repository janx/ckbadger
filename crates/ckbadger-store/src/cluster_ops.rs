//! Cluster aggregate operations.

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::ClusterAggregate;

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

impl CkbadgerStore {
    /// Get pre-aggregated data for a cluster.
    pub fn get_cluster_aggregate(
        &self,
        cluster_id: &[u8],
    ) -> anyhow::Result<Option<ClusterAggregate>> {
        match self.get_cf(self.cf_cluster_agg(), cluster_id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// List all cluster aggregates. Scans the small `cluster_agg` CF.
    pub fn list_cluster_aggregates(&self) -> anyhow::Result<Vec<(Vec<u8>, ClusterAggregate)>> {
        let iter = self.iterator_cf(self.cf_cluster_agg(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate cluster_agg in list_cluster_aggregates: {}",
                    e
                )
            })?;
            let agg: ClusterAggregate = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize cluster aggregate in list_cluster_aggregates: cluster_id=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push((key.to_vec(), agg));
        }
        Ok(results)
    }

    /// Get the live spore count for a specific owner in a cluster.
    pub fn get_cluster_owner_count(
        &self,
        cluster_id: &[u8],
        lock_hash: &[u8],
    ) -> anyhow::Result<i64> {
        if cluster_id.len() != 32 {
            anyhow::bail!(
                "invalid cluster_id length in get_cluster_owner_count: expected 32, got {}",
                cluster_id.len()
            );
        }
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "invalid lock_hash length in get_cluster_owner_count: expected 32, got {}",
                lock_hash.len()
            );
        }
        let key = keys::encode_cluster_owner_key(cluster_id, lock_hash);
        match self.get_cf(self.cf_stats_spore(), &key)? {
            Some(value) if value.len() == 8 => {
                Ok(i64::from_le_bytes(value[..8].try_into().unwrap()))
            }
            _ => Ok(0),
        }
    }

    /// List all owners and live spore counts for a cluster.
    pub fn list_cluster_owner_counts(
        &self,
        cluster_id: &[u8],
    ) -> anyhow::Result<Vec<(Vec<u8>, i64)>> {
        if cluster_id.len() != 32 {
            anyhow::bail!(
                "invalid cluster_id length in list_cluster_owner_counts: expected 32, got {}",
                cluster_id.len()
            );
        }
        let prefix = keys::encode_cluster_owner_prefix(cluster_id);
        let iter = self.prefix_iterator_cf(self.cf_stats_spore(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_spore in list_cluster_owner_counts: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::CLUSTER_OWNER_KEY_SIZE {
                anyhow::bail!(
                    "invalid cluster owner key length: expected {}, got {}",
                    keys::CLUSTER_OWNER_KEY_SIZE,
                    key.len()
                );
            }
            if value.len() != 8 {
                anyhow::bail!(
                    "invalid cluster owner value length: expected 8, got {}",
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_get_cluster_aggregate_missing() {
        let (_dir, store) = test_store();
        let id = [0x01u8; 32];
        assert!(store.get_cluster_aggregate(&id).unwrap().is_none());
    }

    #[test]
    fn test_put_and_get_cluster_aggregate() {
        let (_dir, store) = test_store();
        let id = [0x01u8; 32];
        let agg = ClusterAggregate {
            name: Some("Test Cluster".to_string()),
            description: Some("A test".to_string()),
            total_count: 100,
            live_count: 80,
            owner_count: 25,
            ..Default::default()
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_cluster_aggregate(&id, &agg);
        batch.commit().unwrap();

        let result = store.get_cluster_aggregate(&id).unwrap().unwrap();
        assert_eq!(result.name.as_deref(), Some("Test Cluster"));
        assert_eq!(result.total_count, 100);
        assert_eq!(result.live_count, 80);
        assert_eq!(result.owner_count, 25);
    }

    #[test]
    fn test_list_cluster_aggregates() {
        let (_dir, store) = test_store();
        let id_a = [0x01u8; 32];
        let id_b = [0x02u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cluster_aggregate(
            &id_a,
            &ClusterAggregate {
                name: Some("A".to_string()),
                total_count: 10,
                live_count: 8,
                owner_count: 3,
                ..Default::default()
            },
        );
        batch.put_cluster_aggregate(
            &id_b,
            &ClusterAggregate {
                name: Some("B".to_string()),
                total_count: 20,
                live_count: 15,
                owner_count: 7,
                ..Default::default()
            },
        );
        batch.commit().unwrap();

        let results = store.list_cluster_aggregates().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_cluster_owner_count() {
        let (_dir, store) = test_store();
        let cluster_id = [0x01u8; 32];
        let lock_hash = [0xAAu8; 32];

        // Default is 0
        assert_eq!(
            store
                .get_cluster_owner_count(&cluster_id, &lock_hash)
                .unwrap(),
            0
        );

        let mut batch = StoreBatch::new(&store);
        batch.put_cluster_owner_count(&cluster_id, &lock_hash, 5);
        batch.commit().unwrap();

        assert_eq!(
            store
                .get_cluster_owner_count(&cluster_id, &lock_hash)
                .unwrap(),
            5
        );
    }

    #[test]
    fn test_delete_cluster_owner() {
        let (_dir, store) = test_store();
        let cluster_id = [0x01u8; 32];
        let lock_hash = [0xBBu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cluster_owner_count(&cluster_id, &lock_hash, 3);
        batch.commit().unwrap();

        assert_eq!(
            store
                .get_cluster_owner_count(&cluster_id, &lock_hash)
                .unwrap(),
            3
        );

        let mut batch = StoreBatch::new(&store);
        batch.delete_cluster_owner(&cluster_id, &lock_hash);
        batch.commit().unwrap();

        assert_eq!(
            store
                .get_cluster_owner_count(&cluster_id, &lock_hash)
                .unwrap(),
            0
        );
    }

    #[test]
    fn test_list_cluster_owner_counts() {
        let (_dir, store) = test_store();
        let cluster_id = [0x01u8; 32];
        let lock_hash_a = [0xAAu8; 32];
        let lock_hash_b = [0xBBu8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cluster_owner_count(&cluster_id, &lock_hash_a, 3);
        batch.put_cluster_owner_count(&cluster_id, &lock_hash_b, 1);
        batch.commit().unwrap();

        let mut rows = store.list_cluster_owner_counts(&cluster_id).unwrap();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], (lock_hash_a.to_vec(), 3));
        assert_eq!(rows[1], (lock_hash_b.to_vec(), 1));
    }

    #[test]
    fn test_cluster_aggregate_default() {
        let agg = ClusterAggregate::default();
        assert!(agg.name.is_none());
        assert_eq!(agg.total_count, 0);
        assert_eq!(agg.live_count, 0);
        assert_eq!(agg.owner_count, 0);
    }

    #[test]
    fn test_list_cluster_owner_counts_rejects_invalid_cluster_id_length() {
        let (_dir, store) = test_store();
        let err = store.list_cluster_owner_counts(&[0x11; 31]).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid cluster_id length in list_cluster_owner_counts"));
    }

    #[test]
    fn test_list_cluster_aggregates_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        store
            .put_cf(store.cf_cluster_agg(), &[0x01; 32], b"invalid-cluster-agg")
            .unwrap();

        let err = store.list_cluster_aggregates().unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize cluster aggregate in list_cluster_aggregates"));
    }
}
