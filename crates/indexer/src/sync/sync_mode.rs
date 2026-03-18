use anyhow::{bail, Result};

// ---------------------------------------------------------------------------
// Free functions — moved from indexer.rs (call sites unchanged for now)
// ---------------------------------------------------------------------------

pub(crate) fn is_bulk_sync_active_by_lag(blocks_behind: u64, bulk_sync_threshold: u64) -> bool {
    blocks_behind > bulk_sync_threshold
}

pub(crate) fn is_bulk_sync_batch(chain_tip: u64, batch_end: u64, bulk_sync_threshold: u64) -> bool {
    let blocks_behind = chain_tip.checked_sub(batch_end).unwrap_or_else(|| {
        panic!(
            "invalid bulk-sync batch range: batch_end={} exceeds chain_tip={}",
            batch_end, chain_tip
        )
    });
    blocks_behind > bulk_sync_threshold
}

pub(crate) fn is_effective_bulk_sync_batch(
    chain_tip: u64,
    batch_end: u64,
    bulk_sync_threshold: u64,
    bulk_sync_allowed: bool,
) -> bool {
    bulk_sync_allowed && is_bulk_sync_batch(chain_tip, batch_end, bulk_sync_threshold)
}

pub(crate) fn should_run_reorg_handling(blocks_behind: u64, bulk_sync_threshold: u64) -> bool {
    blocks_behind <= bulk_sync_threshold
}

pub(crate) fn ensure_bulk_sync_fresh_start(
    bulk_sync_mode: bool,
    sync_tip_block: i64,
    sync_tip_hash: &Option<Vec<u8>>,
    append_only_store: &ckbadger_store::CkbadgerStore,
) -> Result<()> {
    if !bulk_sync_mode {
        return Ok(());
    }
    if sync_tip_block == 0 && sync_tip_hash.is_none() {
        // Domain store is fresh; also verify append-only store is empty.
        // Bulk sync skips per-key existence probes, so stale CF_CELLS data
        // would be silently overwritten, violating append-only semantics.
        if append_only_store.has_any_data_in_cells_cf()? {
            bail!(
                "bulk sync fail-fast: domain store is fresh but append-only store contains \
                 existing CF_CELLS data. Both stores must be empty for bulk sync. \
                 Delete both RocksDB directories and restart from genesis"
            );
        }
        return Ok(());
    }
    bail!(
        "bulk sync fail-fast: bulk sync only supports fresh-db rebuilds from genesis; \
         detected existing sync tip state (tip_block={}, tip_hash_present={}). \
         delete RocksDB and restart from genesis",
        sync_tip_block,
        sync_tip_hash.is_some()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Moved free-function tests ---

    #[test]
    fn test_is_bulk_sync_active_by_lag_threshold() {
        assert!(!is_bulk_sync_active_by_lag(1000, 1000));
        assert!(is_bulk_sync_active_by_lag(1001, 1000));
        assert!(!is_bulk_sync_active_by_lag(0, 1000));
    }

    #[test]
    fn test_is_bulk_sync_batch_uses_tip_distance() {
        assert!(!is_bulk_sync_batch(10_000, 9_000, 1000));
        assert!(is_bulk_sync_batch(10_001, 9_000, 1000));
    }

    #[test]
    fn test_is_effective_bulk_sync_batch_requires_bulk_sync_to_be_allowed() {
        assert!(!is_effective_bulk_sync_batch(10_001, 9_000, 1000, false));
        assert!(is_effective_bulk_sync_batch(10_001, 9_000, 1000, true));
    }

    #[test]
    fn test_should_run_reorg_handling_only_in_live_sync_window() {
        assert!(should_run_reorg_handling(0, 1000));
        assert!(should_run_reorg_handling(1000, 1000));
        assert!(!should_run_reorg_handling(1001, 1000));
    }

    #[test]
    #[should_panic(expected = "invalid bulk-sync batch range")]
    fn test_is_bulk_sync_batch_panics_when_batch_end_exceeds_tip() {
        let _ = is_bulk_sync_batch(100, 150, 1000);
    }

    fn make_empty_append_only_store() -> (tempfile::TempDir, ckbadger_store::CkbadgerStore) {
        let dir = tempfile::tempdir().unwrap();
        let store =
            ckbadger_store::CkbadgerStore::open_append_only(dir.path().join("append")).unwrap();
        (dir, store)
    }

    #[test]
    fn test_ensure_bulk_sync_fresh_start_allows_empty_tip_state() {
        let (_dir, append_store) = make_empty_append_only_store();
        ensure_bulk_sync_fresh_start(true, 0, &None, &append_store).unwrap();
    }

    #[test]
    fn test_ensure_bulk_sync_fresh_start_rejects_existing_tip_in_bulk_mode() {
        let (_dir, append_store) = make_empty_append_only_store();
        let err = ensure_bulk_sync_fresh_start(true, 0, &Some(vec![0x11; 32]), &append_store)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("bulk sync only supports fresh-db rebuilds from genesis"));
    }

    #[test]
    fn test_ensure_bulk_sync_fresh_start_skips_check_when_not_bulk() {
        let (_dir, append_store) = make_empty_append_only_store();
        ensure_bulk_sync_fresh_start(false, 123, &Some(vec![0x22; 32]), &append_store).unwrap();
    }

    #[test]
    fn test_ensure_bulk_sync_fresh_start_rejects_nonempty_append_only_store() {
        // Create an append-only store with some data
        let dir = tempfile::tempdir().unwrap();
        let append_store =
            ckbadger_store::CkbadgerStore::open_append_only(dir.path().join("append")).unwrap();
        // Write a dummy cell entry
        let mut batch = ckbadger_store::StoreBatch::new(&append_store);
        batch.put_cell_payload_by_outpoint(
            &[0x01; 32],
            0,
            &ckbadger_store::LiveCellInfo {
                capacity: 100,
                lock_script_hash: vec![0xAA; 32],
                lock_code_hash: vec![0xBB; 32],
                lock_hash_type: 1,
                lock_args: vec![],
                type_script_hash: None,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                data_size: 0,
                occupied_capacity: 61,
                udt_amount: None,
                data_hash: None,
            },
        );
        batch.commit().unwrap();

        let err = ensure_bulk_sync_fresh_start(true, 0, &None, &append_store).unwrap_err();
        assert!(err.to_string().contains("append-only store contains"));
    }
}
