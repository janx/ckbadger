//! Integration tests for address balance operations in ckbadger-store.
//!
//! Tests balance insertion, updates, cumulative modifications, and
//! top_addresses ordering.

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::AddressBalance;
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap());
    std::mem::forget(dir);
    store
}

#[allow(clippy::too_many_arguments)]
fn make_balance(
    balance: i128,
    live_cells: i32,
    total_cells: i64,
    txs: i64,
    first_block: i64,
    first_tx_byte: u8,
    last_block: i64,
    last_tx_byte: u8,
) -> AddressBalance {
    AddressBalance {
        balance,
        occupied_capacity: 0,
        live_cells_count: live_cells,
        total_cells_count: total_cells,
        txs_count: txs,
        first_seen_block: first_block,
        first_seen_tx: vec![first_tx_byte; 32],
        last_activity_block: last_block,
        last_activity_tx: vec![last_tx_byte; 32],
    }
}

#[test]
fn test_receive_cells_balance_increases() {
    let store = setup_store();
    let lock_hash = vec![0xAAu8; 32];

    let balance = make_balance(100_00000000, 1, 1, 1, 1000, 0x01, 1000, 0x01);

    let mut batch = StoreBatch::new(&store);
    batch.put_addr_balance(&lock_hash, &balance);
    batch.commit().unwrap();

    let retrieved = store.get_addr_balance(&lock_hash).unwrap();
    assert!(retrieved.is_some(), "balance should exist after insertion");

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.balance, 100_00000000);
    assert_eq!(retrieved.live_cells_count, 1);
    assert_eq!(retrieved.total_cells_count, 1);
    assert_eq!(retrieved.txs_count, 1);
    assert_eq!(retrieved.first_seen_block, 1000);
    assert_eq!(retrieved.first_seen_tx, vec![0x01u8; 32]);
    assert_eq!(retrieved.last_activity_block, 1000);
}

#[test]
fn test_send_cells_balance_decreases() {
    let store = setup_store();
    let lock_hash = vec![0xBBu8; 32];

    // Initial balance: received 100 CKB
    let initial = make_balance(100_00000000, 1, 1, 1, 1000, 0x01, 1000, 0x01);
    let mut batch = StoreBatch::new(&store);
    batch.put_addr_balance(&lock_hash, &initial);
    batch.commit().unwrap();

    // Simulate spending: balance decreases to 30 CKB
    let after_send = make_balance(30_00000000, 0, 2, 2, 1000, 0x01, 2000, 0x02);
    let mut batch = StoreBatch::new(&store);
    batch.put_addr_balance(&lock_hash, &after_send);
    batch.commit().unwrap();

    let retrieved = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(retrieved.balance, 30_00000000);
    assert_eq!(retrieved.live_cells_count, 0);
    assert_eq!(retrieved.total_cells_count, 2);
    assert_eq!(retrieved.txs_count, 2);
    assert_eq!(retrieved.last_activity_block, 2000);
    assert_eq!(retrieved.last_activity_tx, vec![0x02u8; 32]);
}

#[test]
fn test_multiple_operations_cumulative() {
    let store = setup_store();
    let lock_hash = vec![0xCCu8; 32];

    // Step 1: receive 50 CKB
    let step1 = make_balance(50_00000000, 1, 1, 1, 100, 0x01, 100, 0x01);
    let mut batch = StoreBatch::new(&store);
    batch.put_addr_balance(&lock_hash, &step1);
    batch.commit().unwrap();

    let r1 = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(r1.balance, 50_00000000);

    // Step 2: receive another 75 CKB (cumulative = 125)
    let step2 = make_balance(125_00000000, 2, 2, 2, 100, 0x01, 200, 0x02);
    let mut batch = StoreBatch::new(&store);
    batch.put_addr_balance(&lock_hash, &step2);
    batch.commit().unwrap();

    let r2 = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(r2.balance, 125_00000000);
    assert_eq!(r2.live_cells_count, 2);

    // Step 3: spend 40 CKB (cumulative = 85)
    let step3 = make_balance(85_00000000, 1, 3, 3, 100, 0x01, 300, 0x03);
    let mut batch = StoreBatch::new(&store);
    batch.put_addr_balance(&lock_hash, &step3);
    batch.commit().unwrap();

    let r3 = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(r3.balance, 85_00000000);
    assert_eq!(r3.live_cells_count, 1);
    assert_eq!(r3.total_cells_count, 3);
    assert_eq!(r3.txs_count, 3);
    assert_eq!(r3.first_seen_block, 100);
    assert_eq!(r3.last_activity_block, 300);
}

#[test]
fn test_top_addresses_ordering() {
    let store = setup_store();

    let addr1 = vec![0x01u8; 32]; // 500 CKB - should be rank 2
    let addr2 = vec![0x02u8; 32]; // 1000 CKB - should be rank 1
    let addr3 = vec![0x03u8; 32]; // 100 CKB - should be rank 4
    let addr4 = vec![0x04u8; 32]; // 250 CKB - should be rank 3

    let bal1 = make_balance(500_00000000, 3, 5, 5, 100, 0x01, 500, 0x10);
    let bal2 = make_balance(1000_00000000, 5, 10, 10, 50, 0x02, 600, 0x20);
    let bal3 = make_balance(100_00000000, 1, 2, 2, 200, 0x03, 300, 0x30);
    let bal4 = make_balance(250_00000000, 2, 3, 3, 150, 0x04, 400, 0x40);

    let mut batch = StoreBatch::new(&store);
    batch.put_addr_balance(&addr1, &bal1);
    batch.put_addr_balance(&addr2, &bal2);
    batch.put_addr_balance(&addr3, &bal3);
    batch.put_addr_balance(&addr4, &bal4);
    batch.commit().unwrap();

    // Request top 4
    let top = store.top_addresses(4).unwrap();
    assert_eq!(top.len(), 4);

    // Verify descending balance order
    assert_eq!(top[0].1.balance, 1000_00000000);
    assert_eq!(top[0].0, addr2);

    assert_eq!(top[1].1.balance, 500_00000000);
    assert_eq!(top[1].0, addr1);

    assert_eq!(top[2].1.balance, 250_00000000);
    assert_eq!(top[2].0, addr4);

    assert_eq!(top[3].1.balance, 100_00000000);
    assert_eq!(top[3].0, addr3);

    // Verify limit truncation
    let top2 = store.top_addresses(2).unwrap();
    assert_eq!(top2.len(), 2);
    assert_eq!(top2[0].1.balance, 1000_00000000);
    assert_eq!(top2[1].1.balance, 500_00000000);
}

#[test]
fn test_occupied_capacity_stored_and_retrieved() {
    let store = setup_store();
    let lock_hash = vec![0xDDu8; 32];

    let balance = AddressBalance {
        balance: 200_00000000,
        occupied_capacity: 6100_00000000,
        live_cells_count: 2,
        total_cells_count: 2,
        txs_count: 1,
        first_seen_block: 1000,
        first_seen_tx: vec![0x01; 32],
        last_activity_block: 1000,
        last_activity_tx: vec![0x01; 32],
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_addr_balance(&lock_hash, &balance);
    batch.commit().unwrap();

    let retrieved = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(retrieved.balance, 200_00000000);
    assert_eq!(retrieved.occupied_capacity, 6100_00000000);
    assert_eq!(retrieved.live_cells_count, 2);
}

#[test]
fn test_occupied_capacity_zero_by_default() {
    let store = setup_store();
    let lock_hash = vec![0xEEu8; 32];

    // Simulate pre-existing data that was created before occupied_capacity was added:
    // just use the Default-derived zero value.
    let balance = make_balance(100_00000000, 1, 1, 1, 100, 0x01, 100, 0x01);

    let mut batch = StoreBatch::new(&store);
    batch.put_addr_balance(&lock_hash, &balance);
    batch.commit().unwrap();

    let retrieved = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(
        retrieved.occupied_capacity, 0,
        "occupied_capacity should default to 0"
    );
}
