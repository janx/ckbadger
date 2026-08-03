//! Integration tests for address balance operations in ckbadger-store.
//!
//! Tests balance insertion, updates, and cumulative modifications.

use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::sync::materialize_address_balances_for_test;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::AddressBalance;
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

/// Real mainnet cellbase first witness (block 12,000,000): block parsing
/// requires every non-genesis cellbase to carry a valid RFC-0022
/// `CellbaseWitness`.
const TEST_CELLBASE_WITNESS: &str = "0x7a0000000c00000055000000490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce801140000008211f1b938a107cd53b6302cc752a6fc3965638d210000000000000020302e3131332e3020283832383731613320323032342d30312d303929";

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

fn bulk_build_address_fixture() -> BlockResponseWithCycles {
    let lock_a_args = format!("0x{}", "01".repeat(20));
    let lock_b_args = format!("0x{}", "02".repeat(20));
    let create_tx = TransactionView {
        hash: format!("0x{}", "aa".repeat(32)),
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
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 200_00000000u64),
            lock: fixture_lock_script(&lock_a_args),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        // Block parsing requires a valid CellbaseWitness in the first tx's
        // first witness (real mainnet block 12,000,000 vector).
        witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
    };

    let split_tx = TransactionView {
        hash: format!("0x{}", "bb".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: create_tx.hash.clone(),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![
            CellOutput {
                capacity: format!("0x{:x}", 100_00000000u64),
                lock: fixture_lock_script(&lock_a_args),
                type_: None,
            },
            CellOutput {
                capacity: format!("0x{:x}", 100_00000000u64),
                lock: fixture_lock_script(&lock_b_args),
                type_: None,
            },
        ],
        outputs_data: vec!["0x".to_string(), "0x".to_string()],
        witnesses: vec!["0x".to_string()],
    };

    BlockResponseWithCycles {
        block: BlockView {
            header: fixture_header(14_000_500),
            uncles: vec![],
            transactions: vec![create_tx, split_tx],
            proposals: vec![],
        },
        cycles: None,
    }
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
        used_capacity: 0,
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
fn test_used_capacity_stored_and_retrieved() {
    let store = setup_store();
    let lock_hash = vec![0xDDu8; 32];

    let balance = AddressBalance {
        balance: 200_00000000,
        used_capacity: 6100_00000000,
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
    assert_eq!(retrieved.used_capacity, 6100_00000000);
    assert_eq!(retrieved.live_cells_count, 2);
}

#[test]
fn test_used_capacity_zero_by_default() {
    let store = setup_store();
    let lock_hash = vec![0xEEu8; 32];

    // Simulate pre-existing data that was created before used_capacity was added:
    // just use the Default-derived zero value.
    let balance = make_balance(100_00000000, 1, 1, 1, 100, 0x01, 100, 0x01);

    let mut batch = StoreBatch::new(&store);
    batch.put_addr_balance(&lock_hash, &balance);
    batch.commit().unwrap();

    let retrieved = store.get_addr_balance(&lock_hash).unwrap().unwrap();
    assert_eq!(
        retrieved.used_capacity, 0,
        "used_capacity should default to 0"
    );
}

#[test]
fn bulk_build_address_owner_materializes_final_balances_without_db_reads() {
    let balances =
        materialize_address_balances_for_test(&[bulk_build_address_fixture()]).expect("balances");
    assert_eq!(balances.len(), 2);

    let balance_a = balances
        .values()
        .find(|balance| balance.first_seen_tx == vec![0xaa; 32])
        .expect("address A balance");
    assert_eq!(balance_a.balance, 100_00000000);
    assert_eq!(balance_a.used_capacity, 61_00000000);
    assert_eq!(balance_a.live_cells_count, 1);
    assert_eq!(balance_a.total_cells_count, 2);
    assert_eq!(balance_a.txs_count, 2);
    assert_eq!(balance_a.first_seen_block, 14_000_500);
    assert_eq!(balance_a.first_seen_tx, vec![0xaa; 32]);
    assert_eq!(balance_a.last_activity_block, 14_000_500);
    assert_eq!(balance_a.last_activity_tx, vec![0xbb; 32]);

    let balance_b = balances
        .values()
        .find(|balance| balance.first_seen_tx == vec![0xbb; 32])
        .expect("address B balance");
    assert_eq!(balance_b.balance, 100_00000000);
    assert_eq!(balance_b.used_capacity, 61_00000000);
    assert_eq!(balance_b.live_cells_count, 1);
    assert_eq!(balance_b.total_cells_count, 1);
    assert_eq!(balance_b.txs_count, 1);
    assert_eq!(balance_b.first_seen_block, 14_000_500);
    assert_eq!(balance_b.first_seen_tx, vec![0xbb; 32]);
    assert_eq!(balance_b.last_activity_block, 14_000_500);
    assert_eq!(balance_b.last_activity_tx, vec![0xbb; 32]);
}
