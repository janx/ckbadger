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

async fn get_live_cells_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM live_cells")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn get_live_cell(pool: &PgPool, tx_hash: &[u8], output_index: i16) -> Option<(i64, i64)> {
    sqlx::query_as::<_, (i64, i64)>(
        "SELECT capacity, created_at_block FROM live_cells WHERE tx_hash = $1 AND output_index = $2",
    )
    .bind(tx_hash)
    .bind(output_index)
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_cells_creates_live_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)])
        .await
        .unwrap();

    assert_eq!(get_live_cells_count(&pool).await, 1);

    let (capacity, block) = get_live_cell(&pool, &tx_hash, 0).await.unwrap();
    assert_eq!(capacity, 100_00000000);
    assert_eq!(block, 1000);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_live_cells_stores_lock_args(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)])
        .await
        .unwrap();

    let lock_args: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT lock_args FROM live_cells WHERE tx_hash = $1 AND output_index = $2",
    )
    .bind(&tx_hash)
    .bind(0i16)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(lock_args.is_some());
    assert_eq!(lock_args.unwrap(), vec![0x22u8; 20]);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_insert_multiple_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash1 = vec![0x01u8; 32];
    let tx_hash2 = vec![0x02u8; 32];
    let cell1 = make_parsed_cell(100_00000000);
    let cell2 = make_parsed_cell(200_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash1, 0, &cell1, 1000), (&tx_hash2, 0, &cell2, 1001)])
        .await
        .unwrap();

    assert_eq!(get_live_cells_count(&pool).await, 2);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_consume_cells_removes_from_live_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let consuming_tx = vec![0x02u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)])
        .await
        .unwrap();

    assert_eq!(get_live_cells_count(&pool).await, 1);

    writer
        .consume_cells_batch(&[(&tx_hash, 0, 1000, &consuming_tx, 1001, 0)], false)
        .await
        .unwrap();

    assert_eq!(get_live_cells_count(&pool).await, 0);
    assert!(get_live_cell(&pool, &tx_hash, 0).await.is_none());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_get_cells_info_batch_queries_live_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let tx_hash = vec![0x01u8; 32];
    let cell = make_parsed_cell(100_00000000);

    writer
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)])
        .await
        .unwrap();

    let result = writer.get_cells_info_batch(&[(&tx_hash, 0)]).await.unwrap();

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
        .insert_cells_batch(&[(&tx_hash, 0, &cell, 1000)])
        .await
        .unwrap();

    writer
        .consume_cells_batch(&[(&tx_hash, 0, 1000, &consuming_tx, 1001, 0)], false)
        .await
        .unwrap();

    let result = writer.get_cells_info_batch(&[(&tx_hash, 0)]).await.unwrap();

    assert!(result.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_reorg_restores_live_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());

    sqlx::query("INSERT INTO sync_status (id, tip_block_number, tip_block_hash) VALUES (1, 0, '') ON CONFLICT (id) DO NOTHING")
        .execute(&pool)
        .await
        .unwrap();

    let block_hash_99 = vec![0x99u8; 32];
    let block_hash_100 = vec![0xAAu8; 32];
    let block_hash_101 = vec![0xBBu8; 32];

    for (num, hash, parent) in [
        (99i64, &block_hash_99, &vec![0x98u8; 32]),
        (100, &block_hash_100, &block_hash_99),
        (101, &block_hash_101, &block_hash_100),
    ] {
        sqlx::query(
            r#"INSERT INTO blocks (number, hash, parent_hash, timestamp, transactions_count, 
               proposals_count, uncles_count, epoch_number, epoch_index, epoch_length,
               nonce, transactions_root, proposals_hash, extra_hash, uncles_hash,
               compact_target, version, dao)
               VALUES ($1, $2, $3, NOW(), 1, 0, 0, 1, 1, 1800, '', '', '', '', '', 0, 0, '')"#,
        )
        .bind(num)
        .bind(hash.as_slice())
        .bind(parent.as_slice())
        .execute(&pool)
        .await
        .unwrap();
    }

    let tx_hash_consumed = vec![0x01u8; 32];
    let tx_hash_created = vec![0x02u8; 32];

    sqlx::query(
        r#"INSERT INTO cells (tx_hash, output_index, capacity, lock_code_hash, lock_hash_type, 
           lock_args, lock_script_hash, data_hash, data_size, status, created_at_block, consumed_at_block)
           VALUES ($1, 0, 100, '', 0, '', $2, '', 0, 1, 50, 100)"#,
    )
    .bind(&tx_hash_consumed)
    .bind(vec![0x33u8; 32])
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"INSERT INTO cells (tx_hash, output_index, capacity, lock_code_hash, lock_hash_type, 
           lock_args, lock_script_hash, data_hash, data_size, status, created_at_block)
           VALUES ($1, 0, 200, '', 0, '', $2, '', 0, 0, 100)"#,
    )
    .bind(&tx_hash_created)
    .bind(vec![0x44u8; 32])
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO live_cells (tx_hash, output_index, created_at_block, capacity, lock_script_hash, lock_code_hash, lock_args, data_size) VALUES ($1, 0, 100, 200, $2, '', '', 0)",
    )
    .bind(&tx_hash_created)
    .bind(vec![0x44u8; 32])
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(get_live_cells_count(&pool).await, 1);

    let new_tip_hash = vec![0xCCu8; 32];
    writer
        .execute_reorg(99, &block_hash_99, 101, &block_hash_101, 101, &new_tip_hash)
        .await
        .unwrap();

    let live_count = get_live_cells_count(&pool).await;
    assert_eq!(live_count, 1);

    let restored = get_live_cell(&pool, &tx_hash_consumed, 0).await;
    assert!(restored.is_some());

    let removed = get_live_cell(&pool, &tx_hash_created, 0).await;
    assert!(removed.is_none());
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
        .insert_cells_batch(&[
            (&tx_p0, 0, &cell, 1_000_000),
            (&tx_p1, 0, &cell, 6_000_000),
            (&tx_p2, 0, &cell, 11_000_000),
        ])
        .await
        .unwrap();

    assert_eq!(get_live_cells_count(&pool).await, 3);

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

    assert_eq!(get_live_cells_count(&pool).await, 0);

    let cells_status: Vec<(i16,)> = sqlx::query_as(
        "SELECT status FROM cells WHERE tx_hash IN ($1, $2, $3) ORDER BY created_at_block",
    )
    .bind(&tx_p0)
    .bind(&tx_p1)
    .bind(&tx_p2)
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(cells_status.len(), 3);
    assert!(cells_status.iter().all(|(s,)| *s == 1));
}
