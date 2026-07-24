use ckbadger_indexer::rpc::{
    BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
    TransactionView,
};
use ckbadger_indexer::sync::materialize_dao_state_for_test;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::DaoDepositCacheEntry;
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
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

fn fixture_header(number: u64, ar: u64) -> HeaderView {
    let mut dao = [0u8; 32];
    dao[8..16].copy_from_slice(&ar.to_le_bytes());

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
        dao: format!("0x{}", hex::encode(dao)),
        nonce: "0x1".to_string(),
        hash: format!("0x{}", "55".repeat(32)),
    }
}

fn bulk_build_dao_fixture() -> Vec<BlockResponseWithCycles> {
    let dao_type = Script {
        code_hash: "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e".to_string(),
        hash_type: "type".to_string(),
        args: "0x".to_string(),
    };

    let deposit_tx = TransactionView {
        hash: format!("0x{}", "a1".repeat(32)),
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
            lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
            type_: Some(dao_type.clone()),
        }],
        outputs_data: vec![format!("0x{}", "00".repeat(8))],
        witnesses: vec!["0x".to_string()],
    };

    let request_tx = TransactionView {
        hash: format!("0x{}", "a2".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: deposit_tx.hash.clone(),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 200_00000000u64),
            lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
            type_: Some(dao_type),
        }],
        outputs_data: vec![format!("0x{}", hex::encode(100u64.to_le_bytes()))],
        witnesses: vec!["0x".to_string()],
    };

    let completion_tx = TransactionView {
        hash: format!("0x{}", "a3".repeat(32)),
        version: "0x0".to_string(),
        cell_deps: vec![],
        header_deps: vec![],
        inputs: vec![CellInput {
            since: "0x0".to_string(),
            previous_output: OutPoint {
                tx_hash: request_tx.hash.clone(),
                index: "0x0".to_string(),
            },
        }],
        outputs: vec![CellOutput {
            capacity: format!("0x{:x}", 219_60000000u64),
            lock: fixture_lock_script(&format!("0x{}", "01".repeat(20))),
            type_: None,
        }],
        outputs_data: vec!["0x".to_string()],
        witnesses: vec!["0x".to_string()],
    };

    vec![
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(100, 10_000),
                uncles: vec![],
                transactions: vec![deposit_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(101, 12_000),
                uncles: vec![],
                transactions: vec![request_tx],
                proposals: vec![],
            },
            cycles: None,
        },
        BlockResponseWithCycles {
            block: BlockView {
                header: fixture_header(102, 13_000),
                uncles: vec![],
                transactions: vec![completion_tx],
                proposals: vec![],
            },
            cycles: None,
        },
    ]
}

/// Phase 1: deposit with status=0.
#[test]
fn test_dao_deposit_creation() {
    let store = setup_store();

    let outpoint_key = ckbadger_store::keys::encode_outpoint(&[0xaa; 32], 0);
    let entry = DaoDepositCacheEntry {
        capacity: 100_000_000_000,
        occupied_capacity: 102_00000000,
        deposit_block_number: 5000,
        deposit_timestamp: 0,
        lock_script_hash: vec![0x11; 32],
        deposit_ar: 10_000_000_000,
        status: 0, // deposited
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_dao_deposit(&outpoint_key, &entry);
    batch.commit().unwrap();

    let retrieved = store.get_dao_deposit(&outpoint_key).unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.capacity, 100_000_000_000);
    assert_eq!(retrieved.deposit_block_number, 5000);
    assert_eq!(retrieved.lock_script_hash, vec![0x11; 32]);
    assert_eq!(retrieved.deposit_ar, 10_000_000_000);
    assert_eq!(retrieved.status, 0);
    assert!(retrieved.withdraw_request_tx.is_none());
    assert!(retrieved.compensation.is_none());
}

/// Phase 2: update deposit to withdraw-requested (status=1).
#[test]
fn test_dao_withdraw_request() {
    let store = setup_store();

    let outpoint_key = ckbadger_store::keys::encode_outpoint(&[0xbb; 32], 0);
    let entry = DaoDepositCacheEntry {
        capacity: 200_000_000_000,
        occupied_capacity: 102_00000000,
        deposit_block_number: 6000,
        deposit_timestamp: 0,
        lock_script_hash: vec![0x22; 32],
        deposit_ar: 10_000_000_000,
        status: 0,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };

    // First, deposit
    let mut batch = StoreBatch::new(&store);
    batch.put_dao_deposit(&outpoint_key, &entry);
    batch.commit().unwrap();

    // Then, update to withdraw-requested
    let updated_entry = DaoDepositCacheEntry {
        capacity: 200_000_000_000,
        occupied_capacity: 102_00000000,
        deposit_block_number: 6000,
        deposit_timestamp: 0,
        lock_script_hash: vec![0x22; 32],
        deposit_ar: 10_000_000_000,
        status: 1, // withdraw_requested
        withdraw_request_tx: Some(vec![0xcc; 32]),
        withdraw_request_output_index: Some(0),
        withdraw_request_block: Some(7000),
        withdraw_request_ar: Some(10_200_000_000),
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_dao_deposit(&outpoint_key, &updated_entry);
    batch.commit().unwrap();

    let retrieved = store.get_dao_deposit(&outpoint_key).unwrap().unwrap();
    assert_eq!(retrieved.status, 1);
    assert_eq!(retrieved.withdraw_request_tx, Some(vec![0xcc; 32]));
    assert_eq!(retrieved.withdraw_request_block, Some(7000));
    assert_eq!(retrieved.withdraw_request_ar, Some(10_200_000_000));
    assert!(retrieved.withdraw_block.is_none());
    assert!(retrieved.compensation.is_none());
}

#[test]
fn test_dao_put_twice_in_same_batch_keeps_secondary_indexes_consistent() {
    let store = setup_store();

    let outpoint_key = ckbadger_store::keys::encode_outpoint(&[0xbc; 32], 0);
    let first_entry = DaoDepositCacheEntry {
        capacity: 210_000_000_000,
        occupied_capacity: 102_00000000,
        deposit_block_number: 6000,
        deposit_timestamp: 0,
        lock_script_hash: vec![0x31; 32],
        deposit_ar: 10_000_000_000,
        status: 0,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };
    let second_entry = DaoDepositCacheEntry {
        capacity: 210_000_000_000,
        occupied_capacity: 102_00000000,
        deposit_block_number: 6001,
        deposit_timestamp: 0,
        lock_script_hash: vec![0x32; 32],
        deposit_ar: 10_000_000_000,
        status: 1,
        withdraw_request_tx: Some(vec![0xcd; 32]),
        withdraw_request_output_index: Some(0),
        withdraw_request_block: Some(7001),
        withdraw_request_ar: Some(10_200_000_000),
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_dao_deposit(&outpoint_key, &first_entry);
    batch.put_dao_deposit(&outpoint_key, &second_entry);
    batch.commit().unwrap();

    let retrieved = store.get_dao_deposit(&outpoint_key).unwrap().unwrap();
    assert_eq!(retrieved.status, 1);
    assert_eq!(retrieved.lock_script_hash, vec![0x32; 32]);
    assert_eq!(retrieved.deposit_block_number, 6001);

    let status_zero = store
        .list_dao_deposits_by_status_paginated(0, 10, None)
        .unwrap();
    assert!(status_zero.is_empty());

    let status_one = store
        .list_dao_deposits_by_status_paginated(1, 10, None)
        .unwrap();
    assert_eq!(status_one.len(), 1);
    assert_eq!(status_one[0].0, outpoint_key);

    let by_old_lock = store
        .list_dao_deposits_by_lock_paginated(&[0x31; 32], 10, None)
        .unwrap();
    assert!(by_old_lock.is_empty());
    let by_new_lock = store
        .list_dao_deposits_by_lock_paginated(&[0x32; 32], 10, None)
        .unwrap();
    assert_eq!(by_new_lock.len(), 1);
    assert_eq!(by_new_lock[0].0, outpoint_key);
}

/// Phase 3: complete withdrawal (status=2) with compensation.
#[test]
fn test_dao_withdrawal_completion() {
    let store = setup_store();

    let outpoint_key = ckbadger_store::keys::encode_outpoint(&[0xdd; 32], 0);
    let entry = DaoDepositCacheEntry {
        capacity: 300_000_000_000,
        occupied_capacity: 102_00000000,
        deposit_block_number: 8000,
        deposit_timestamp: 0,
        lock_script_hash: vec![0x33; 32],
        deposit_ar: 10_000_000_000,
        status: 2, // withdrawn
        withdraw_request_tx: Some(vec![0xee; 32]),
        withdraw_request_output_index: Some(0),
        withdraw_request_block: Some(8500),
        withdraw_request_ar: Some(10_300_000_000),
        withdraw_block: Some(9000),
        withdraw_tx: Some(vec![0xff; 32]),
        withdraw_to_output_index: Some(0),
        compensation: Some(1_500_000_000),
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_dao_deposit(&outpoint_key, &entry);
    batch.commit().unwrap();

    let retrieved = store.get_dao_deposit(&outpoint_key).unwrap().unwrap();
    assert_eq!(retrieved.status, 2);
    assert_eq!(retrieved.withdraw_block, Some(9000));
    assert_eq!(retrieved.withdraw_tx, Some(vec![0xff; 32]));
    assert_eq!(retrieved.compensation, Some(1_500_000_000));
}

/// Insert multiple deposits and list them all.
#[test]
fn test_list_dao_deposits() {
    let store = setup_store();

    let entries = vec![
        (
            ckbadger_store::keys::encode_outpoint(&[0x01; 32], 0),
            0i16,
            100_000_000_000i64,
        ),
        (
            ckbadger_store::keys::encode_outpoint(&[0x02; 32], 0),
            1,
            200_000_000_000,
        ),
        (
            ckbadger_store::keys::encode_outpoint(&[0x03; 32], 0),
            2,
            300_000_000_000,
        ),
    ];

    let mut batch = StoreBatch::new(&store);
    for (key, status, capacity) in &entries {
        let entry = DaoDepositCacheEntry {
            capacity: *capacity,
            occupied_capacity: 102_00000000,
            deposit_block_number: 1000,
            deposit_timestamp: 0,
            lock_script_hash: vec![0x44; 32],
            deposit_ar: 10_000_000_000,
            status: *status,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        batch.put_dao_deposit(key, &entry);
    }
    batch.commit().unwrap();

    let all = store.list_dao_deposits().unwrap();
    assert_eq!(all.len(), 3);

    // Verify all capacities are present
    let capacities: Vec<i64> = all.iter().map(|(_, e)| e.capacity).collect();
    assert!(capacities.contains(&100_000_000_000));
    assert!(capacities.contains(&200_000_000_000));
    assert!(capacities.contains(&300_000_000_000));
}

/// Insert active (status=0) and withdrawn (status=2) deposits,
/// verify list_active_dao_deposits returns only active ones.
#[test]
fn test_list_active_dao_deposits() {
    let store = setup_store();

    // Active deposit (status=0)
    let active_entry = DaoDepositCacheEntry {
        capacity: 500_000_000_000,
        occupied_capacity: 102_00000000,
        deposit_block_number: 2000,
        deposit_timestamp: 0,
        lock_script_hash: vec![0x55; 32],
        deposit_ar: 10_000_000_000,
        status: 0,
        withdraw_request_tx: None,
        withdraw_request_output_index: None,
        withdraw_request_block: None,
        withdraw_request_ar: None,
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };

    // Withdraw-requested deposit (status=1)
    let requested_entry = DaoDepositCacheEntry {
        capacity: 600_000_000_000,
        occupied_capacity: 102_00000000,
        deposit_block_number: 2500,
        deposit_timestamp: 0,
        lock_script_hash: vec![0x66; 32],
        deposit_ar: 10_000_000_000,
        status: 1,
        withdraw_request_tx: Some(vec![0x77; 32]),
        withdraw_request_output_index: Some(0),
        withdraw_request_block: Some(3000),
        withdraw_request_ar: Some(10_100_000_000),
        withdraw_block: None,
        withdraw_tx: None,
        withdraw_to_output_index: None,
        compensation: None,
    };

    // Withdrawn deposit (status=2)
    let withdrawn_entry = DaoDepositCacheEntry {
        capacity: 700_000_000_000,
        occupied_capacity: 102_00000000,
        deposit_block_number: 3000,
        deposit_timestamp: 0,
        lock_script_hash: vec![0x88; 32],
        deposit_ar: 10_000_000_000,
        status: 2,
        withdraw_request_tx: Some(vec![0x99; 32]),
        withdraw_request_output_index: Some(0),
        withdraw_request_block: Some(3500),
        withdraw_request_ar: Some(10_200_000_000),
        withdraw_block: Some(4000),
        withdraw_tx: Some(vec![0xaa; 32]),
        withdraw_to_output_index: Some(0),
        compensation: Some(1_000_000_000),
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0x10; 32], 0),
        &active_entry,
    );
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0x20; 32], 0),
        &requested_entry,
    );
    batch.put_dao_deposit(
        &ckbadger_store::keys::encode_outpoint(&[0x30; 32], 0),
        &withdrawn_entry,
    );
    batch.commit().unwrap();

    // list_active_dao_deposits filters status == 0 only
    let active = store.list_active_dao_deposits().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].1.capacity, 500_000_000_000);
    assert_eq!(active[0].1.status, 0);
}

#[test]
fn bulk_build_dao_owner_materializes_final_deposit_status_and_indexes_without_db_reads() {
    let snapshot = materialize_dao_state_for_test(&bulk_build_dao_fixture()).expect("dao snapshot");

    assert_eq!(snapshot.deposits.len(), 1);
    let (outpoint_key, entry) = snapshot.deposits.iter().next().expect("dao entry");
    assert_eq!(entry.capacity, 200_00000000);
    assert_eq!(entry.deposit_block_number, 100);
    assert_eq!(entry.deposit_ar, 10_000);
    assert_eq!(entry.status, 2);
    assert_eq!(entry.withdraw_request_tx, Some(vec![0xa2; 32]));
    assert_eq!(entry.withdraw_request_output_index, Some(0));
    assert_eq!(entry.withdraw_request_block, Some(101));
    assert_eq!(
        entry.withdraw_request_ar,
        Some(12_000),
        "completed bulk entries retain request AR for phase-2 rollback"
    );
    assert_eq!(entry.withdraw_block, Some(102));
    assert_eq!(entry.withdraw_tx, Some(vec![0xa3; 32]));
    assert_eq!(entry.withdraw_to_output_index, Some(0));
    assert_eq!(entry.compensation, Some(19_60000000));

    assert_eq!(
        snapshot
            .withdraw_lookup
            .get(&vec![0xa2; 32])
            .and_then(|rows| rows.get(&0)),
        Some(outpoint_key)
    );
    assert!(snapshot
        .by_status
        .get(&0)
        .is_some_and(|rows| rows.is_empty()));
    assert!(snapshot
        .by_status
        .get(&1)
        .is_some_and(|rows| rows.is_empty()));
    assert_eq!(
        snapshot.by_status.get(&2),
        Some(&vec![outpoint_key.clone()])
    );
    assert_eq!(snapshot.by_lock.len(), 1);
    let (lock_hash, rows) = snapshot.by_lock.iter().next().expect("dao lock index");
    assert_eq!(lock_hash, &entry.lock_script_hash);
    assert_eq!(rows, &vec![outpoint_key.clone()]);
}
