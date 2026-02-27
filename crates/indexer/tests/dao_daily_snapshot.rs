use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::SecondaryIssuance;
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
    std::mem::forget(dir);
    store
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
