#![allow(clippy::type_complexity)]

use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::parser::cell::ParsedCell;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

fn make_parsed_cell(capacity: i64) -> ParsedCell {
    ParsedCell {
        capacity,
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
    }
}

async fn get_cells_count(pool: &PgPool, status: i16) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM cells WHERE status = $1")
        .bind(status)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_cells_creates_live_cell(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], false)
        .await
        .unwrap();

    assert_eq!(get_cells_count(&pool, 0).await, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_multiple_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash1 = vec![0x01u8; 32];
    let tx_hash2 = vec![0x02u8; 32];
    let cell1 = make_parsed_cell(100_00000000);
    let cell2 = make_parsed_cell(200_00000000);

    writer
        .insert_cells_batch(
            &[(&tx_hash1, 0, &cell1, 1000), (&tx_hash2, 0, &cell2, 1001)],
            false,
        )
        .await
        .unwrap();

    assert_eq!(get_cells_count(&pool, 0).await, 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_consume_cells_marks_consumed(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let consuming_tx = vec![0x02u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], false)
        .await
        .unwrap();

    assert_eq!(get_cells_count(&pool, 0).await, 1);

    writer
        .consume_cells_batch(&[(&tx_hash, 0, 1000, &consuming_tx, 1001, 0)], false)
        .await
        .unwrap();

    assert_eq!(get_cells_count(&pool, 0).await, 0);
    assert_eq!(get_cells_count(&pool, 1).await, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_cells_info_batch_returns_live_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], false)
        .await
        .unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx_hash, 0)], false)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    let (capacity, block, lock_hash, data_size) = result.get(&(tx_hash.clone(), 0)).unwrap();
    assert_eq!(*capacity, 100_00000000);
    assert_eq!(*block, 1000);
    assert_eq!(lock_hash.len(), 32);
    assert_eq!(*data_size, 100);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_cells_info_batch_returns_empty_for_consumed(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let consuming_tx = vec![0x02u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], false)
        .await
        .unwrap();

    writer
        .consume_cells_batch(&[(&tx_hash, 0, 1000, &consuming_tx, 1001, 0)], false)
        .await
        .unwrap();

    let result = writer
        .get_cells_info_batch(&[(&tx_hash, 0)], false)
        .await
        .unwrap();

    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_consume_cells_across_partitions(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let tx_p0 = vec![0x01u8; 32];
    let tx_p1 = vec![0x02u8; 32];
    let tx_p2 = vec![0x03u8; 32];
    let consuming_tx = vec![0xFFu8; 32];

    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(
            &[
                (&tx_p0, 0, &cell, 1_000_000),
                (&tx_p1, 0, &cell, 6_000_000),
                (&tx_p2, 0, &cell, 11_000_000),
            ],
            false,
        )
        .await
        .unwrap();

    assert_eq!(get_cells_count(&pool, 0).await, 3);

    writer
        .consume_cells_batch(
            &[
                (&tx_p0, 0, 1_000_000, &consuming_tx, 13_000_000, 0),
                (&tx_p1, 0, 6_000_000, &consuming_tx, 13_000_000, 1),
                (&tx_p2, 0, 11_000_000, &consuming_tx, 13_000_000, 2),
            ],
            false,
        )
        .await
        .unwrap();

    assert_eq!(get_cells_count(&pool, 0).await, 0);
    assert_eq!(get_cells_count(&pool, 1).await, 3);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_bulk_sync_mode_stores_in_rocksdb(pool: PgPool) {
    use ckbadger_indexer::db::{LiveCellStorage, RocksDbLiveCellStore};
    use ckbadger_indexer::CacheInvalidator;
    use std::sync::Arc;

    let tmp_dir = tempfile::TempDir::new().unwrap();
    let store = Arc::new(RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap());
    let cache = CacheInvalidator::new(None).await;
    let writer = BatchWriter::with_live_cell_store(pool.clone(), true, store.clone(), cache);

    let tx_hash = vec![0x01u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], true)
        .await
        .unwrap();

    let in_store = store.get(&tx_hash, 0);
    assert!(in_store.is_some());
    assert_eq!(in_store.unwrap().capacity, 100_00000000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_rocksdb_store_persists_cells(pool: PgPool) {
    use ckbadger_indexer::db::{LiveCellStorage, RocksDbLiveCellStore};
    use ckbadger_indexer::CacheInvalidator;
    use std::sync::Arc;

    let tmp_dir = tempfile::TempDir::new().unwrap();
    let path = tmp_dir.path().to_path_buf();

    let tx_hash = vec![0x01u8; 32];
    let cell = make_parsed_cell(100_00000000);

    {
        let store = Arc::new(RocksDbLiveCellStore::open(&path, true).unwrap());
        let cache = CacheInvalidator::new(None).await;
        let writer = BatchWriter::with_live_cell_store(pool.clone(), true, store.clone(), cache);

        writer
            .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], true)
            .await
            .unwrap();

        assert_eq!(store.len(), 1);
    }

    {
        let store = RocksDbLiveCellStore::open(&path, true).unwrap();
        assert_eq!(store.len(), 1);
        let retrieved = store.get(&tx_hash, 0);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().capacity, 100_00000000);
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_cells_info_batch_falls_back_to_cells_table(pool: PgPool) {
    use ckbadger_indexer::db::RocksDbLiveCellStore;
    use ckbadger_indexer::CacheInvalidator;
    use std::sync::Arc;

    let tmp_dir = tempfile::TempDir::new().unwrap();
    let store = Arc::new(RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap());
    let cache = CacheInvalidator::new(None).await;
    let writer = BatchWriter::with_live_cell_store(pool.clone(), true, store, cache);

    let tx_hash = vec![0x01u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], true)
        .await
        .unwrap();

    let cell_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cells WHERE tx_hash = $1)")
            .bind(&tx_hash)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(cell_exists);

    drop(writer);
    let writer_no_store = BatchWriter::new(pool.clone());
    let result = writer_no_store
        .get_cells_info_batch(&[(&tx_hash, 0)], false)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    let (capacity, block, lock_hash, data_size) = result.get(&(tx_hash.clone(), 0)).unwrap();
    assert_eq!(*capacity, 100_00000000);
    assert_eq!(*block, 1000);
    assert_eq!(lock_hash.len(), 32);
    assert_eq!(*data_size, 100);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_cells_info_batch_bulk_sync_skips_db_fallback(pool: PgPool) {
    use ckbadger_indexer::db::{LiveCellStorage, RocksDbLiveCellStore};
    use ckbadger_indexer::CacheInvalidator;
    use std::sync::Arc;

    let tmp_dir = tempfile::TempDir::new().unwrap();
    let store = Arc::new(RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap());
    let cache = CacheInvalidator::new(None).await;
    let writer = BatchWriter::with_live_cell_store(pool.clone(), true, store.clone(), cache);

    let tx_hash_in_rocksdb = vec![0x01u8; 32];
    let tx_hash_only_in_db = vec![0x02u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash_in_rocksdb, 0, &cell, 1000)], true)
        .await
        .unwrap();

    assert!(store.get(&tx_hash_in_rocksdb, 0).is_some());

    let writer_no_store = BatchWriter::new(pool.clone());
    writer_no_store
        .insert_cells_batch(&[(&tx_hash_only_in_db, 0, &cell, 1001)], false)
        .await
        .unwrap();

    assert!(store.get(&tx_hash_only_in_db, 0).is_none());

    let result_bulk = writer
        .get_cells_info_batch(&[(&tx_hash_in_rocksdb, 0), (&tx_hash_only_in_db, 0)], true)
        .await
        .unwrap();
    assert_eq!(result_bulk.len(), 1);
    assert!(result_bulk.contains_key(&(tx_hash_in_rocksdb.clone(), 0)));
    assert!(!result_bulk.contains_key(&(tx_hash_only_in_db.clone(), 0)));

    let result_non_bulk = writer
        .get_cells_info_batch(&[(&tx_hash_in_rocksdb, 0), (&tx_hash_only_in_db, 0)], false)
        .await
        .unwrap();
    assert_eq!(result_non_bulk.len(), 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_cells_code_hashes_batch_bulk_sync_skips_db_fallback(pool: PgPool) {
    use ckbadger_indexer::db::{LiveCellStorage, RocksDbLiveCellStore};
    use ckbadger_indexer::CacheInvalidator;
    use std::sync::Arc;

    let tmp_dir = tempfile::TempDir::new().unwrap();
    let store = Arc::new(RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap());
    let cache = CacheInvalidator::new(None).await;
    let writer = BatchWriter::with_live_cell_store(pool.clone(), true, store.clone(), cache);

    let tx_hash_in_rocksdb = vec![0x01u8; 32];
    let tx_hash_only_in_db = vec![0x02u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash_in_rocksdb, 0, &cell, 1000)], false)
        .await
        .unwrap();

    assert!(store.get(&tx_hash_in_rocksdb, 0).is_some());

    let writer_no_store = BatchWriter::new(pool.clone());
    writer_no_store
        .insert_cells_batch(&[(&tx_hash_only_in_db, 0, &cell, 1001)], false)
        .await
        .unwrap();

    assert!(store.get(&tx_hash_only_in_db, 0).is_none());

    let result_bulk = writer
        .get_cells_code_hashes_batch(&[(&tx_hash_in_rocksdb, 0), (&tx_hash_only_in_db, 0)], true)
        .await
        .unwrap();
    assert_eq!(result_bulk.len(), 1);
    assert!(result_bulk.contains_key(&(tx_hash_in_rocksdb.clone(), 0)));
    assert!(!result_bulk.contains_key(&(tx_hash_only_in_db.clone(), 0)));

    let result_non_bulk = writer
        .get_cells_code_hashes_batch(&[(&tx_hash_in_rocksdb, 0), (&tx_hash_only_in_db, 0)], false)
        .await
        .unwrap();
    assert_eq!(result_non_bulk.len(), 2);
}
