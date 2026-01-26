#![allow(clippy::type_complexity)]

use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::parser::{ParsedUdtCell, UdtStandard};
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

fn make_udt_cell(amount: u128, standard: UdtStandard) -> ParsedUdtCell {
    ParsedUdtCell {
        type_script_hash: vec![0x11u8; 32],
        type_code_hash: vec![0x22u8; 32],
        type_hash_type: 1,
        type_args: vec![0x33u8; 32],
        lock_script_hash: vec![0x44u8; 32],
        amount,
        standard,
    }
}

async fn get_udt_cells_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM udt_cells")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn get_live_udt_cells_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM udt_cells WHERE is_live = TRUE")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_udt_cells_batch(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell = make_udt_cell(1000_00000000, UdtStandard::Sudt);

    writer
        .insert_udt_cells_batch(&[(&tx_hash, 0, &cell, 1000)])
        .await
        .unwrap();

    assert_eq!(get_udt_cells_count(&pool).await, 1);
    assert_eq!(get_live_udt_cells_count(&pool).await, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_multiple_udt_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash1 = vec![0x01u8; 32];
    let tx_hash2 = vec![0x02u8; 32];
    let cell1 = make_udt_cell(1000_00000000, UdtStandard::Sudt);
    let cell2 = make_udt_cell(2000_00000000, UdtStandard::Xudt);

    writer
        .insert_udt_cells_batch(&[(&tx_hash1, 0, &cell1, 1000), (&tx_hash2, 0, &cell2, 1001)])
        .await
        .unwrap();

    assert_eq!(get_udt_cells_count(&pool).await, 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_consume_udt_cells_batch(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let consuming_tx = vec![0x02u8; 32];
    let cell = make_udt_cell(1000_00000000, UdtStandard::Sudt);

    writer
        .insert_udt_cells_batch(&[(&tx_hash, 0, &cell, 1000)])
        .await
        .unwrap();

    assert_eq!(get_live_udt_cells_count(&pool).await, 1);

    writer
        .consume_udt_cells_batch(&[(&tx_hash, 0, 1001, &consuming_tx)])
        .await
        .unwrap();

    assert_eq!(get_live_udt_cells_count(&pool).await, 0);
    assert_eq!(get_udt_cells_count(&pool).await, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_udt_cells_info_batch(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell = make_udt_cell(1000_00000000, UdtStandard::Xudt);

    writer
        .insert_udt_cells_batch(&[(&tx_hash, 0, &cell, 1000)])
        .await
        .unwrap();

    let result = writer
        .get_udt_cells_info_batch(&[(&tx_hash, 0)])
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    let (
        type_script_hash,
        type_code_hash,
        type_hash_type,
        type_args,
        lock_script_hash,
        amount,
        standard,
    ) = result.get(&(tx_hash.clone(), 0)).unwrap();

    assert_eq!(type_script_hash, &vec![0x11u8; 32]);
    assert_eq!(type_code_hash, &vec![0x22u8; 32]);
    assert_eq!(*type_hash_type, 1);
    assert_eq!(type_args, &vec![0x33u8; 32]);
    assert_eq!(lock_script_hash, &vec![0x44u8; 32]);
    assert_eq!(*amount, 1000_00000000u128);
    assert_eq!(standard, "xudt");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_udt_cells_info_batch_returns_empty_for_consumed(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let consuming_tx = vec![0x02u8; 32];
    let cell = make_udt_cell(1000_00000000, UdtStandard::Sudt);

    writer
        .insert_udt_cells_batch(&[(&tx_hash, 0, &cell, 1000)])
        .await
        .unwrap();

    writer
        .consume_udt_cells_batch(&[(&tx_hash, 0, 1001, &consuming_tx)])
        .await
        .unwrap();

    let result = writer
        .get_udt_cells_info_batch(&[(&tx_hash, 0)])
        .await
        .unwrap();

    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_udt_cell_upsert_on_reuse(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let consuming_tx = vec![0x02u8; 32];

    let cell1 = make_udt_cell(1000_00000000, UdtStandard::Sudt);
    writer
        .insert_udt_cells_batch(&[(&tx_hash, 0, &cell1, 1000)])
        .await
        .unwrap();

    writer
        .consume_udt_cells_batch(&[(&tx_hash, 0, 1001, &consuming_tx)])
        .await
        .unwrap();

    let mut cell2 = make_udt_cell(2000_00000000, UdtStandard::Sudt);
    cell2.lock_script_hash = vec![0x55u8; 32];
    writer
        .insert_udt_cells_batch(&[(&tx_hash, 0, &cell2, 2000)])
        .await
        .unwrap();

    assert_eq!(get_udt_cells_count(&pool).await, 1);
    assert_eq!(get_live_udt_cells_count(&pool).await, 1);

    let result = writer
        .get_udt_cells_info_batch(&[(&tx_hash, 0)])
        .await
        .unwrap();
    let (_, _, _, _, lock_script_hash, amount, _) = result.get(&(tx_hash.clone(), 0)).unwrap();
    assert_eq!(*amount, 2000_00000000u128);
    assert_eq!(lock_script_hash, &vec![0x55u8; 32]);
}
