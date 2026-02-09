//! Integration tests for batch NFT outpoint lookup methods.
//!
//! Tests the UNNEST-based batch lookup queries added in the performance optimization
//! (replacing N+1 per-input queries during live sync).

use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

const OWNER_LOCK: [u8; 32] = [0x55u8; 32];

// -- Spore batch lookup tests --

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_spore_batch_lookup_empty_input(pool: PgPool) {
    let writer = BatchWriter::new(pool);
    let result = writer
        .get_spore_ids_by_outpoints_batch(&[], &[])
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_spore_batch_lookup_no_matches(pool: PgPool) {
    let writer = BatchWriter::new(pool);
    let tx_hashes = vec![vec![0x01u8; 32]];
    let indices = vec![0i16];
    let result = writer
        .get_spore_ids_by_outpoints_batch(&tx_hashes, &indices)
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_spore_batch_lookup_finds_live_spore(pool: PgPool) {
    let spore_id = vec![0xAAu8; 32];
    let type_script_hash = vec![0xBBu8; 32];
    let tx_hash = vec![0x01u8; 32];
    let output_index: i16 = 0;
    let block_number: i64 = 100;

    sqlx::query(
        r#"INSERT INTO spore_cells (spore_id, type_script_hash, tx_hash, output_index,
           content_type, content_size, owner_lock_hash, created_at_block, created_at_tx, is_live)
           VALUES ($1, $2, $3, $4, 'image/png', 1024, $5, $6, $3, true)"#,
    )
    .bind(&spore_id)
    .bind(&type_script_hash)
    .bind(&tx_hash)
    .bind(output_index)
    .bind(OWNER_LOCK.as_slice())
    .bind(block_number)
    .execute(&pool)
    .await
    .unwrap();

    let writer = BatchWriter::new(pool);
    let result = writer
        .get_spore_ids_by_outpoints_batch(std::slice::from_ref(&tx_hash), &[output_index])
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, tx_hash);
    assert_eq!(result[0].1, output_index);
    assert_eq!(result[0].2, spore_id);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_spore_batch_lookup_skips_consumed(pool: PgPool) {
    let spore_id = vec![0xAAu8; 32];
    let type_script_hash = vec![0xBBu8; 32];
    let tx_hash = vec![0x01u8; 32];

    sqlx::query(
        r#"INSERT INTO spore_cells (spore_id, type_script_hash, tx_hash, output_index,
           content_type, content_size, owner_lock_hash, created_at_block, created_at_tx, is_live)
           VALUES ($1, $2, $3, 0, 'image/png', 1024, $4, 100, $3, false)"#,
    )
    .bind(&spore_id)
    .bind(&type_script_hash)
    .bind(&tx_hash)
    .bind(OWNER_LOCK.as_slice())
    .execute(&pool)
    .await
    .unwrap();

    let writer = BatchWriter::new(pool);
    let result = writer
        .get_spore_ids_by_outpoints_batch(&[tx_hash], &[0i16])
        .await
        .unwrap();

    assert!(result.is_empty(), "consumed spores should not be returned");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_spore_batch_lookup_multiple_outpoints(pool: PgPool) {
    let type_script_hash = vec![0xBBu8; 32];

    for i in 0u8..2 {
        let spore_id = vec![0xA0 + i; 32];
        let tx_hash = vec![0x10 + i; 32];
        sqlx::query(
            r#"INSERT INTO spore_cells (spore_id, type_script_hash, tx_hash, output_index,
               content_type, content_size, owner_lock_hash, created_at_block, created_at_tx, is_live)
               VALUES ($1, $2, $3, 0, 'image/png', 512, $4, $5, $3, true)"#,
        )
        .bind(&spore_id)
        .bind(&type_script_hash)
        .bind(&tx_hash)
        .bind(OWNER_LOCK.as_slice())
        .bind(100i64 + i as i64)
        .execute(&pool)
        .await
        .unwrap();
    }

    let writer = BatchWriter::new(pool);
    let tx_hashes = vec![vec![0x10u8; 32], vec![0x11u8; 32], vec![0x99u8; 32]];
    let indices = vec![0i16, 0i16, 0i16];
    let result = writer
        .get_spore_ids_by_outpoints_batch(&tx_hashes, &indices)
        .await
        .unwrap();

    assert_eq!(result.len(), 2, "should find 2 of 3 outpoints");
}

// -- mNFT batch lookup tests --

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_mnft_batch_lookup_empty_input(pool: PgPool) {
    let writer = BatchWriter::new(pool);
    let result = writer
        .get_mnft_token_ids_by_outpoints_batch(&[], &[])
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_mnft_batch_lookup_no_matches(pool: PgPool) {
    let writer = BatchWriter::new(pool);
    let result = writer
        .get_mnft_token_ids_by_outpoints_batch(&[vec![0x01u8; 32]], &[0i16])
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_mnft_batch_lookup_finds_live_token(pool: PgPool) {
    let token_id = vec![0xCCu8; 32];
    let type_script_hash = vec![0xDDu8; 32];
    let class_id = vec![0xEEu8; 32];
    let tx_hash = vec![0x01u8; 32];

    sqlx::query(
        r#"INSERT INTO mnft_tokens (token_id, type_script_hash, tx_hash, output_index,
           class_id, token_index, owner_lock_hash, created_at_block, created_at_tx, is_live)
           VALUES ($1, $2, $3, 0, $4, 0, $5, 100, $3, true)"#,
    )
    .bind(&token_id)
    .bind(&type_script_hash)
    .bind(&tx_hash)
    .bind(&class_id)
    .bind(OWNER_LOCK.as_slice())
    .execute(&pool)
    .await
    .unwrap();

    let writer = BatchWriter::new(pool);
    let result = writer
        .get_mnft_token_ids_by_outpoints_batch(std::slice::from_ref(&tx_hash), &[0i16])
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].2, token_id);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_mnft_batch_lookup_skips_consumed(pool: PgPool) {
    let token_id = vec![0xCCu8; 32];
    let type_script_hash = vec![0xDDu8; 32];
    let class_id = vec![0xEEu8; 32];
    let tx_hash = vec![0x01u8; 32];

    sqlx::query(
        r#"INSERT INTO mnft_tokens (token_id, type_script_hash, tx_hash, output_index,
           class_id, token_index, owner_lock_hash, created_at_block, created_at_tx, is_live)
           VALUES ($1, $2, $3, 0, $4, 0, $5, 100, $3, false)"#,
    )
    .bind(&token_id)
    .bind(&type_script_hash)
    .bind(&tx_hash)
    .bind(&class_id)
    .bind(OWNER_LOCK.as_slice())
    .execute(&pool)
    .await
    .unwrap();

    let writer = BatchWriter::new(pool);
    let result = writer
        .get_mnft_token_ids_by_outpoints_batch(&[tx_hash], &[0i16])
        .await
        .unwrap();

    assert!(result.is_empty());
}

// -- .bit batch lookup tests --

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_dotbit_batch_lookup_empty_input(pool: PgPool) {
    let writer = BatchWriter::new(pool);
    let result = writer
        .get_dotbit_account_ids_by_outpoints_batch(&[], &[])
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_dotbit_batch_lookup_no_matches(pool: PgPool) {
    let writer = BatchWriter::new(pool);
    let result = writer
        .get_dotbit_account_ids_by_outpoints_batch(&[vec![0x01u8; 32]], &[0i16])
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_dotbit_batch_lookup_finds_live_account(pool: PgPool) {
    let account_id = vec![0xFFu8; 20];
    let type_script_hash = vec![0xAAu8; 32];
    let tx_hash = vec![0x01u8; 32];

    sqlx::query(
        r#"INSERT INTO dotbit_accounts (account_id, type_script_hash, tx_hash, output_index,
           account_name, owner_lock_hash, created_at_block, created_at_tx, is_live)
           VALUES ($1, $2, $3, 0, 'test.bit', $4, 100, $3, true)"#,
    )
    .bind(&account_id)
    .bind(&type_script_hash)
    .bind(&tx_hash)
    .bind(OWNER_LOCK.as_slice())
    .execute(&pool)
    .await
    .unwrap();

    let writer = BatchWriter::new(pool);
    let result = writer
        .get_dotbit_account_ids_by_outpoints_batch(std::slice::from_ref(&tx_hash), &[0i16])
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].2, account_id);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_dotbit_batch_lookup_skips_consumed(pool: PgPool) {
    let account_id = vec![0xFFu8; 20];
    let type_script_hash = vec![0xAAu8; 32];
    let tx_hash = vec![0x01u8; 32];

    sqlx::query(
        r#"INSERT INTO dotbit_accounts (account_id, type_script_hash, tx_hash, output_index,
           account_name, owner_lock_hash, created_at_block, created_at_tx, is_live)
           VALUES ($1, $2, $3, 0, 'test.bit', $4, 100, $3, false)"#,
    )
    .bind(&account_id)
    .bind(&type_script_hash)
    .bind(&tx_hash)
    .bind(OWNER_LOCK.as_slice())
    .execute(&pool)
    .await
    .unwrap();

    let writer = BatchWriter::new(pool);
    let result = writer
        .get_dotbit_account_ids_by_outpoints_batch(&[tx_hash], &[0i16])
        .await
        .unwrap();

    assert!(result.is_empty());
}
