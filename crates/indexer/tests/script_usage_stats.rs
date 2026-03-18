//! Integration tests for script usage statistics in ckbadger-store.
//!
//! Tests ScriptInfo insertion for lock and type scripts, stat adjustments
//! on cell consumption, and listing of all script infos.

use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::sync::materialize_script_infos_for_test;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::CkbadgerStore;
use ckbadger_store::ScriptInfo;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap());
    std::mem::forget(dir);
    store
}

fn fixture_lock_script(args_hex: &str) -> Script {
    Script {
        code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8".to_string(),
        hash_type: "type".to_string(),
        args: args_hex.to_string(),
    }
}

fn fixture_header(number: u64) -> HeaderView {
    HeaderView {
        version: "0x0".to_string(),
        compact_target: "0x1a08a97e".to_string(),
        timestamp: "0x18c7b3b2b00".to_string(),
        number: format!("0x{number:x}"),
        epoch: "0x7080006000028".to_string(),
        parent_hash: format!("0x{}", "11".repeat(32)),
        transactions_root: format!("0x{}", "22".repeat(32)),
        proposals_hash: format!("0x{}", "33".repeat(32)),
        extra_hash: format!("0x{}", "44".repeat(32)),
        dao: format!("0x{}", "00".repeat(32)),
        nonce: "0x1".to_string(),
        hash: format!("0x{}", "55".repeat(32)),
    }
}

fn bulk_build_script_fixture() -> BlockResponseWithCycles {
    let create_tx = TransactionView {
        hash: format!("0x{}", "ca".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: format!("0x{}", "00".repeat(32)),
                index: "0xffffffff".to_string(),
            },
        }],
        outputs: vec![
            CellOutput {
                capacity: format!("0x{:x}", 100_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
                type_: None,
            },
            CellOutput {
                capacity: format!("0x{:x}", 200_00000000u64),
                lock: fixture_lock_script(&format!("0x{}", "02".repeat(20))),
                type_: Some(Script {
                    code_hash: "0xc5e5dcf215d99af62867164d6fb9d17ce68a45b9e2aecd49c19634426f2056a3"
                        .to_string(),
                    hash_type: "type".to_string(),
                    args: format!("0x{}", "12".repeat(32)),
                }),
            },
        ],
        outputs_data: vec!["0x".to_string(), format!("0x{}", "2a".repeat(16))],
        witnesses: vec!["0x".to_string()],
    };

    let consume_tx = TransactionView {
        hash: format!("0x{}", "cb".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: create_tx.hash.clone(),
                index: "0x1".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 80_00000000u64),
            lock: fixture_lock_script(&format!("0x{}", "03".repeat(20))),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec!["0x".to_string()],
    };

    BlockResponseWithCycles {
        block: BlockView {
            header: fixture_header(14_000_700),
            uncles: vec![],
            transactions: vec![create_tx, consume_tx],
            proposals: vec![],
        },
        cycles: None,
    }
}

fn make_script_info_lock(
    code_hash_byte: u8,
    cells_count: i64,
    live_cells_count: i64,
    capacity_sum: i128,
    live_capacity_sum: i128,
) -> ScriptInfo {
    let used_capacity_sum = capacity_sum / 2;
    let live_used_capacity_sum = live_capacity_sum / 2;
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
        lock_used_capacity_sum: used_capacity_sum,
        lock_live_used_capacity_sum: live_used_capacity_sum,
        type_cells_count: 0,
        type_live_cells_count: 0,
        type_capacity_sum: 0,
        type_live_capacity_sum: 0,
        type_used_capacity_sum: 0,
        type_live_used_capacity_sum: 0,
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
    let used_capacity_sum = capacity_sum / 2;
    let live_used_capacity_sum = live_capacity_sum / 2;
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
        lock_used_capacity_sum: 0,
        lock_live_used_capacity_sum: 0,
        type_cells_count: cells_count,
        type_live_cells_count: live_cells_count,
        type_capacity_sum: capacity_sum,
        type_live_capacity_sum: live_capacity_sum,
        type_used_capacity_sum: used_capacity_sum,
        type_live_used_capacity_sum: live_used_capacity_sum,
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
    assert_eq!(retrieved.lock_used_capacity_sum, 250_00000000);
    assert_eq!(retrieved.lock_live_used_capacity_sum, 150_00000000);
    // Type fields should be zero
    assert_eq!(retrieved.type_cells_count, 0);
    assert_eq!(retrieved.type_live_cells_count, 0);
    assert_eq!(retrieved.type_capacity_sum, 0);
    assert_eq!(retrieved.type_live_capacity_sum, 0);
    assert_eq!(retrieved.type_used_capacity_sum, 0);
    assert_eq!(retrieved.type_live_used_capacity_sum, 0);
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
    assert_eq!(retrieved.type_used_capacity_sum, 500_00000000);
    assert_eq!(retrieved.type_live_used_capacity_sum, 400_00000000);
    // Lock fields should be zero
    assert_eq!(retrieved.lock_cells_count, 0);
    assert_eq!(retrieved.lock_live_cells_count, 0);
    assert_eq!(retrieved.lock_capacity_sum, 0);
    assert_eq!(retrieved.lock_live_capacity_sum, 0);
    assert_eq!(retrieved.lock_used_capacity_sum, 0);
    assert_eq!(retrieved.lock_live_used_capacity_sum, 0);
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
        lock_used_capacity_sum: 250_00000000,
        lock_live_used_capacity_sum: 150_00000000,
        type_cells_count: 0,
        type_live_cells_count: 0,
        type_capacity_sum: 0,
        type_live_capacity_sum: 0,
        type_used_capacity_sum: 0,
        type_live_used_capacity_sum: 0,
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
        lock_used_capacity_sum: 325_00000000,
        lock_live_used_capacity_sum: 225_00000000,
        type_cells_count: 0,
        type_live_cells_count: 0,
        type_capacity_sum: 0,
        type_live_capacity_sum: 0,
        type_used_capacity_sum: 0,
        type_live_used_capacity_sum: 0,
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

#[test]
fn bulk_build_script_owner_materializes_lock_and_type_usage_without_db_reads() {
    let infos =
        materialize_script_infos_for_test(&[bulk_build_script_fixture()]).expect("script infos");

    let lock_code_hash = vec![
        0x9b, 0xd7, 0xe0, 0x6f, 0x3e, 0xcf, 0x4b, 0xe0, 0xf2, 0xfc, 0xd2, 0x18, 0x8b, 0x23, 0xf1,
        0xb9, 0xfc, 0xc8, 0x8e, 0x5d, 0x4b, 0x65, 0xa8, 0x63, 0x7b, 0x17, 0x72, 0x3b, 0xbd, 0xa3,
        0xcc, 0xe8,
    ];
    let type_code_hash = vec![
        0xc5, 0xe5, 0xdc, 0xf2, 0x15, 0xd9, 0x9a, 0xf6, 0x28, 0x67, 0x16, 0x4d, 0x6f, 0xb9, 0xd1,
        0x7c, 0xe6, 0x8a, 0x45, 0xb9, 0xe2, 0xae, 0xcd, 0x49, 0xc1, 0x96, 0x34, 0x42, 0x6f, 0x20,
        0x56, 0xa3,
    ];

    let lock_info = infos.get(&lock_code_hash).expect("lock script info");
    assert_eq!(lock_info.hash_type, 1);
    assert_eq!(lock_info.lock_cells_count, 3);
    assert_eq!(lock_info.lock_live_cells_count, 2);
    assert_eq!(lock_info.lock_capacity_sum, 380_00000000);
    assert_eq!(lock_info.lock_live_capacity_sum, 180_00000000);
    assert_eq!(lock_info.lock_used_capacity_sum, 264_00000000);
    assert_eq!(lock_info.lock_live_used_capacity_sum, 122_00000000);

    let type_info = infos.get(&type_code_hash).expect("type script info");
    assert_eq!(type_info.hash_type, 1);
    assert_eq!(type_info.type_cells_count, 1);
    assert_eq!(type_info.type_live_cells_count, 0);
    assert_eq!(type_info.type_capacity_sum, 200_00000000);
    assert_eq!(type_info.type_live_capacity_sum, 0);
    assert_eq!(type_info.type_used_capacity_sum, 142_00000000);
    assert_eq!(type_info.type_live_used_capacity_sum, 0);
}
