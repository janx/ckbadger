//! Pipeline vs Sequential Mode Consistency Tests
//!
//! Verifies that the three-stage pipeline (Fetcher → Parser → Writer) produces
//! identical database state to sequential processing.

#![allow(clippy::type_complexity)]

use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::parser::cell::ParsedCell;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;
use std::collections::HashMap;

fn make_cell(capacity: i64, data_size: i32, lock_hash_byte: u8) -> ParsedCell {
    ParsedCell {
        capacity,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        lock_script_hash: vec![lock_hash_byte; 32],
        type_code_hash: Some(vec![0x44u8; 32]),
        type_hash_type: Some(1),
        type_args: Some(vec![0x55u8; 20]),
        type_script_hash: Some(vec![0x66u8; 32]),
        data_hash: vec![0x77u8; 32],
        data_size,
        data: vec![0u8; data_size as usize],
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_cell_info_lookup_returns_all_fields(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell = make_cell(100_00000000, 256, 0xAA);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], false)
        .await
        .unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx_hash, 0)], false)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    let (capacity, created_at_block, lock_script_hash, data_size) =
        result.get(&(tx_hash.clone(), 0)).unwrap();

    assert_eq!(*capacity, 100_00000000);
    assert_eq!(*created_at_block, 1000);
    assert_eq!(*lock_script_hash, vec![0xAAu8; 32]);
    assert_eq!(*data_size, 256);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_cell_info_batch_lookup_multiple_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let tx1 = vec![0x01u8; 32];
    let tx2 = vec![0x02u8; 32];
    let tx3 = vec![0x03u8; 32];

    let cell1 = make_cell(100_00000000, 100, 0xAA);
    let cell2 = make_cell(200_00000000, 200, 0xBB);
    let cell3 = make_cell(300_00000000, 300, 0xCC);

    writer
        .insert_cells_batch(
            &[
                (&tx1, 0, &cell1, 1000),
                (&tx2, 0, &cell2, 2000),
                (&tx3, 0, &cell3, 3000),
            ],
            false,
        )
        .await
        .unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx1, 0), (&tx2, 0), (&tx3, 0)], false)
        .await
        .unwrap();

    assert_eq!(result.len(), 3);

    let (cap1, block1, _, size1) = result.get(&(tx1.clone(), 0)).unwrap();
    assert_eq!(*cap1, 100_00000000);
    assert_eq!(*block1, 1000);
    assert_eq!(*size1, 100);

    let (cap2, block2, _, size2) = result.get(&(tx2.clone(), 0)).unwrap();
    assert_eq!(*cap2, 200_00000000);
    assert_eq!(*block2, 2000);
    assert_eq!(*size2, 200);

    let (cap3, block3, _, size3) = result.get(&(tx3.clone(), 0)).unwrap();
    assert_eq!(*cap3, 300_00000000);
    assert_eq!(*block3, 3000);
    assert_eq!(*size3, 300);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_code_hash_lookup_returns_lock_and_type(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell = ParsedCell {
        capacity: 100_00000000,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        lock_script_hash: vec![0x33u8; 32],
        type_code_hash: Some(vec![0x44u8; 32]),
        type_hash_type: Some(1),
        type_args: Some(vec![0x55u8; 20]),
        type_script_hash: Some(vec![0x66u8; 32]),
        data_hash: vec![0x77u8; 32],
        data_size: 100,
        data: vec![0u8; 100],
    };

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], false)
        .await
        .unwrap();

    let result = writer
        .get_cells_code_hashes_batch(&[(&tx_hash, 0)], false)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    let (lock_code_hash, type_code_hash) = result.get(&(tx_hash.clone(), 0)).unwrap();

    assert_eq!(*lock_code_hash, vec![0x11u8; 32]);
    assert_eq!(*type_code_hash, Some(vec![0x44u8; 32]));
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_code_hash_lookup_no_type_script(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell = ParsedCell {
        capacity: 100_00000000,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        lock_script_hash: vec![0x33u8; 32],
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        type_script_hash: None,
        data_hash: vec![0x77u8; 32],
        data_size: 100,
        data: vec![0u8; 100],
    };

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], false)
        .await
        .unwrap();

    let result = writer
        .get_cells_code_hashes_batch(&[(&tx_hash, 0)], false)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    let (lock_code_hash, type_code_hash) = result.get(&(tx_hash.clone(), 0)).unwrap();

    assert_eq!(*lock_code_hash, vec![0x11u8; 32]);
    assert_eq!(*type_code_hash, None);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_same_batch_cell_consumption(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let creating_tx = vec![0x01u8; 32];
    let consuming_tx = vec![0x02u8; 32];
    let cell = make_cell(100_00000000, 100, 0xAA);

    writer
        .insert_cells_batch(&[(&creating_tx, 0, &cell, 1000)], false)
        .await
        .unwrap();

    writer
        .consume_cells_batch(&[(&creating_tx, 0, 1000, &consuming_tx, 1000, 0)], false)
        .await
        .unwrap();

    let status: Option<(i16,)> =
        sqlx::query_as("SELECT status FROM cells WHERE tx_hash = $1 AND output_index = 0")
            .bind(&creating_tx)
            .fetch_optional(&pool)
            .await
            .unwrap();

    assert!(status.is_some());
    assert_eq!(status.unwrap().0, 1);

    let live: Option<(i64,)> =
        sqlx::query_as("SELECT capacity FROM live_cells WHERE tx_hash = $1 AND output_index = 0")
            .bind(&creating_tx)
            .fetch_optional(&pool)
            .await
            .unwrap();

    assert!(live.is_none());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_script_usage_cell_creation(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let lock_code_hash = vec![0x11u8; 32];
    let mut changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();

    changes.insert(
        (lock_code_hash.clone(), false),
        (1, 1, 100_00000000, 100_00000000),
    );

    writer.update_script_usage_batch(&changes).await.unwrap();

    let result: Option<(i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT cells_count, live_cells_count, capacity_sum::bigint, live_capacity_sum::bigint 
         FROM script_usage_stats WHERE code_hash = $1 AND script_kind = 'lock'",
    )
    .bind(&lock_code_hash)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(result.is_some());
    let (cells_total, cells_live, cap_total, cap_live) = result.unwrap();
    assert_eq!(cells_total, 1);
    assert_eq!(cells_live, 1);
    assert_eq!(cap_total, 100_00000000);
    assert_eq!(cap_live, 100_00000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_script_usage_cell_consumption(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let lock_code_hash = vec![0x11u8; 32];

    let mut create_changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    create_changes.insert(
        (lock_code_hash.clone(), false),
        (1, 1, 100_00000000, 100_00000000),
    );
    writer
        .update_script_usage_batch(&create_changes)
        .await
        .unwrap();

    let mut consume_changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    consume_changes.insert((lock_code_hash.clone(), false), (0, -1, 0, -100_00000000));
    writer
        .update_script_usage_batch(&consume_changes)
        .await
        .unwrap();

    let result: Option<(i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT cells_count, live_cells_count, capacity_sum::bigint, live_capacity_sum::bigint 
         FROM script_usage_stats WHERE code_hash = $1 AND script_kind = 'lock'",
    )
    .bind(&lock_code_hash)
    .fetch_optional(&pool)
    .await
    .unwrap();

    let (cells_total, cells_live, cap_total, cap_live) = result.unwrap();
    assert_eq!(cells_total, 1);
    assert_eq!(cells_live, 0);
    assert_eq!(cap_total, 100_00000000);
    assert_eq!(cap_live, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_address_balance_update_receive(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let lock_hash = vec![0xAAu8; 32];
    let tx_hash = vec![0x01u8; 32];

    let changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> = [(
        lock_hash.clone(),
        (100_00000000, 1, 1, 1, 1000, tx_hash.as_slice()),
    )]
    .into_iter()
    .collect();

    writer
        .update_address_balances_batch(&changes)
        .await
        .unwrap();

    let result: Option<(i64, i32, i64)> = sqlx::query_as(
        "SELECT balance::bigint, live_cells_count, transactions_count 
         FROM address_balances WHERE lock_script_hash = $1",
    )
    .bind(&lock_hash)
    .fetch_optional(&pool)
    .await
    .unwrap();

    let (balance, cells, txs) = result.unwrap();
    assert_eq!(balance, 100_00000000);
    assert_eq!(cells, 1);
    assert_eq!(txs, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_address_balance_update_send(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let lock_hash = vec![0xAAu8; 32];
    let tx_hash1 = vec![0x01u8; 32];
    let tx_hash2 = vec![0x02u8; 32];

    let receive: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> = [(
        lock_hash.clone(),
        (100_00000000, 1, 1, 1, 1000, tx_hash1.as_slice()),
    )]
    .into_iter()
    .collect();
    writer
        .update_address_balances_batch(&receive)
        .await
        .unwrap();

    let send: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8])> = [(
        lock_hash.clone(),
        (-30_00000000, 0, 1, 1, 2000, tx_hash2.as_slice()),
    )]
    .into_iter()
    .collect();
    writer.update_address_balances_batch(&send).await.unwrap();

    let result: Option<(i64, i32, i64)> = sqlx::query_as(
        "SELECT balance::bigint, live_cells_count, transactions_count 
         FROM address_balances WHERE lock_script_hash = $1",
    )
    .bind(&lock_hash)
    .fetch_optional(&pool)
    .await
    .unwrap();

    let (balance, cells, txs) = result.unwrap();
    assert_eq!(balance, 70_00000000);
    assert_eq!(cells, 1);
    assert_eq!(txs, 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_multiple_outputs_same_tx(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell0 = make_cell(100_00000000, 100, 0xAA);
    let cell1 = make_cell(200_00000000, 200, 0xBB);
    let cell2 = make_cell(300_00000000, 300, 0xCC);

    writer
        .insert_cells_batch(
            &[
                (&tx_hash, 0, &cell0, 1000),
                (&tx_hash, 1, &cell1, 1000),
                (&tx_hash, 2, &cell2, 1000),
            ],
            false,
        )
        .await
        .unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx_hash, 0), (&tx_hash, 1), (&tx_hash, 2)], false)
        .await
        .unwrap();

    assert_eq!(result.len(), 3);

    let (cap0, _, _, _) = result.get(&(tx_hash.clone(), 0)).unwrap();
    assert_eq!(*cap0, 100_00000000);

    let (cap1, _, _, _) = result.get(&(tx_hash.clone(), 1)).unwrap();
    assert_eq!(*cap1, 200_00000000);

    let (cap2, _, _, _) = result.get(&(tx_hash.clone(), 2)).unwrap();
    assert_eq!(*cap2, 300_00000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_consumed_cell_not_in_info_batch(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let consuming_tx = vec![0x02u8; 32];
    let cell = make_cell(100_00000000, 100, 0xAA);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], false)
        .await
        .unwrap();
    writer
        .consume_cells_batch(&[(&tx_hash, 0, 1000, &consuming_tx, 2000, 0)], false)
        .await
        .unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx_hash, 0)], false)
        .await
        .unwrap();
    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_cross_partition_cell_lookup(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_p0 = vec![0x01u8; 32];
    let tx_p1 = vec![0x02u8; 32];
    let cell = make_cell(100_00000000, 100, 0xAA);

    writer
        .insert_cells_batch(
            &[(&tx_p0, 0, &cell, 1_000_000), (&tx_p1, 0, &cell, 6_000_000)],
            false,
        )
        .await
        .unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx_p0, 0), (&tx_p1, 0)], false)
        .await
        .unwrap();

    assert_eq!(result.len(), 2);

    let (_, block0, _, _) = result.get(&(tx_p0.clone(), 0)).unwrap();
    assert_eq!(*block0, 1_000_000);

    let (_, block1, _, _) = result.get(&(tx_p1.clone(), 0)).unwrap();
    assert_eq!(*block1, 6_000_000);
}
