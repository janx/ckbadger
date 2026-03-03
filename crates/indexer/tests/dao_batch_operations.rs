use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::DaoDepositCacheEntry;
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
    std::mem::forget(dir);
    store
}

/// Phase 1: deposit with status=0.
#[test]
fn test_dao_deposit_creation() {
    let store = setup_store();

    let outpoint_key = ckbadger_store::keys::encode_outpoint(&[0xaa; 32], 0);
    let entry = DaoDepositCacheEntry {
        capacity: 100_000_000_000,
        deposit_block_number: 5000,
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
        deposit_block_number: 6000,
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
        deposit_block_number: 6000,
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

/// Phase 3: complete withdrawal (status=2) with compensation.
#[test]
fn test_dao_withdrawal_completion() {
    let store = setup_store();

    let outpoint_key = ckbadger_store::keys::encode_outpoint(&[0xdd; 32], 0);
    let entry = DaoDepositCacheEntry {
        capacity: 300_000_000_000,
        deposit_block_number: 8000,
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
            deposit_block_number: 1000,
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
        deposit_block_number: 2000,
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
        deposit_block_number: 2500,
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
        deposit_block_number: 3000,
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
