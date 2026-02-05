use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use ckbadger_indexer::db::{BatchWriter, DaoWithdrawalContextTrait};
use ckbadger_indexer::parser::ParsedDaoDeposit;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

async fn insert_test_block_with_dao_ar(pool: &PgPool, number: i64, date: NaiveDate) {
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

fn create_test_deposit(id_seed: u8, capacity: i64) -> ParsedDaoDeposit {
    ParsedDaoDeposit {
        tx_hash: vec![id_seed; 32],
        output_index: 0,
        lock_script_hash: vec![id_seed + 100; 32],
        capacity,
    }
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

async fn get_deposit_status(pool: &PgPool, tx_hash: &[u8], output_index: i16) -> Option<i16> {
    let row: Option<(i16,)> =
        sqlx::query_as("SELECT status FROM dao_deposits WHERE tx_hash = $1 AND output_index = $2")
            .bind(tx_hash)
            .bind(output_index)
            .fetch_optional(pool)
            .await
            .unwrap();
    row.map(|(s,)| s)
}

#[allow(clippy::too_many_arguments)]
async fn insert_dao_deposit_directly(
    pool: &PgPool,
    tx_hash: &[u8],
    output_index: i16,
    capacity: i64,
    deposit_block: i64,
    status: i16,
    withdraw_request_tx: Option<&[u8]>,
    withdraw_request_block: Option<i64>,
) -> i64 {
    let timestamp = Utc::now();
    let lock_hash = vec![0u8; 32];

    let row: (i64,) = sqlx::query_as(
        r#"
        INSERT INTO dao_deposits (
            tx_hash, output_index, lock_script_hash, capacity,
            deposit_block_number, deposit_tx_hash, deposit_timestamp, deposit_ar,
            status, withdraw_request_tx, withdraw_request_block
        )
        VALUES ($1, $2, $3, $4, $5, $1, $6, 10000000000000000, $7, $8, $9)
        RETURNING id
        "#,
    )
    .bind(tx_hash)
    .bind(output_index)
    .bind(&lock_hash)
    .bind(capacity)
    .bind(deposit_block)
    .bind(timestamp)
    .bind(status)
    .bind(withdraw_request_tx)
    .bind(withdraw_request_block)
    .fetch_one(pool)
    .await
    .unwrap();

    row.0
}

#[allow(clippy::type_complexity)]
struct TestWithdrawalContext {
    consumed_deposits: Vec<(i64, Vec<u8>, i16, String, i64, i16)>,
    new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
    block_number: i64,
    consuming_tx_hash: Vec<u8>,
    timestamp: DateTime<Utc>,
}

impl DaoWithdrawalContextTrait for TestWithdrawalContext {
    fn consumed_deposits(&self) -> &[(i64, Vec<u8>, i16, String, i64, i16)] {
        &self.consumed_deposits
    }

    fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)] {
        &self.new_dao_outputs
    }

    fn block_number(&self) -> i64 {
        self.block_number
    }

    fn consuming_tx_hash(&self) -> &[u8] {
        &self.consuming_tx_hash
    }

    fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_dao_deposits_batch_single(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block_with_dao_ar(&pool, 100, jan1).await;

    let deposit = create_test_deposit(1, 1000_00000000);
    let timestamp = Utc.from_utc_datetime(&jan1.and_hms_opt(12, 0, 0).unwrap());
    let ar: i64 = 10_000_000_000_000_000;

    let deposits = vec![(deposit, 100i64, timestamp, ar)];
    writer
        .insert_dao_deposits_batch(&deposits, None, false)
        .await
        .unwrap();

    assert_eq!(get_deposit_status(&pool, &[1u8; 32], 0).await, Some(0));

    let (total_deposited, active_deposits) = get_dao_statistics(&pool).await;
    assert_eq!(total_deposited, 1000_00000000);
    assert_eq!(active_deposits, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_dao_deposits_batch_multiple(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block_with_dao_ar(&pool, 100, jan1).await;

    let timestamp = Utc.from_utc_datetime(&jan1.and_hms_opt(12, 0, 0).unwrap());
    let ar: i64 = 10_000_000_000_000_000;

    let deposits = vec![
        (create_test_deposit(1, 100_00000000), 100i64, timestamp, ar),
        (create_test_deposit(2, 200_00000000), 100i64, timestamp, ar),
        (create_test_deposit(3, 300_00000000), 100i64, timestamp, ar),
    ];
    writer
        .insert_dao_deposits_batch(&deposits, None, false)
        .await
        .unwrap();

    assert_eq!(get_deposit_status(&pool, &[1u8; 32], 0).await, Some(0));
    assert_eq!(get_deposit_status(&pool, &[2u8; 32], 0).await, Some(0));
    assert_eq!(get_deposit_status(&pool, &[3u8; 32], 0).await, Some(0));

    let (total_deposited, active_deposits) = get_dao_statistics(&pool).await;
    assert_eq!(total_deposited, 600_00000000);
    assert_eq!(active_deposits, 3);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_dao_deposits_batch_empty(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let deposits: Vec<(ParsedDaoDeposit, i64, DateTime<Utc>, i64)> = vec![];
    writer
        .insert_dao_deposits_batch(&deposits, None, false)
        .await
        .unwrap();

    let (total_deposited, active_deposits) = get_dao_statistics(&pool).await;
    assert_eq!(total_deposited, 0);
    assert_eq!(active_deposits, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_dao_deposits_batch_duplicate_ignored(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    insert_test_block_with_dao_ar(&pool, 100, jan1).await;

    let timestamp = Utc.from_utc_datetime(&jan1.and_hms_opt(12, 0, 0).unwrap());
    let ar: i64 = 10_000_000_000_000_000;

    let deposits1 = vec![(create_test_deposit(1, 100_00000000), 100i64, timestamp, ar)];
    writer
        .insert_dao_deposits_batch(&deposits1, None, false)
        .await
        .unwrap();

    let deposits2 = vec![(create_test_deposit(1, 100_00000000), 100i64, timestamp, ar)];
    writer
        .insert_dao_deposits_batch(&deposits2, None, false)
        .await
        .unwrap();

    let (total_deposited, active_deposits) = get_dao_statistics(&pool).await;
    assert_eq!(total_deposited, 100_00000000);
    assert_eq!(active_deposits, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_find_consumed_dao_deposits_batch_by_outpoint(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let tx_hash1 = vec![1u8; 32];
    let tx_hash2 = vec![2u8; 32];

    insert_dao_deposit_directly(&pool, &tx_hash1, 0, 100_00000000, 100, 0, None, None).await;
    insert_dao_deposit_directly(&pool, &tx_hash2, 0, 200_00000000, 100, 0, None, None).await;

    let inputs: Vec<(&[u8], i16)> = vec![(tx_hash1.as_slice(), 0), (tx_hash2.as_slice(), 0)];
    let result = writer
        .find_consumed_dao_deposits_batch(&inputs, None)
        .await
        .unwrap();

    assert_eq!(result.len(), 2);
    assert!(result.contains_key(&(tx_hash1.clone(), 0)));
    assert!(result.contains_key(&(tx_hash2.clone(), 0)));

    let deposit1 = result.get(&(tx_hash1, 0)).unwrap();
    assert_eq!(deposit1.3, "10000000000");
    assert_eq!(deposit1.5, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_find_consumed_dao_deposits_batch_by_withdraw_request_tx(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let original_tx = vec![1u8; 32];
    let request_tx = vec![2u8; 32];

    insert_dao_deposit_directly(
        &pool,
        &original_tx,
        0,
        100_00000000,
        100,
        1,
        Some(&request_tx),
        Some(200),
    )
    .await;

    let inputs: Vec<(&[u8], i16)> = vec![(request_tx.as_slice(), 0)];
    let result = writer
        .find_consumed_dao_deposits_batch(&inputs, None)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    assert!(result.contains_key(&(request_tx.clone(), 0)));

    let deposit = result.get(&(request_tx, 0)).unwrap();
    assert_eq!(deposit.5, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_find_consumed_dao_deposits_batch_empty_input(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let inputs: Vec<(&[u8], i16)> = vec![];
    let result = writer
        .find_consumed_dao_deposits_batch(&inputs, None)
        .await
        .unwrap();

    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_find_consumed_dao_deposits_batch_not_found(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let tx_hash = vec![99u8; 32];
    let inputs: Vec<(&[u8], i16)> = vec![(tx_hash.as_slice(), 0)];
    let result = writer
        .find_consumed_dao_deposits_batch(&inputs, None)
        .await
        .unwrap();

    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_process_dao_withdrawals_batch_phase1_request(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    insert_test_block_with_dao_ar(&pool, 100, jan1).await;
    insert_test_block_with_dao_ar(&pool, 200, jan2).await;

    let original_tx = vec![1u8; 32];
    let request_tx = vec![2u8; 32];
    let capacity: i64 = 100_00000000;

    let deposit_id =
        insert_dao_deposit_directly(&pool, &original_tx, 0, capacity, 100, 0, None, None).await;

    sqlx::query("UPDATE dao_statistics SET total_deposited = $1, active_deposits = 1 WHERE id = 1")
        .bind(capacity)
        .execute(&pool)
        .await
        .unwrap();

    let timestamp = Utc.from_utc_datetime(&jan2.and_hms_opt(12, 0, 0).unwrap());
    let context = TestWithdrawalContext {
        consumed_deposits: vec![(
            deposit_id,
            original_tx.clone(),
            0,
            capacity.to_string(),
            100,
            0,
        )],
        new_dao_outputs: vec![(request_tx.clone(), 0, vec![0u8; 32], capacity, 0)],
        block_number: 200,
        consuming_tx_hash: request_tx.clone(),
        timestamp,
    };

    writer
        .process_dao_withdrawals_batch(&[context])
        .await
        .unwrap();

    assert_eq!(get_deposit_status(&pool, &original_tx, 0).await, Some(1));

    let row: (Vec<u8>,) = sqlx::query_as(
        "SELECT withdraw_request_tx FROM dao_deposits WHERE tx_hash = $1 AND output_index = 0",
    )
    .bind(&original_tx)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, request_tx);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_process_dao_withdrawals_batch_phase2_complete(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let jan3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();
    insert_test_block_with_dao_ar(&pool, 100, jan1).await;
    insert_test_block_with_dao_ar(&pool, 200, jan2).await;
    insert_test_block_with_dao_ar(&pool, 300, jan3).await;

    let original_tx = vec![1u8; 32];
    let request_tx = vec![2u8; 32];
    let complete_tx = vec![3u8; 32];
    let capacity: i64 = 100_00000000;

    let deposit_id = insert_dao_deposit_directly(
        &pool,
        &original_tx,
        0,
        capacity,
        100,
        1,
        Some(&request_tx),
        Some(200),
    )
    .await;

    sqlx::query("UPDATE dao_statistics SET total_deposited = $1, active_deposits = 1 WHERE id = 1")
        .bind(capacity)
        .execute(&pool)
        .await
        .unwrap();

    let timestamp = Utc.from_utc_datetime(&jan3.and_hms_opt(12, 0, 0).unwrap());
    let context = TestWithdrawalContext {
        consumed_deposits: vec![(
            deposit_id,
            original_tx.clone(),
            0,
            capacity.to_string(),
            100,
            1,
        )],
        new_dao_outputs: vec![],
        block_number: 300,
        consuming_tx_hash: complete_tx.clone(),
        timestamp,
    };

    writer
        .process_dao_withdrawals_batch(&[context])
        .await
        .unwrap();

    assert_eq!(get_deposit_status(&pool, &original_tx, 0).await, Some(2));

    let row: (Vec<u8>,) = sqlx::query_as(
        "SELECT withdraw_tx FROM dao_deposits WHERE tx_hash = $1 AND output_index = 0",
    )
    .bind(&original_tx)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, complete_tx);

    let (total_deposited, active_deposits) = get_dao_statistics(&pool).await;
    assert_eq!(total_deposited, 0);
    assert_eq!(active_deposits, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_process_dao_withdrawals_batch_empty(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let contexts: Vec<TestWithdrawalContext> = vec![];
    writer
        .process_dao_withdrawals_batch(&contexts)
        .await
        .unwrap();

    let (total_deposited, active_deposits) = get_dao_statistics(&pool).await;
    assert_eq!(total_deposited, 0);
    assert_eq!(active_deposits, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_process_dao_withdrawals_batch_multiple(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let jan1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let jan2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    insert_test_block_with_dao_ar(&pool, 100, jan1).await;
    insert_test_block_with_dao_ar(&pool, 200, jan2).await;

    let tx1 = vec![1u8; 32];
    let tx2 = vec![2u8; 32];
    let req_tx1 = vec![11u8; 32];
    let req_tx2 = vec![12u8; 32];

    let id1 = insert_dao_deposit_directly(&pool, &tx1, 0, 100_00000000, 100, 0, None, None).await;
    let id2 = insert_dao_deposit_directly(&pool, &tx2, 0, 200_00000000, 100, 0, None, None).await;

    sqlx::query("UPDATE dao_statistics SET total_deposited = 300_00000000, active_deposits = 2 WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();

    let timestamp = Utc.from_utc_datetime(&jan2.and_hms_opt(12, 0, 0).unwrap());

    let context1 = TestWithdrawalContext {
        consumed_deposits: vec![(id1, tx1.clone(), 0, "10000000000".to_string(), 100, 0)],
        new_dao_outputs: vec![(req_tx1.clone(), 0, vec![0u8; 32], 100_00000000, 0)],
        block_number: 200,
        consuming_tx_hash: req_tx1.clone(),
        timestamp,
    };

    let context2 = TestWithdrawalContext {
        consumed_deposits: vec![(id2, tx2.clone(), 0, "20000000000".to_string(), 100, 0)],
        new_dao_outputs: vec![(req_tx2.clone(), 0, vec![0u8; 32], 200_00000000, 0)],
        block_number: 200,
        consuming_tx_hash: req_tx2.clone(),
        timestamp,
    };

    writer
        .process_dao_withdrawals_batch(&[context1, context2])
        .await
        .unwrap();

    assert_eq!(get_deposit_status(&pool, &tx1, 0).await, Some(1));
    assert_eq!(get_deposit_status(&pool, &tx2, 0).await, Some(1));
}
