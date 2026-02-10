use chrono::DateTime;
use ckbadger_store::types::{DailyBlockStats, DailyStats, EpochStats, HourlyStats, MinerStats};
use ckbadger_store::CkbadgerStore;
use std::sync::Arc;

fn setup_store() -> Arc<CkbadgerStore> {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
    std::mem::forget(dir);
    store
}

/// Put daily stats for a single date and verify retrieval.
#[test]
fn test_daily_stats_put_get() {
    let store = setup_store();

    let stats = DailyStats {
        blocks_count: 144,
        transactions_count: 1200,
        cells_created: 3000,
        cells_consumed: 2500,
        capacity_transferred: 50_000_000_000_000,
        total_live_cells: 100_000,
        total_dead_cells: 80_000,
        total_all_cells: 180_000,
        total_data_size: 500_000_000,
        knowledge_size: Some(200_000_000),
        avg_block_time_ms: Some(8200),
    };

    store.put_daily_stats("2024-01-01", &stats).unwrap();

    let retrieved = store.get_daily_stats("2024-01-01").unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.blocks_count, 144);
    assert_eq!(retrieved.transactions_count, 1200);
    assert_eq!(retrieved.cells_created, 3000);
    assert_eq!(retrieved.cells_consumed, 2500);
    assert_eq!(retrieved.capacity_transferred, 50_000_000_000_000);
    assert_eq!(retrieved.total_live_cells, 100_000);
    assert_eq!(retrieved.total_dead_cells, 80_000);
    assert_eq!(retrieved.total_all_cells, 180_000);
    assert_eq!(retrieved.total_data_size, 500_000_000);
    assert_eq!(retrieved.knowledge_size, Some(200_000_000));
    assert_eq!(retrieved.avg_block_time_ms, Some(8200));

    // Non-existent date returns None
    assert!(store.get_daily_stats("2099-12-31").unwrap().is_none());
}

/// Put hourly stats and verify retrieval.
#[test]
fn test_hourly_stats_put_get() {
    let store = setup_store();

    let stats = HourlyStats {
        hour: 2024010112,
        blocks_count: 6,
        transactions_count: 50,
        cells_created: 120,
        cells_consumed: 90,
        capacity_transferred: 2_000_000_000_000,
    };

    store.put_hourly_stats("2024010112", &stats).unwrap();

    let retrieved = store.get_hourly_stats("2024010112").unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.hour, 2024010112);
    assert_eq!(retrieved.blocks_count, 6);
    assert_eq!(retrieved.transactions_count, 50);
    assert_eq!(retrieved.cells_created, 120);
    assert_eq!(retrieved.cells_consumed, 90);
    assert_eq!(retrieved.capacity_transferred, 2_000_000_000_000);

    // Non-existent hour returns None
    assert!(store.get_hourly_stats("9999999999").unwrap().is_none());
}

/// Put epoch stats and verify retrieval with DateTime<Utc>.
#[test]
fn test_epoch_stats_put_get() {
    let store = setup_store();

    let start_ts = DateTime::from_timestamp(1704067200, 0).unwrap_or_default(); // 2024-01-01 00:00:00 UTC
    let end_ts = DateTime::from_timestamp(1704153600, 0).unwrap_or_default(); // 2024-01-02 00:00:00 UTC

    let stats = EpochStats {
        epoch_number: 100,
        start_block: 10_000,
        end_block: Some(11_799),
        blocks_count: 1800,
        length: 1800,
        start_timestamp: start_ts,
        end_timestamp: Some(end_ts),
        transactions_count: 15_000,
    };

    store.put_epoch_stats(100, &stats).unwrap();

    let retrieved = store.get_epoch_stats(100).unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.epoch_number, 100);
    assert_eq!(retrieved.start_block, 10_000);
    assert_eq!(retrieved.end_block, Some(11_799));
    assert_eq!(retrieved.blocks_count, 1800);
    assert_eq!(retrieved.length, 1800);
    assert_eq!(retrieved.start_timestamp, start_ts);
    assert_eq!(retrieved.end_timestamp, Some(end_ts));
    assert_eq!(retrieved.transactions_count, 15_000);

    // Non-existent epoch returns None
    assert!(store.get_epoch_stats(999).unwrap().is_none());
}

/// Put 2 miners for the same date and list all.
#[test]
fn test_miner_stats_aggregation() {
    let store = setup_store();

    let miner1_hash = vec![0x11; 32];
    let miner2_hash = vec![0x22; 32];

    let stats1 = MinerStats {
        miner_lock_hash: miner1_hash.clone(),
        blocks_count: 50,
        last_block_number: 10_500,
    };

    let stats2 = MinerStats {
        miner_lock_hash: miner2_hash.clone(),
        blocks_count: 30,
        last_block_number: 10_480,
    };

    store
        .put_miner_stats("2024-01-01", &miner1_hash, &stats1)
        .unwrap();
    store
        .put_miner_stats("2024-01-01", &miner2_hash, &stats2)
        .unwrap();

    let all = store.list_miner_stats().unwrap();
    assert_eq!(all.len(), 2);

    // Verify both miners are present
    let blocks: Vec<i32> = all.iter().map(|m| m.blocks_count).collect();
    assert!(blocks.contains(&50));
    assert!(blocks.contains(&30));

    let hashes: Vec<&Vec<u8>> = all.iter().map(|m| &m.miner_lock_hash).collect();
    assert!(hashes.contains(&&miner1_hash));
    assert!(hashes.contains(&&miner2_hash));
}

/// Put daily block stats and verify compact target and uncle count.
#[test]
fn test_daily_block_stats() {
    let store = setup_store();

    let stats = DailyBlockStats {
        avg_compact_target: 0.0042,
        block_count: 144,
        total_uncles: 12,
        avg_block_time_ms: Some(8100),
    };

    store.put_daily_block_stats("2024-01-15", &stats).unwrap();

    let retrieved = store.get_daily_block_stats("2024-01-15").unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert!((retrieved.avg_compact_target - 0.0042).abs() < f64::EPSILON);
    assert_eq!(retrieved.block_count, 144);
    assert_eq!(retrieved.total_uncles, 12);
    assert_eq!(retrieved.avg_block_time_ms, Some(8100));

    // Also verify through the list method
    let all = store.list_daily_block_stats().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].0, "2024-01-15");
    assert_eq!(all[0].1.block_count, 144);
}

/// Put multiple dates and list all daily stats.
#[test]
fn test_list_daily_stats() {
    let store = setup_store();

    let dates = ["2024-01-01", "2024-01-02", "2024-01-03"];

    for (i, date) in dates.iter().enumerate() {
        let stats = DailyStats {
            blocks_count: 144 + i as i32,
            transactions_count: 1000 + (i as i32 * 100),
            cells_created: 2000 + (i as i32 * 200),
            cells_consumed: 1500 + (i as i32 * 150),
            capacity_transferred: 10_000_000_000_000 * (i as i64 + 1),
            total_live_cells: 50_000 + (i as i64 * 1000),
            total_dead_cells: 40_000 + (i as i64 * 800),
            total_all_cells: 90_000 + (i as i64 * 1800),
            total_data_size: 100_000_000 + (i as i64 * 10_000_000),
            knowledge_size: None,
            avg_block_time_ms: None,
        };
        store.put_daily_stats(date, &stats).unwrap();
    }

    let all = store.list_daily_stats().unwrap();
    assert_eq!(all.len(), 3);

    // Stats should be sorted by date key (lexicographic)
    assert_eq!(all[0].blocks_count, 144);
    assert_eq!(all[1].blocks_count, 145);
    assert_eq!(all[2].blocks_count, 146);

    // Verify individual gets still work
    let jan2 = store.get_daily_stats("2024-01-02").unwrap().unwrap();
    assert_eq!(jan2.transactions_count, 1100);
    assert_eq!(jan2.cells_created, 2200);
}
