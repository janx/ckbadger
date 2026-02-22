//! Integration tests for script usage statistics in ckbadger-store.
//!
//! Tests ScriptInfo insertion for lock and type scripts, stat adjustments
//! on cell consumption, and listing of all script infos.

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::CkbadgerStore;
use ckbadger_store::ScriptInfo;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap());
    std::mem::forget(dir);
    store
}

fn make_script_info_lock(
    code_hash_byte: u8,
    cells_count: i64,
    live_cells_count: i64,
    capacity_sum: i128,
    live_capacity_sum: i128,
) -> ScriptInfo {
    let occupied_capacity_sum = capacity_sum / 2;
    let live_occupied_capacity_sum = live_capacity_sum / 2;
    ScriptInfo {
        code_hash: vec![code_hash_byte; 32],
        hash_type: 0,
        name: Some(format!("Lock-{:02x}", code_hash_byte)),
        category: Some("lock".to_string()),
        website: None,
        description: Some("A lock script".to_string()),
        cells_count,
        capacity_used: capacity_sum,
        lock_cells_count: cells_count,
        lock_live_cells_count: live_cells_count,
        lock_capacity_sum: capacity_sum,
        lock_live_capacity_sum: live_capacity_sum,
        lock_occupied_capacity_sum: occupied_capacity_sum,
        lock_live_occupied_capacity_sum: live_occupied_capacity_sum,
        type_cells_count: 0,
        type_live_cells_count: 0,
        type_capacity_sum: 0,
        type_live_capacity_sum: 0,
        type_occupied_capacity_sum: 0,
        type_live_occupied_capacity_sum: 0,
        dep_type_hash: None,
        dep_data_hash: None,
        code_cell_tx_hash: None,
        code_cell_output_index: None,
    }
}

fn make_script_info_type(
    code_hash_byte: u8,
    cells_count: i64,
    live_cells_count: i64,
    capacity_sum: i128,
    live_capacity_sum: i128,
) -> ScriptInfo {
    let occupied_capacity_sum = capacity_sum / 2;
    let live_occupied_capacity_sum = live_capacity_sum / 2;
    ScriptInfo {
        code_hash: vec![code_hash_byte; 32],
        hash_type: 1,
        name: Some(format!("Type-{:02x}", code_hash_byte)),
        category: Some("type".to_string()),
        website: None,
        description: Some("A type script".to_string()),
        cells_count,
        capacity_used: capacity_sum,
        lock_cells_count: 0,
        lock_live_cells_count: 0,
        lock_capacity_sum: 0,
        lock_live_capacity_sum: 0,
        lock_occupied_capacity_sum: 0,
        lock_live_occupied_capacity_sum: 0,
        type_cells_count: cells_count,
        type_live_cells_count: live_cells_count,
        type_capacity_sum: capacity_sum,
        type_live_capacity_sum: live_capacity_sum,
        type_occupied_capacity_sum: occupied_capacity_sum,
        type_live_occupied_capacity_sum: live_occupied_capacity_sum,
        dep_type_hash: None,
        dep_data_hash: None,
        code_cell_tx_hash: None,
        code_cell_output_index: None,
    }
}

#[test]
fn test_lock_script_usage_creation() {
    let store = setup_store();
    let code_hash = vec![0x11u8; 32];

    let info = make_script_info_lock(0x11, 5, 3, 500_00000000, 300_00000000);

    let mut batch = StoreBatch::new(&store);
    batch.put_script_info(&code_hash, &info);
    batch.commit().unwrap();

    let retrieved = store.get_script_info(&code_hash).unwrap();
    assert!(
        retrieved.is_some(),
        "script info should exist after insertion"
    );

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.code_hash, vec![0x11u8; 32]);
    assert_eq!(retrieved.hash_type, 0);
    assert_eq!(retrieved.name, Some("Lock-11".to_string()));
    assert_eq!(retrieved.category, Some("lock".to_string()));
    assert_eq!(retrieved.cells_count, 5);
    assert_eq!(retrieved.lock_cells_count, 5);
    assert_eq!(retrieved.lock_live_cells_count, 3);
    assert_eq!(retrieved.lock_capacity_sum, 500_00000000);
    assert_eq!(retrieved.lock_live_capacity_sum, 300_00000000);
    assert_eq!(retrieved.lock_occupied_capacity_sum, 250_00000000);
    assert_eq!(retrieved.lock_live_occupied_capacity_sum, 150_00000000);
    // Type fields should be zero
    assert_eq!(retrieved.type_cells_count, 0);
    assert_eq!(retrieved.type_live_cells_count, 0);
    assert_eq!(retrieved.type_capacity_sum, 0);
    assert_eq!(retrieved.type_live_capacity_sum, 0);
    assert_eq!(retrieved.type_occupied_capacity_sum, 0);
    assert_eq!(retrieved.type_live_occupied_capacity_sum, 0);
}

#[test]
fn test_type_script_usage_creation() {
    let store = setup_store();
    let code_hash = vec![0x22u8; 32];

    let info = make_script_info_type(0x22, 10, 8, 1000_00000000, 800_00000000);

    let mut batch = StoreBatch::new(&store);
    batch.put_script_info(&code_hash, &info);
    batch.commit().unwrap();

    let retrieved = store.get_script_info(&code_hash).unwrap();
    assert!(retrieved.is_some());

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.code_hash, vec![0x22u8; 32]);
    assert_eq!(retrieved.hash_type, 1);
    assert_eq!(retrieved.name, Some("Type-22".to_string()));
    assert_eq!(retrieved.category, Some("type".to_string()));
    assert_eq!(retrieved.cells_count, 10);
    assert_eq!(retrieved.type_cells_count, 10);
    assert_eq!(retrieved.type_live_cells_count, 8);
    assert_eq!(retrieved.type_capacity_sum, 1000_00000000);
    assert_eq!(retrieved.type_live_capacity_sum, 800_00000000);
    assert_eq!(retrieved.type_occupied_capacity_sum, 500_00000000);
    assert_eq!(retrieved.type_live_occupied_capacity_sum, 400_00000000);
    // Lock fields should be zero
    assert_eq!(retrieved.lock_cells_count, 0);
    assert_eq!(retrieved.lock_live_cells_count, 0);
    assert_eq!(retrieved.lock_capacity_sum, 0);
    assert_eq!(retrieved.lock_live_capacity_sum, 0);
    assert_eq!(retrieved.lock_occupied_capacity_sum, 0);
    assert_eq!(retrieved.lock_live_occupied_capacity_sum, 0);
}

#[test]
fn test_consume_cells_adjusts_stats() {
    let store = setup_store();
    let code_hash = vec![0x33u8; 32];

    // Initial state: 5 total cells, 5 live, 500 CKB total/live
    let initial = make_script_info_lock(0x33, 5, 5, 500_00000000, 500_00000000);

    let mut batch = StoreBatch::new(&store);
    batch.put_script_info(&code_hash, &initial);
    batch.commit().unwrap();

    let r1 = store.get_script_info(&code_hash).unwrap().unwrap();
    assert_eq!(r1.lock_cells_count, 5);
    assert_eq!(r1.lock_live_cells_count, 5);
    assert_eq!(r1.lock_capacity_sum, 500_00000000);
    assert_eq!(r1.lock_live_capacity_sum, 500_00000000);

    // Simulate consuming 2 cells (200 CKB capacity):
    // total stays 5, live drops to 3, total capacity stays, live capacity drops
    let after_consume = ScriptInfo {
        code_hash: vec![0x33u8; 32],
        hash_type: 0,
        name: Some("Lock-33".to_string()),
        category: Some("lock".to_string()),
        website: None,
        description: Some("A lock script".to_string()),
        cells_count: 5,
        capacity_used: 500_00000000,
        lock_cells_count: 5,
        lock_live_cells_count: 3,
        lock_capacity_sum: 500_00000000,
        lock_live_capacity_sum: 300_00000000,
        lock_occupied_capacity_sum: 250_00000000,
        lock_live_occupied_capacity_sum: 150_00000000,
        type_cells_count: 0,
        type_live_cells_count: 0,
        type_capacity_sum: 0,
        type_live_capacity_sum: 0,
        type_occupied_capacity_sum: 0,
        type_live_occupied_capacity_sum: 0,
        dep_type_hash: None,
        dep_data_hash: None,
        code_cell_tx_hash: None,
        code_cell_output_index: None,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_script_info(&code_hash, &after_consume);
    batch.commit().unwrap();

    let r2 = store.get_script_info(&code_hash).unwrap().unwrap();
    assert_eq!(r2.lock_cells_count, 5, "total cells should remain 5");
    assert_eq!(
        r2.lock_live_cells_count, 3,
        "live cells should drop to 3 after consuming 2"
    );
    assert_eq!(
        r2.lock_capacity_sum, 500_00000000,
        "total capacity should remain unchanged"
    );
    assert_eq!(
        r2.lock_live_capacity_sum, 300_00000000,
        "live capacity should decrease by consumed amount"
    );

    // Simulate adding 1 new cell (150 CKB) on top of consumed state
    let after_create = ScriptInfo {
        code_hash: vec![0x33u8; 32],
        hash_type: 0,
        name: Some("Lock-33".to_string()),
        category: Some("lock".to_string()),
        website: None,
        description: Some("A lock script".to_string()),
        cells_count: 6,
        capacity_used: 650_00000000,
        lock_cells_count: 6,
        lock_live_cells_count: 4,
        lock_capacity_sum: 650_00000000,
        lock_live_capacity_sum: 450_00000000,
        lock_occupied_capacity_sum: 325_00000000,
        lock_live_occupied_capacity_sum: 225_00000000,
        type_cells_count: 0,
        type_live_cells_count: 0,
        type_capacity_sum: 0,
        type_live_capacity_sum: 0,
        type_occupied_capacity_sum: 0,
        type_live_occupied_capacity_sum: 0,
        dep_type_hash: None,
        dep_data_hash: None,
        code_cell_tx_hash: None,
        code_cell_output_index: None,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_script_info(&code_hash, &after_create);
    batch.commit().unwrap();

    let r3 = store.get_script_info(&code_hash).unwrap().unwrap();
    assert_eq!(r3.lock_cells_count, 6);
    assert_eq!(r3.lock_live_cells_count, 4);
    assert_eq!(r3.lock_capacity_sum, 650_00000000);
    assert_eq!(r3.lock_live_capacity_sum, 450_00000000);
}

#[test]
fn test_list_script_infos() {
    let store = setup_store();

    let code1 = vec![0xAAu8; 32];
    let code2 = vec![0xBBu8; 32];
    let code3 = vec![0xCCu8; 32];

    let info1 = make_script_info_lock(0xAA, 10, 8, 1000_00000000, 800_00000000);
    let info2 = make_script_info_type(0xBB, 20, 15, 2000_00000000, 1500_00000000);
    let info3 = make_script_info_lock(0xCC, 5, 5, 500_00000000, 500_00000000);

    let mut batch = StoreBatch::new(&store);
    batch.put_script_info(&code1, &info1);
    batch.put_script_info(&code2, &info2);
    batch.put_script_info(&code3, &info3);
    batch.commit().unwrap();

    let all = store.list_script_infos().unwrap();
    assert_eq!(all.len(), 3, "should have 3 script infos");

    // Collect all code hashes from results
    let code_hashes: Vec<Vec<u8>> = all.iter().map(|(k, _)| k.clone()).collect();
    assert!(code_hashes.contains(&code1));
    assert!(code_hashes.contains(&code2));
    assert!(code_hashes.contains(&code3));

    // Verify specific info values
    for (key, info) in &all {
        if *key == code1 {
            assert_eq!(info.lock_cells_count, 10);
            assert_eq!(info.lock_live_cells_count, 8);
            assert_eq!(info.name, Some("Lock-aa".to_string()));
        } else if *key == code2 {
            assert_eq!(info.type_cells_count, 20);
            assert_eq!(info.type_live_cells_count, 15);
            assert_eq!(info.name, Some("Type-bb".to_string()));
        } else if *key == code3 {
            assert_eq!(info.lock_cells_count, 5);
            assert_eq!(info.lock_live_cells_count, 5);
            assert_eq!(info.name, Some("Lock-cc".to_string()));
        }
    }

    // Verify non-existent code hash returns None
    let missing = store.get_script_info(&[0xFFu8; 32]).unwrap();
    assert!(missing.is_none());
}
