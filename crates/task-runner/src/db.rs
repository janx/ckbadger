use anyhow::Result;
use ckbadger_common::{Task, TaskBuilder};
use sqlx::PgPool;
use uuid::Uuid;

pub struct TaskDb {
    pool: PgPool,
}

#[allow(dead_code)]
impl TaskDb {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Check if bulk sync is still in progress by looking at actual block data.
    ///
    /// Uses the latest block timestamp to determine if the indexer has caught up
    /// to the chain tip. If the latest block is within 1 hour of current time,
    /// bulk sync is considered complete.
    ///
    /// This avoids a circular dependency: deferred flags (indexes_deferred, etc.)
    /// are cleared BY rebuild tasks, so checking them here would prevent those
    /// same tasks from ever running.
    pub async fn is_bulk_sync_active(&self) -> Result<bool> {
        let row: Option<(bool,)> = sqlx::query_as(
            r#"
            SELECT CASE
                WHEN MAX(timestamp) IS NULL THEN TRUE
                WHEN MAX(timestamp) < NOW() - INTERVAL '1 hour' THEN TRUE
                ELSE FALSE
            END
            FROM blocks_index
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((is_bulk,)) => Ok(is_bulk),
            None => Ok(true),
        }
    }

    /// Check if a specific task type is currently running.
    /// Used to prevent I/O-heavy tasks from running concurrently.
    pub async fn is_task_type_running(&self, task_type: &str) -> Result<bool> {
        let row: Option<(i64,)> = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM tasks
            WHERE task_type = $1 AND status = 'running'
            "#,
        )
        .bind(task_type)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.0).unwrap_or(0) > 0)
    }

    pub async fn defer_task(&self, task_id: Uuid, reason: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE tasks
            SET status = 'pending',
                runner_id = NULL,
                error_message = $2,
                heartbeat_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(task_id)
        .bind(reason)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Reset orphaned tasks that were left in 'running' state by a previous runner instance.
    /// A task is considered orphaned if its heartbeat is older than the given timeout.
    /// Returns the number of tasks recovered.
    pub async fn recover_orphaned_tasks(&self, timeout_secs: i64) -> Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE tasks
            SET status = 'pending',
                runner_id = NULL,
                error_message = COALESCE(error_message || E'\n', '') || 'Recovered: runner died (stale heartbeat)'
            WHERE status = 'running'
              AND heartbeat_at < NOW() - make_interval(secs => $1)
            "#,
        )
        .bind(timeout_secs as f64)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    pub async fn claim_next_task(&self, runner_id: &str) -> Result<Option<Task>> {
        let task: Option<Task> = sqlx::query_as(
            r#"
            UPDATE tasks
            SET status = 'running',
                runner_id = $1,
                started_at = COALESCE(started_at, NOW()),
                heartbeat_at = NOW()
            WHERE id = (
                SELECT id FROM tasks
                WHERE status = 'pending'
                ORDER BY priority DESC, created_at ASC
                LIMIT 1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING *
            "#,
        )
        .bind(runner_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(task)
    }

    /// Claim all pending tasks at the highest available priority level.
    /// This enables parallel execution of independent tasks at the same priority.
    pub async fn claim_tasks_at_same_priority(&self, runner_id: &str) -> Result<Vec<Task>> {
        let tasks: Vec<Task> = sqlx::query_as(
            r#"
            UPDATE tasks
            SET status = 'running',
                runner_id = $1,
                started_at = COALESCE(started_at, NOW()),
                heartbeat_at = NOW()
            WHERE id = ANY(
                SELECT id FROM tasks
                WHERE status = 'pending'
                AND priority = (
                    SELECT MAX(priority) FROM tasks WHERE status = 'pending'
                )
                FOR UPDATE SKIP LOCKED
            )
            RETURNING *
            "#,
        )
        .bind(runner_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(tasks)
    }

    pub async fn update_progress(
        &self,
        task_id: Uuid,
        current: i64,
        total: i64,
        message: Option<&str>,
        rate_ema: Option<f64>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE tasks
            SET progress_current = $2,
                progress_total = $3,
                progress_message = $4,
                rate_ema = $5,
                heartbeat_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(task_id)
        .bind(current)
        .bind(total)
        .bind(message)
        .bind(rate_ema)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_result(&self, task_id: Uuid, result: &serde_json::Value) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE tasks
            SET result = $2,
                heartbeat_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(task_id)
        .bind(result)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn append_log(&self, task_id: Uuid, line: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE tasks
            SET log_tail = CASE 
                WHEN log_tail IS NULL THEN $2
                ELSE (
                    SELECT string_agg(line, E'\n')
                    FROM (
                        SELECT unnest(string_to_array(log_tail || E'\n' || $2, E'\n')) AS line
                        ORDER BY ctid DESC
                        LIMIT 100
                    ) sub
                )
            END,
            heartbeat_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(task_id)
        .bind(line)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn complete_task(
        &self,
        task_id: Uuid,
        result: Option<serde_json::Value>,
    ) -> Result<()> {
        let completion_message = result.as_ref().and_then(|r| r.as_object()).and_then(|obj| {
            let mut parts = Vec::new();
            for (key, label) in [
                ("udt_labels_imported", "UDT labels"),
                ("script_labels_imported", "scripts"),
                ("cells_populated", "cells"),
                ("blocks_processed", "blocks"),
                ("transactions_updated", "txs"),
            ] {
                if let Some(count) = obj.get(key).and_then(|v| v.as_i64()) {
                    if count > 0 {
                        parts.push(format!("{} {}", count, label));
                    }
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(format!("Completed: {}", parts.join(", ")))
            }
        });

        sqlx::query(
            r#"
            UPDATE tasks
            SET status = 'completed',
                completed_at = NOW(),
                progress_current = progress_total,
                progress_message = COALESCE($3, progress_message),
                result = COALESCE($2, result),
                heartbeat_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(task_id)
        .bind(result)
        .bind(completion_message)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn fail_task(&self, task_id: Uuid, error: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE tasks
            SET status = CASE 
                WHEN retry_count < max_retries THEN 'pending'
                ELSE 'failed'
            END,
            error_message = $2,
            retry_count = retry_count + 1,
            runner_id = NULL,
            heartbeat_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(task_id)
        .bind(error)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn heartbeat(&self, task_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE tasks SET heartbeat_at = NOW() WHERE id = $1")
            .bind(task_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn check_cancelled(&self, task_id: Uuid) -> Result<bool> {
        let row: (String,) = sqlx::query_as("SELECT status FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&self.pool)
            .await?;

        Ok(row.0 == "cancelled")
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
