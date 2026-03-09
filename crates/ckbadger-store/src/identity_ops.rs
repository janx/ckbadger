//! Identity-specific store operations (.bit, did:ckb).

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{
    AssetAction, IdentityCollectionAggregate, IdentityEntry, ObjectCollectionActivityEntry,
};

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

impl CkbadgerStore {
    pub fn get_identity(&self, id: &[u8]) -> anyhow::Result<Option<IdentityEntry>> {
        match self.get_cf(self.cf_identity_data(), id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// List all identities.
    pub fn list_identities(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, IdentityEntry)>> {
        let iter = self.iterator_cf(self.cf_identity_data(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate identity_data in list_identities: {}", e)
            })?;
            let entry: IdentityEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize identity entry in list_identities: identity_id=0x{}, error={}",
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

    pub fn get_identity_collection_aggregate(
        &self,
        collection_id: &[u8],
    ) -> anyhow::Result<Option<IdentityCollectionAggregate>> {
        match self.get_cf(self.cf_identity_agg(), collection_id)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// List pre-computed activities for an identity collection, newest first.
    ///
    /// Returns `(block_number, tx_index, entry)` tuples. Simple prefix scan
    /// on `CF_IDENTITY_COLLECTION_ACTIVITIES` with early termination at `limit`.
    pub fn list_identity_collection_activities(
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
            self.cf_identity_collection_activities(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        let action_filter_parsed = action_filter.map(|s| match s {
            "mint" => AssetAction::Mint,
            "transfer" => AssetAction::Transfer,
            "burn" => AssetAction::Burn,
            "recycle" => AssetAction::Recycle,
            "renew" => AssetAction::Renew,
            "update" => AssetAction::Update,
            _ => AssetAction::Mint, // unreachable if caller validates
        });

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate identity_collection_activities in list_identity_collection_activities: {}",
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
            let entry: ObjectCollectionActivityEntry = bincode::deserialize(&value)?;
            if entry.tx_hash != tx_hash_from_key {
                anyhow::bail!(
                    "identity_collection_activities key/value tx_hash mismatch in list_identity_collection_activities: collection_id=0x{}, block_num={}, tx_idx={}, key_tx_hash=0x{}, value_tx_hash=0x{}",
                    bytes_to_hex(&prefix),
                    block_num,
                    tx_idx,
                    bytes_to_hex(&tx_hash_from_key),
                    bytes_to_hex(&entry.tx_hash)
                );
            }
            if entry.block_hash != block_hash_from_key {
                anyhow::bail!(
                    "identity_collection_activities key/value block_hash mismatch in list_identity_collection_activities: collection_id=0x{}, block_num={}, tx_idx={}, key_block_hash=0x{}, value_block_hash=0x{}",
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use crate::types::{IdentityExtra, IdentityStandard};
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        (dir, store)
    }

    fn test_append_only_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        (dir, store)
    }

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
    fn test_get_identity_missing() {
        let (_dir, store) = test_store();
        let id = [0x01u8; 20];
        assert!(store.get_identity(&id).unwrap().is_none());
    }

    #[test]
    fn test_put_and_get_identity() {
        let (_dir, store) = test_store();
        let id = [0x01u8; 20];
        let entry = IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: Some(vec![0x11; 32]),
            name: Some("example.bit".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![0x22; 32],
            extra: IdentityExtra::DotBit {
                expired_at: Some(1700000000),
                registered_at: Some(1600000000),
                status: Some(0),
            },
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_identity(&id, &entry);
        batch.commit().unwrap();

        let result = store.get_identity(&id).unwrap().unwrap();
        assert_eq!(result.standard, IdentityStandard::DotBit);
        assert_eq!(result.name.as_deref(), Some("example.bit"));
        assert!(result.is_live);
        assert_eq!(result.created_at_block, 100);
    }

    #[test]
    fn test_list_identities() {
        let (_dir, store) = test_store();
        let id_a = [0x01u8; 20];
        let id_b = [0x02u8; 20];

        let make_entry = |name: &str, standard: IdentityStandard| IdentityEntry {
            standard,
            owner_lock_hash: Some(vec![0x11; 32]),
            name: Some(name.to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![0x22; 32],
            extra: IdentityExtra::DotBit {
                expired_at: None,
                registered_at: None,
                status: None,
            },
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_identity(&id_a, &make_entry("alice.bit", IdentityStandard::DotBit));
        batch.put_identity(
            &id_b,
            &make_entry("did:ckb:example", IdentityStandard::DidCkb),
        );
        batch.commit().unwrap();

        let results = store.list_identities(10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_list_identities_with_limit() {
        let (_dir, store) = test_store();

        let entry = IdentityEntry {
            standard: IdentityStandard::DotBit,
            owner_lock_hash: None,
            name: None,
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![],
            extra: IdentityExtra::DotBit {
                expired_at: None,
                registered_at: None,
                status: None,
            },
        };

        let mut batch = StoreBatch::new(&store);
        for i in 0..5u8 {
            batch.put_identity(&[i; 20], &entry);
        }
        batch.commit().unwrap();

        let results = store.list_identities(3).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_list_identities_fails_on_invalid_payload() {
        let (_dir, store) = test_store();
        store
            .put_cf(
                store.cf_identity_data(),
                &[0x11; 20],
                b"invalid-identity-payload",
            )
            .unwrap();

        let err = store.list_identities(10).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize identity entry in list_identities"));
    }

    #[test]
    fn test_identity_collection_aggregate_round_trip() {
        let (_dir, store) = test_store();
        let collection_id = *b"dotbit_collection_______________";

        let agg = IdentityCollectionAggregate {
            name: Some(".bit".to_string()),
            standard: IdentityStandard::DotBit,
            total_count: 100,
            live_count: 80,
            holders_count: 50,
            activities_count: 200,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_identity_collection_aggregate(&collection_id, &agg);
        batch.commit().unwrap();

        let loaded = store
            .get_identity_collection_aggregate(&collection_id)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.name, Some(".bit".to_string()));
        assert_eq!(loaded.activities_count, 200);
        assert_eq!(loaded.total_count, 100);
        assert_eq!(loaded.live_count, 80);
        assert_eq!(loaded.holders_count, 50);
    }

    #[test]
    fn test_identity_collection_aggregate_missing_returns_none() {
        let (_dir, store) = test_store();
        let result = store
            .get_identity_collection_aggregate(&[0x99; 32])
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_identity_collection_activities_round_trip() {
        let (_dir, store) = test_append_only_store();
        let collection_id = *b"dotbit_collection_______________";

        let entry = make_activity(&[0x01; 32], 1000, vec![AssetAction::Mint]);

        let mut batch = StoreBatch::new(&store);
        batch.put_identity_collection_activity(&collection_id, 100, 0, &entry);
        batch.commit().unwrap();

        let results = store
            .list_identity_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 100); // block_number
        assert_eq!(results[0].1, 0); // tx_idx
        assert!(matches!(results[0].2.actions[0], AssetAction::Mint));
    }

    #[test]
    fn test_list_identity_collection_activities_empty() {
        let (_dir, store) = test_append_only_store();
        let cid = [0x01u8; 32];
        let results = store
            .list_identity_collection_activities(&cid, 10, None, None)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_list_identity_collection_activities_pagination() {
        let (_dir, store) = test_append_only_store();
        let cid = *b"dotbit_collection_______________";

        let mut batch = StoreBatch::new(&store);
        for block in 100..105 {
            batch.put_identity_collection_activity(
                &cid,
                block,
                0,
                &make_activity(&[block as u8; 32], block * 1000, vec![AssetAction::Mint]),
            );
        }
        batch.commit().unwrap();

        // Request limit=3
        let page1 = store
            .list_identity_collection_activities(&cid, 3, None, None)
            .unwrap();
        assert_eq!(page1.len(), 3);
        // Should be newest first: 104, 103, 102
        assert_eq!(page1[0].0, 104);
        assert_eq!(page1[1].0, 103);
        assert_eq!(page1[2].0, 102);

        // Page 2 using cursor
        let cursor = (page1[2].0, page1[2].1);
        let page2 = store
            .list_identity_collection_activities(&cid, 3, Some(cursor), None)
            .unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].0, 101);
        assert_eq!(page2[1].0, 100);
    }

    #[test]
    fn test_list_identity_collection_activities_action_filter() {
        let (_dir, store) = test_append_only_store();
        let cid = *b"dotbit_collection_______________";

        let mut batch = StoreBatch::new(&store);
        batch.put_identity_collection_activity(
            &cid,
            100,
            0,
            &make_activity(&[1u8; 32], 100000, vec![AssetAction::Mint]),
        );
        batch.put_identity_collection_activity(
            &cid,
            200,
            0,
            &make_activity(&[2u8; 32], 200000, vec![AssetAction::Transfer]),
        );
        batch.put_identity_collection_activity(
            &cid,
            300,
            0,
            &make_activity(&[3u8; 32], 300000, vec![AssetAction::Burn]),
        );
        batch.commit().unwrap();

        let mints = store
            .list_identity_collection_activities(&cid, 10, None, Some("mint"))
            .unwrap();
        assert_eq!(mints.len(), 1);
        assert_eq!(mints[0].0, 100);

        let transfers = store
            .list_identity_collection_activities(&cid, 10, None, Some("transfer"))
            .unwrap();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].0, 200);
    }
}
