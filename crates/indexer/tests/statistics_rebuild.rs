use chrono::{NaiveDate, TimeZone, Utc};
use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

async fn insert_test_block(pool: &PgPool, number: i64, timestamp: chrono::DateTime<Utc>) {
    sqlx::query(
        r#"
        INSERT INTO blocks (
            number, hash, parent_hash, timestamp, version, compact_target,
            transactions_count, proposals_count, uncles_count, epoch_number,
            epoch_index, epoch_length, dao, nonce, extra_hash
        ) VALUES (
            $1, $2, $3, $4, 0, 100000,
            1, 0, 0, 0,
            0, 1800, $5, E'\\x0000000000000000', E'\\x0000000000000000'
        )
        "#,
    )
    .bind(number)
    .bind(format!("\\x{:064x}", number).as_bytes())
    .bind(format!("\\x{:064x}", number.saturating_sub(1)).as_bytes())
    .bind(timestamp)
    .bind(vec![0u8; 32])
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_test_transaction(pool: &PgPool, hash: &[u8], block_number: i64, tx_index: i32) {
    sqlx::query(
        r#"
        INSERT INTO transactions (
            hash, block_number, tx_index, version, inputs_count, outputs_count,
            witnesses_count, cell_deps_count, header_deps_count,
            total_input_capacity, total_output_capacity, fee, tx_size, is_cellbase, timestamp
        ) VALUES (
            $1, $2, $3, 0, 0, 1,
            0, 0, 0,
            0, 100000000, 0, 100, $3 = 0, NOW()
        )
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
    output_index: i16,
    capacity: i64,
    data_size: i32,
    created_at_block: i64,
    lock_script_hash: &[u8],
) {
    sqlx::query(
        r#"
        INSERT INTO cells (
            tx_hash, output_index, capacity, lock_code_hash, lock_hash_type, lock_args,
            lock_script_hash, data_hash, data_size, data, created_at_block
        ) VALUES (
            $1, $2, $3, E'\\x0000000000000000000000000000000000000000000000000000000000000000', 0, E'\\x',
            $4, E'\\x0000000000000000000000000000000000000000000000000000000000000000', $5, E'\\x', $6
        )
        "#,
    )
    .bind(tx_hash)
    .bind(output_index)
    .bind(capacity)
    .bind(lock_script_hash)
    .bind(data_size)
    .bind(created_at_block)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_rebuild_daily_block_stats(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let date1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let ts1 = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 10).unwrap();

    insert_test_block(&pool, 1, ts1).await;
    insert_test_block(&pool, 2, ts2).await;

    writer.rebuild_all_statistics().await.unwrap();

    let row = sqlx::query_as::<_, (i32, i32)>(
        "SELECT block_count, total_uncles FROM daily_block_stats WHERE date = $1",
    )
    .bind(date1)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(row.is_some(), "daily_block_stats should have an entry");
    let (block_count, _total_uncles) = row.unwrap();
    assert_eq!(block_count, 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_rebuild_miner_statistics(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let ts1 = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();

    insert_test_block(&pool, 1, ts1).await;

    let tx_hash = vec![0u8; 32];
    let lock_hash = vec![1u8; 32];

    insert_test_transaction(&pool, &tx_hash, 1, 0).await;
    insert_test_cell(&pool, &tx_hash, 0, 100_00000000, 0, 1, &lock_hash).await;

    writer.rebuild_all_statistics().await.unwrap();

    let count = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM miner_statistics")
        .fetch_one(&pool)
        .await
        .unwrap()
        .0;

    assert_eq!(count, 1, "Should have one miner entry");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_rebuild_hourly_statistics(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let ts1 = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2024, 1, 1, 12, 30, 0).unwrap();
    let ts3 = Utc.with_ymd_and_hms(2024, 1, 1, 13, 0, 0).unwrap();

    insert_test_block(&pool, 1, ts1).await;
    insert_test_block(&pool, 2, ts2).await;
    insert_test_block(&pool, 3, ts3).await;

    writer.rebuild_all_statistics().await.unwrap();

    let count = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM hourly_statistics")
        .fetch_one(&pool)
        .await
        .unwrap()
        .0;

    assert_eq!(
        count, 2,
        "Should have entries for 2 hours (12:00 and 13:00)"
    );

    let hour_12 =
        sqlx::query_as::<_, (i32,)>("SELECT blocks_count FROM hourly_statistics WHERE hour = $1")
            .bind(Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap()
            .0;

    assert_eq!(hour_12, 2, "Hour 12:00 should have 2 blocks");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_rebuild_epoch_time_distribution(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let start_ts = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end_ts = Utc.with_ymd_and_hms(2024, 1, 1, 4, 0, 0).unwrap();

    sqlx::query(
        r#"
        INSERT INTO epoch_statistics (
            epoch_number, start_block, end_block, blocks_count, length,
            start_timestamp, end_timestamp, difficulty, transactions_count
        ) VALUES ($1, 0, 1799, 1800, 1800, $2, $3, 1000000, 0)
        "#,
    )
    .bind(0i64)
    .bind(start_ts)
    .bind(end_ts)
    .execute(&pool)
    .await
    .unwrap();

    writer.rebuild_all_statistics().await.unwrap();

    let count = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM epoch_time_distribution")
        .fetch_one(&pool)
        .await
        .unwrap()
        .0;

    assert!(count >= 1, "Should have at least one epoch time bucket");
}
