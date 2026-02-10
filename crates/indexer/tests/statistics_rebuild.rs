use chrono::{NaiveDate, TimeZone, Utc};
use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

async fn insert_test_block(pool: &PgPool, number: i64, timestamp: chrono::DateTime<Utc>) {
    sqlx::query(
        r#"
        INSERT INTO blocks_index (
            number, hash, timestamp, tx_count, proposals_count, uncles_count,
            epoch_number, epoch_index, epoch_length, compact_target, dao
        ) VALUES (
            $1, $2, $3, 1, 0, 0,
            0, 0, 1800, 100000, $4
        )
        "#,
    )
    .bind(number)
    .bind(format!("\\x{:064x}", number).as_bytes())
    .bind(timestamp)
    .bind(vec![0u8; 32]) // dao
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_test_transaction(pool: &PgPool, hash: &[u8], block_number: i64, tx_index: i32) {
    sqlx::query(
        r#"
        INSERT INTO transactions_index (
            hash, block_number, tx_index, is_cellbase, timestamp,
            inputs_count, outputs_count, fee, tx_size
        ) VALUES (
            $1, $2, $3, $3 = 0, NOW(),
            0, 1, 0, 100
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

async fn insert_mnft_class(pool: &PgPool, class_id: &[u8], name: &str) {
    sqlx::query(
        r#"
        INSERT INTO mnft_classes (
            class_id, type_script_hash, issuer_id, name, description,
            total, issued, holders_count, transfers_count, transfers_24h,
            owner_lock_hash, is_live, created_at_block, created_at_tx
        ) VALUES ($1, $2, $3, $4, 'Test', 100, 10, 0, 0, 0, $5, TRUE, 100, $6)
        "#,
    )
    .bind(class_id)
    .bind(vec![0u8; 32])
    .bind(vec![0u8; 20])
    .bind(name)
    .bind(vec![0u8; 32])
    .bind(vec![0u8; 32])
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_nft_activity(
    pool: &PgPool,
    class_id: &[u8],
    block_number: i64,
    to_lock_hash: &[u8],
    activity_type: &str,
) {
    let activity_id = vec![block_number as u8; 32];
    let tx_hash = vec![block_number as u8; 32];
    let mut asset_id = class_id.to_vec();
    asset_id.extend_from_slice(&[0, 0, 0, block_number as u8]);

    let metadata = serde_json::json!({ "nftType": "mnft" });

    sqlx::query(
        r#"
        INSERT INTO activities (
            activity_id, activity_type, activity_category, block_number, tx_hash,
            tx_index, activity_index, from_lock_hash, to_lock_hash, amount, asset_id,
            metadata, timestamp
        ) VALUES ($1, $2, 'nft', $3, $4, 0, 0, NULL, $5, 1, $6, $7, NOW())
        "#,
    )
    .bind(&activity_id)
    .bind(activity_type)
    .bind(block_number)
    .bind(&tx_hash)
    .bind(to_lock_hash)
    .bind(&asset_id)
    .bind(metadata)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_rebuild_mnft_statistics(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let class_id = vec![0x01u8; 24];
    insert_mnft_class(&pool, &class_id, "Test NFT").await;

    let holder1 = vec![0x10u8; 32];
    let holder2 = vec![0x20u8; 32];
    let holder3 = vec![0x30u8; 32];

    insert_nft_activity(&pool, &class_id, 1, &holder1, "NFT_MINT").await;
    insert_nft_activity(&pool, &class_id, 2, &holder2, "NFT_TRANSFER").await;
    insert_nft_activity(&pool, &class_id, 3, &holder3, "NFT_TRANSFER").await;
    insert_nft_activity(&pool, &class_id, 4, &holder1, "NFT_TRANSFER").await;

    let result = writer.rebuild_mnft_statistics().await.unwrap();
    assert_eq!(result, 1, "Should update 1 class");

    let (holders_count, transfers_count): (i32, i64) = sqlx::query_as(
        "SELECT holders_count, transfers_count FROM mnft_classes WHERE class_id = $1",
    )
    .bind(&class_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(holders_count, 3, "Should have 3 unique holders");
    assert_eq!(transfers_count, 4, "Should have 4 transfers");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_refresh_mnft_24h_transfers(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let ts_now = Utc::now();
    let ts_recent = ts_now - chrono::Duration::hours(12);
    let ts_old = ts_now - chrono::Duration::hours(48);

    insert_test_block(&pool, 1, ts_old).await;
    insert_test_block(&pool, 100, ts_recent).await;
    insert_test_block(&pool, 200, ts_now).await;

    let class_id = vec![0x02u8; 24];
    insert_mnft_class(&pool, &class_id, "24h Test NFT").await;

    let holder = vec![0x40u8; 32];
    insert_nft_activity(&pool, &class_id, 1, &holder, "NFT_MINT").await;
    insert_nft_activity(&pool, &class_id, 100, &holder, "NFT_TRANSFER").await;
    insert_nft_activity(&pool, &class_id, 200, &holder, "NFT_TRANSFER").await;

    let result = writer.refresh_mnft_24h_transfers().await.unwrap();
    assert!(result >= 1, "Should update at least 1 class");

    let transfers_24h: i32 =
        sqlx::query_as::<_, (i32,)>("SELECT transfers_24h FROM mnft_classes WHERE class_id = $1")
            .bind(&class_id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .0;

    assert_eq!(transfers_24h, 2, "Should have 2 transfers in last 24h");
}
