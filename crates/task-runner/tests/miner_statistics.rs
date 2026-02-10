use chrono::{NaiveDate, TimeZone, Utc};
use ckbadger_common::StatisticsRebuildConfig;
use ckbadger_task_runner::db::TaskDb;
use ckbadger_task_runner::executor::statistics::execute;
use ckbadger_task_runner::MIGRATOR;
use sqlx::PgPool;

async fn insert_test_block(pool: &PgPool, number: i64, date: NaiveDate) {
    let timestamp = Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap());
    let hash: Vec<u8> = vec![number as u8; 32];
    let dao_bytes: Vec<u8> = vec![0u8; 32];

    sqlx::query(
        r#"
        INSERT INTO blocks_index (number, hash, timestamp, tx_count, proposals_count,
            uncles_count, epoch_number, epoch_index, epoch_length, compact_target, dao)
        VALUES ($1, $2, $3, 1, 0, 0, 0, 0, 1, 0, $4)
        "#,
    )
    .bind(number)
    .bind(&hash)
    .bind(timestamp)
    .bind(&dao_bytes)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_test_transaction(
    pool: &PgPool,
    tx_hash: &[u8],
    block_number: i64,
    tx_index: i32,
    timestamp: chrono::DateTime<Utc>,
) {
    sqlx::query(
        r#"
        INSERT INTO transactions_index (hash, block_number, tx_index,
            inputs_count, outputs_count, is_cellbase, timestamp)
        VALUES ($1, $2, $3, 0, 1, $3 = 0, $4)
        "#,
    )
    .bind(tx_hash)
    .bind(block_number)
    .bind(tx_index)
    .bind(timestamp)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_test_cell(
    pool: &PgPool,
    tx_hash: &[u8],
    output_index: i32,
    capacity: i64,
    block_number: i64,
    lock_script_hash: &[u8],
) {
    let data_hash: Vec<u8> = vec![0u8; 32];
    sqlx::query(
        r#"
        INSERT INTO cells (tx_hash, output_index, capacity, created_at_block, lock_script_hash,
            lock_code_hash, lock_hash_type, lock_args, data_hash, status)
        VALUES ($1, $2, $3, $4, $5, $5, 0, '', $6, 0)
        "#,
    )
    .bind(tx_hash)
    .bind(output_index)
    .bind(capacity)
    .bind(block_number)
    .bind(lock_script_hash)
    .bind(&data_hash)
    .execute(pool)
    .await
    .unwrap();
}

async fn create_pending_task(pool: &PgPool) -> uuid::Uuid {
    let task_id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO tasks (id, task_type, status, priority, created_at)
        VALUES ($1, 'statistics_rebuild', 'running', 5, NOW())
        "#,
    )
    .bind(task_id)
    .execute(pool)
    .await
    .unwrap();
    task_id
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_rebuild_miner_statistics_via_cellbase_join(pool: PgPool) {
    let date1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let date2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let ts1 = Utc.from_utc_datetime(&date1.and_hms_opt(12, 0, 0).unwrap());
    let ts2 = Utc.from_utc_datetime(&date2.and_hms_opt(12, 0, 0).unwrap());

    insert_test_block(&pool, 1, date1).await;
    insert_test_block(&pool, 2, date1).await;
    insert_test_block(&pool, 3, date2).await;

    let miner1_hash: Vec<u8> = vec![0xAA; 32];
    let miner2_hash: Vec<u8> = vec![0xBB; 32];

    let tx1: Vec<u8> = vec![1; 32];
    let tx2: Vec<u8> = vec![2; 32];
    let tx3: Vec<u8> = vec![3; 32];

    insert_test_transaction(&pool, &tx1, 1, 0, ts1).await;
    insert_test_transaction(&pool, &tx2, 2, 0, ts1).await;
    insert_test_transaction(&pool, &tx3, 3, 0, ts2).await;

    insert_test_cell(&pool, &tx1, 0, 100_00000000, 1, &miner1_hash).await;
    insert_test_cell(&pool, &tx2, 0, 100_00000000, 2, &miner1_hash).await;
    insert_test_cell(&pool, &tx3, 0, 100_00000000, 3, &miner2_hash).await;

    let task_id = create_pending_task(&pool).await;
    let task_db = TaskDb::new(pool.clone());

    let config = StatisticsRebuildConfig {
        tables: Some(vec!["miner_statistics".to_string()]),
    };

    execute(&task_db, &pool, task_id, &config).await.unwrap();

    let rows = sqlx::query_as::<_, (NaiveDate, Vec<u8>, i32)>(
        "SELECT date, miner_lock_hash, blocks_count FROM miner_statistics ORDER BY date, blocks_count DESC",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(
        rows.len(),
        2,
        "Should have 2 miner entries: (date1, miner1, 2 blocks) and (date2, miner2, 1 block)"
    );

    let (d1, hash1, count1) = &rows[0];
    assert_eq!(*d1, date1);
    assert_eq!(*hash1, miner1_hash);
    assert_eq!(*count1, 2, "Miner1 mined 2 blocks on date1");

    let (d2, hash2, count2) = &rows[1];
    assert_eq!(*d2, date2);
    assert_eq!(*hash2, miner2_hash);
    assert_eq!(*count2, 1, "Miner2 mined 1 block on date2");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_rebuild_miner_statistics_with_null_miner_lock_hash_in_blocks(pool: PgPool) {
    let date1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let ts1 = Utc.from_utc_datetime(&date1.and_hms_opt(12, 0, 0).unwrap());
    insert_test_block(&pool, 1, date1).await;

    let miner_hash: Vec<u8> = vec![0xCC; 32];
    let tx_hash: Vec<u8> = vec![1; 32];

    insert_test_transaction(&pool, &tx_hash, 1, 0, ts1).await;
    insert_test_cell(&pool, &tx_hash, 0, 100_00000000, 1, &miner_hash).await;

    let miner_lock_in_blocks: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT miner_lock_hash FROM blocks_index WHERE number = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        miner_lock_in_blocks.is_none(),
        "blocks_index.miner_lock_hash should be NULL (simulating bulk sync)"
    );

    let task_id = create_pending_task(&pool).await;
    let task_db = TaskDb::new(pool.clone());
    let config = StatisticsRebuildConfig {
        tables: Some(vec!["miner_statistics".to_string()]),
    };

    execute(&task_db, &pool, task_id, &config).await.unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM miner_statistics")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        count, 1,
        "Should find miner via cellbase JOIN even when blocks.miner_lock_hash is NULL"
    );

    let (found_hash,): (Vec<u8>,) = sqlx::query_as("SELECT miner_lock_hash FROM miner_statistics")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(found_hash, miner_hash);
}
