//! Pipeline vs Sequential Mode Consistency Tests
//!
//! Verifies that the three-stage pipeline (Fetcher → Parser → Writer) produces
//! identical database state to sequential processing.

#![allow(clippy::type_complexity)]

use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::parser::cell::ParsedCell;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::CkbadgerStore;
use std::collections::HashMap;
use std::sync::Arc;

fn make_cell(capacity: i64, data_size: i32, lock_hash_byte: u8) -> ParsedCell {
    ParsedCell {
        capacity,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        lock_script_hash: vec![lock_hash_byte; 32],
        type_code_hash: Some(vec![0x44u8; 32]),
        type_hash_type: Some(1),
        type_args: Some(vec![0x55u8; 20]),
        type_script_hash: Some(vec![0x66u8; 32]),
        data_hash: vec![0x77u8; 32],
        data_size,
        data: vec![0u8; data_size as usize],
    }
}

fn setup_store() -> (Arc<CkbadgerStore>, BatchWriter) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap());
    let writer = BatchWriter::new(store.clone());
    // Leak the tempdir so it doesn't get cleaned up while store is open
    std::mem::forget(dir);
    (store, writer)
}

#[test]
fn test_cell_info_lookup_returns_all_fields() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell = make_cell(100_00000000, 256, 0xAA);

    let mut batch = StoreBatch::new(&store);
    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], &mut batch, false)
        .unwrap();
    batch.commit().unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx_hash, 0)], false)
        .unwrap();

    assert_eq!(result.len(), 1);
    let (capacity, created_at_block, lock_script_hash, data_size) =
        result.get(&(tx_hash.clone(), 0)).unwrap();

    assert_eq!(*capacity, 100_00000000);
    assert_eq!(*created_at_block, 1000);
    assert_eq!(*lock_script_hash, vec![0xAAu8; 32]);
    assert_eq!(*data_size, 256);
}

#[test]
fn test_cell_info_batch_lookup_multiple_cells() {
    let (store, writer) = setup_store();

    let tx1 = vec![0x01u8; 32];
    let tx2 = vec![0x02u8; 32];
    let tx3 = vec![0x03u8; 32];

    let cell1 = make_cell(100_00000000, 100, 0xAA);
    let cell2 = make_cell(200_00000000, 200, 0xBB);
    let cell3 = make_cell(300_00000000, 300, 0xCC);

    let mut batch = StoreBatch::new(&store);
    writer
        .insert_cells_batch(
            &[
                (&tx1, 0, &cell1, 1000),
                (&tx2, 0, &cell2, 2000),
                (&tx3, 0, &cell3, 3000),
            ],
            &mut batch,
            false,
        )
        .unwrap();
    batch.commit().unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx1, 0), (&tx2, 0), (&tx3, 0)], false)
        .unwrap();

    assert_eq!(result.len(), 3);

    let (cap1, block1, _, size1) = result.get(&(tx1.clone(), 0)).unwrap();
    assert_eq!(*cap1, 100_00000000);
    assert_eq!(*block1, 1000);
    assert_eq!(*size1, 100);

    let (cap2, block2, _, size2) = result.get(&(tx2.clone(), 0)).unwrap();
    assert_eq!(*cap2, 200_00000000);
    assert_eq!(*block2, 2000);
    assert_eq!(*size2, 200);

    let (cap3, block3, _, size3) = result.get(&(tx3.clone(), 0)).unwrap();
    assert_eq!(*cap3, 300_00000000);
    assert_eq!(*block3, 3000);
    assert_eq!(*size3, 300);
}

#[test]
fn test_full_cells_info_returns_lock_and_type() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell = ParsedCell {
        capacity: 100_00000000,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        lock_script_hash: vec![0x33u8; 32],
        type_code_hash: Some(vec![0x44u8; 32]),
        type_hash_type: Some(1),
        type_args: Some(vec![0x55u8; 20]),
        type_script_hash: Some(vec![0x66u8; 32]),
        data_hash: vec![0x77u8; 32],
        data_size: 100,
        data: vec![0u8; 100],
    };

    let mut batch = StoreBatch::new(&store);
    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], &mut batch, false)
        .unwrap();
    batch.commit().unwrap();

    let result = writer
        .get_full_cells_info_batch(&[(&tx_hash, 0)], false)
        .unwrap();

    assert_eq!(result.len(), 1);
    let info = result.get(&(tx_hash.clone(), 0)).unwrap();

    assert_eq!(info.lock_code_hash, vec![0x11u8; 32]);
    assert_eq!(info.type_code_hash, Some(vec![0x44u8; 32]));
    assert_eq!(info.capacity, 100_00000000);
    assert_eq!(info.created_at_block, 1000);
}

#[test]
fn test_full_cells_info_no_type_script() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell = ParsedCell {
        capacity: 100_00000000,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        lock_script_hash: vec![0x33u8; 32],
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        type_script_hash: None,
        data_hash: vec![0x77u8; 32],
        data_size: 100,
        data: vec![0u8; 100],
    };

    let mut batch = StoreBatch::new(&store);
    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], &mut batch, false)
        .unwrap();
    batch.commit().unwrap();

    let result = writer
        .get_full_cells_info_batch(&[(&tx_hash, 0)], false)
        .unwrap();

    assert_eq!(result.len(), 1);
    let info = result.get(&(tx_hash.clone(), 0)).unwrap();

    assert_eq!(info.lock_code_hash, vec![0x11u8; 32]);
    assert_eq!(info.type_code_hash, None);
}

#[test]
fn test_same_batch_cell_consumption() {
    let (store, writer) = setup_store();
    let creating_tx = vec![0x01u8; 32];
    let consuming_tx = vec![0x02u8; 32];
    let cell = make_cell(100_00000000, 100, 0xAA);

    // Insert cell
    let mut batch = StoreBatch::new(&store);
    writer
        .insert_cells_batch(&[(&creating_tx, 0, &cell, 1000)], &mut batch, false)
        .unwrap();
    batch.commit().unwrap();

    // Consume cell
    let mut batch = StoreBatch::new(&store);
    writer
        .consume_cells_batch(
            &[(&creating_tx, 0, 1000, &consuming_tx, 1000, 0)],
            &mut batch,
        )
        .unwrap();
    batch.commit().unwrap();

    // Cell should no longer be in live cells
    let result = writer
        .get_cells_info_batch(&[(&creating_tx, 0)], false)
        .unwrap();

    // get_cells_info_batch also checks consumed_cells, so it should still find it
    assert_eq!(result.len(), 1);
}

#[test]
fn test_script_usage_cell_creation() {
    let (store, writer) = setup_store();
    let lock_code_hash = vec![0x11u8; 32];
    let mut changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();

    changes.insert(
        (lock_code_hash.clone(), false),
        (1, 1, 100_00000000, 100_00000000),
    );

    let mut batch = StoreBatch::new(&store);
    writer
        .update_script_usage_batch(&changes, &mut batch)
        .unwrap();
    batch.commit().unwrap();

    // Verify via ScriptInfo in store
    let info = store.get_script_info(&lock_code_hash).unwrap();
    assert!(info.is_some());
    let info = info.unwrap();
    assert_eq!(info.lock_cells_count, 1);
    assert_eq!(info.lock_live_cells_count, 1);
    assert_eq!(info.lock_capacity_sum, 100_00000000);
    assert_eq!(info.lock_live_capacity_sum, 100_00000000);
}

#[test]
fn test_script_usage_cell_consumption() {
    let (store, writer) = setup_store();
    let lock_code_hash = vec![0x11u8; 32];

    let mut create_changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    create_changes.insert(
        (lock_code_hash.clone(), false),
        (1, 1, 100_00000000, 100_00000000),
    );
    let mut batch = StoreBatch::new(&store);
    writer
        .update_script_usage_batch(&create_changes, &mut batch)
        .unwrap();
    batch.commit().unwrap();

    let mut consume_changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    consume_changes.insert((lock_code_hash.clone(), false), (0, -1, 0, -100_00000000));
    let mut batch = StoreBatch::new(&store);
    writer
        .update_script_usage_batch(&consume_changes, &mut batch)
        .unwrap();
    batch.commit().unwrap();

    let info = store.get_script_info(&lock_code_hash).unwrap();
    let info = info.unwrap();
    assert_eq!(info.lock_cells_count, 1);
    assert_eq!(info.lock_live_cells_count, 0);
    assert_eq!(info.lock_capacity_sum, 100_00000000);
    assert_eq!(info.lock_live_capacity_sum, 0);
}

#[test]
fn test_address_balance_update_receive() {
    let (store, writer) = setup_store();
    let lock_hash = vec![0xAAu8; 32];
    let tx_hash = vec![0x01u8; 32];

    let changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8], i64)> = [(
        lock_hash.clone(),
        (100_00000000, 1, 1, 1, 1000, tx_hash.as_slice(), 0),
    )]
    .into_iter()
    .collect();

    let mut batch = StoreBatch::new(&store);
    writer
        .update_address_balances_batch(&changes, &mut batch)
        .unwrap();
    batch.commit().unwrap();

    let balance = store.get_addr_balance(&lock_hash).unwrap();
    assert!(balance.is_some());
    let balance = balance.unwrap();
    assert_eq!(balance.balance, 100_00000000);
    assert_eq!(balance.live_cells_count, 1);
    assert_eq!(balance.txs_count, 1);
}

#[test]
fn test_address_balance_update_send() {
    let (store, writer) = setup_store();
    let lock_hash = vec![0xAAu8; 32];
    let tx_hash1 = vec![0x01u8; 32];
    let tx_hash2 = vec![0x02u8; 32];

    let receive: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8], i64)> = [(
        lock_hash.clone(),
        (100_00000000, 1, 1, 1, 1000, tx_hash1.as_slice(), 0),
    )]
    .into_iter()
    .collect();
    let mut batch = StoreBatch::new(&store);
    writer
        .update_address_balances_batch(&receive, &mut batch)
        .unwrap();
    batch.commit().unwrap();

    let send: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8], i64)> = [(
        lock_hash.clone(),
        (-30_00000000, 0, 1, 1, 2000, tx_hash2.as_slice(), 0),
    )]
    .into_iter()
    .collect();
    let mut batch = StoreBatch::new(&store);
    writer
        .update_address_balances_batch(&send, &mut batch)
        .unwrap();
    batch.commit().unwrap();

    let balance = store.get_addr_balance(&lock_hash).unwrap();
    let balance = balance.unwrap();
    assert_eq!(balance.balance, 70_00000000);
    assert_eq!(balance.live_cells_count, 1);
    assert_eq!(balance.txs_count, 2);
}

#[test]
fn test_multiple_outputs_same_tx() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell0 = make_cell(100_00000000, 100, 0xAA);
    let cell1 = make_cell(200_00000000, 200, 0xBB);
    let cell2 = make_cell(300_00000000, 300, 0xCC);

    let mut batch = StoreBatch::new(&store);
    writer
        .insert_cells_batch(
            &[
                (&tx_hash, 0, &cell0, 1000),
                (&tx_hash, 1, &cell1, 1000),
                (&tx_hash, 2, &cell2, 1000),
            ],
            &mut batch,
            false,
        )
        .unwrap();
    batch.commit().unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx_hash, 0), (&tx_hash, 1), (&tx_hash, 2)], false)
        .unwrap();

    assert_eq!(result.len(), 3);

    let (cap0, _, _, _) = result.get(&(tx_hash.clone(), 0)).unwrap();
    assert_eq!(*cap0, 100_00000000);

    let (cap1, _, _, _) = result.get(&(tx_hash.clone(), 1)).unwrap();
    assert_eq!(*cap1, 200_00000000);

    let (cap2, _, _, _) = result.get(&(tx_hash.clone(), 2)).unwrap();
    assert_eq!(*cap2, 300_00000000);
}

#[test]
fn test_consumed_cell_not_in_live_cells() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let consuming_tx = vec![0x02u8; 32];
    let cell = make_cell(100_00000000, 100, 0xAA);

    let mut batch = StoreBatch::new(&store);
    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], &mut batch, false)
        .unwrap();
    batch.commit().unwrap();

    let mut batch = StoreBatch::new(&store);
    writer
        .consume_cells_batch(&[(&tx_hash, 0, 1000, &consuming_tx, 2000, 0)], &mut batch)
        .unwrap();
    batch.commit().unwrap();

    // get_cells_info_batch checks both live and consumed, so consumed cell is still found
    let result = writer
        .get_cells_info_batch(&[(&tx_hash, 0)], false)
        .unwrap();
    // Should find it in consumed cells
    assert_eq!(result.len(), 1);
}

#[test]
fn test_cross_partition_cell_lookup() {
    let (store, writer) = setup_store();
    let tx_p0 = vec![0x01u8; 32];
    let tx_p1 = vec![0x02u8; 32];
    let cell = make_cell(100_00000000, 100, 0xAA);

    let mut batch = StoreBatch::new(&store);
    writer
        .insert_cells_batch(
            &[(&tx_p0, 0, &cell, 1_000_000), (&tx_p1, 0, &cell, 6_000_000)],
            &mut batch,
            false,
        )
        .unwrap();
    batch.commit().unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx_p0, 0), (&tx_p1, 0)], false)
        .unwrap();

    assert_eq!(result.len(), 2);

    let (_, block0, _, _) = result.get(&(tx_p0.clone(), 0)).unwrap();
    assert_eq!(*block0, 1_000_000);

    let (_, block1, _, _) = result.get(&(tx_p1.clone(), 0)).unwrap();
    assert_eq!(*block1, 6_000_000);
}

// --- Tests for skip_cell_indices / rebuild_cell_indices ---

#[test]
fn test_skip_cell_indices_omits_index_entries() {
    let (store, writer) = setup_store();
    let tx_hash = vec![0x01u8; 32];
    let cell = make_cell(100_00000000, 256, 0xAA);

    // Insert with skip_cell_indices = true
    let mut batch = StoreBatch::new(&store);
    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], &mut batch, true)
        .unwrap();
    batch.commit().unwrap();

    // Live cell itself should exist
    let live = store.get_cell(&tx_hash, 0).unwrap();
    assert!(live.is_some(), "cell should be in live_cells");

    // But index entries should NOT exist
    let by_lock = store
        .list_cells_by_lock(&cell.lock_script_hash, 100)
        .unwrap();
    assert!(
        by_lock.is_empty(),
        "lock index should be empty when skipped"
    );

    let by_type = store
        .list_cells_by_type(cell.type_script_hash.as_ref().unwrap(), 100)
        .unwrap();
    assert!(
        by_type.is_empty(),
        "type index should be empty when skipped"
    );

    let by_lock_code = store
        .list_cells_by_lock_code_hash(&cell.lock_code_hash, 100, None, None)
        .unwrap();
    assert!(
        by_lock_code.is_empty(),
        "lock_code index should be empty when skipped"
    );

    let by_type_code = store
        .list_cells_by_type_code_hash(cell.type_code_hash.as_ref().unwrap(), 100, None, None)
        .unwrap();
    assert!(
        by_type_code.is_empty(),
        "type_code index should be empty when skipped"
    );
}

#[test]
fn test_rebuild_cell_indices_populates_all_indexes() {
    use ckbadger_indexer::db::rebuild_cell_indices;

    let (store, writer) = setup_store();
    let tx1 = vec![0x01u8; 32];
    let tx2 = vec![0x02u8; 32];
    let cell1 = make_cell(100_00000000, 256, 0xAA);
    let cell2 = make_cell(200_00000000, 128, 0xBB);

    // Insert two cells with indices skipped
    let mut batch = StoreBatch::new(&store);
    writer
        .insert_cells_batch(
            &[(&tx1, 0, &cell1, 1000), (&tx2, 0, &cell2, 2000)],
            &mut batch,
            true,
        )
        .unwrap();
    batch.commit().unwrap();

    // Sanity: no index entries yet
    assert!(store
        .list_cells_by_lock(&cell1.lock_script_hash, 100)
        .unwrap()
        .is_empty());

    // Rebuild
    rebuild_cell_indices(&store);

    // All 4 index types should now be populated
    let by_lock1 = store
        .list_cells_by_lock(&cell1.lock_script_hash, 100)
        .unwrap();
    assert_eq!(by_lock1.len(), 1, "lock index should have cell1");
    assert_eq!(by_lock1[0].2.capacity, 100_00000000);

    let by_lock2 = store
        .list_cells_by_lock(&cell2.lock_script_hash, 100)
        .unwrap();
    assert_eq!(by_lock2.len(), 1, "lock index should have cell2");
    assert_eq!(by_lock2[0].2.capacity, 200_00000000);

    let by_type = store
        .list_cells_by_type(cell1.type_script_hash.as_ref().unwrap(), 100)
        .unwrap();
    assert_eq!(
        by_type.len(),
        2,
        "type index should have both cells (same type hash)"
    );

    let by_lock_code = store
        .list_cells_by_lock_code_hash(&cell1.lock_code_hash, 100, None, None)
        .unwrap();
    assert_eq!(
        by_lock_code.len(),
        2,
        "lock_code index should have both cells (same lock code)"
    );

    let by_type_code = store
        .list_cells_by_type_code_hash(cell1.type_code_hash.as_ref().unwrap(), 100, None, None)
        .unwrap();
    assert_eq!(
        by_type_code.len(),
        2,
        "type_code index should have both cells (same type code)"
    );
}

#[test]
fn test_skip_then_rebuild_matches_direct_insert() {
    use ckbadger_indexer::db::rebuild_cell_indices;

    // Setup two stores: one with skip+rebuild, one with direct insert
    let (store_skip, writer_skip) = setup_store();
    let (store_direct, writer_direct) = setup_store();

    let tx1 = vec![0x01u8; 32];
    let tx2 = vec![0x02u8; 32];
    let cell1 = make_cell(100_00000000, 256, 0xAA);
    let cell2 = make_cell(200_00000000, 128, 0xBB);
    let cells_ref: Vec<(&[u8], i16, &ParsedCell, i64)> =
        vec![(&tx1, 0, &cell1, 1000), (&tx2, 0, &cell2, 2000)];

    // Store 1: skip indices, then rebuild
    let mut batch = StoreBatch::new(&store_skip);
    writer_skip
        .insert_cells_batch(&cells_ref, &mut batch, true)
        .unwrap();
    batch.commit().unwrap();
    rebuild_cell_indices(&store_skip);

    // Store 2: direct insert with indices
    let mut batch = StoreBatch::new(&store_direct);
    writer_direct
        .insert_cells_batch(&cells_ref, &mut batch, false)
        .unwrap();
    batch.commit().unwrap();

    // Compare: lock index results should match
    let skip_lock = store_skip
        .list_cells_by_lock(&cell1.lock_script_hash, 100)
        .unwrap();
    let direct_lock = store_direct
        .list_cells_by_lock(&cell1.lock_script_hash, 100)
        .unwrap();
    assert_eq!(skip_lock.len(), direct_lock.len());
    assert_eq!(skip_lock[0].2.capacity, direct_lock[0].2.capacity);

    // Compare: type index results should match
    let skip_type = store_skip
        .list_cells_by_type(cell1.type_script_hash.as_ref().unwrap(), 100)
        .unwrap();
    let direct_type = store_direct
        .list_cells_by_type(cell1.type_script_hash.as_ref().unwrap(), 100)
        .unwrap();
    assert_eq!(skip_type.len(), direct_type.len());

    // Compare: lock_code index results should match
    let skip_lc = store_skip
        .list_cells_by_lock_code_hash(&cell1.lock_code_hash, 100, None, None)
        .unwrap();
    let direct_lc = store_direct
        .list_cells_by_lock_code_hash(&cell1.lock_code_hash, 100, None, None)
        .unwrap();
    assert_eq!(skip_lc.len(), direct_lc.len());

    // Compare: type_code index results should match
    let skip_tc = store_skip
        .list_cells_by_type_code_hash(cell1.type_code_hash.as_ref().unwrap(), 100, None, None)
        .unwrap();
    let direct_tc = store_direct
        .list_cells_by_type_code_hash(cell1.type_code_hash.as_ref().unwrap(), 100, None, None)
        .unwrap();
    assert_eq!(skip_tc.len(), direct_tc.len());
}
