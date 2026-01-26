use chrono::{NaiveDate, TimeZone, Utc};
use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

async fn insert_test_block(pool: &PgPool, number: i64, date: NaiveDate) {
    let timestamp = Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap());
    let dao_bytes: Vec<u8> = vec![0u8; 32];
    let hash: Vec<u8> = vec![number as u8; 32];

    sqlx::query(
        r#"
        INSERT INTO blocks (number, hash, parent_hash, timestamp, version, compact_target,
            transactions_count, epoch_number, epoch_index, epoch_length, dao, nonce,
            extra_hash, proposals_hash, transactions_root, uncles_hash)
        VALUES ($1, $2, $2, $3, 0, 0, 0, 0, 0, 1, $4, $2, $2, $2, $2, $2)
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

async fn insert_dao_deposit(
    pool: &PgPool,
    id_seed: i64,
    deposit_date: NaiveDate,
    withdraw_date: Option<NaiveDate>,
    capacity: i64,
) {
    insert_dao_deposit_with_blocks(
        pool,
        id_seed,
        id_seed,
        deposit_date,
        withdraw_date,
        None,
        capacity,
    )
    .await;
}

async fn insert_dao_deposit_with_blocks(
    pool: &PgPool,
    id_seed: i64,
    deposit_block: i64,
    deposit_date: NaiveDate,
    withdraw_date: Option<NaiveDate>,
    withdraw_block: Option<i64>,
    capacity: i64,
) {
    let deposit_ts = Utc.from_utc_datetime(&deposit_date.and_hms_opt(12, 0, 0).unwrap());
    let withdraw_ts =
        withdraw_date.map(|d| Utc.from_utc_datetime(&d.and_hms_opt(12, 0, 0).unwrap()));
    let tx_hash: Vec<u8> = vec![id_seed as u8; 32];
    let lock_hash: Vec<u8> = vec![(id_seed + 100) as u8; 32];

    let status: i16 = if withdraw_date.is_some() { 2 } else { 0 };

    sqlx::query(
        r#"
        INSERT INTO dao_deposits (
            tx_hash, output_index, lock_script_hash, capacity,
            deposit_block_number, deposit_tx_hash, deposit_timestamp, deposit_ar,
            status, withdraw_timestamp, withdraw_block
        )
        VALUES ($1, 0, $2, $3, $4, $1, $5, 10000000000000000, $6, $7, $8)
        "#,
    )
    .bind(&tx_hash)
    .bind(&lock_hash)
    .bind(capacity)
    .bind(deposit_block)
    .bind(deposit_ts)
    .bind(status)
    .bind(withdraw_ts)
    .bind(withdraw_block)
    .execute(pool)
    .await
    .unwrap();
}

async fn get_snapshot_total_deposit(pool: &PgPool, date: NaiveDate) -> Option<i64> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT total_deposit::text FROM dao_daily_snapshots WHERE date = $1")
            .bind(date)
            .fetch_optional(pool)
            .await
            .unwrap();

    row.map(|(s,)| s.parse().unwrap())
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_active_deposits_are_counted(pool: PgPool) {
    // Given: An active deposit (never withdrawn) made on Jan 1
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_dao_deposit(&pool, 1, jan1, None, 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());

    // When: We create a snapshot for Jan 2
    writer.update_dao_daily_snapshot(jan2).await.unwrap();

    // Then: The active deposit should be counted
    let total = get_snapshot_total_deposit(&pool, jan2).await.unwrap();
    assert_eq!(total, 1000_00000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_withdrawn_deposits_not_counted(pool: PgPool) {
    // Given: A deposit made Jan 1, withdrawn Jan 2
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let jan3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_test_block(&pool, 3, jan3).await;
    insert_dao_deposit(&pool, 1, jan1, Some(jan2), 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());

    // When: We create a snapshot for Jan 3 (after withdrawal)
    writer.update_dao_daily_snapshot(jan3).await.unwrap();

    // Then: The withdrawn deposit should NOT be counted
    let total = get_snapshot_total_deposit(&pool, jan3).await.unwrap();
    assert_eq!(total, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_deposit_counted_before_withdrawal_date(pool: PgPool) {
    // Given: A deposit made Jan 1, withdrawn Jan 3
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let jan3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_test_block(&pool, 3, jan3).await;
    insert_dao_deposit(&pool, 1, jan1, Some(jan3), 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());

    // When: We create a snapshot for Jan 2 (before withdrawal)
    writer.update_dao_daily_snapshot(jan2).await.unwrap();

    // Then: The deposit should be counted (not yet withdrawn)
    let total = get_snapshot_total_deposit(&pool, jan2).await.unwrap();
    assert_eq!(total, 1000_00000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_multiple_deposits_mixed_states(pool: PgPool) {
    // Given: Multiple deposits with different states
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let jan3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();
    let jan5 = NaiveDate::from_ymd_opt(2024, 1, 5).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_test_block(&pool, 3, jan3).await;
    insert_test_block(&pool, 5, jan5).await;

    // Deposit 1: Active (never withdrawn) - 100 CKB
    insert_dao_deposit(&pool, 1, jan1, None, 100_00000000).await;
    // Deposit 2: Withdrawn on Jan 2 - 200 CKB (should not count on Jan 3)
    insert_dao_deposit(&pool, 2, jan1, Some(jan2), 200_00000000).await;
    // Deposit 3: Will be withdrawn on Jan 5 - 300 CKB (should count on Jan 3)
    insert_dao_deposit(&pool, 3, jan1, Some(jan5), 300_00000000).await;

    let writer = BatchWriter::new(pool.clone());

    // When: We create a snapshot for Jan 3
    writer.update_dao_daily_snapshot(jan3).await.unwrap();

    // Then: Only deposits 1 and 3 should be counted (100 + 300 = 400 CKB)
    let total = get_snapshot_total_deposit(&pool, jan3).await.unwrap();
    assert_eq!(total, 400_00000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_deposit_not_counted_before_deposit_date(pool: PgPool) {
    // Given: A deposit made on Jan 2
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_dao_deposit(&pool, 1, jan2, None, 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());

    // When: We create a snapshot for Jan 1 (before deposit)
    writer.update_dao_daily_snapshot(jan1).await.unwrap();

    // Then: The deposit should NOT be counted (not yet deposited)
    let total = get_snapshot_total_deposit(&pool, jan1).await.unwrap();
    assert_eq!(total, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_withdrawal_on_snapshot_date_not_counted(pool: PgPool) {
    // Given: A deposit made Jan 1, withdrawn on Jan 2 (snapshot date)
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_dao_deposit(&pool, 1, jan1, Some(jan2), 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());

    // When: We create a snapshot for Jan 2 (same day as withdrawal)
    writer.update_dao_daily_snapshot(jan2).await.unwrap();

    // Then: The deposit should NOT be counted (withdrawn on this day)
    let total = get_snapshot_total_deposit(&pool, jan2).await.unwrap();
    assert_eq!(total, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_dao_deposits_at_block_active_deposit(pool: PgPool) {
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 100, jan1).await;
    insert_test_block(&pool, 200, jan1).await;

    insert_dao_deposit_with_blocks(&pool, 1, 100, jan1, None, None, 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());
    let deposits = writer.get_dao_deposits_at_block(200).await.unwrap();
    assert_eq!(deposits, 1000_00000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_dao_deposits_at_block_withdrawn_not_counted(pool: PgPool) {
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 100, jan1).await;
    insert_test_block(&pool, 150, jan1).await;
    insert_test_block(&pool, 200, jan1).await;

    insert_dao_deposit_with_blocks(&pool, 1, 100, jan1, Some(jan1), Some(150), 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());
    let deposits = writer.get_dao_deposits_at_block(200).await.unwrap();
    assert_eq!(deposits, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_dao_deposits_at_block_before_withdrawal(pool: PgPool) {
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 100, jan1).await;
    insert_test_block(&pool, 150, jan1).await;
    insert_test_block(&pool, 200, jan1).await;

    insert_dao_deposit_with_blocks(&pool, 1, 100, jan1, Some(jan1), Some(200), 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());
    let deposits = writer.get_dao_deposits_at_block(150).await.unwrap();
    assert_eq!(deposits, 1000_00000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_dao_deposits_at_block_before_deposit(pool: PgPool) {
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 100, jan1).await;
    insert_test_block(&pool, 200, jan1).await;

    insert_dao_deposit_with_blocks(&pool, 1, 200, jan1, None, None, 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());
    let deposits = writer.get_dao_deposits_at_block(100).await.unwrap();
    assert_eq!(deposits, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_dao_deposits_at_block_multiple_deposits(pool: PgPool) {
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 100, jan1).await;
    insert_test_block(&pool, 150, jan1).await;
    insert_test_block(&pool, 200, jan1).await;
    insert_test_block(&pool, 300, jan1).await;

    insert_dao_deposit_with_blocks(&pool, 1, 100, jan1, None, None, 100_00000000).await;
    insert_dao_deposit_with_blocks(&pool, 2, 100, jan1, Some(jan1), Some(150), 200_00000000).await;
    insert_dao_deposit_with_blocks(&pool, 3, 100, jan1, Some(jan1), Some(300), 300_00000000).await;

    let writer = BatchWriter::new(pool.clone());
    let deposits = writer.get_dao_deposits_at_block(200).await.unwrap();
    assert_eq!(deposits, 400_00000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_dao_deposits_at_block_same_block_as_deposit(pool: PgPool) {
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 100, jan1).await;

    insert_dao_deposit_with_blocks(&pool, 1, 100, jan1, None, None, 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());
    let deposits = writer.get_dao_deposits_at_block(100).await.unwrap();
    assert_eq!(deposits, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_dao_deposits_at_block_same_block_as_withdrawal(pool: PgPool) {
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 100, jan1).await;
    insert_test_block(&pool, 200, jan1).await;

    insert_dao_deposit_with_blocks(&pool, 1, 100, jan1, Some(jan1), Some(200), 1000_00000000).await;

    let writer = BatchWriter::new(pool.clone());
    let deposits = writer.get_dao_deposits_at_block(200).await.unwrap();
    assert_eq!(deposits, 1000_00000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_multi_day_batch_creates_all_snapshots(pool: PgPool) {
    // Given: Blocks spanning 3 days with deposits on each day
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let jan3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_test_block(&pool, 3, jan3).await;

    insert_dao_deposit(&pool, 1, jan1, None, 100_00000000).await;
    insert_dao_deposit(&pool, 2, jan2, None, 200_00000000).await;
    insert_dao_deposit(&pool, 3, jan3, None, 300_00000000).await;

    let writer = BatchWriter::new(pool.clone());

    // When: Update snapshots for ALL dates in chronological order (simulating batch sync)
    let dates = vec![jan1, jan2, jan3];
    for date in dates {
        writer.update_dao_daily_snapshot(date).await.unwrap();
    }

    // Then: Each day should have its correct cumulative snapshot
    let total_jan1 = get_snapshot_total_deposit(&pool, jan1).await.unwrap();
    let total_jan2 = get_snapshot_total_deposit(&pool, jan2).await.unwrap();
    let total_jan3 = get_snapshot_total_deposit(&pool, jan3).await.unwrap();

    assert_eq!(total_jan1, 100_00000000, "Jan 1 should have 100 CKB");
    assert_eq!(
        total_jan2, 300_00000000,
        "Jan 2 should have 100+200=300 CKB"
    );
    assert_eq!(
        total_jan3, 600_00000000,
        "Jan 3 should have 100+200+300=600 CKB"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_only_last_day_updated_creates_gap(pool: PgPool) {
    // Given: Blocks spanning 3 days
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let jan3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_test_block(&pool, 3, jan3).await;

    insert_dao_deposit(&pool, 1, jan1, None, 100_00000000).await;
    insert_dao_deposit(&pool, 2, jan2, None, 200_00000000).await;

    let writer = BatchWriter::new(pool.clone());

    // When: Only update the LAST day (simulating the bug)
    writer.update_dao_daily_snapshot(jan3).await.unwrap();

    // Then: Only jan3 has a snapshot, jan1 and jan2 are missing
    let total_jan1 = get_snapshot_total_deposit(&pool, jan1).await;
    let total_jan2 = get_snapshot_total_deposit(&pool, jan2).await;
    let total_jan3 = get_snapshot_total_deposit(&pool, jan3).await;

    assert!(total_jan1.is_none(), "Jan 1 snapshot should be missing");
    assert!(total_jan2.is_none(), "Jan 2 snapshot should be missing");
    assert!(total_jan3.is_some(), "Jan 3 snapshot should exist");
}
