use ckbadger_task_runner::db::TaskDb;
use ckbadger_task_runner::executor::TaskExecutor;
use ckbadger_task_runner::MIGRATOR;
use sqlx::PgPool;

async fn insert_block_with_timestamp(pool: &PgPool, number: i64, hours_ago: f64) {
    let hash = vec![0u8; 32];
    let nonce = vec![0u8; 16];
    sqlx::query(
        r#"
        INSERT INTO blocks (number, hash, parent_hash, timestamp, version, compact_target,
                           transactions_count, proposals_count, uncles_count, epoch_number,
                           epoch_index, epoch_length, dao, nonce, extra_hash,
                           proposals_hash, transactions_root, uncles_hash,
                           total_difficulty, miner_lock_hash)
        VALUES ($1, $2, $3, NOW() - ($4 || ' hours')::INTERVAL, 0, 0, 1, 0, 0, $1, 0, 1000,
                $5, $6, $7, $8, $9, $10, 0, $11)
        "#,
    )
    .bind(number)
    .bind(&hash)
    .bind(&hash)
    .bind(hours_ago.to_string())
    .bind(&hash) // dao
    .bind(&nonce) // nonce
    .bind(&hash) // extra_hash
    .bind(&hash) // proposals_hash
    .bind(&hash) // transactions_root
    .bind(&hash) // uncles_hash
    .bind(&hash) // miner_lock_hash
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_no_blocks(pool: PgPool) {
    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(is_bulk, "Should be in bulk sync when no blocks exist");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_old_blocks(pool: PgPool) {
    insert_block_with_timestamp(&pool, 100, 2.0).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        is_bulk,
        "Should be in bulk sync when latest block is 2 hours old"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_recent_blocks(pool: PgPool) {
    insert_block_with_timestamp(&pool, 18000000, 0.1).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        !is_bulk,
        "Should not be in bulk sync when latest block is 6 minutes old"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_boundary(pool: PgPool) {
    insert_block_with_timestamp(&pool, 18000000, 0.5).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        !is_bulk,
        "Should not be in bulk sync when latest block is 30 minutes old"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_defer_task_sets_pending_status(pool: PgPool) {
    let task_id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO tasks (id, task_type, status, priority, runner_id, created_at)
        VALUES ($1, 'statistics_rebuild', 'running', 5, 'test-runner', NOW())
        "#,
    )
    .bind(task_id)
    .execute(&pool)
    .await
    .unwrap();

    let db = TaskDb::new(pool.clone());
    db.defer_task(task_id, "Bulk sync in progress")
        .await
        .unwrap();

    let (status, runner_id, error_msg): (String, Option<String>, Option<String>) =
        sqlx::query_as("SELECT status, runner_id, error_message FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(
        status, "pending",
        "Task status should be pending after defer"
    );
    assert!(
        runner_id.is_none(),
        "Runner ID should be cleared after defer"
    );
    assert_eq!(
        error_msg.as_deref(),
        Some("Bulk sync in progress"),
        "Error message should contain defer reason"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_with_empty_blocks_table(pool: PgPool) {
    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        is_bulk,
        "Empty blocks table means no sync has happened - should be bulk sync"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_run_once_returns_false_when_task_deferred(pool: PgPool) {
    let db_url = "unused".to_string();
    let executor = TaskExecutor::new(
        pool.clone(),
        db_url,
        "test-runner".to_string(),
        "http://unused:8114".to_string(),
        "/nonexistent".to_string(),
        4,
        1000,
        8,
    );

    sqlx::query(
        "INSERT INTO tasks (task_type, status, priority) VALUES ('index_rebuild', 'pending', 10)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = executor.run_once().await.unwrap();

    assert!(
        !result,
        "run_once should return false when task is deferred (triggers poll sleep)"
    );

    let (status,): (String,) =
        sqlx::query_as("SELECT status FROM tasks WHERE task_type = 'index_rebuild'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "pending", "Deferred task should be back to pending");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_run_once_returns_true_when_task_executes(pool: PgPool) {
    insert_block_with_timestamp(&pool, 18000000, 0.1).await;

    let db_url = "unused".to_string();
    let executor = TaskExecutor::new(
        pool.clone(),
        db_url,
        "test-runner".to_string(),
        "http://unused:8114".to_string(),
        "/nonexistent".to_string(),
        4,
        1000,
        8,
    );

    sqlx::query(
        "INSERT INTO tasks (task_type, status, priority) VALUES ('label_import', 'pending', 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let result = executor.run_once().await.unwrap();

    assert!(
        result,
        "run_once should return true when task actually executes (no sleep)"
    );
}
