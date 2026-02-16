use ckbadger_store::types::{DeepForkInfo, SyncStatus};
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
    std::mem::forget(dir);
    store
}

/// Set address_balances_deferred flag and verify.
#[test]
fn test_sync_status_address_balances_deferred() {
    let store = setup_store();

    let status = SyncStatus {
        tip_block_number: 2000,
        tip_block_hash: vec![0xbb; 32],
        total_transactions: 10_000,
        total_cells_created: 15_000,
        total_cells_consumed: 6000,
        last_synced_at: 1700001000,
        address_balances_deferred: true,
        activities_deferred: false,
        deep_fork_detected: false,
        deep_fork_info: None,

        avg_block_time_rebuilt: false,
    };

    store.set_sync_status(&status).unwrap();

    let retrieved = store.get_sync_status().unwrap();
    assert!(retrieved.address_balances_deferred);
    assert_eq!(retrieved.tip_block_number, 2000);
    assert_eq!(retrieved.total_cells_created, 15_000);
}

/// Use update_sync_status closure to toggle a flag.
#[test]
fn test_update_sync_status_closure() {
    let store = setup_store();

    // Set initial status
    let initial = SyncStatus {
        tip_block_number: 3000,
        tip_block_hash: vec![0xcc; 32],
        total_transactions: 20_000,
        total_cells_created: 30_000,
        total_cells_consumed: 12_000,
        last_synced_at: 1700002000,
        address_balances_deferred: false,
        activities_deferred: false,
        deep_fork_detected: false,
        deep_fork_info: None,

        avg_block_time_rebuilt: false,
    };
    store.set_sync_status(&initial).unwrap();

    // Toggle address_balances_deferred via closure
    store
        .update_sync_status(|s| {
            s.address_balances_deferred = true;
            s.tip_block_number = 3500;
        })
        .unwrap();

    let updated = store.get_sync_status().unwrap();
    assert!(updated.address_balances_deferred);
    assert_eq!(updated.tip_block_number, 3500);
    // Other fields should remain unchanged
    assert_eq!(updated.total_transactions, 20_000);

    // Toggle it back
    store
        .update_sync_status(|s| {
            s.address_balances_deferred = false;
        })
        .unwrap();

    let final_status = store.get_sync_status().unwrap();
    assert!(!final_status.address_balances_deferred);
    assert_eq!(final_status.tip_block_number, 3500);
}

/// Set activities_deferred flag, verify persistence, then clear via closure.
#[test]
fn test_activities_deferred_lifecycle() {
    let store = setup_store();

    // Default should be false
    let status = store.get_sync_status().unwrap();
    assert!(!status.activities_deferred);

    // Simulate bulk sync start: set activities_deferred = true
    store
        .update_sync_status(|s| {
            s.activities_deferred = true;
            s.tip_block_number = 5000;
        })
        .unwrap();

    let status = store.get_sync_status().unwrap();
    assert!(status.activities_deferred);
    assert_eq!(status.tip_block_number, 5000);

    // Simulate rebuild completion: clear activities_deferred
    store
        .update_sync_status(|s| {
            s.activities_deferred = false;
        })
        .unwrap();

    let status = store.get_sync_status().unwrap();
    assert!(!status.activities_deferred);
    // Other fields preserved
    assert_eq!(status.tip_block_number, 5000);
}

/// Both deferred flags can be independently set and cleared.
#[test]
fn test_deferred_flags_independent() {
    let store = setup_store();

    // Set only activities_deferred
    store
        .update_sync_status(|s| {
            s.activities_deferred = true;
            s.address_balances_deferred = false;
        })
        .unwrap();

    let status = store.get_sync_status().unwrap();
    assert!(status.activities_deferred);
    assert!(!status.address_balances_deferred);

    // Set only address_balances_deferred
    store
        .update_sync_status(|s| {
            s.activities_deferred = false;
            s.address_balances_deferred = true;
        })
        .unwrap();

    let status = store.get_sync_status().unwrap();
    assert!(!status.activities_deferred);
    assert!(status.address_balances_deferred);

    // Set both
    store
        .update_sync_status(|s| {
            s.activities_deferred = true;
            s.address_balances_deferred = true;
        })
        .unwrap();

    let status = store.get_sync_status().unwrap();
    assert!(status.activities_deferred);
    assert!(status.address_balances_deferred);
}

/// Toggle bulk sync mode on and off.
#[test]
fn test_bulk_sync_mode_toggle() {
    let store = setup_store();

    // Default is false
    assert!(!store.is_bulk_sync_mode());

    // Enable
    store.set_bulk_sync_mode(true);
    assert!(store.is_bulk_sync_mode());

    // Disable
    store.set_bulk_sync_mode(false);
    assert!(!store.is_bulk_sync_mode());

    // Re-enable to confirm toggling works repeatedly
    store.set_bulk_sync_mode(true);
    assert!(store.is_bulk_sync_mode());
}

/// Set a deep fork, verify it is detected, then clear it.
#[test]
fn test_deep_fork_set_and_clear() {
    let store = setup_store();

    // Initially no deep fork
    assert!(!store.has_unresolved_deep_fork().unwrap());
    assert!(store.get_deep_fork_info().unwrap().is_none());

    // Set deep fork
    let fork_info = DeepForkInfo {
        db_tip: 50_000,
        db_tip_hash: vec![0xdd; 32],
        chain_tip: 50_010,
        chain_tip_hash: vec![0xee; 32],
        depth: 10,
        fork_point: 49_990,
    };
    store.set_deep_fork(fork_info).unwrap();

    // Verify detected
    assert!(store.has_unresolved_deep_fork().unwrap());
    assert!(store.get_deep_fork_info().unwrap().is_some());

    // Clear deep fork
    store.clear_deep_fork().unwrap();

    // Verify cleared
    assert!(!store.has_unresolved_deep_fork().unwrap());
    assert!(store.get_deep_fork_info().unwrap().is_none());
}

/// Set deep fork info with specific fields and verify round-trip fidelity.
#[test]
fn test_deep_fork_info_round_trip() {
    let store = setup_store();

    let fork_info = DeepForkInfo {
        db_tip: 100_000,
        db_tip_hash: vec![0x11; 32],
        chain_tip: 100_050,
        chain_tip_hash: vec![0x22; 32],
        depth: 50,
        fork_point: 99_950,
    };

    store.set_deep_fork(fork_info).unwrap();

    let retrieved = store.get_deep_fork_info().unwrap();
    assert!(retrieved.is_some());
    let info = retrieved.unwrap();
    assert_eq!(info.db_tip, 100_000);
    assert_eq!(info.db_tip_hash, vec![0x11; 32]);
    assert_eq!(info.chain_tip, 100_050);
    assert_eq!(info.chain_tip_hash, vec![0x22; 32]);
    assert_eq!(info.depth, 50);
    assert_eq!(info.fork_point, 99_950);

    // Also verify sync status reflects the fork
    let sync = store.get_sync_status().unwrap();
    assert!(sync.deep_fork_detected);
    assert!(sync.deep_fork_info.is_some());
    let embedded = sync.deep_fork_info.unwrap();
    assert_eq!(embedded.depth, 50);
    assert_eq!(embedded.fork_point, 99_950);
}
