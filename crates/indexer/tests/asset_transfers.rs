#![allow(clippy::type_complexity)]

use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_address_asset_transfers_batch(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let lock_hash1 = vec![0x11u8; 32];
    let lock_hash2 = vec![0x22u8; 32];
    let tx_hash = vec![0xAAu8; 32];
    let asset_id = vec![0xBBu8; 32];
    let now = chrono::Utc::now();

    let records = vec![
        (
            lock_hash1.clone(),
            tx_hash.clone(),
            1000i64,
            0i32,
            0i16,
            "token".to_string(),
            "sudt".to_string(),
            Some(asset_id.clone()),
            1i16,
            Some(lock_hash2.clone()),
            Some("1000000".to_string()),
            None::<String>,
            now,
        ),
        (
            lock_hash2.clone(),
            tx_hash.clone(),
            1000i64,
            0i32,
            1i16,
            "token".to_string(),
            "sudt".to_string(),
            Some(asset_id.clone()),
            2i16,
            Some(lock_hash1.clone()),
            Some("1000000".to_string()),
            None::<String>,
            now,
        ),
    ];

    writer
        .insert_address_asset_transfers_batch(&records)
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM address_asset_transfers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);

    let (direction,): (i16,) =
        sqlx::query_as("SELECT direction FROM address_asset_transfers WHERE lock_script_hash = $1")
            .bind(&lock_hash1)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(direction, 1);

    let (direction,): (i16,) =
        sqlx::query_as("SELECT direction FROM address_asset_transfers WHERE lock_script_hash = $1")
            .bind(&lock_hash2)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(direction, 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_address_asset_transfers_batch_empty(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    writer
        .insert_address_asset_transfers_batch(&[])
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM address_asset_transfers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_dob_transfer(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let dob_id = vec![0xDDu8; 32];
    let cluster_id = vec![0xCCu8; 32];
    let tx_hash = vec![0xAAu8; 32];
    let from_lock = vec![0xF1u8; 32];
    let to_lock = vec![0xF2u8; 32];
    let now = chrono::Utc::now();

    writer
        .insert_dob_transfer(
            &dob_id,
            Some(&cluster_id),
            "spore",
            &tx_hash,
            1000,
            Some(&from_lock),
            &to_lock,
            "transfer",
            Some("image/png"),
            now,
        )
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dob_transfers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let (event_type,): (String,) =
        sqlx::query_as("SELECT event_type FROM dob_transfers WHERE dob_id = $1")
            .bind(&dob_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(event_type, "transfer");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_dob_transfer_mint(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let dob_id = vec![0xDDu8; 32];
    let tx_hash = vec![0xAAu8; 32];
    let to_lock = vec![0xF2u8; 32];
    let now = chrono::Utc::now();

    writer
        .insert_dob_transfer(
            &dob_id,
            None,
            "spore",
            &tx_hash,
            1000,
            None,
            &to_lock,
            "mint",
            Some("dob/0"),
            now,
        )
        .await
        .unwrap();

    let (from_lock,): (Option<Vec<u8>>,) =
        sqlx::query_as("SELECT from_lock_hash FROM dob_transfers WHERE dob_id = $1")
            .bind(&dob_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(from_lock.is_none());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_nft_transfer(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let nft_id = vec![0xEEu8; 32];
    let issuer_id = vec![0x11u8; 32];
    let class_id = vec![0xC0u8; 32];
    let tx_hash = vec![0xAAu8; 32];
    let from_lock = vec![0xF1u8; 32];
    let to_lock = vec![0xF2u8; 32];
    let now = chrono::Utc::now();

    writer
        .insert_nft_transfer(
            &nft_id,
            "mnft",
            Some(&issuer_id),
            Some(&class_id),
            &tx_hash,
            1000,
            Some(&from_lock),
            &to_lock,
            "transfer",
            Some("My NFT #1"),
            now,
        )
        .await
        .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nft_transfers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let (name,): (Option<String>,) =
        sqlx::query_as("SELECT name FROM nft_transfers WHERE nft_id = $1")
            .bind(&nft_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(name, Some("My NFT #1".to_string()));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_nft_transfer_dotbit(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let nft_id = vec![0xEEu8; 32];
    let tx_hash = vec![0xAAu8; 32];
    let to_lock = vec![0xF2u8; 32];
    let now = chrono::Utc::now();

    writer
        .insert_nft_transfer(
            &nft_id,
            "dotbit",
            None,
            None,
            &tx_hash,
            1000,
            None,
            &to_lock,
            "register",
            Some("alice.bit"),
            now,
        )
        .await
        .unwrap();

    let (nft_type, event_type, name): (String, String, Option<String>) =
        sqlx::query_as("SELECT nft_type, event_type, name FROM nft_transfers WHERE nft_id = $1")
            .bind(&nft_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(nft_type, "dotbit");
    assert_eq!(event_type, "register");
    assert_eq!(name, Some("alice.bit".to_string()));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_token_balance_decremented_on_burn(pool: PgPool) {
    use ckbadger_indexer::parser::{ParsedUdtTransfer, UdtStandard};

    let writer = BatchWriter::new(pool.clone());
    let now = chrono::Utc::now();

    let type_script_hash = vec![0x11u8; 32];
    let sender_lock = vec![0xAAu8; 32];
    let tx_hash1 = vec![0x01u8; 32];
    let tx_hash2 = vec![0x02u8; 32];

    let mint_transfer = ParsedUdtTransfer {
        type_script_hash: type_script_hash.clone(),
        type_code_hash: vec![0x22u8; 32],
        type_hash_type: 1,
        type_args: vec![0x33u8; 20],
        from_lock_hash: None,
        to_lock_hash: sender_lock.clone(),
        amount: 10000,
        standard: UdtStandard::Xudt,
        is_mint: true,
        is_burn: false,
    };

    writer
        .process_udt_transfer(&mint_transfer, &tx_hash1, 1000, now)
        .await
        .unwrap();

    let balance: i64 = sqlx::query_scalar(
        "SELECT balance::bigint FROM token_balances WHERE lock_script_hash = $1",
    )
    .bind(&sender_lock)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(balance, 10000);

    let burn_transfer = ParsedUdtTransfer {
        type_script_hash: type_script_hash.clone(),
        type_code_hash: vec![0x22u8; 32],
        type_hash_type: 1,
        type_args: vec![0x33u8; 20],
        from_lock_hash: Some(sender_lock.clone()),
        to_lock_hash: vec![],
        amount: 10000,
        standard: UdtStandard::Xudt,
        is_mint: false,
        is_burn: true,
    };

    writer
        .process_udt_transfer(&burn_transfer, &tx_hash2, 1001, now)
        .await
        .unwrap();

    let balance_after: Option<i64> = sqlx::query_scalar(
        "SELECT balance::bigint FROM token_balances WHERE lock_script_hash = $1",
    )
    .bind(&sender_lock)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(
        balance_after.is_none(),
        "Balance should be deleted when zero"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_asset_transfer_records_both_directions(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let now = chrono::Utc::now();

    let sender_lock = vec![0xAAu8; 32];
    let receiver_lock = vec![0xBBu8; 32];
    let tx_hash = vec![0x01u8; 32];
    let asset_id = vec![0xCCu8; 32];

    let records = vec![
        (
            sender_lock.clone(),
            tx_hash.clone(),
            1000i64,
            0i32,
            0i16,
            "token".to_string(),
            "xudt".to_string(),
            Some(asset_id.clone()),
            2i16,
            Some(receiver_lock.clone()),
            Some("5000".to_string()),
            None::<String>,
            now,
        ),
        (
            receiver_lock.clone(),
            tx_hash.clone(),
            1000i64,
            0i32,
            1i16,
            "token".to_string(),
            "xudt".to_string(),
            Some(asset_id.clone()),
            1i16,
            Some(sender_lock.clone()),
            Some("5000".to_string()),
            None::<String>,
            now,
        ),
    ];

    writer
        .insert_address_asset_transfers_batch(&records)
        .await
        .unwrap();

    let sender_record: (i16, String) = sqlx::query_as(
        "SELECT direction, amount::text FROM address_asset_transfers WHERE lock_script_hash = $1",
    )
    .bind(&sender_lock)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(sender_record.0, 2);
    assert_eq!(sender_record.1, "5000");

    let receiver_record: (i16, String) = sqlx::query_as(
        "SELECT direction, amount::text FROM address_asset_transfers WHERE lock_script_hash = $1",
    )
    .bind(&receiver_lock)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(receiver_record.0, 1);
    assert_eq!(receiver_record.1, "5000");
}
