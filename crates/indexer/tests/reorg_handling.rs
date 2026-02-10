#![allow(clippy::type_complexity)]

use chrono::Timelike;
use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

async fn insert_test_block(pool: &PgPool, number: i64, hash: &[u8], _parent_hash: &[u8]) {
    let dao = vec![0u8; 32];

    sqlx::query(
        r#"
        INSERT INTO blocks_index (
            number, hash, timestamp, tx_count, proposals_count, uncles_count,
            epoch_number, epoch_index, epoch_length, compact_target, dao
        ) VALUES ($1, $2, NOW(), 2, 0, 0, 100, 50, 1800, 0, $3)
        "#,
    )
    .bind(number)
    .bind(hash)
    .bind(&dao)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_test_transaction(pool: &PgPool, hash: &[u8], block_number: i64, tx_index: i32) {
    sqlx::query(
        r#"
        INSERT INTO transactions_index (
            hash, block_number, tx_index, inputs_count, outputs_count,
            fee, is_cellbase, timestamp, tx_size, cycles
        ) VALUES ($1, $2, $3, 1, 1, 1000, false, NOW(), 500, 1000000)
        "#,
    )
    .bind(hash)
    .bind(block_number)
    .bind(tx_index)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_test_cell(
    pool: &PgPool,
    tx_hash: &[u8],
    output_index: i32,
    created_at_block: i64,
    status: i16,
    consumed_at_block: Option<i64>,
) {
    let lock_hash = vec![0xAAu8; 32];
    let lock_code_hash = vec![0u8; 32];
    let data_hash = vec![0u8; 32];

    sqlx::query(
        r#"
        INSERT INTO cells (
            tx_hash, output_index, capacity,
            lock_code_hash, lock_hash_type, lock_args, lock_script_hash,
            data_hash, data_size, status, created_at_block, consumed_at_block
        ) VALUES ($1, $2, 10000000000, $3, 0, '', $4, $5, 0, $6, $7, $8)
        "#,
    )
    .bind(tx_hash)
    .bind(output_index)
    .bind(&lock_code_hash)
    .bind(&lock_hash)
    .bind(&data_hash)
    .bind(status)
    .bind(created_at_block)
    .bind(consumed_at_block)
    .execute(pool)
    .await
    .unwrap();
}

async fn get_block_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM blocks_index")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn get_transaction_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM transactions_index")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn get_cell_count(pool: &PgPool, status: i16) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM cells WHERE status = $1")
        .bind(status)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn get_reorg_event_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM reorg_events")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn get_orphaned_block_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM orphaned_blocks")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn get_orphaned_tx_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM orphaned_transactions")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn is_deep_fork_detected(pool: &PgPool) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE(deep_fork_detected, FALSE) FROM sync_status WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_record_deep_fork(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let fork_hash = vec![0x10u8; 32];
    let db_tip_hash = vec![0x20u8; 32];
    let chain_tip_hash = vec![0x30u8; 32];

    let event_id = writer
        .record_deep_fork(100, &fork_hash, 150, &db_tip_hash, 200, &chain_tip_hash, 50)
        .await
        .unwrap();

    assert!(event_id > 0);
    assert!(is_deep_fork_detected(&pool).await);

    let event: (String, i32, i64, i64, i64) = sqlx::query_as(
        "SELECT event_type, depth, fork_point_number, old_tip_number, new_tip_number FROM reorg_events WHERE id = $1",
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(event.0, "deep");
    assert_eq!(event.1, 50);
    assert_eq!(event.2, 100);
    assert_eq!(event.3, 150);
    assert_eq!(event.4, 200);

    let status: (Option<i64>, Option<i64>, Option<i32>, Option<i64>) = sqlx::query_as(
        "SELECT deep_fork_db_tip, deep_fork_chain_tip, deep_fork_depth, deep_fork_fork_point FROM sync_status WHERE id = 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(status.0, Some(150));
    assert_eq!(status.1, Some(200));
    assert_eq!(status.2, Some(50));
    assert_eq!(status.3, Some(100));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_clear_deep_fork_flag(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let fork_hash = vec![0x10u8; 32];
    let db_tip_hash = vec![0x20u8; 32];
    let chain_tip_hash = vec![0x30u8; 32];

    writer
        .record_deep_fork(100, &fork_hash, 150, &db_tip_hash, 200, &chain_tip_hash, 50)
        .await
        .unwrap();

    assert!(is_deep_fork_detected(&pool).await);

    writer.clear_deep_fork_flag().await.unwrap();

    assert!(!is_deep_fork_detected(&pool).await);

    let status: (Option<i64>, Option<i32>) =
        sqlx::query_as("SELECT deep_fork_db_tip, deep_fork_depth FROM sync_status WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();

    assert!(status.0.is_none());
    assert!(status.1.is_none());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_execute_reorg_rolls_back_blocks(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block_98_hash = vec![98u8; 32];
    let block_99_hash = vec![99u8; 32];
    let block_100_hash = vec![100u8; 32];
    let block_101_hash = vec![101u8; 32];
    let block_102_hash = vec![102u8; 32];

    insert_test_block(&pool, 98, &block_98_hash, &[97u8; 32]).await;
    insert_test_block(&pool, 99, &block_99_hash, &block_98_hash).await;
    insert_test_block(&pool, 100, &block_100_hash, &block_99_hash).await;
    insert_test_block(&pool, 101, &block_101_hash, &block_100_hash).await;
    insert_test_block(&pool, 102, &block_102_hash, &block_101_hash).await;

    assert_eq!(get_block_count(&pool).await, 5);

    let new_tip_hash = vec![0xFFu8; 32];

    let result = writer
        .execute_reorg(
            100,
            &block_100_hash,
            102,
            &block_102_hash,
            103,
            &new_tip_hash,
        )
        .await
        .unwrap();

    assert_eq!(result.depth, 2);
    assert_eq!(result.orphaned_blocks, 2);

    assert_eq!(get_block_count(&pool).await, 3);

    let max_block: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks_index")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(max_block, 100);

    assert_eq!(get_orphaned_block_count(&pool).await, 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_execute_reorg_rolls_back_transactions(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block_99_hash = vec![99u8; 32];
    let block_100_hash = vec![100u8; 32];
    let block_101_hash = vec![101u8; 32];

    insert_test_block(&pool, 99, &block_99_hash, &[98u8; 32]).await;
    insert_test_block(&pool, 100, &block_100_hash, &block_99_hash).await;
    insert_test_block(&pool, 101, &block_101_hash, &block_100_hash).await;

    let tx1 = vec![0x11u8; 32];
    let tx2 = vec![0x22u8; 32];
    let tx3 = vec![0x33u8; 32];
    let tx4 = vec![0x44u8; 32];

    insert_test_transaction(&pool, &tx1, 99, 0).await;
    insert_test_transaction(&pool, &tx2, 100, 0).await;
    insert_test_transaction(&pool, &tx3, 101, 0).await;
    insert_test_transaction(&pool, &tx4, 101, 1).await;

    assert_eq!(get_transaction_count(&pool).await, 4);

    let new_tip_hash = vec![0xFFu8; 32];

    let result = writer
        .execute_reorg(
            100,
            &block_100_hash,
            101,
            &block_101_hash,
            102,
            &new_tip_hash,
        )
        .await
        .unwrap();

    assert_eq!(result.orphaned_txs, 2);

    assert_eq!(get_transaction_count(&pool).await, 2);

    assert_eq!(get_orphaned_tx_count(&pool).await, 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_execute_reorg_reverts_consumed_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block_99_hash = vec![99u8; 32];
    let block_100_hash = vec![100u8; 32];
    let block_101_hash = vec![101u8; 32];

    insert_test_block(&pool, 99, &block_99_hash, &[98u8; 32]).await;
    insert_test_block(&pool, 100, &block_100_hash, &block_99_hash).await;
    insert_test_block(&pool, 101, &block_101_hash, &block_100_hash).await;

    let tx1 = vec![0x11u8; 32];
    let tx2 = vec![0x22u8; 32];

    insert_test_cell(&pool, &tx1, 0, 99, 0, None).await;
    insert_test_cell(&pool, &tx1, 1, 99, 1, Some(101)).await;
    insert_test_cell(&pool, &tx2, 0, 101, 0, None).await;

    assert_eq!(get_cell_count(&pool, 0).await, 2);
    assert_eq!(get_cell_count(&pool, 1).await, 1);

    let new_tip_hash = vec![0xFFu8; 32];

    writer
        .execute_reorg(
            100,
            &block_100_hash,
            101,
            &block_101_hash,
            102,
            &new_tip_hash,
        )
        .await
        .unwrap();

    assert_eq!(get_cell_count(&pool, 0).await, 2);
    assert_eq!(get_cell_count(&pool, 1).await, 0);

    let cell_status: i16 =
        sqlx::query_scalar("SELECT status FROM cells WHERE tx_hash = $1 AND output_index = 1")
            .bind(&tx1)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(cell_status, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_execute_reorg_deletes_new_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block_99_hash = vec![99u8; 32];
    let block_100_hash = vec![100u8; 32];
    let block_101_hash = vec![101u8; 32];

    insert_test_block(&pool, 99, &block_99_hash, &[98u8; 32]).await;
    insert_test_block(&pool, 100, &block_100_hash, &block_99_hash).await;
    insert_test_block(&pool, 101, &block_101_hash, &block_100_hash).await;

    let tx1 = vec![0x11u8; 32];
    let tx2 = vec![0x22u8; 32];

    insert_test_cell(&pool, &tx1, 0, 99, 0, None).await;
    insert_test_cell(&pool, &tx2, 0, 101, 0, None).await;

    let total_cells: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cells")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total_cells, 2);

    let new_tip_hash = vec![0xFFu8; 32];

    writer
        .execute_reorg(
            100,
            &block_100_hash,
            101,
            &block_101_hash,
            102,
            &new_tip_hash,
        )
        .await
        .unwrap();

    let total_cells: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cells")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total_cells, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_execute_reorg_updates_sync_status(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block_99_hash = vec![99u8; 32];
    let block_100_hash = vec![100u8; 32];
    let block_101_hash = vec![101u8; 32];

    insert_test_block(&pool, 99, &block_99_hash, &[98u8; 32]).await;
    insert_test_block(&pool, 100, &block_100_hash, &block_99_hash).await;
    insert_test_block(&pool, 101, &block_101_hash, &block_100_hash).await;

    let new_tip_hash = vec![0xFFu8; 32];

    writer
        .execute_reorg(
            100,
            &block_100_hash,
            101,
            &block_101_hash,
            102,
            &new_tip_hash,
        )
        .await
        .unwrap();

    let (last_reorg_depth, deep_fork_detected): (Option<i32>, bool) =
        sqlx::query_as("SELECT last_reorg_depth, deep_fork_detected FROM sync_status WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(last_reorg_depth, Some(1));
    assert!(!deep_fork_detected);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_execute_reorg_creates_event_record(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block_99_hash = vec![99u8; 32];
    let block_100_hash = vec![100u8; 32];

    insert_test_block(&pool, 99, &block_99_hash, &[98u8; 32]).await;
    insert_test_block(&pool, 100, &block_100_hash, &block_99_hash).await;

    assert_eq!(get_reorg_event_count(&pool).await, 0);

    let new_tip_hash = vec![0xFFu8; 32];

    let result = writer
        .execute_reorg(99, &block_99_hash, 100, &block_100_hash, 101, &new_tip_hash)
        .await
        .unwrap();

    assert_eq!(get_reorg_event_count(&pool).await, 1);

    let event: (String, i32) =
        sqlx::query_as("SELECT event_type, depth FROM reorg_events WHERE id = $1")
            .bind(result.event_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(event.0, "auto");
    assert_eq!(event.1, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_resolve_deep_fork(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let fork_hash = vec![0x10u8; 32];
    let db_tip_hash = vec![0x20u8; 32];
    let chain_tip_hash = vec![0x30u8; 32];

    writer
        .record_deep_fork(100, &fork_hash, 150, &db_tip_hash, 200, &chain_tip_hash, 50)
        .await
        .unwrap();

    assert!(is_deep_fork_detected(&pool).await);

    writer
        .resolve_deep_fork(
            "dismissed",
            Some("admin"),
            Some("Manual verification complete"),
        )
        .await
        .unwrap();

    assert!(!is_deep_fork_detected(&pool).await);

    let event: (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT event_type, resolution_action, resolution_notes FROM reorg_events WHERE event_type = 'resolved'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(event.0, "resolved");
    assert_eq!(event.1, Some("dismissed".to_string()));
    assert_eq!(event.2, Some("Manual verification complete".to_string()));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_execute_reorg_rolls_back_hourly_statistics(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block_99_hash = vec![0x99u8; 32];
    let block_100_hash = vec![0xA0u8; 32];
    let block_101_hash = vec![0xA1u8; 32];
    let block_102_hash = vec![0xA2u8; 32];
    let new_tip_hash = vec![0xFFu8; 32];

    let base_time = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(10, 0, 0)
        .unwrap()
        .and_utc();

    for (i, hash) in [
        &block_99_hash,
        &block_100_hash,
        &block_101_hash,
        &block_102_hash,
    ]
    .iter()
    .enumerate()
    {
        let block_num = 99 + i as i64;
        let _parent = if i == 0 {
            vec![0x98u8; 32]
        } else {
            [&block_99_hash, &block_100_hash, &block_101_hash][i - 1].clone()
        };
        let timestamp = base_time + chrono::Duration::seconds(i as i64 * 10);

        sqlx::query(
            r#"
            INSERT INTO blocks_index (
                number, hash, timestamp, tx_count, proposals_count, uncles_count,
                epoch_number, epoch_index, epoch_length, compact_target, dao
            ) VALUES ($1, $2, $3, $4, 0, 0, 100, 50, 1800, 0, $5)
            "#,
        )
        .bind(block_num)
        .bind(*hash)
        .bind(timestamp)
        .bind(if block_num >= 101 { 5 } else { 2 })
        .bind(vec![0u8; 32])
        .execute(&pool)
        .await
        .unwrap();
    }

    let hour = base_time
        .with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap();

    sqlx::query(
        r#"
        INSERT INTO hourly_statistics (hour, blocks_count, transactions_count, cells_created, cells_consumed)
        VALUES ($1, 4, 14, 10, 5)
        "#,
    )
    .bind(hour)
    .execute(&pool)
    .await
    .unwrap();

    writer
        .execute_reorg(
            100,
            &block_100_hash,
            101,
            &block_101_hash,
            102,
            &new_tip_hash,
        )
        .await
        .unwrap();

    let (blocks_after, txs_after): (i32, i32) = sqlx::query_as(
        "SELECT blocks_count, transactions_count FROM hourly_statistics WHERE hour = $1",
    )
    .bind(hour)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(blocks_after, 2, "Should subtract 2 rolled back blocks");
    assert_eq!(
        txs_after, 4,
        "Should subtract 10 transactions (5+5) from rolled back blocks"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_execute_reorg_rolls_back_daily_statistics(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block_99_hash = vec![0x99u8; 32];
    let block_100_hash = vec![0xA0u8; 32];
    let block_101_hash = vec![0xA1u8; 32];
    let new_tip_hash = vec![0xFFu8; 32];

    let today = chrono::Utc::now().date_naive();
    let base_time = today.and_hms_opt(12, 0, 0).unwrap().and_utc();

    for (i, (hash, tx_count)) in [
        (&block_99_hash, 2),
        (&block_100_hash, 3),
        (&block_101_hash, 5),
    ]
    .iter()
    .enumerate()
    {
        let block_num = 99 + i as i64;
        let _parent = if i == 0 {
            vec![0x98u8; 32]
        } else {
            [&block_99_hash, &block_100_hash][i - 1].clone()
        };

        sqlx::query(
            r#"
            INSERT INTO blocks_index (
                number, hash, timestamp, tx_count, proposals_count, uncles_count,
                epoch_number, epoch_index, epoch_length, compact_target, dao
            ) VALUES ($1, $2, $3, $4, 0, 0, 100, 50, 1800, 0, $5)
            "#,
        )
        .bind(block_num)
        .bind(*hash)
        .bind(base_time + chrono::Duration::seconds(i as i64 * 10))
        .bind(*tx_count)
        .bind(vec![0u8; 32])
        .execute(&pool)
        .await
        .unwrap();
    }

    sqlx::query(
        r#"
        INSERT INTO daily_statistics (date, blocks_count, transactions_count, cells_created, cells_consumed, total_live_cells, total_data_size)
        VALUES ($1, 3, 10, 6, 2, 100, 5000)
        "#,
    )
    .bind(today)
    .execute(&pool)
    .await
    .unwrap();

    writer
        .execute_reorg(
            100,
            &block_100_hash,
            100,
            &block_100_hash,
            101,
            &new_tip_hash,
        )
        .await
        .unwrap();

    let (blocks_after, txs_after): (i32, i32) = sqlx::query_as(
        "SELECT blocks_count, transactions_count FROM daily_statistics WHERE date = $1",
    )
    .bind(today)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(blocks_after, 2, "Should subtract 1 rolled back block");
    assert_eq!(
        txs_after, 5,
        "Should subtract 5 transactions from rolled back block"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_execute_reorg_rolls_back_miner_statistics(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block_99_hash = vec![0x99u8; 32];
    let block_100_hash = vec![0xA0u8; 32];
    let block_101_hash = vec![0xA1u8; 32];
    let new_tip_hash = vec![0xFFu8; 32];
    let miner_hash = vec![0xBBu8; 32];

    let today = chrono::Utc::now().date_naive();
    let base_time = today.and_hms_opt(12, 0, 0).unwrap().and_utc();

    for (i, hash) in [&block_99_hash, &block_100_hash, &block_101_hash]
        .iter()
        .enumerate()
    {
        let block_num = 99 + i as i64;
        let _parent = if i == 0 {
            vec![0x98u8; 32]
        } else {
            [&block_99_hash, &block_100_hash][i - 1].clone()
        };

        sqlx::query(
            r#"
            INSERT INTO blocks_index (
                number, hash, timestamp, tx_count, proposals_count, uncles_count,
                epoch_number, epoch_index, epoch_length, compact_target, dao, miner_lock_hash
            ) VALUES ($1, $2, $3, 2, 0, 0, 100, 50, 1800, 0, $4, $5)
            "#,
        )
        .bind(block_num)
        .bind(*hash)
        .bind(base_time + chrono::Duration::seconds(i as i64 * 10))
        .bind(vec![0u8; 32])
        .bind(&miner_hash)
        .execute(&pool)
        .await
        .unwrap();
    }

    sqlx::query(
        r#"
        INSERT INTO miner_statistics (date, miner_lock_hash, blocks_count, last_block_number)
        VALUES ($1, $2, 3, 101)
        "#,
    )
    .bind(today)
    .bind(&miner_hash)
    .execute(&pool)
    .await
    .unwrap();

    writer
        .execute_reorg(
            100,
            &block_100_hash,
            100,
            &block_100_hash,
            101,
            &new_tip_hash,
        )
        .await
        .unwrap();

    let (blocks_after,): (i32,) = sqlx::query_as(
        "SELECT blocks_count FROM miner_statistics WHERE date = $1 AND miner_lock_hash = $2",
    )
    .bind(today)
    .bind(&miner_hash)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        blocks_after, 2,
        "Should subtract 1 rolled back block from miner stats"
    );
}
