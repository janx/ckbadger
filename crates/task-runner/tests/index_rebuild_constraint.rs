use ckbadger_task_runner::executor::index::{rebuild_partitioned_constraint, DeferrableConstraint};
use ckbadger_task_runner::MIGRATOR;
use sqlx::PgPool;

const CELLS_CONSTRAINT: DeferrableConstraint = DeferrableConstraint {
    name: "created_at_block_tx_hash_output_index_key",
    table: "cells",
    columns: "created_at_block, tx_hash, output_index",
};

const RANGE_PARTITION_SUFFIXES: [&str; 10] = [
    "_p00", "_p01", "_p02", "_p03", "_p04", "_p05", "_p06", "_p07", "_p08", "_p09",
];

async fn drop_cells_constraint(pool: &PgPool) {
    sqlx::query(
        "ALTER TABLE cells DROP CONSTRAINT IF EXISTS cells_created_at_block_tx_hash_output_index_key CASCADE",
    )
    .execute(pool)
    .await
    .unwrap();

    for suffix in RANGE_PARTITION_SUFFIXES {
        let table = format!("cells{}", suffix);
        let constraint = format!("{}_created_at_block_tx_hash_output_index_key", table);
        let sql = format!(
            "ALTER TABLE {} DROP CONSTRAINT IF EXISTS {}",
            table, constraint
        );
        sqlx::query(&sql).execute(pool).await.unwrap();
    }
}

async fn constraint_exists(pool: &PgPool, table: &str, constraint: &str) -> bool {
    let sql = format!(
        "SELECT 1 FROM pg_constraint WHERE conname = '{}' AND conrelid = '{}'::regclass",
        constraint, table
    );
    let exists: Option<(i32,)> = sqlx::query_as(&sql).fetch_optional(pool).await.unwrap();
    exists.is_some()
}

async fn insert_cell(pool: &PgPool, created_at_block: i64, tx_hash: &[u8], output_index: i16) {
    let lock_hash: Vec<u8> = vec![2u8; 32];
    let data_hash: Vec<u8> = vec![3u8; 32];

    sqlx::query(
        r#"
        INSERT INTO cells (
            tx_hash,
            output_index,
            capacity,
            lock_code_hash,
            lock_hash_type,
            lock_args,
            lock_script_hash,
            data_hash,
            data_size,
            created_at_block
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(tx_hash)
    .bind(output_index)
    .bind(1000_i64)
    .bind(&lock_hash)
    .bind(0_i16)
    .bind(&lock_hash)
    .bind(&lock_hash)
    .bind(&data_hash)
    .bind(0_i32)
    .bind(created_at_block)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_rebuild_constraint_creates_parent_constraint(pool: PgPool) {
    drop_cells_constraint(&pool).await;

    assert!(
        !constraint_exists(
            &pool,
            "cells",
            "cells_created_at_block_tx_hash_output_index_key"
        )
        .await
    );
    assert!(
        !constraint_exists(
            &pool,
            "cells_p00",
            "cells_p00_created_at_block_tx_hash_output_index_key"
        )
        .await
    );

    rebuild_partitioned_constraint(&pool, &CELLS_CONSTRAINT, 4)
        .await
        .unwrap();

    assert!(
        constraint_exists(
            &pool,
            "cells",
            "cells_created_at_block_tx_hash_output_index_key"
        )
        .await
    );
    assert!(
        constraint_exists(
            &pool,
            "cells_p00",
            "cells_p00_created_at_block_tx_hash_output_index_key"
        )
        .await
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_rebuild_constraint_enables_on_conflict(pool: PgPool) {
    drop_cells_constraint(&pool).await;
    rebuild_partitioned_constraint(&pool, &CELLS_CONSTRAINT, 4)
        .await
        .unwrap();

    let created_at_block = 1_i64;
    let tx_hash: Vec<u8> = vec![9u8; 32];
    let output_index = 0_i16;
    let lock_hash: Vec<u8> = vec![2u8; 32];
    let data_hash: Vec<u8> = vec![3u8; 32];

    insert_cell(&pool, created_at_block, &tx_hash, output_index).await;

    sqlx::query(
        r#"
        INSERT INTO cells (
            tx_hash,
            output_index,
            capacity,
            lock_code_hash,
            lock_hash_type,
            lock_args,
            lock_script_hash,
            data_hash,
            data_size,
            created_at_block
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (created_at_block, tx_hash, output_index) DO NOTHING
        "#,
    )
    .bind(&tx_hash)
    .bind(output_index)
    .bind(1000_i64)
    .bind(&lock_hash)
    .bind(0_i16)
    .bind(&lock_hash)
    .bind(&lock_hash)
    .bind(&data_hash)
    .bind(0_i32)
    .bind(created_at_block)
    .execute(&pool)
    .await
    .unwrap();

    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM cells WHERE created_at_block = $1 AND tx_hash = $2 AND output_index = $3",
    )
    .bind(created_at_block)
    .bind(&tx_hash)
    .bind(output_index)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(count, 1);
}
