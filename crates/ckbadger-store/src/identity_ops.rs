//! Identity-specific store operations (.bit, did:ckb).

use crate::store::CkbadgerStore;
use crate::types::IdentityEntry;

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
}
