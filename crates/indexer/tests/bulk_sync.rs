use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

async fn count_indexes(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM pg_indexes 
        WHERE schemaname = 'public' 
        AND indexname NOT LIKE '%_pkey'
        "#,
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn count_droppable_indexes(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM index_config WHERE drop_during_sync = TRUE")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_drop_sync_indexes_returns_count(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let droppable = count_droppable_indexes(&pool).await;
    let indexes_before = count_indexes(&pool).await;

    let dropped = writer.drop_sync_indexes().await.unwrap();

    let indexes_after = count_indexes(&pool).await;

    assert!(dropped >= 0);
    assert_eq!(indexes_before - indexes_after, dropped as i64);
    assert!(dropped as i64 <= droppable);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_drop_sync_indexes_idempotent(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let first_drop = writer.drop_sync_indexes().await.unwrap();
    let second_drop = writer.drop_sync_indexes().await.unwrap();

    assert!(first_drop >= 0);
    assert_eq!(second_drop, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_recreate_sync_indexes_after_drop(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    let indexes_before = count_indexes(&pool).await;

    let dropped = writer.drop_sync_indexes().await.unwrap();
    assert!(dropped > 0 || count_droppable_indexes(&pool).await == 0);

    let recreated: i32 = sqlx::query_scalar("SELECT recreate_sync_indexes()")
        .fetch_one(&pool)
        .await
        .unwrap();

    let indexes_after = count_indexes(&pool).await;

    assert_eq!(dropped, recreated);
    assert_eq!(indexes_before, indexes_after);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_cells_with_skip_live_cells(pool: PgPool) {
    use ckbadger_indexer::parser::cell::ParsedCell;

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
        data_size: 0,
        data: vec![],
    };

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)], true)
        .await
        .unwrap();

    let cells_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cells")
        .fetch_one(&pool)
        .await
        .unwrap();

    let live_cells_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM live_cells")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(cells_count, 1);
    assert_eq!(live_cells_count, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_cells_without_skip_live_cells(pool: PgPool) {
    use ckbadger_indexer::parser::cell::ParsedCell;

    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x02u8; 32];
    let cell = ParsedCell {
        capacity: 200_00000000,
        lock_code_hash: vec![0x11u8; 32],
        lock_hash_type: 0,
        lock_args: vec![0x22u8; 20],
        lock_script_hash: vec![0x33u8; 32],
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        type_script_hash: None,
        data_hash: vec![0x88u8; 32],
        data_size: 0,
        data: vec![],
    };

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 2000)], false)
        .await
        .unwrap();

    let cells_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cells")
        .fetch_one(&pool)
        .await
        .unwrap();

    let live_cells_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM live_cells")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(cells_count, 1);
    assert_eq!(live_cells_count, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_migrate_live_cells_populates_from_cells(pool: PgPool) {
    use ckbadger_indexer::parser::cell::ParsedCell;

    let writer = BatchWriter::new(pool.clone());

    let tx_hash1 = vec![0x01u8; 32];
    let tx_hash2 = vec![0x02u8; 32];

    let make_cell = |cap: i64| ParsedCell {
        capacity: cap,
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
    };

    writer
        .insert_cells_batch(
            &[
                (&tx_hash1, 0, &make_cell(100_00000000), 1000),
                (&tx_hash2, 0, &make_cell(200_00000000), 1001),
            ],
            true,
        )
        .await
        .unwrap();

    let live_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM live_cells")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(live_before, 0);

    let migrated = writer.migrate_live_cells().await.unwrap();
    assert_eq!(migrated, 2);

    let live_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM live_cells")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(live_after, 2);
}
