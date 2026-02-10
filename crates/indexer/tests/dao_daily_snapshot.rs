use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{DaoStats, SecondaryIssuance};
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
    std::mem::forget(dir);
    store
}

/// Insert DAO stats with the "global" key and retrieve them.
#[test]
fn test_dao_stats_put_get() {
    let store = setup_store();

    let stats = DaoStats {
        total_deposited: 5_000_000_000_000_000,
        total_depositors: 150,
        total_compensation: 200_000_000_000,
        total_deposits: 500,
        total_withdrawals: 120,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_dao_stats(b"global", &stats);
    batch.commit().unwrap();

    let retrieved = store.get_dao_stats(b"global").unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.total_deposited, 5_000_000_000_000_000);
    assert_eq!(retrieved.total_depositors, 150);
    assert_eq!(retrieved.total_compensation, 200_000_000_000);
    assert_eq!(retrieved.total_deposits, 500);
    assert_eq!(retrieved.total_withdrawals, 120);
}

/// Insert DAO stats under different keys (e.g., per-epoch snapshots).
#[test]
fn test_dao_stats_with_different_keys() {
    let store = setup_store();

    let stats_epoch_100 = DaoStats {
        total_deposited: 1_000_000_000_000,
        total_depositors: 10,
        total_compensation: 5_000_000_000,
        total_deposits: 20,
        total_withdrawals: 5,
    };

    let stats_epoch_200 = DaoStats {
        total_deposited: 2_000_000_000_000,
        total_depositors: 25,
        total_compensation: 15_000_000_000,
        total_deposits: 50,
        total_withdrawals: 15,
    };

    let stats_epoch_300 = DaoStats {
        total_deposited: 3_500_000_000_000,
        total_depositors: 40,
        total_compensation: 30_000_000_000,
        total_deposits: 80,
        total_withdrawals: 25,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_dao_stats(b"epoch:100", &stats_epoch_100);
    batch.put_dao_stats(b"epoch:200", &stats_epoch_200);
    batch.put_dao_stats(b"epoch:300", &stats_epoch_300);
    batch.commit().unwrap();

    // Retrieve each key independently
    let r100 = store.get_dao_stats(b"epoch:100").unwrap().unwrap();
    assert_eq!(r100.total_deposited, 1_000_000_000_000);
    assert_eq!(r100.total_depositors, 10);

    let r200 = store.get_dao_stats(b"epoch:200").unwrap().unwrap();
    assert_eq!(r200.total_deposited, 2_000_000_000_000);
    assert_eq!(r200.total_depositors, 25);

    let r300 = store.get_dao_stats(b"epoch:300").unwrap().unwrap();
    assert_eq!(r300.total_deposited, 3_500_000_000_000);
    assert_eq!(r300.total_deposits, 80);

    // Non-existent key returns None
    let missing = store.get_dao_stats(b"epoch:999").unwrap();
    assert!(missing.is_none());
}

/// Insert block issuance data for a single block and retrieve it.
#[test]
fn test_block_issuance_put_get() {
    let store = setup_store();

    let issuance = SecondaryIssuance {
        miner_reward: 1_000_000_000,
        dao_reward: 500_000_000,
        treasury: 300_000_000,
    };

    let mut batch = StoreBatch::new(&store);
    batch.put_block_issuance(10_000, &issuance);
    batch.commit().unwrap();

    let retrieved = store.get_block_issuance(10_000).unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.miner_reward, 1_000_000_000);
    assert_eq!(retrieved.dao_reward, 500_000_000);
    assert_eq!(retrieved.treasury, 300_000_000);

    // Non-existent block returns None
    let missing = store.get_block_issuance(99_999).unwrap();
    assert!(missing.is_none());
}

/// Insert block issuance for multiple blocks and verify each independently.
#[test]
fn test_block_issuance_multiple_blocks() {
    let store = setup_store();

    let blocks = vec![
        (
            100i64,
            SecondaryIssuance {
                miner_reward: 100_000_000,
                dao_reward: 50_000_000,
                treasury: 30_000_000,
            },
        ),
        (
            200,
            SecondaryIssuance {
                miner_reward: 200_000_000,
                dao_reward: 100_000_000,
                treasury: 60_000_000,
            },
        ),
        (
            300,
            SecondaryIssuance {
                miner_reward: 300_000_000,
                dao_reward: 150_000_000,
                treasury: 90_000_000,
            },
        ),
    ];

    let mut batch = StoreBatch::new(&store);
    for (block_num, issuance) in &blocks {
        batch.put_block_issuance(*block_num, issuance);
    }
    batch.commit().unwrap();

    // Verify each block
    let r100 = store.get_block_issuance(100).unwrap().unwrap();
    assert_eq!(r100.miner_reward, 100_000_000);
    assert_eq!(r100.dao_reward, 50_000_000);
    assert_eq!(r100.treasury, 30_000_000);

    let r200 = store.get_block_issuance(200).unwrap().unwrap();
    assert_eq!(r200.miner_reward, 200_000_000);
    assert_eq!(r200.dao_reward, 100_000_000);

    let r300 = store.get_block_issuance(300).unwrap().unwrap();
    assert_eq!(r300.miner_reward, 300_000_000);
    assert_eq!(r300.treasury, 90_000_000);

    // Gaps return None
    assert!(store.get_block_issuance(150).unwrap().is_none());
    assert!(store.get_block_issuance(250).unwrap().is_none());
}
