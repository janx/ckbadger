//! Integration tests for live cell lifecycle in ckbadger-store.
//!
//! Tests cell insertion, consumption, batch operations, and prefix scans
//! via lock and type script indexes.

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::CkbadgerStore;
use ckbadger_store::LiveCellInfo;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap());
    std::mem::forget(dir);
    store
}

fn make_live_cell(
    capacity: i64,
    block: i64,
    lock_hash_byte: u8,
    type_hash_byte: Option<u8>,
    data_size: i32,
) -> LiveCellInfo {
    LiveCellInfo {
        capacity,
        created_at_block: block,
        lock_script_hash: vec![lock_hash_byte; 32],
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 1,
        lock_args: vec![0x22u8; 20],
        type_script_hash: type_hash_byte.map(|b| vec![b; 32]),
        type_code_hash: type_hash_byte.map(|_| vec![0x44u8; 32]),
        data_size,
    }
}

#[test]
fn test_insert_cell_visible_in_live_cells() {
    let store = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell = make_live_cell(100_00000000, 1000, 0xAA, Some(0xBB), 256);

    let mut batch = StoreBatch::new(&store);
    batch.put_cell(&tx_hash, 0, &cell);
    batch.commit().unwrap();

    // Verify the cell exists in live_cells via raw get_cf
    let key = keys::encode_outpoint(&tx_hash, 0);
    let raw = store.get_cf(store.cf_live_cells(), &key).unwrap();
    assert!(raw.is_some(), "cell should be present in live_cells CF");

    let decoded: LiveCellInfo = bincode::deserialize(&raw.unwrap()).unwrap();
    assert_eq!(decoded.capacity, 100_00000000);
    assert_eq!(decoded.created_at_block, 1000);
    assert_eq!(decoded.lock_script_hash, vec![0xAAu8; 32]);
    assert_eq!(decoded.data_size, 256);
}

#[test]
fn test_consume_cell_moves_to_consumed() {
    let store = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell = make_live_cell(200_00000000, 500, 0xCC, None, 128);

    // Insert into live_cells
    let mut batch = StoreBatch::new(&store);
    batch.put_cell(&tx_hash, 0, &cell);
    batch.commit().unwrap();

    // Verify it exists in live_cells
    let live_result = store.get_cell(&tx_hash, 0).unwrap();
    assert!(live_result.is_some(), "cell should be live after insertion");

    // Consume: move to consumed_cells and delete from live_cells
    let mut batch = StoreBatch::new(&store);
    batch.put_consumed_cell(&tx_hash, 0, &cell);
    batch.delete_cell(&tx_hash, 0);
    batch.commit().unwrap();

    // Verify live_cells no longer has it
    let live_after = store.get_cell(&tx_hash, 0).unwrap();
    assert!(
        live_after.is_none(),
        "cell should no longer be in live_cells after consumption"
    );

    // Verify consumed_cells has it
    let consumed = store.get_consumed_cell(&tx_hash, 0).unwrap();
    assert!(
        consumed.is_some(),
        "cell should be present in consumed_cells"
    );
    let consumed = consumed.unwrap();
    assert_eq!(consumed.capacity, 200_00000000);
    assert_eq!(consumed.created_at_block, 500);
}

#[test]
fn test_batch_insert_multiple_cells() {
    let store = setup_store();
    let tx1 = vec![0x01u8; 32];
    let tx2 = vec![0x02u8; 32];
    let tx3 = vec![0x03u8; 32];

    let cell1 = make_live_cell(100_00000000, 1000, 0xAA, None, 100);
    let cell2 = make_live_cell(200_00000000, 2000, 0xBB, None, 200);
    let cell3 = make_live_cell(300_00000000, 3000, 0xCC, None, 300);

    // Insert all three in a single batch
    let mut batch = StoreBatch::new(&store);
    batch.put_cell(&tx1, 0, &cell1);
    batch.put_cell(&tx2, 0, &cell2);
    batch.put_cell(&tx3, 0, &cell3);
    batch.commit().unwrap();

    // Verify all three exist
    let r1 = store.get_cell(&tx1, 0).unwrap();
    assert!(r1.is_some());
    assert_eq!(r1.unwrap().capacity, 100_00000000);

    let r2 = store.get_cell(&tx2, 0).unwrap();
    assert!(r2.is_some());
    assert_eq!(r2.unwrap().capacity, 200_00000000);

    let r3 = store.get_cell(&tx3, 0).unwrap();
    assert!(r3.is_some());
    assert_eq!(r3.unwrap().capacity, 300_00000000);
}

#[test]
fn test_list_cells_by_lock_prefix_scan() {
    let store = setup_store();
    let lock_hash = vec![0xAAu8; 32];

    let tx1 = vec![0x01u8; 32];
    let tx2 = vec![0x02u8; 32];
    let tx3 = vec![0x03u8; 32];

    let cell1 = make_live_cell(100_00000000, 1000, 0xAA, None, 50);
    let cell2 = make_live_cell(200_00000000, 2000, 0xAA, None, 60);
    let cell3 = make_live_cell(300_00000000, 3000, 0xAA, None, 70);

    let mut batch = StoreBatch::new(&store);
    // Insert cells into live_cells
    batch.put_cell(&tx1, 0, &cell1);
    batch.put_cell(&tx2, 0, &cell2);
    batch.put_cell(&tx3, 0, &cell3);
    // Insert lock index entries
    batch.put_cell_by_lock(&lock_hash, 1000, &tx1, 0);
    batch.put_cell_by_lock(&lock_hash, 2000, &tx2, 0);
    batch.put_cell_by_lock(&lock_hash, 3000, &tx3, 0);
    batch.commit().unwrap();

    let results = store.list_cells_by_lock(&lock_hash, 100).unwrap();
    assert_eq!(results.len(), 3, "should find all 3 cells by lock hash");

    // Results should be ordered by block_num (ascending from prefix iterator)
    assert_eq!(results[0].2.capacity, 100_00000000);
    assert_eq!(results[1].2.capacity, 200_00000000);
    assert_eq!(results[2].2.capacity, 300_00000000);

    // Verify limit works
    let limited = store.list_cells_by_lock(&lock_hash, 2).unwrap();
    assert_eq!(limited.len(), 2, "limit should restrict result count");
}

#[test]
fn test_list_cells_by_type_prefix_scan() {
    let store = setup_store();
    let type_hash = vec![0xBBu8; 32];

    let tx1 = vec![0x10u8; 32];
    let tx2 = vec![0x20u8; 32];

    let cell1 = make_live_cell(150_00000000, 500, 0xCC, Some(0xBB), 80);
    let cell2 = make_live_cell(250_00000000, 600, 0xDD, Some(0xBB), 90);

    let mut batch = StoreBatch::new(&store);
    batch.put_cell(&tx1, 0, &cell1);
    batch.put_cell(&tx2, 1, &cell2);
    batch.put_cell_by_type(&type_hash, 500, &tx1, 0);
    batch.put_cell_by_type(&type_hash, 600, &tx2, 1);
    batch.commit().unwrap();

    let results = store.list_cells_by_type(&type_hash, 100).unwrap();
    assert_eq!(results.len(), 2, "should find both cells by type hash");

    assert_eq!(results[0].2.capacity, 150_00000000);
    assert_eq!(results[1].2.capacity, 250_00000000);

    // Verify consumed cells are not returned by type prefix scan
    let mut batch = StoreBatch::new(&store);
    batch.put_consumed_cell(&tx1, 0, &cell1);
    batch.delete_cell(&tx1, 0);
    batch.delete_cell_by_type(&type_hash, 500, &tx1, 0);
    batch.commit().unwrap();

    let after_consume = store.list_cells_by_type(&type_hash, 100).unwrap();
    assert_eq!(
        after_consume.len(),
        1,
        "consumed cell should not appear in type prefix scan"
    );
    assert_eq!(after_consume[0].2.capacity, 250_00000000);
}
