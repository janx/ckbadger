//! Integration tests for chain reorganization (rollback) handling via ckbadger-store.

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{ActivityEntry, UndoLogEntry, UndoLogStoreTarget};
use ckbadger_store::CkbadgerStore;
use ckbadger_store::{
    keys, CachedBlockHeader, DeepForkInfo, LiveCellInfo, RollbackResult, TxIndexEntry,
    CF_ACTIVITIES,
};
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap());
    std::mem::forget(dir);
    store
}

fn setup_split_stores() -> (Arc<CkbadgerStore>, Arc<CkbadgerStore>) {
    let domain_dir = tempfile::tempdir().unwrap();
    let append_dir = tempfile::tempdir().unwrap();
    let domain = Arc::new(CkbadgerStore::open_domain(domain_dir.path()).unwrap());
    let append = Arc::new(CkbadgerStore::open_append_only(append_dir.path()).unwrap());
    std::mem::forget(domain_dir);
    std::mem::forget(append_dir);
    (domain, append)
}

fn make_header(block_num: i64) -> CachedBlockHeader {
    let mut hash = vec![0u8; 32];
    hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    CachedBlockHeader {
        hash,
        timestamp: 1_000_000 + block_num * 1000,
        epoch_number: block_num / 1800,
        epoch_index: (block_num % 1800) as i32,
        epoch_length: 1800,
        dao: vec![0u8; 32],
        transactions_count: 2,
    }
}

fn make_tx_entry(is_cellbase: bool) -> TxIndexEntry {
    TxIndexEntry {
        is_cellbase,
        timestamp: 1_000_000,
        inputs_count: if is_cellbase { 0 } else { 2 },
        outputs_count: 3,
        fee: if is_cellbase { 0 } else { 1000 },
        tx_size: 512,
        cycles: Some(100_000),
    }
}

fn make_cell(block_num: i64, lock_hash: &[u8]) -> LiveCellInfo {
    LiveCellInfo {
        capacity: 10_000_000_000,
        created_at_block: block_num,
        lock_script_hash: lock_hash.to_vec(),
        lock_code_hash: vec![0xAA; 32],
        lock_hash_type: 1,
        lock_args: vec![0xBB; 20],
        type_script_hash: None,
        type_code_hash: None,
        type_args: None,
        data_size: 0,
        occupied_capacity: 0,
        udt_amount: None,
    }
}

/// Insert a fully populated block (header + txs + cells + indexes).
fn insert_full_block(store: &CkbadgerStore, block_num: i64, lock_hash: &[u8]) {
    let header = make_header(block_num);
    let cellbase = make_tx_entry(true);
    let normal_tx = make_tx_entry(false);

    let mut cellbase_hash = vec![0u8; 32];
    cellbase_hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    cellbase_hash[8] = 0x00; // tx index 0

    let mut tx_hash = vec![0u8; 32];
    tx_hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    tx_hash[8] = 0x01; // tx index 1

    let cell = make_cell(block_num, lock_hash);

    let mut batch = StoreBatch::new(store);
    batch.put_block_header(block_num, &header);
    batch.put_tx_index(block_num, 0, &cellbase);
    batch.put_tx_index(block_num, 1, &normal_tx);
    batch.put_tx_hash_map(&cellbase_hash, block_num, 0);
    batch.put_tx_hash_map(&tx_hash, block_num, 1);
    batch.put_cell(&tx_hash, 0, &cell);
    batch.put_cell_by_lock(lock_hash, block_num, &tx_hash, 0);
    batch.commit().unwrap();
}

#[test]
fn test_rollback_removes_blocks() {
    let (store, append_store) = setup_split_stores();
    let lock_hash = vec![0xFF; 32];

    // Insert blocks 1 through 10
    for i in 1..=10 {
        insert_full_block(&store, i, &lock_hash);
    }

    // Verify all 10 blocks exist
    for i in 1..=10 {
        assert!(
            store.get_block_header(i).unwrap().is_some(),
            "block {} should exist before rollback",
            i
        );
    }

    // Rollback to block 5: blocks 6-10 should be removed
    let result: RollbackResult = store
        .rollback_to_block_with_tx_index_store(5, Some(append_store.as_ref()))
        .unwrap();
    assert_eq!(result.blocks_removed, 5, "should remove blocks 6-10");

    // Blocks 1-5 should still exist
    for i in 1..=5 {
        assert!(
            store.get_block_header(i).unwrap().is_some(),
            "block {} should survive rollback",
            i
        );
    }

    // Blocks 6-10 should be gone
    for i in 6..=10 {
        assert!(
            store.get_block_header(i).unwrap().is_none(),
            "block {} should be removed after rollback",
            i
        );
    }
}

#[test]
fn test_rollback_removes_transactions() {
    let (store, append_store) = setup_split_stores();
    let lock_hash = vec![0xEE; 32];

    // Insert blocks 1 through 6
    for i in 1..=6 {
        insert_full_block(&store, i, &lock_hash);
    }

    // Verify txs in block 6 exist
    let txs = store.list_block_txs(6).unwrap();
    assert_eq!(txs.len(), 2, "block 6 should have 2 txs before rollback");

    // Rollback to block 3
    let result = store
        .rollback_to_block_with_tx_index_store(3, Some(append_store.as_ref()))
        .unwrap();
    // Blocks 4, 5, 6 removed => 3 blocks, each with 2 txs => 6 txs removed
    assert_eq!(result.blocks_removed, 3);
    assert_eq!(result.txs_removed, 6);

    // Block 6 txs should be gone
    let txs = store.list_block_txs(6).unwrap();
    assert_eq!(txs.len(), 0, "block 6 txs should be removed");

    // Block 3 txs should survive
    let txs = store.list_block_txs(3).unwrap();
    assert_eq!(txs.len(), 2, "block 3 txs should survive");
}

#[test]
fn test_rollback_removes_cells_and_indexes() {
    let (store, append_store) = setup_split_stores();
    let lock_hash = vec![0xDD; 32];

    // Insert blocks 1 through 4
    for i in 1..=4 {
        insert_full_block(&store, i, &lock_hash);
    }

    // Verify cells exist via lock index (4 blocks, 1 cell each)
    let cells_before = store.list_cells_by_lock(&lock_hash, 100, None).unwrap();
    assert_eq!(cells_before.len(), 4, "should have 4 cells before rollback");

    // Rollback to block 2: blocks 3-4 removed
    let result = store
        .rollback_to_block_with_tx_index_store(2, Some(append_store.as_ref()))
        .unwrap();
    assert_eq!(result.blocks_removed, 2);
    assert_eq!(result.cells_removed, 2, "cells from blocks 3-4 removed");

    // Cells from blocks 1-2 should survive via lock index
    let cells_after = store.list_cells_by_lock(&lock_hash, 100, None).unwrap();
    assert_eq!(
        cells_after.len(),
        2,
        "should have 2 cells after rollback (blocks 1-2)"
    );
}

#[test]
fn test_rollback_result_counts() {
    let (store, append_store) = setup_split_stores();
    let lock_hash = vec![0xCC; 32];

    // Insert blocks 1 through 8
    for i in 1..=8 {
        insert_full_block(&store, i, &lock_hash);
    }

    // Rollback to block 5: remove blocks 6, 7, 8
    let result = store
        .rollback_to_block_with_tx_index_store(5, Some(append_store.as_ref()))
        .unwrap();

    assert_eq!(result.blocks_removed, 3, "3 blocks removed (6, 7, 8)");
    assert_eq!(
        result.txs_removed, 6,
        "6 txs removed (2 per block * 3 blocks)"
    );
    assert_eq!(
        result.cells_removed, 3,
        "3 cells removed (1 per block * 3 blocks)"
    );
    // cells_restored depends on consumed cell history, which we don't
    // populate in this test, so it stays 0
    assert_eq!(result.cells_restored, 0);
}

#[test]
fn test_deep_fork_flag() {
    let store = setup_store();

    // Initially no deep fork
    assert!(!store.has_unresolved_deep_fork().unwrap());

    // Set a deep fork
    let fork_info = DeepForkInfo {
        db_tip: 1000,
        db_tip_hash: vec![0x11; 32],
        chain_tip: 995,
        chain_tip_hash: vec![0x22; 32],
        depth: 12,
        fork_point: 988,
    };
    store.set_deep_fork(fork_info).unwrap();

    assert!(store.has_unresolved_deep_fork().unwrap());

    let info = store.get_deep_fork_info().unwrap().unwrap();
    assert_eq!(info.db_tip, 1000);
    assert_eq!(info.depth, 12);
    assert_eq!(info.fork_point, 988);

    // Clear deep fork
    store.clear_deep_fork().unwrap();
    assert!(!store.has_unresolved_deep_fork().unwrap());
}

#[test]
fn test_rollback_preserves_activities_history() {
    let root = tempfile::tempdir().unwrap();
    let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
    let append = CkbadgerStore::open_append_only(root.path().join("append-only")).unwrap();
    let lock_hash = vec![0xAA; 32];

    // Insert activities at blocks 100, 200, 300, 400, 500
    let mut append_batch = StoreBatch::new(&append);
    let mut domain_batch = StoreBatch::new(&domain);
    for (rollback_seq, i) in (1..=5).enumerate() {
        let block = i * 100;
        let entry = ActivityEntry {
            tx_hash: vec![i as u8; 32],
            block_number: block,
            tx_index: 0,
            timestamp: 1_700_000_000 + block,
            ckb_delta: block as i128 * 100_000_000,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![],
            peers: vec![],
        };
        append_batch.put_activity(&lock_hash, block, 0, &entry);
        let activity_key = keys::encode_activity_key(&lock_hash, block, 0, &entry.tx_hash);
        domain_batch.put_reorg_undo_log_by_block(
            block,
            rollback_seq as u64,
            &UndoLogEntry::KeyMutation {
                target_store: UndoLogStoreTarget::AppendOnly,
                cf_name: CF_ACTIVITIES.to_string(),
                key: activity_key,
                previous_value: None,
            },
        );
    }
    append_batch.commit().unwrap();
    domain_batch.commit().unwrap();

    // Verify all 5 exist
    let before = append.list_activities(&lock_hash, 100, None, None).unwrap();
    assert_eq!(before.len(), 5);

    // Rollback to block 300: append-only activities history is preserved.
    // Need to also insert block headers so rollback_to_block works
    let mut batch = StoreBatch::new(&domain);
    for i in 1..=5 {
        let block = i * 100;
        batch.put_block_header(block, &make_header(block));
    }
    batch.commit().unwrap();

    domain
        .rollback_to_block_with_tx_index_store(300, Some(&append))
        .unwrap();
    domain.rollback_via_undo_log(&append, 300).unwrap();

    let after = append.list_activities(&lock_hash, 100, None, None).unwrap();
    assert_eq!(after.len(), 5, "append-only history should be preserved");
    // Activities remain in descending block order.
    assert_eq!(after[0].0, 500);
    assert_eq!(after[1].0, 400);
    assert_eq!(after[2].0, 300);
    assert_eq!(after[3].0, 200);
    assert_eq!(after[4].0, 100);
}
