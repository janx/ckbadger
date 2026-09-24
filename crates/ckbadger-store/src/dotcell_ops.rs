//! Read operations for `.cell` (DotCell) names.
//!
//! Ownership is stored as the chain stores it — a 20-byte lock-hash prefix —
//! so the owner index and the per-owner counter are both keyed by those 20
//! bytes. Resolving a prefix to an address is a read-time concern
//! (`resolve_lock_hash_prefix`), never a write-time guess.

use rocksdb::IteratorMode;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::DotCellRingRoot;

impl CkbadgerStore {
    /// The `.cell` name ids owned by one 20-byte owner prefix, in ascending id
    /// order. `cursor` is exclusive: pass the last id of the previous page.
    pub fn list_dotcell_names_by_owner20(
        &self,
        owner20: &[u8; 20],
        cursor: Option<[u8; 20]>,
        limit: usize,
    ) -> anyhow::Result<Vec<[u8; 20]>> {
        let prefix = keys::encode_dotcell_name_by_owner_prefix(owner20);
        let mut out = Vec::new();
        for item in self.prefix_iterator_cf(self.cf_dotcell_name_by_owner(), &prefix) {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate dotcell_name_by_owner: owner20=0x{}, {}",
                    hex::encode(owner20),
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::DOTCELL_NAME_BY_OWNER_KEY_SIZE {
                anyhow::bail!(
                    "dotcell_name_by_owner key is not {} bytes: len={}, key=0x{}",
                    keys::DOTCELL_NAME_BY_OWNER_KEY_SIZE,
                    key.len(),
                    hex::encode(&key)
                );
            }
            let (_, id) = keys::decode_dotcell_name_by_owner_key(&key);
            if let Some(cursor) = cursor {
                if id <= cursor {
                    continue;
                }
            }
            out.push(id);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// How many `.cell` names one 20-byte owner prefix holds live, from the
    /// per-owner counter in `CF_STATS_IDENTITY`.
    pub fn get_dotcell_owner20_count(
        &self,
        collection_id: &[u8],
        owner20: &[u8; 20],
    ) -> anyhow::Result<i64> {
        let key = keys::encode_identity_owner20_key(collection_id, owner20);
        match self.get_cf(self.cf_stats_identity(), &key)? {
            Some(value) => {
                if value.len() != 8 {
                    anyhow::bail!(
                        "invalid identity owner value length: expected 8, got {} (owner20=0x{})",
                        value.len(),
                        hex::encode(owner20)
                    );
                }
                Ok(i64::from_le_bytes(value[..8].try_into().expect("8 bytes")))
            }
            None => Ok(0),
        }
    }

    /// The ring root recorded for one namespace, if the root cell has been
    /// indexed yet.
    pub fn get_dotcell_ring(
        &self,
        namespace_args: &[u8; 20],
    ) -> anyhow::Result<Option<DotCellRingRoot>> {
        match self.get_cf(self.cf_dotcell_ring(), namespace_args)? {
            Some(value) => Ok(Some(bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize DotCellRingRoot: namespace=0x{}, error={}",
                    hex::encode(namespace_args),
                    e
                )
            })?)),
            None => Ok(None),
        }
    }

    /// Every namespace's ring root. A network has exactly one; more than one
    /// would make bare 20-byte ids collide on identical labels, which the
    /// write path refuses.
    pub fn list_dotcell_rings(&self) -> anyhow::Result<Vec<([u8; 20], DotCellRingRoot)>> {
        let mut out = Vec::new();
        for item in self.iterator_cf(self.cf_dotcell_ring(), IteratorMode::Start) {
            let (key, value) =
                item.map_err(|e| anyhow::anyhow!("failed to iterate dotcell_ring: {}", e))?;
            if key.len() != 20 {
                anyhow::bail!(
                    "dotcell_ring key is not 20 bytes: len={}, key=0x{}",
                    key.len(),
                    hex::encode(&key)
                );
            }
            let namespace: [u8; 20] = key[..20].try_into().expect("20 bytes");
            let root: DotCellRingRoot = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize DotCellRingRoot: namespace=0x{}, error={}",
                    hex::encode(namespace),
                    e
                )
            })?;
            out.push((namespace, root));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use crate::batch::StoreBatch;
    use crate::keys;
    use crate::store::CkbadgerStore;
    use crate::types::{DotCellRingRoot, DOTCELL_SENTINEL_COLLECTION};
    use tempfile::TempDir;

    fn id(byte: u8) -> [u8; 20] {
        [byte; 20]
    }

    #[test]
    fn list_dotcell_names_by_owner20_scans_prefix_in_id_order() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let owner_a = id(0xAA);
        let owner_b = id(0xBB);

        let mut batch = StoreBatch::new(&store);
        for name in [id(0x30), id(0x10), id(0x20)] {
            batch.put_dotcell_name_by_owner(&owner_a, &name);
        }
        batch.put_dotcell_name_by_owner(&owner_b, &id(0x99));
        batch.commit().unwrap();

        assert_eq!(
            store
                .list_dotcell_names_by_owner20(&owner_a, None, 10)
                .unwrap(),
            vec![id(0x10), id(0x20), id(0x30)]
        );
        assert_eq!(
            store
                .list_dotcell_names_by_owner20(&owner_b, None, 10)
                .unwrap(),
            vec![id(0x99)]
        );
        assert_eq!(
            store
                .list_dotcell_names_by_owner20(&owner_a, Some(id(0x10)), 10)
                .unwrap(),
            vec![id(0x20), id(0x30)],
            "the cursor is exclusive"
        );
        assert_eq!(
            store
                .list_dotcell_names_by_owner20(&owner_a, None, 2)
                .unwrap()
                .len(),
            2
        );
        assert!(store
            .list_dotcell_names_by_owner20(&id(0xCC), None, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn deleting_an_owner_row_removes_it_from_the_listing() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let owner = id(0xAA);

        let mut batch = StoreBatch::new(&store);
        batch.put_dotcell_name_by_owner(&owner, &id(0x10));
        batch.put_dotcell_name_by_owner(&owner, &id(0x20));
        batch.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.delete_dotcell_name_by_owner(&owner, &id(0x10));
        batch.commit().unwrap();

        assert_eq!(
            store
                .list_dotcell_names_by_owner20(&owner, None, 10)
                .unwrap(),
            vec![id(0x20)]
        );
    }

    #[test]
    fn dotcell_ring_roundtrip_and_namespace_guard_read() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let namespace = id(0x42);
        let root = DotCellRingRoot {
            root_tx_hash: vec![0x77; 32],
            root_output_index: 0,
            first_id: id(0x11),
            created_at_block: 20_515_882,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_dotcell_ring(&namespace, &root);
        batch.commit().unwrap();

        assert_eq!(
            store.get_dotcell_ring(&namespace).unwrap(),
            Some(root.clone())
        );
        assert_eq!(store.get_dotcell_ring(&id(0x43)).unwrap(), None);
        assert_eq!(store.list_dotcell_rings().unwrap(), vec![(namespace, root)]);
    }

    #[test]
    fn owner20_counts_read_back_through_the_explicit_20_byte_key() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let owner = id(0x5A);

        let mut batch = StoreBatch::new(&store);
        batch.put_identity_owner20_count(&DOTCELL_SENTINEL_COLLECTION, &owner, 3);
        batch.commit().unwrap();

        assert_eq!(
            store
                .get_dotcell_owner20_count(&DOTCELL_SENTINEL_COLLECTION, &owner)
                .unwrap(),
            3
        );
        assert_eq!(
            store
                .get_dotcell_owner20_count(&DOTCELL_SENTINEL_COLLECTION, &id(0x5B))
                .unwrap(),
            0
        );
        // The generic listing hands back the 32-byte owner segment; a dotcell
        // caller decodes the 20 bytes the chain actually gave it.
        let rows = store
            .list_identity_owner_counts(&DOTCELL_SENTINEL_COLLECTION)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(keys::decode_identity_owner20(&rows[0].0), owner);
        assert_eq!(rows[0].1, 3);
    }
}
