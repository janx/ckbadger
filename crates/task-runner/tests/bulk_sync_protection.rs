use ckbadger_task_runner::db::TaskDb;
use ckbadger_task_runner::MIGRATOR;
use sqlx::PgPool;

async fn setup_sync_status(pool: &PgPool, tip_block_number: i64) {
    sqlx::query(
        r#"
        INSERT INTO sync_status (id, tip_block_number)
        VALUES (1, $1)
        ON CONFLICT (id) DO UPDATE SET tip_block_number = $1
        "#,
    )
    .bind(tip_block_number)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_blocks_up_to(pool: &PgPool, max_block: i64) {
    for number in 0..=max_block {
        let hash: Vec<u8> = vec![number as u8; 32];
        let dao_bytes: Vec<u8> = vec![0u8; 32];

        sqlx::query(
            r#"
            INSERT INTO blocks (number, hash, parent_hash, timestamp, version, compact_target,
                transactions_count, epoch_number, epoch_index, epoch_length, dao, nonce,
                extra_hash, proposals_hash, transactions_root, uncles_hash)
            VALUES ($1, $2, $2, NOW(), 0, 0, 1, 0, 0, 1, $3, $2, $2, $2, $2, $2)
            "#,
        )
        .bind(number)
        .bind(&hash)
        .bind(&dao_bytes)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_when_far_behind(pool: PgPool) {
    setup_sync_status(&pool, 10000).await;
    insert_blocks_up_to(&pool, 100).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(is_bulk, "Should be in bulk sync when 9900 blocks behind");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_when_caught_up(pool: PgPool) {
    setup_sync_status(&pool, 10000).await;
    insert_blocks_up_to(&pool, 9500).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        !is_bulk,
        "Should not be in bulk sync when only 500 blocks behind"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_at_threshold(pool: PgPool) {
    setup_sync_status(&pool, 10000).await;
    insert_blocks_up_to(&pool, 9000).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        !is_bulk,
        "Should not be in bulk sync when exactly 1000 blocks behind (threshold)"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn test_is_bulk_sync_active_just_over_threshold(pool: PgPool) {
    setup_sync_status(&pool, 10000).await;
    insert_blocks_up_to(&pool, 8999).await;

    let db = TaskDb::new(pool);
    let is_bulk = db.is_bulk_sync_active().await.unwrap();

    assert!(
        is_bulk,
        "Should be in bulk sync when 1001 blocks behind (over threshold)"
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
