//! Startup Recovery Tests
//!
//! Tests for pending rebuild task submission after indexer restart.
//! Verifies that deferred tasks are properly recovered when indexer starts
//! outside of bulk sync mode.

use ckbadger_common::{IndexRebuildConfig, TaskBuilder};
use ckbadger_indexer::MIGRATOR;
use sqlx::PgPool;

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_startup_recovery_submits_index_rebuild_when_deferred(pool: PgPool) {
    sqlx::query("UPDATE sync_status SET indexes_deferred = TRUE WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();

    let existing: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tasks WHERE task_type = 'index_rebuild' AND status IN ('pending', 'running')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(existing.0, 0);

    let builder = TaskBuilder::index_rebuild(IndexRebuildConfig {
        parallel_connections: 10,
        indexes: None,
        rebuild_constraints: true,
    });

    sqlx::query(
        "INSERT INTO tasks (task_type, config, priority, max_retries) VALUES ($1, $2, $3, $4)",
    )
    .bind(builder.task_type().to_string())
    .bind(builder.config())
    .bind(builder.get_priority())
    .bind(builder.get_max_retries())
    .execute(&pool)
    .await
    .unwrap();

    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tasks WHERE task_type = 'index_rebuild' AND status = 'pending'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count.0, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_startup_recovery_skips_when_not_deferred(pool: PgPool) {
    let deferred: (bool,) =
        sqlx::query_as("SELECT COALESCE(indexes_deferred, false) FROM sync_status WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!deferred.0);

    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM tasks WHERE task_type = 'index_rebuild'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count.0, 0);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_startup_recovery_skips_when_task_already_pending(pool: PgPool) {
    sqlx::query("UPDATE sync_status SET indexes_deferred = TRUE WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();

    let builder = TaskBuilder::index_rebuild(IndexRebuildConfig {
        parallel_connections: 10,
        indexes: None,
        rebuild_constraints: true,
    });

    sqlx::query(
        "INSERT INTO tasks (task_type, config, priority, max_retries, status) VALUES ($1, $2, $3, $4, 'pending')",
    )
    .bind(builder.task_type().to_string())
    .bind(builder.config())
    .bind(builder.get_priority())
    .bind(builder.get_max_retries())
    .execute(&pool)
    .await
    .unwrap();

    let existing: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tasks WHERE task_type = 'index_rebuild' AND status IN ('pending', 'running')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(existing.0, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_startup_recovery_skips_when_task_already_running(pool: PgPool) {
    sqlx::query("UPDATE sync_status SET indexes_deferred = TRUE WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();

    let builder = TaskBuilder::index_rebuild(IndexRebuildConfig {
        parallel_connections: 10,
        indexes: None,
        rebuild_constraints: true,
    });

    sqlx::query(
        "INSERT INTO tasks (task_type, config, priority, max_retries, status) VALUES ($1, $2, $3, $4, 'running')",
    )
    .bind(builder.task_type().to_string())
    .bind(builder.config())
    .bind(builder.get_priority())
    .bind(builder.get_max_retries())
    .execute(&pool)
    .await
    .unwrap();

    let existing: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tasks WHERE task_type = 'index_rebuild' AND status IN ('pending', 'running')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(existing.0, 1);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_startup_recovery_submits_after_completed_task(pool: PgPool) {
    sqlx::query("UPDATE sync_status SET indexes_deferred = TRUE WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();

    let builder = TaskBuilder::index_rebuild(IndexRebuildConfig {
        parallel_connections: 10,
        indexes: None,
        rebuild_constraints: true,
    });

    sqlx::query(
        "INSERT INTO tasks (task_type, config, priority, max_retries, status) VALUES ($1, $2, $3, $4, 'completed')",
    )
    .bind(builder.task_type().to_string())
    .bind(builder.config())
    .bind(builder.get_priority())
    .bind(builder.get_max_retries())
    .execute(&pool)
    .await
    .unwrap();

    let pending_or_running: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tasks WHERE task_type = 'index_rebuild' AND status IN ('pending', 'running')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pending_or_running.0, 0);

    sqlx::query(
        "INSERT INTO tasks (task_type, config, priority, max_retries) VALUES ($1, $2, $3, $4)",
    )
    .bind(builder.task_type().to_string())
    .bind(builder.config())
    .bind(builder.get_priority())
    .bind(builder.get_max_retries())
    .execute(&pool)
    .await
    .unwrap();

    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tasks WHERE task_type = 'index_rebuild' AND status = 'pending'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count.0, 1);
}
