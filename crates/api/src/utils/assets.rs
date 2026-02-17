use ckbadger_store::types::DobStandard;
use ckbadger_store::CkbadgerStore;

fn non_empty_name(name: Option<&str>) -> Option<String> {
    let trimmed = name?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Resolve a DOB collection display name.
///
/// Priority:
/// 1) `cluster_agg.name` (if non-empty)
/// 2) cluster entry name from `spore_data` (if non-empty)
pub fn resolve_dob_collection_name(
    store: &CkbadgerStore,
    cluster_id: &[u8],
    aggregate_name: Option<&str>,
) -> Option<String> {
    if let Some(name) = non_empty_name(aggregate_name) {
        return Some(name);
    }

    match store.get_spore(cluster_id) {
        Ok(Some(entry)) if entry.standard == DobStandard::SporeCluster => {
            non_empty_name(entry.name.as_deref())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::{DobEntry, DobExtra};
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn resolve_name_prefers_aggregate_name() {
        let (_dir, store) = test_store();
        let cluster_id = [0x11u8; 32];

        let resolved = resolve_dob_collection_name(&store, &cluster_id, Some("Agg Name"));
        assert_eq!(resolved.as_deref(), Some("Agg Name"));
    }

    #[test]
    fn resolve_name_falls_back_to_cluster_entry_name() {
        let (_dir, store) = test_store();
        let cluster_id = [0x22u8; 32];

        let entry = DobEntry {
            standard: DobStandard::SporeCluster,
            collection_id: None,
            owner_lock_hash: Some(vec![0x33; 32]),
            name: Some("Cluster Entry Name".to_string()),
            description: Some("desc".to_string()),
            is_live: true,
            created_at_block: 100,
            created_at_tx: vec![0x44; 32],
            extra: DobExtra::SporeCluster,
        };
        store.put_spore_direct(&cluster_id, &entry).unwrap();

        let resolved = resolve_dob_collection_name(&store, &cluster_id, None);
        assert_eq!(resolved.as_deref(), Some("Cluster Entry Name"));
    }

    #[test]
    fn resolve_name_treats_blank_as_missing() {
        let (_dir, store) = test_store();
        let cluster_id = [0x55u8; 32];

        let entry = DobEntry {
            standard: DobStandard::SporeCluster,
            collection_id: None,
            owner_lock_hash: Some(vec![0x66; 32]),
            name: Some("   ".to_string()),
            description: None,
            is_live: true,
            created_at_block: 1,
            created_at_tx: vec![0x77; 32],
            extra: DobExtra::SporeCluster,
        };
        store.put_spore_direct(&cluster_id, &entry).unwrap();

        assert!(resolve_dob_collection_name(&store, &cluster_id, Some("  ")).is_none());
    }
}
