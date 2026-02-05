use chrono::{NaiveDate, TimeZone, Utc};
use ckbadger_indexer::db::{BatchWriter, RocksDbLiveCellStore};
use ckbadger_indexer::parser::ParsedDaoDeposit;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;
use tempfile::TempDir;

async fn insert_test_block(pool: &PgPool, number: i64, date: NaiveDate) {
    let timestamp = Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap());
    let ar: u64 = 10_000_000_000_000_000;
    let mut dao_bytes = vec![0u8; 32];
    dao_bytes[8..16].copy_from_slice(&ar.to_le_bytes());
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

fn make_deposit(id_seed: u8, capacity: i64) -> ParsedDaoDeposit {
    ParsedDaoDeposit {
        tx_hash: vec![id_seed; 32],
        output_index: 0,
        lock_script_hash: vec![id_seed + 100; 32],
        capacity,
    }
}

async fn count_dao_deposits(pool: &PgPool) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM dao_deposits")
        .fetch_one(pool)
        .await
        .unwrap();
    row.0
}

async fn get_dao_statistics(pool: &PgPool) -> (i64, i32) {
    let row: (String, i32) = sqlx::query_as(
        "SELECT total_deposited::text, active_deposits FROM dao_statistics WHERE id = 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    (row.0.parse().unwrap_or(0), row.1)
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_dao_deferred_skips_pg_insert(pool: PgPool) {
    let tmp_dir = TempDir::new().unwrap();
    let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();
    let writer = BatchWriter::new(pool.clone());

    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 100, jan1).await;

    let deposit = make_deposit(1, 1000_00000000);
    let timestamp = Utc.from_utc_datetime(&jan1.and_hms_opt(12, 0, 0).unwrap());
    let ar: i64 = 10_000_000_000_000_000;

    let deposits = vec![(deposit, 100i64, timestamp, ar)];
    writer
        .insert_dao_deposits_batch(&deposits, Some(&store), true)
        .await
        .unwrap();

    assert_eq!(
        count_dao_deposits(&pool).await,
        0,
        "PG should have 0 rows when dao_deferred=true"
    );

    let cached = store.get_dao_deposit(&[1u8; 32], 0);
    assert!(cached.is_some(), "RocksDB should have the deposit");
    let cached = cached.unwrap();
    assert_eq!(cached.capacity, 1000_00000000);
    assert_eq!(cached.deposit_block_number, 100);
    assert_eq!(cached.status, 0);

    let (total_deposited, active_deposits) = get_dao_statistics(&pool).await;
    assert_eq!(total_deposited, 0, "dao_statistics should be untouched");
    assert_eq!(active_deposits, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_dao_deferred_skips_pg_update(pool: PgPool) {
    let tmp_dir = TempDir::new().unwrap();
    let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();
    let writer = BatchWriter::new(pool.clone());

    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    insert_test_block(&pool, 100, jan1).await;
    insert_test_block(&pool, 200, jan2).await;

    let deposit = make_deposit(1, 500_00000000);
    let timestamp = Utc.from_utc_datetime(&jan1.and_hms_opt(12, 0, 0).unwrap());
    let ar: i64 = 10_000_000_000_000_000;

    let deposits = vec![(deposit, 100i64, timestamp, ar)];
    writer
        .insert_dao_deposits_batch(&deposits, Some(&store), true)
        .await
        .unwrap();

    assert_eq!(count_dao_deposits(&pool).await, 0);

    let request_tx = vec![0x55u8; 32];
    let request = ckbadger_indexer::parser::ParsedDaoWithdrawRequest {
        tx_hash: request_tx.clone(),
        output_index: 0,
        lock_script_hash: vec![101u8; 32],
        capacity: 500_00000000,
        deposit_block_number: 100,
        original_tx_hash: vec![1u8; 32],
        original_output_index: 0,
    };
    let ts2 = Utc.from_utc_datetime(&jan2.and_hms_opt(12, 0, 0).unwrap());

    writer
        .update_dao_withdraw_request(&request, 200, ts2, ar, Some(&store), true)
        .await
        .unwrap();

    assert_eq!(
        count_dao_deposits(&pool).await,
        0,
        "PG dao_deposits should still have 0 rows after deferred withdrawal"
    );

    let cached = store.get_dao_deposit(&[1u8; 32], 0).unwrap();
    assert_eq!(cached.status, 1, "RocksDB should have updated status to 1");
    assert_eq!(
        cached.withdraw_request_tx.as_ref().unwrap(),
        &request_tx,
        "RocksDB should have the withdraw_request_tx"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_dao_deferred_skips_statistics(pool: PgPool) {
    let tmp_dir = TempDir::new().unwrap();
    let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();
    let writer = BatchWriter::new(pool.clone());

    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 100, jan1).await;

    let (initial_deposited, initial_active) = get_dao_statistics(&pool).await;

    let deposits: Vec<(ParsedDaoDeposit, i64, chrono::DateTime<Utc>, i64)> = (1..=5)
        .map(|i| {
            let d = make_deposit(i, 100_00000000 * i as i64);
            let ts = Utc.from_utc_datetime(&jan1.and_hms_opt(12, 0, 0).unwrap());
            (d, 100i64, ts, 10_000_000_000_000_000i64)
        })
        .collect();

    writer
        .insert_dao_deposits_batch(&deposits, Some(&store), true)
        .await
        .unwrap();

    let (after_deposited, after_active) = get_dao_statistics(&pool).await;
    assert_eq!(
        after_deposited, initial_deposited,
        "total_deposited unchanged when deferred"
    );
    assert_eq!(
        after_active, initial_active,
        "active_deposits unchanged when deferred"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_dao_rocksdb_lookup_matches_pg(pool: PgPool) {
    let tmp_dir = TempDir::new().unwrap();
    let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();
    let writer = BatchWriter::new(pool.clone());

    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block(&pool, 100, jan1).await;

    let deposit = make_deposit(1, 1000_00000000);
    let timestamp = Utc.from_utc_datetime(&jan1.and_hms_opt(12, 0, 0).unwrap());
    let ar: i64 = 10_000_000_000_000_000;

    let deposits = vec![(deposit, 100i64, timestamp, ar)];
    writer
        .insert_dao_deposits_batch(&deposits, Some(&store), false)
        .await
        .unwrap();

    assert_eq!(
        count_dao_deposits(&pool).await,
        1,
        "PG should have 1 row when dao_deferred=false"
    );

    let pg_row: (Vec<u8>, i16, i64, i64, i16) = sqlx::query_as(
        "SELECT tx_hash, output_index, capacity::bigint, deposit_block_number, status FROM dao_deposits LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let rocks_entry = store.get_dao_deposit(&[1u8; 32], 0).unwrap();

    assert_eq!(pg_row.0, vec![1u8; 32], "tx_hash matches");
    assert_eq!(pg_row.1, 0, "output_index matches");
    assert_eq!(pg_row.2, rocks_entry.capacity, "capacity matches");
    assert_eq!(
        pg_row.3, rocks_entry.deposit_block_number,
        "deposit_block_number matches"
    );
    assert_eq!(pg_row.4, rocks_entry.status, "status matches");
}
