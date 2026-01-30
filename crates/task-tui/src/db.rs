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
