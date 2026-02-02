use chrono::{NaiveDate, TimeZone, Utc};
use ckbadger_task_runner::executor::statistics::{
    rebuild_dao_daily_snapshots, update_dao_daily_snapshot,
};
use ckbadger_task_runner::MIGRATOR;
use sqlx::PgPool;

async fn insert_test_block(pool: &PgPool, number: i64, date: NaiveDate) {
    let timestamp = Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap());
    let dao_bytes: Vec<u8> = {
        let mut bytes = vec![0u8; 32];
        let total_issuance: u64 = 3_360_000_000_000_000_000;
        bytes[0..8].copy_from_slice(&total_issuance.to_le_bytes());
        bytes
    };
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
            status, withdraw_timestamp
        )
        VALUES ($1, 0, $2, $3, $4, $1, $5, 10000000000000000, $6, $7)
        "#,
    )
    .bind(&tx_hash)
    .bind(&lock_hash)
    .bind(capacity)
    .bind(id_seed)
    .bind(deposit_ts)
    .bind(status)
    .bind(withdraw_ts)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_block_secondary_issuance(
    pool: &PgPool,
    block_number: i64,
    date: NaiveDate,
    burnt: i64,
    miner_secondary: i64,
    dao_compensation: i64,
) {
    let timestamp = Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap());

    sqlx::query(
        r#"
        INSERT INTO block_secondary_issuance (
            block_number, block_timestamp, secondary_issuance, miner_secondary, dao_compensation, burnt
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(block_number)
    .bind(timestamp)
    .bind(burnt + miner_secondary + dao_compensation)
    .bind(miner_secondary)
    .bind(dao_compensation)
    .bind(burnt)
    .execute(pool)
    .await
    .unwrap();
}

async fn get_snapshot(pool: &PgPool, date: NaiveDate) -> Option<DaoSnapshot> {
    sqlx::query_as::<_, DaoSnapshot>(
        r#"
        SELECT 
            total_deposit::text,
            depositors_count,
            total_issuance::text,
            cumulative_burnt,
            cumulative_mining_reward,
            cumulative_deposit_compensation
        FROM dao_daily_snapshots 
        WHERE date = $1
        "#,
    )
    .bind(date)
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[derive(sqlx::FromRow, Debug)]
#[allow(dead_code)]
struct DaoSnapshot {
    total_deposit: String,
    depositors_count: i32,
    total_issuance: String,
    cumulative_burnt: Option<String>,
    cumulative_mining_reward: Option<String>,
    cumulative_deposit_compensation: Option<String>,
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_snapshot_includes_cumulative_secondary_issuance(pool: PgPool) {
    // Given: blocks with secondary issuance data
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;

    insert_block_secondary_issuance(&pool, 1, jan1, 1000, 100, 50).await;
    insert_block_secondary_issuance(&pool, 2, jan2, 2000, 200, 100).await;

    // When: create snapshot for jan2
    update_dao_daily_snapshot(&pool, jan2).await.unwrap();

    // Then: cumulative values should be sum of jan1 + jan2
    let snapshot = get_snapshot(&pool, jan2).await.unwrap();
    assert_eq!(snapshot.cumulative_burnt, Some("3000".to_string()));
    assert_eq!(snapshot.cumulative_mining_reward, Some("300".to_string()));
    assert_eq!(
        snapshot.cumulative_deposit_compensation,
        Some("150".to_string())
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_snapshot_includes_total_deposit(pool: PgPool) {
    // Given: active deposits
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_dao_deposit(&pool, 1, jan1, None, 1000_00000000).await;

    // When: create snapshot for jan2
    update_dao_daily_snapshot(&pool, jan2).await.unwrap();

    // Then: total_deposit should include active deposit
    let snapshot = get_snapshot(&pool, jan2).await.unwrap();
    assert_eq!(snapshot.total_deposit, "100000000000");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_snapshot_excludes_withdrawn_deposits(pool: PgPool) {
    // Given: a withdrawn deposit
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let jan3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_test_block(&pool, 3, jan3).await;
    insert_dao_deposit(&pool, 1, jan1, Some(jan2), 1000_00000000).await;

    // When: create snapshot for jan3 (after withdrawal)
    update_dao_daily_snapshot(&pool, jan3).await.unwrap();

    // Then: total_deposit should be 0
    let snapshot = get_snapshot(&pool, jan3).await.unwrap();
    assert_eq!(snapshot.total_deposit, "0");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_snapshot_includes_total_issuance_from_dao_field(pool: PgPool) {
    // Given: a block with DAO field containing total_issuance
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 1, jan1).await;

    // When: create snapshot
    update_dao_daily_snapshot(&pool, jan1).await.unwrap();

    // Then: total_issuance should be extracted from DAO field
    let snapshot = get_snapshot(&pool, jan1).await.unwrap();
    assert_eq!(snapshot.total_issuance, "3360000000000000000");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_snapshot_handles_missing_secondary_issuance(pool: PgPool) {
    // Given: block without secondary issuance data
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 1, jan1).await;

    // When: create snapshot
    update_dao_daily_snapshot(&pool, jan1).await.unwrap();

    // Then: cumulative values should be "0", not NULL
    let snapshot = get_snapshot(&pool, jan1).await.unwrap();
    assert_eq!(snapshot.cumulative_burnt, Some("0".to_string()));
    assert_eq!(snapshot.cumulative_mining_reward, Some("0".to_string()));
    assert_eq!(
        snapshot.cumulative_deposit_compensation,
        Some("0".to_string())
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_bulk_rebuild_creates_all_snapshots_with_correct_cumulative_values(pool: PgPool) {
    // Given: 3 days of blocks with secondary issuance and deposits
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let jan3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_test_block(&pool, 3, jan3).await;

    insert_block_secondary_issuance(&pool, 1, jan1, 1000, 100, 50).await;
    insert_block_secondary_issuance(&pool, 2, jan2, 2000, 200, 100).await;
    insert_block_secondary_issuance(&pool, 3, jan3, 3000, 300, 150).await;

    insert_dao_deposit(&pool, 1, jan1, None, 1000_00000000).await;
    insert_dao_deposit(&pool, 2, jan2, None, 2000_00000000).await;

    // When: bulk rebuild all snapshots
    rebuild_dao_daily_snapshots(&pool).await.unwrap();

    // Then: all 3 days should have snapshots with correct cumulative values
    let snap1 = get_snapshot(&pool, jan1).await.unwrap();
    assert_eq!(snap1.cumulative_burnt, Some("1000".to_string()));
    assert_eq!(snap1.cumulative_mining_reward, Some("100".to_string()));
    assert_eq!(
        snap1.cumulative_deposit_compensation,
        Some("50".to_string())
    );
    assert_eq!(snap1.total_deposit, "100000000000");
    assert_eq!(snap1.depositors_count, 1);

    let snap2 = get_snapshot(&pool, jan2).await.unwrap();
    assert_eq!(snap2.cumulative_burnt, Some("3000".to_string()));
    assert_eq!(snap2.cumulative_mining_reward, Some("300".to_string()));
    assert_eq!(
        snap2.cumulative_deposit_compensation,
        Some("150".to_string())
    );
    assert_eq!(snap2.total_deposit, "300000000000");
    assert_eq!(snap2.depositors_count, 2);

    let snap3 = get_snapshot(&pool, jan3).await.unwrap();
    assert_eq!(snap3.cumulative_burnt, Some("6000".to_string()));
    assert_eq!(snap3.cumulative_mining_reward, Some("600".to_string()));
    assert_eq!(
        snap3.cumulative_deposit_compensation,
        Some("300".to_string())
    );
    assert_eq!(snap3.total_deposit, "300000000000");
    assert_eq!(snap3.depositors_count, 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_bulk_rebuild_tracks_withdrawn_deposits_correctly(pool: PgPool) {
    // Given: deposit on day 1, withdrawn on day 2
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let jan3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();

    insert_test_block(&pool, 1, jan1).await;
    insert_test_block(&pool, 2, jan2).await;
    insert_test_block(&pool, 3, jan3).await;

    insert_dao_deposit(&pool, 1, jan1, Some(jan2), 1000_00000000).await;

    // When: bulk rebuild
    rebuild_dao_daily_snapshots(&pool).await.unwrap();

    // Then: day 1 should have deposit, day 2+ should not
    let snap1 = get_snapshot(&pool, jan1).await.unwrap();
    assert_eq!(snap1.total_deposit, "100000000000");
    assert_eq!(snap1.depositors_count, 1);

    let snap2 = get_snapshot(&pool, jan2).await.unwrap();
    assert_eq!(snap2.total_deposit, "0");
    assert_eq!(snap2.depositors_count, 0);

    let snap3 = get_snapshot(&pool, jan3).await.unwrap();
    assert_eq!(snap3.total_deposit, "0");
    assert_eq!(snap3.depositors_count, 0);
}
