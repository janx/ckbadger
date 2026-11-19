//! Integration tests for script usage statistics in ckbadger-store.
//!
//! Tests a script's label (`script_info`) and usage (`script_reference_info`)
//! rows, both keyed by the script's pair (code hash, hash type), stat
//! adjustments on cell consumption, and listing of all script labels.

use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::sync::materialize_script_reference_infos_for_test;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::CkbadgerStore;
use ckbadger_store::{ScriptInfo, ScriptReferenceInfo};
use std::sync::Arc;

/// Real mainnet cellbase first witness (block 12,000,000): block parsing
/// requires every non-genesis cellbase to carry a valid RFC-0022
/// `CellbaseWitness`.
const TEST_CELLBASE_WITNESS: &str = "0x7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323032342d30312d303929";

fn setup_store() -> Arc<CkbadgerStore> {
    Arc::new(CkbadgerStore::open_domain_temp().unwrap())
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
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
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

/// The registry's label of a script and its usage row, both keyed by the
/// script's pair (code hash, hash type): `script_info` holds the label only,
/// `script_reference_info` the usage (lane rd/api-lock-names: one row by the
/// code hash alone used to hold both, every hash type's cells summed).
fn make_label(name: &str, category: &str) -> ScriptInfo {
    ScriptInfo {
        name: name.to_string(),
        deprecated: false,
        category: Some(category.to_string()),
        website: None,
        description: Some(format!("A {category} script")),
    }
}

fn make_lock_usage(
    code_hash_byte: u8,
    hash_type: u8,
    cells_count: i64,
    live_cells_count: i64,
    capacity_sum: i128,
    owned_capacity_sum: i128,
) -> ScriptReferenceInfo {
    ScriptReferenceInfo {
        reference_hash: vec![code_hash_byte; 32],
        hash_type,
        lock_cells_count: cells_count,
        lock_live_cells_count: live_cells_count,
        lock_capacity_sum: capacity_sum,
        lock_owned_capacity_sum: owned_capacity_sum,
        lock_used_capacity_sum: capacity_sum / 2,
        lock_owned_knowledge_sum: owned_capacity_sum / 2,
        ..Default::default()
    }
}

fn make_type_usage(
    code_hash_byte: u8,
    hash_type: u8,
    cells_count: i64,
    live_cells_count: i64,
    capacity_sum: i128,
    owned_capacity_sum: i128,
) -> ScriptReferenceInfo {
    ScriptReferenceInfo {
        reference_hash: vec![code_hash_byte; 32],
        hash_type,
        type_cells_count: cells_count,
        type_live_cells_count: live_cells_count,
        type_capacity_sum: capacity_sum,
        type_owned_capacity_sum: owned_capacity_sum,
        type_used_capacity_sum: capacity_sum / 2,
        type_owned_knowledge_sum: owned_capacity_sum / 2,
        ..Default::default()
    }
}

#[test]
fn test_lock_script_usage_creation() {
    let store = setup_store();
    let code_hash = vec![0x11u8; 32];

    store
        .put_script_info_direct(&code_hash, 0, &make_label("Lock-11", "lock"))
        .unwrap();
    let mut batch = StoreBatch::new(&store);
    batch.put_script_reference_info(
        0,
        &code_hash,
        &make_lock_usage(0x11, 0, 5, 3, 500_00000000, 300_00000000),
    );
    batch.commit().unwrap();

    let label = store
        .get_script_info(&code_hash, 0)
        .unwrap()
        .expect("the label of the pair (code hash, data)");
    assert_eq!(label.name, "Lock-11");
    assert_eq!(label.category, Some("lock".to_string()));
    let retrieved = store
        .get_script_reference_info(0, &code_hash)
        .unwrap()
        .expect("the usage of the pair (code hash, data)");
    assert_eq!(retrieved.reference_hash, vec![0x11u8; 32]);
    assert_eq!(retrieved.hash_type, 0);
    assert_eq!(retrieved.lock_cells_count, 5);
    assert_eq!(retrieved.lock_live_cells_count, 3);
    assert_eq!(retrieved.lock_capacity_sum, 500_00000000);
    assert_eq!(retrieved.lock_owned_capacity_sum, 300_00000000);
    assert_eq!(retrieved.lock_used_capacity_sum, 250_00000000);
    assert_eq!(retrieved.lock_owned_knowledge_sum, 150_00000000);
    // Type fields should be zero
    assert_eq!(retrieved.type_cells_count, 0);
    assert_eq!(retrieved.type_live_cells_count, 0);
    assert_eq!(retrieved.type_capacity_sum, 0);
    assert_eq!(retrieved.type_owned_capacity_sum, 0);
    assert_eq!(retrieved.type_used_capacity_sum, 0);
    assert_eq!(retrieved.type_owned_knowledge_sum, 0);
    // The same code hash by another hash type is another script.
    assert!(store.get_script_info(&code_hash, 1).unwrap().is_none());
    assert!(store
        .get_script_reference_info(1, &code_hash)
        .unwrap()
        .is_none());
}

#[test]
fn test_type_script_usage_creation() {
    let store = setup_store();
    let code_hash = vec![0x22u8; 32];

    store
        .put_script_info_direct(&code_hash, 1, &make_label("Type-22", "type"))
        .unwrap();
    let mut batch = StoreBatch::new(&store);
    batch.put_script_reference_info(
        1,
        &code_hash,
        &make_type_usage(0x22, 1, 10, 8, 1000_00000000, 800_00000000),
    );
    batch.commit().unwrap();

    let label = store.get_script_info(&code_hash, 1).unwrap().unwrap();
    assert_eq!(label.name, "Type-22");
    assert_eq!(label.category, Some("type".to_string()));
    let retrieved = store
        .get_script_reference_info(1, &code_hash)
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.reference_hash, vec![0x22u8; 32]);
    assert_eq!(retrieved.hash_type, 1);
    assert_eq!(retrieved.type_cells_count, 10);
    assert_eq!(retrieved.type_live_cells_count, 8);
    assert_eq!(retrieved.type_capacity_sum, 1000_00000000);
    assert_eq!(retrieved.type_owned_capacity_sum, 800_00000000);
    assert_eq!(retrieved.type_used_capacity_sum, 500_00000000);
    assert_eq!(retrieved.type_owned_knowledge_sum, 400_00000000);
    // Lock fields should be zero
    assert_eq!(retrieved.lock_cells_count, 0);
    assert_eq!(retrieved.lock_live_cells_count, 0);
    assert_eq!(retrieved.lock_capacity_sum, 0);
    assert_eq!(retrieved.lock_owned_capacity_sum, 0);
    assert_eq!(retrieved.lock_used_capacity_sum, 0);
    assert_eq!(retrieved.lock_owned_knowledge_sum, 0);
}

#[test]
fn test_consume_cells_adjusts_stats() {
    let store = setup_store();
    let code_hash = vec![0x33u8; 32];
    let put = |usage: &ScriptReferenceInfo| {
        let mut batch = StoreBatch::new(&store);
        batch.put_script_reference_info(0, &code_hash, usage);
        batch.commit().unwrap();
    };

    // Initial state: 5 total cells, 5 live, 500 CKB total/live
    put(&make_lock_usage(0x33, 0, 5, 5, 500_00000000, 500_00000000));
    let r1 = store
        .get_script_reference_info(0, &code_hash)
        .unwrap()
        .unwrap();
    assert_eq!(r1.lock_cells_count, 5);
    assert_eq!(r1.lock_live_cells_count, 5);
    assert_eq!(r1.lock_capacity_sum, 500_00000000);
    assert_eq!(r1.lock_owned_capacity_sum, 500_00000000);

    // Simulate consuming 2 cells (200 CKB capacity):
    // total stays 5, live drops to 3, total capacity stays, owned capacity drops
    put(&ScriptReferenceInfo {
        lock_live_cells_count: 3,
        lock_owned_capacity_sum: 300_00000000,
        lock_owned_knowledge_sum: 150_00000000,
        ..make_lock_usage(0x33, 0, 5, 5, 500_00000000, 500_00000000)
    });
    let r2 = store
        .get_script_reference_info(0, &code_hash)
        .unwrap()
        .unwrap();
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
        r2.lock_owned_capacity_sum, 300_00000000,
        "owned capacity should decrease by consumed amount"
    );

    // Simulate adding 1 new cell (150 CKB) on top of consumed state
    put(&make_lock_usage(0x33, 0, 6, 4, 650_00000000, 450_00000000));
    let r3 = store
        .get_script_reference_info(0, &code_hash)
        .unwrap()
        .unwrap();
    assert_eq!(r3.lock_cells_count, 6);
    assert_eq!(r3.lock_live_cells_count, 4);
    assert_eq!(r3.lock_capacity_sum, 650_00000000);
    assert_eq!(r3.lock_owned_capacity_sum, 450_00000000);
}

#[test]
fn test_list_script_infos() {
    let store = setup_store();

    let code1 = vec![0xAAu8; 32];
    let code2 = vec![0xBBu8; 32];

    // Three labels; two share a code hash under two hash types, two scripts.
    store
        .put_script_info_direct(&code1, 0, &make_label("Lock-aa", "lock"))
        .unwrap();
    store
        .put_script_info_direct(&code1, 1, &make_label("Lock-aa by type", "lock"))
        .unwrap();
    store
        .put_script_info_direct(&code2, 1, &make_label("Type-bb", "type"))
        .unwrap();

    let all = store.list_script_infos().unwrap();
    assert_eq!(all.len(), 3, "should have 3 script labels");
    let names: Vec<((Vec<u8>, u8), String)> = all
        .into_iter()
        .map(|(pair, info)| (pair, info.name))
        .collect();
    assert_eq!(
        names,
        vec![
            ((code1.clone(), 0), "Lock-aa".to_string()),
            ((code1.clone(), 1), "Lock-aa by type".to_string()),
            ((code2.clone(), 1), "Type-bb".to_string()),
        ]
    );

    // Verify non-existent pairs return None
    assert!(store.get_script_info(&[0xFFu8; 32], 1).unwrap().is_none());
    assert!(store.get_script_info(&code2, 0).unwrap().is_none());
}

#[test]
fn bulk_build_script_owner_materializes_exact_reference_usage_without_db_reads() {
    let references = materialize_script_reference_infos_for_test(&[bulk_build_script_fixture()])
        .expect("script reference infos");

    let lock_reference_hash = vec![
        0x9b, 0xd7, 0xe0, 0x6f, 0x3e, 0xcf, 0x4b, 0xe0, 0xf2, 0xfc, 0xd2, 0x18, 0x8b, 0x23, 0xf1,
        0xb9, 0xfc, 0xc8, 0x8e, 0x5d, 0x4b, 0x65, 0xa8, 0x63, 0x7b, 0x17, 0x72, 0x3b, 0xbd, 0xa3,
        0xcc, 0xe8,
    ];
    let type_reference_hash = vec![
        0xc5, 0xe5, 0xdc, 0xf2, 0x15, 0xd9, 0x9a, 0xf6, 0x28, 0x67, 0x16, 0x4d, 0x6f, 0xb9, 0xd1,
        0x7c, 0xe6, 0x8a, 0x45, 0xb9, 0xe2, 0xae, 0xcd, 0x49, 0xc1, 0x96, 0x34, 0x42, 0x6f, 0x20,
        0x56, 0xa3,
    ];

    let lock_reference = references
        .get(&(lock_reference_hash.clone(), 1))
        .expect("lock reference");
    assert_eq!(lock_reference.lock_cells_count, 3);
    assert_eq!(lock_reference.lock_live_cells_count, 2);
    assert_eq!(lock_reference.lock_capacity_sum, 380_00000000);
    assert_eq!(lock_reference.lock_owned_capacity_sum, 180_00000000);
    assert_eq!(lock_reference.lock_used_capacity_sum, 264_00000000);
    assert_eq!(lock_reference.lock_owned_knowledge_sum, 122_00000000);
    assert_eq!(lock_reference.type_cells_count, 0);

    let type_reference = references
        .get(&(type_reference_hash.clone(), 1))
        .expect("type reference");
    assert_eq!(type_reference.type_cells_count, 1);
    assert_eq!(type_reference.type_live_cells_count, 0);
    assert_eq!(type_reference.type_capacity_sum, 200_00000000);
    assert_eq!(type_reference.type_owned_capacity_sum, 0);
    assert_eq!(type_reference.type_used_capacity_sum, 142_00000000);
    assert_eq!(type_reference.type_owned_knowledge_sum, 0);
    assert_eq!(type_reference.lock_cells_count, 0);
}
