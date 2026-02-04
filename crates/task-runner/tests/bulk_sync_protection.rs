use ckbadger_task_runner::db::TaskDb;
use ckbadger_task_runner::MIGRATOR;
use sqlx::PgPool;

async fn setup_sync_status(
    pool: &PgPool,
    indexes_deferred: bool,
    address_balances_deferred: bool,
    token_deferred: bool,
) {
    sqlx::query(
        r#"
        UPDATE sync_status 
        SET indexes_deferred = $1,
            address_balances_deferred = $2,
            token_deferred = $3
        WHERE id = 1
        "#,
    )
    .bind(indexes_deferred)
    .bind(address_balances_deferred)
    .bind(token_deferred)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_when_indexes_deferred(pool: PgPool) {
    setup_sync_status(&pool, true, false, false).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        is_bulk,
        "Should be in bulk sync when indexes_deferred is true"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_when_all_flags_false(pool: PgPool) {
    setup_sync_status(&pool, false, false, false).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        !is_bulk,
        "Should not be in bulk sync when all deferred flags are false"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_when_address_balances_deferred(pool: PgPool) {
    setup_sync_status(&pool, false, true, false).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        is_bulk,
        "Should be in bulk sync when address_balances_deferred is true"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_when_token_deferred(pool: PgPool) {
    setup_sync_status(&pool, false, false, true).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        is_bulk,
        "Should be in bulk sync when token_deferred is true"
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
async fn test_is_bulk_sync_active_with_empty_database(pool: PgPool) {
    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        !is_bulk,
        "Empty database should not be considered bulk sync"
    );
}
