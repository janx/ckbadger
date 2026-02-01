use anyhow::Result;
use ckbadger_common::{Task, TaskBuilder};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SyncStatusRow {
    pub tip_block: i64,
    pub chain_tip: i64,
    pub is_syncing: bool,
    pub is_bulk_sync: bool,
    pub progress: f64,
    pub indexes_deferred: bool,
}

pub struct TaskDb {
    pool: PgPool,
}

#[allow(dead_code)]
impl TaskDb {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_sync_status(&self) -> Result<SyncStatusRow> {
        let tip: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
            .fetch_one(&self.pool)
            .await?;

        let chain_tip: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(number), 0) FROM blocks WHERE timestamp > NOW() - INTERVAL '1 hour'",
        )
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(tip);

        let chain_tip = chain_tip.max(tip);

        let indexes_deferred: bool = sqlx::query_scalar(
            "SELECT COALESCE(indexes_deferred, false) FROM sync_status WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(false);

        let blocks_behind = chain_tip - tip;
        let is_syncing = blocks_behind > 100;
        let is_bulk_sync = blocks_behind > 1000;

        let progress = if chain_tip > 0 {
            (tip as f64 / chain_tip as f64 * 100.0).min(100.0)
        } else {
            100.0
        };

        Ok(SyncStatusRow {
            tip_block: tip,
            chain_tip,
            is_syncing,
            is_bulk_sync,
            progress,
            indexes_deferred,
        })
    }

    pub async fn list_tasks(&self, limit: i64) -> Result<Vec<Task>> {
        let tasks: Vec<Task> = sqlx::query_as(
            r#"
            SELECT * FROM tasks
            ORDER BY 
                CASE status 
                    WHEN 'running' THEN 1 
                    WHEN 'pending' THEN 2
                    WHEN 'paused' THEN 3
                    WHEN 'failed' THEN 4
                    WHEN 'completed' THEN 5
                    WHEN 'cancelled' THEN 6
                END,
                created_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(tasks)
    }

    pub async fn get_task(&self, task_id: Uuid) -> Result<Option<Task>> {
        let task: Option<Task> = sqlx::query_as("SELECT * FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(task)
    }

    pub async fn create_task(&self, builder: &TaskBuilder) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO tasks (task_type, config, priority, max_retries)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(builder.task_type().to_string())
        .bind(builder.config())
        .bind(builder.get_priority())
        .bind(builder.get_max_retries())
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0)
    }

    pub async fn cancel_task(&self, task_id: Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE tasks
            SET status = 'cancelled'
            WHERE id = $1 AND status IN ('pending', 'running', 'paused')
            "#,
        )
        .bind(task_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn pause_task(&self, task_id: Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE tasks
            SET status = 'paused'
            WHERE id = $1 AND status = 'running'
            "#,
        )
        .bind(task_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn resume_task(&self, task_id: Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE tasks
            SET status = 'pending', runner_id = NULL
            WHERE id = $1 AND status = 'paused'
            "#,
        )
        .bind(task_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn retry_task(&self, task_id: Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE tasks
            SET status = 'pending', 
                runner_id = NULL,
                error_message = NULL,
                retry_count = 0
            WHERE id = $1 AND status = 'failed'
            "#,
        )
        .bind(task_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_task(&self, task_id: Uuid) -> Result<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM tasks
            WHERE id = $1 AND status IN ('completed', 'failed', 'cancelled')
            "#,
        )
        .bind(task_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}
