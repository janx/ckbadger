#![allow(clippy::type_complexity)]

use ckbadger_indexer::db::BatchWriter;
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;
use std::collections::HashMap;

async fn get_script_usage(
    pool: &PgPool,
    code_hash: &[u8],
    script_kind: &str,
) -> Option<(i64, i64, String, String)> {
    sqlx::query_as::<_, (i64, i64, String, String)>(
        r#"
        SELECT cells_count, live_cells_count, capacity_sum::TEXT, live_capacity_sum::TEXT
        FROM script_usage_stats
        WHERE code_hash = $1 AND script_kind = $2
        "#,
    )
    .bind(code_hash)
    .bind(script_kind)
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_single_script_single_update(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let code_hash = vec![0x01u8; 32];

    let mut changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    changes.insert(
        (code_hash.clone(), false),
        (10, 10, 1000_00000000, 1000_00000000),
    );

    writer.update_script_usage_batch(&changes).await.unwrap();

    let (cells, live, cap, live_cap) = get_script_usage(&pool, &code_hash, "lock").await.unwrap();
    assert_eq!(cells, 10);
    assert_eq!(live, 10);
    assert_eq!(cap, "100000000000");
    assert_eq!(live_cap, "100000000000");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_incremental_updates_accumulate(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let code_hash = vec![0x02u8; 32];

    let mut changes1: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    changes1.insert(
        (code_hash.clone(), false),
        (100, 100, 5000_00000000, 5000_00000000),
    );
    writer.update_script_usage_batch(&changes1).await.unwrap();

    let mut changes2: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    changes2.insert(
        (code_hash.clone(), false),
        (50, 30, 2500_00000000, 1500_00000000),
    );
    writer.update_script_usage_batch(&changes2).await.unwrap();

    let (cells, live, cap, live_cap) = get_script_usage(&pool, &code_hash, "lock").await.unwrap();
    assert_eq!(cells, 150);
    assert_eq!(live, 130);
    assert_eq!(cap, "750000000000");
    assert_eq!(live_cap, "650000000000");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_negative_deltas_for_consumed_cells(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let code_hash = vec![0x03u8; 32];

    let mut initial: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    initial.insert(
        (code_hash.clone(), true),
        (100, 100, 10000_00000000, 10000_00000000),
    );
    writer.update_script_usage_batch(&initial).await.unwrap();

    let mut consume: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    consume.insert((code_hash.clone(), true), (0, -30, 0, -3000_00000000));
    writer.update_script_usage_batch(&consume).await.unwrap();

    let (cells, live, cap, live_cap) = get_script_usage(&pool, &code_hash, "type").await.unwrap();
    assert_eq!(cells, 100);
    assert_eq!(live, 70);
    assert_eq!(cap, "1000000000000");
    assert_eq!(live_cap, "700000000000");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_lock_and_type_tracked_separately(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let code_hash = vec![0x04u8; 32];

    let mut changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    changes.insert(
        (code_hash.clone(), false),
        (50, 50, 5000_00000000, 5000_00000000),
    );
    changes.insert(
        (code_hash.clone(), true),
        (10, 10, 1000_00000000, 1000_00000000),
    );
    writer.update_script_usage_batch(&changes).await.unwrap();

    let (lock_cells, lock_live, lock_cap, lock_live_cap) =
        get_script_usage(&pool, &code_hash, "lock").await.unwrap();
    assert_eq!(lock_cells, 50);
    assert_eq!(lock_live, 50);
    assert_eq!(lock_cap, "500000000000");
    assert_eq!(lock_live_cap, "500000000000");

    let (type_cells, type_live, type_cap, type_live_cap) =
        get_script_usage(&pool, &code_hash, "type").await.unwrap();
    assert_eq!(type_cells, 10);
    assert_eq!(type_live, 10);
    assert_eq!(type_cap, "100000000000");
    assert_eq!(type_live_cap, "100000000000");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_multiple_scripts_in_single_batch(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let code_hash_a = vec![0x05u8; 32];
    let code_hash_b = vec![0x06u8; 32];

    let mut changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    changes.insert(
        (code_hash_a.clone(), false),
        (100, 80, 10000_00000000, 8000_00000000),
    );
    changes.insert(
        (code_hash_b.clone(), false),
        (50, 50, 5000_00000000, 5000_00000000),
    );
    writer.update_script_usage_batch(&changes).await.unwrap();

    let (a_cells, a_live, a_cap, a_live_cap) =
        get_script_usage(&pool, &code_hash_a, "lock").await.unwrap();
    assert_eq!(a_cells, 100);
    assert_eq!(a_live, 80);
    assert_eq!(a_cap, "1000000000000");
    assert_eq!(a_live_cap, "800000000000");

    let (b_cells, b_live, b_cap, b_live_cap) =
        get_script_usage(&pool, &code_hash_b, "lock").await.unwrap();
    assert_eq!(b_cells, 50);
    assert_eq!(b_live, 50);
    assert_eq!(b_cap, "500000000000");
    assert_eq!(b_live_cap, "500000000000");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_empty_batch_is_noop(pool: PgPool) {
    let writer = BatchWriter::new(pool.clone());
    let changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64)> = HashMap::new();
    writer.update_script_usage_batch(&changes).await.unwrap();
}
