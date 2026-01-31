//! Crash Recovery Tests
//!
//! Tests for database consistency detection and partial batch cleanup
//! to ensure the indexer can recover from crashes mid-batch.

use chrono::{DateTime, Utc};
use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::parser::block::ParsedBlock;
use ckbadger_indexer::parser::cell::ParsedCell;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

fn make_block(number: i64) -> ParsedBlock {
    ParsedBlock {
        number,
        hash: vec![number as u8; 32],
        parent_hash: vec![(number.wrapping_sub(1)) as u8; 32],
        timestamp: Utc::now(),
        version: 0,
        compact_target: 0x1a000000,
        transactions_count: 1,
        proposals_count: 0,
        uncles_count: 0,
        epoch_number: number / 1800,
        epoch_index: (number % 1800) as i32,
        epoch_length: 1800,
        dao: vec![0u8; 32],
        nonce: vec![0u8; 16],
        extra_hash: vec![0u8; 32],
        proposals_hash: vec![0u8; 32],
        transactions_root: vec![0u8; 32],
        uncles_hash: vec![0u8; 32],
        proposals: vec![],
    }
}

type TxTuple<'a> = (
    &'a [u8],
    i64,
    i32,
    i32,
    i16,
    i16,
    i16,
    i16,
    i16,
    i64,
    i64,
    i64,
    Option<i32>,
    Option<i64>,
    bool,
    DateTime<Utc>,
);

fn make_tx_tuple(hash: &[u8], block_number: i64) -> TxTuple<'_> {
    (
        hash,
        block_number,
        0,
        0,
        0,
        1,
        1,
        0,
        0,
        0,
        100_00000000,
        0,
        Some(100),
        Some(1000),
        true,
        Utc::now(),
    )
}

fn make_cell(capacity: i64) -> ParsedCell {
    ParsedCell {
        capacity,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        lock_script_hash: vec![0x33u8; 32],
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        type_script_hash: None,
        data_hash: vec![0x77u8; 32],
        data_size: 0,
        data: vec![],
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_find_last_consistent_block_empty_db(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let result = writer.find_last_consistent_block().await.unwrap();
    assert_eq!(result, None);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_find_last_consistent_block_consistent_state(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block = make_block(100);
    writer.insert_blocks_batch(&[&block]).await.unwrap();

    let tx_hash = vec![0x01u8; 32];
    let tx = make_tx_tuple(&tx_hash, 100);
    writer.insert_transactions_batch(&[tx]).await.unwrap();

    let result = writer.find_last_consistent_block().await.unwrap();
    assert_eq!(result, Some(100));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_find_last_consistent_block_detects_inconsistency(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block1 = make_block(100);
    let block2 = make_block(101);
    let block3 = make_block(102);
    writer
        .insert_blocks_batch(&[&block1, &block2, &block3])
        .await
        .unwrap();

    let tx_hash = vec![0x01u8; 32];
    let tx = make_tx_tuple(&tx_hash, 100);
    writer.insert_transactions_batch(&[tx]).await.unwrap();

    let result = writer.find_last_consistent_block().await.unwrap();
    assert_eq!(result, Some(100));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_find_last_consistent_block_blocks_only(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let block = make_block(100);
    writer.insert_blocks_batch(&[&block]).await.unwrap();

    let result = writer.find_last_consistent_block().await.unwrap();
    assert_eq!(result, Some(-1));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_cleanup_batch_range_cleans_transactions(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let tx1_hash = vec![0x01u8; 32];
    let tx2_hash = vec![0x02u8; 32];
    let tx3_hash = vec![0x03u8; 32];
    let tx1 = make_tx_tuple(&tx1_hash, 100);
    let tx2 = make_tx_tuple(&tx2_hash, 101);
    let tx3 = make_tx_tuple(&tx3_hash, 102);
    writer
        .insert_transactions_batch(&[tx1, tx2, tx3])
        .await
        .unwrap();

    let count_before: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transactions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_before.0, 3);

    writer.cleanup_batch_range(101, 102).await.unwrap();

    let count_after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transactions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_after.0, 1);

    let remaining: (i64,) =
        sqlx::query_as("SELECT block_number FROM transactions WHERE block_number = 100")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining.0, 100);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_cleanup_batch_range_cleans_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let tx1 = vec![0x01u8; 32];
    let tx2 = vec![0x02u8; 32];
    let cell = make_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx1, 0, &cell, 100), (&tx2, 0, &cell, 101)], false)
        .await
        .unwrap();

    let count_before: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cells")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_before.0, 2);

    writer.cleanup_batch_range(101, 102).await.unwrap();

    let count_after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cells")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count_after.0, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_cleanup_batch_range_preserves_earlier_data(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let tx1_hash = vec![0x01u8; 32];
    let tx2_hash = vec![0x02u8; 32];
    let tx3_hash = vec![0x03u8; 32];
    let tx1 = make_tx_tuple(&tx1_hash, 50);
    let tx2 = make_tx_tuple(&tx2_hash, 100);
    let tx3 = make_tx_tuple(&tx3_hash, 150);
    writer
        .insert_transactions_batch(&[tx1, tx2, tx3])
        .await
        .unwrap();

    writer.cleanup_batch_range(100, 150).await.unwrap();

    let remaining: Vec<(i64,)> =
        sqlx::query_as("SELECT block_number FROM transactions ORDER BY block_number")
            .fetch_all(&pool)
            .await
            .unwrap();

    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].0, 50);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_cleanup_batch_range_empty_range(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let tx_hash = vec![0x01u8; 32];
    let tx = make_tx_tuple(&tx_hash, 100);
    writer.insert_transactions_batch(&[tx]).await.unwrap();

    writer.cleanup_batch_range(200, 300).await.unwrap();

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transactions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}
