//! Integration tests for crash recovery and consistency detection via ckbadger-store.
//!
//! These tests verify that partial writes (e.g., block header written but txs missing)
//! can be detected and corrected by rolling back to the last consistent block.

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::CkbadgerStore;
use ckbadger_store::{CachedBlockHeader, RollbackResult, TxIndexEntry};
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
        parent_hash: vec![0u8; 32],
        timestamp: 1_600_000_000_000 + block_num * 8000,
        epoch_number: block_num / 1800,
        epoch_index: (block_num % 1800) as i32,
        epoch_length: 1800,
        dao: vec![0u8; 32],
        transactions_count: 3,
        cycles: None,
    }
}

fn make_tx_entry(is_cellbase: bool) -> TxIndexEntry {
    TxIndexEntry {
        is_cellbase,
        timestamp: 1_600_000_000,
        inputs_count: if is_cellbase { 0 } else { 1 },
        outputs_count: 2,
        fee: if is_cellbase { 0 } else { 500 },
        tx_size: 256,
        cycles: Some(50_000),
    }
}

/// Insert a complete block: header + transaction index entries.
fn insert_complete_block(store: &CkbadgerStore, block_num: i64) {
    let header = make_header(block_num);
    let cellbase = make_tx_entry(true);
    let tx1 = make_tx_entry(false);
    let tx2 = make_tx_entry(false);

    let mut batch = StoreBatch::new(store);
    batch.put_block_header(block_num, &header);
    batch.put_tx_index(block_num, 0, &cellbase);
    batch.put_tx_index(block_num, 1, &tx1);
    batch.put_tx_index(block_num, 2, &tx2);

    // Also store tx hash mappings so they can be looked up
    let mut cb_hash = vec![0u8; 32];
    cb_hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    cb_hash[8] = 0xC0;
    batch.put_tx_hash_map(&cb_hash, block_num, 0);

    let mut tx1_hash = vec![0u8; 32];
    tx1_hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    tx1_hash[8] = 0x01;
    batch.put_tx_hash_map(&tx1_hash, block_num, 1);

    let mut tx2_hash = vec![0u8; 32];
    tx2_hash[0..8].copy_from_slice(&block_num.to_le_bytes());
    tx2_hash[8] = 0x02;
    batch.put_tx_hash_map(&tx2_hash, block_num, 2);

    batch.commit().unwrap();
}

/// Insert only a block header (simulates a crash mid-write).
fn insert_header_only(store: &CkbadgerStore, block_num: i64) {
    let header = make_header(block_num);
    let mut batch = StoreBatch::new(store);
    batch.put_block_header(block_num, &header);
    batch.commit().unwrap();
}

#[test]
fn test_detect_partial_block_header_only() {
    let store = setup_store();

    // Insert complete blocks 1-5
    for i in 1..=5 {
        insert_complete_block(&store, i);
    }

    // Insert only the header for block 6 (simulating a crash after header write)
    insert_header_only(&store, 6);

    // Block 6 header exists
    let header = store.get_block_header(6).unwrap();
    assert!(header.is_some(), "block 6 header should exist");

    // But block 6 has no transactions — this is the inconsistency
    let txs = store.list_block_txs(6).unwrap();
    assert!(txs.is_empty(), "block 6 should have no txs (partial write)");

    // For comparison, block 5 is complete
    let txs_5 = store.list_block_txs(5).unwrap();
    assert_eq!(txs_5.len(), 3, "block 5 should have 3 txs (complete block)");

    // Detection logic: iterate from tip downward, find first block where
    // header.transactions_count matches actual tx count
    let header_6 = store.get_block_header(6).unwrap().unwrap();
    assert_eq!(header_6.transactions_count, 3, "header says 3 txs");
    assert_eq!(txs.len(), 0, "but 0 txs actually stored => inconsistent");
}

#[test]
fn test_rollback_restores_consistency() {
    let (store, append_store) = setup_split_stores();

    // Insert complete blocks 1-5
    for i in 1..=5 {
        insert_complete_block(&store, i);
    }

    // Insert partial block 6 (header only)
    insert_header_only(&store, 6);

    // Confirm inconsistency
    assert!(store.get_block_header(6).unwrap().is_some());
    assert!(store.list_block_txs(6).unwrap().is_empty());

    // Rollback to block 5 to restore consistency
    let result: RollbackResult = store
        .rollback_to_block_with_append_only_store(5, Some(append_store.as_ref()))
        .unwrap();
    assert_eq!(
        result.blocks_removed, 1,
        "only block 6 header should be removed"
    );

    // Block 6 header should now be gone
    assert!(
        store.get_block_header(6).unwrap().is_none(),
        "block 6 header should be removed after rollback"
    );

    // Blocks 1-5 should be intact
    for i in 1..=5 {
        assert!(
            store.get_block_header(i).unwrap().is_some(),
            "block {} should survive rollback",
            i
        );
        let txs = store.list_block_txs(i).unwrap();
        assert_eq!(
            txs.len(),
            3,
            "block {} should still have 3 txs after rollback",
            i
        );
    }
}

#[test]
fn test_sync_tip_block_returns_latest() {
    let store = setup_store();

    // Empty store: no tip
    let tip = store.get_sync_tip_block().unwrap();
    assert!(tip.is_none(), "empty store should have no sync tip");

    // Insert blocks 1-5
    for i in 1..=5 {
        insert_complete_block(&store, i);
    }

    // Sync tip should be block 5
    let (tip_num, tip_header) = store.get_sync_tip_block().unwrap().unwrap();
    assert_eq!(tip_num, 5, "sync tip should be block 5");
    assert_eq!(tip_header.transactions_count, 3);

    // The hash should match what we generated for block 5
    let expected_header = make_header(5);
    assert_eq!(tip_header.hash, expected_header.hash);

    // Add block 6
    insert_complete_block(&store, 6);
    let (tip_num, _) = store.get_sync_tip_block().unwrap().unwrap();
    assert_eq!(tip_num, 6, "sync tip should now be block 6");
}
