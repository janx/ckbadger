use anyhow::{bail, Result};

/// Classifies the current sync phase based on how far behind the chain tip the
/// indexer is.  Later tasks will thread `SyncMode` through the batch pipeline,
/// replacing the ~52 scattered `bulk_sync_mode` booleans.
#[allow(dead_code)] // Call-site conversion happens in later refactoring tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncMode {
    Bulk,
    Live,
}

#[allow(dead_code)] // Call-site conversion happens in later refactoring tasks
impl SyncMode {
    pub fn from_lag(blocks_behind: u64, threshold: u64) -> Self {
        if blocks_behind > threshold {
            SyncMode::Bulk
        } else {
            SyncMode::Live
        }
    }

    pub fn is_bulk(&self) -> bool {
        matches!(self, SyncMode::Bulk)
    }

    pub fn should_handle_reorg(&self) -> bool {
        matches!(self, SyncMode::Live)
    }

    pub fn should_cache_proposals(&self) -> bool {
        matches!(self, SyncMode::Live)
    }

    pub fn should_invalidate_caches(&self) -> bool {
        matches!(self, SyncMode::Live)
    }

    pub fn should_accumulate_blocks(&self) -> bool {
        matches!(self, SyncMode::Live)
    }

    pub fn commit_with_wal(&self) -> bool {
        matches!(self, SyncMode::Live)
    }

    pub fn should_use_parallel_writes(&self) -> bool {
        matches!(self, SyncMode::Bulk)
    }

    pub fn fail_fast_on_error(&self) -> bool {
        matches!(self, SyncMode::Bulk)
    }
}

// ---------------------------------------------------------------------------
// Free functions — moved from indexer.rs (call sites unchanged for now)
// ---------------------------------------------------------------------------

pub(crate) fn should_skip_address_balances(_bulk_sync_mode: bool) -> bool {
    // Address balances must always be updated inline to keep bulk sync exact.
    false
}

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

    // --- SyncMode enum tests (NEW) ---

    #[test]
    fn test_sync_mode_from_lag() {
        // Above threshold => Bulk
        assert_eq!(SyncMode::from_lag(1001, 1000), SyncMode::Bulk);
        // At threshold => Live
        assert_eq!(SyncMode::from_lag(1000, 1000), SyncMode::Live);
        // Below threshold => Live
        assert_eq!(SyncMode::from_lag(0, 1000), SyncMode::Live);
        assert_eq!(SyncMode::from_lag(999, 1000), SyncMode::Live);
    }

    #[test]
    fn test_sync_mode_behavioral_methods() {
        let bulk = SyncMode::Bulk;
        let live = SyncMode::Live;

        // is_bulk
        assert!(bulk.is_bulk());
        assert!(!live.is_bulk());

        // should_handle_reorg — only Live
        assert!(!bulk.should_handle_reorg());
        assert!(live.should_handle_reorg());

        // should_cache_proposals — only Live
        assert!(!bulk.should_cache_proposals());
        assert!(live.should_cache_proposals());

        // should_invalidate_caches — only Live
        assert!(!bulk.should_invalidate_caches());
        assert!(live.should_invalidate_caches());

        // should_accumulate_blocks — only Live
        assert!(!bulk.should_accumulate_blocks());
        assert!(live.should_accumulate_blocks());

        // commit_with_wal — only Live
        assert!(!bulk.commit_with_wal());
        assert!(live.commit_with_wal());

        // should_use_parallel_writes — only Bulk
        assert!(bulk.should_use_parallel_writes());
        assert!(!live.should_use_parallel_writes());

        // fail_fast_on_error — only Bulk
        assert!(bulk.fail_fast_on_error());
        assert!(!live.fail_fast_on_error());
    }

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

    #[test]
    fn test_address_balances_are_never_skipped_in_bulk_mode() {
        assert!(!should_skip_address_balances(true));
        assert!(!should_skip_address_balances(false));
    }
}
