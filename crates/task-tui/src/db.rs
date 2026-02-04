use anyhow::Result;
use ckbadger_common::{
    MemoryStatsData, SyncProgressData, SyncStatusData, Task, TaskBuilder, MEMORY_STATS_REDIS_KEY,
    SYNC_PROGRESS_REDIS_KEY, SYNC_STATUS_REDIS_KEY,
};
use redis::AsyncCommands;
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
    pub elapsed_time: Option<String>,
    pub eta: Option<String>,
    /// Real-time rate (10-second sliding window)
    pub rate_realtime: Option<f64>,
    /// EMA rate (smoothed)
    pub rate_ema: Option<f64>,
}

pub struct TaskDb {
    pool: PgPool,
    redis: Option<redis::aio::MultiplexedConnection>,
}

#[allow(dead_code)]
impl TaskDb {
    pub async fn new(pool: PgPool, redis_url: Option<&str>) -> Self {
        let redis = if let Some(url) = redis_url {
            match redis::Client::open(url) {
                Ok(client) => match client.get_multiplexed_async_connection().await {
                    Ok(conn) => Some(conn),
                    Err(e) => {
                        eprintln!("Failed to connect to Redis: {}", e);
                        None
                    }
                },
                Err(e) => {
                    eprintln!("Failed to create Redis client: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Self { pool, redis }
    }

    pub async fn get_sync_status(&self) -> Result<SyncStatusRow> {
        let progress_data: Option<SyncProgressData> =
            self.get_redis_key(SYNC_PROGRESS_REDIS_KEY).await;
        let status_data: Option<SyncStatusData> = self.get_redis_key(SYNC_STATUS_REDIS_KEY).await;

        let indexes_deferred: bool = sqlx::query_scalar(
            "SELECT COALESCE(indexes_deferred, false) FROM sync_status WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(false);

        if let Some(ref progress) = progress_data {
            return Ok(self.build_from_progress(progress, &status_data, indexes_deferred));
        }

        if let Some(ref status) = status_data {
            return self.build_from_status(status, indexes_deferred).await;
        }

        self.build_fallback(indexes_deferred).await
    }

    async fn get_redis_key<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let conn = self.redis.as_ref()?;
        let mut conn = conn.clone();
        let result: Result<Option<String>, _> = conn.get(key).await;
        result
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    fn build_from_progress(
        &self,
        progress: &SyncProgressData,
        status_data: &Option<SyncStatusData>,
        indexes_deferred: bool,
    ) -> SyncStatusRow {
        let tip_block = progress.current_block as i64;
        let chain_tip = progress.target_block as i64;
        let blocks_behind = chain_tip - tip_block;

        let elapsed_time = status_data.as_ref().and_then(|s| {
            s.sync_started_at.map(|started| {
                let end = s
                    .bulk_sync_completed_at
                    .unwrap_or_else(|| chrono::Utc::now().timestamp());
                format_duration_smart((end - started) as f64)
            })
        });

        SyncStatusRow {
            tip_block,
            chain_tip,
            is_syncing: blocks_behind > 100,
            is_bulk_sync: blocks_behind > 1000,
            progress: progress.progress_percentage,
            indexes_deferred,
            elapsed_time,
            eta: Some(progress.eta_formatted.clone()),
            rate_realtime: Some(progress.blocks_per_second),
            rate_ema: Some(progress.ema_blocks_per_second),
        }
    }

    async fn build_from_status(
        &self,
        status: &SyncStatusData,
        indexes_deferred: bool,
    ) -> Result<SyncStatusRow> {
        let tip_block = status.tip_block_number;
        let chain_tip: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
            .fetch_one(&self.pool)
            .await?;

        let blocks_behind = chain_tip - tip_block;
        let is_syncing = blocks_behind > 100;

        let progress = if chain_tip > 0 {
            (tip_block as f64 / chain_tip as f64 * 100.0).min(100.0)
        } else {
            100.0
        };

        let elapsed_time = status.sync_started_at.map(|started| {
            let end = status
                .bulk_sync_completed_at
                .unwrap_or_else(|| chrono::Utc::now().timestamp());
            format_duration_smart((end - started) as f64)
        });

        let eta = if is_syncing {
            status.sync_ema_rate.and_then(|rate| {
                if rate > 0.0 {
                    Some(format_duration_smart(blocks_behind as f64 / rate))
                } else {
                    None
                }
            })
        } else {
            None
        };

        Ok(SyncStatusRow {
            tip_block,
            chain_tip,
            is_syncing,
            is_bulk_sync: blocks_behind > 1000,
            progress,
            indexes_deferred,
            elapsed_time,
            eta,
            rate_realtime: None,
            rate_ema: status.sync_ema_rate,
        })
    }

    async fn build_fallback(&self, indexes_deferred: bool) -> Result<SyncStatusRow> {
        let tip: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
            .fetch_one(&self.pool)
            .await?;

        Ok(SyncStatusRow {
            tip_block: tip,
            chain_tip: tip,
            is_syncing: false,
            is_bulk_sync: false,
            progress: 100.0,
            indexes_deferred,
            elapsed_time: None,
            eta: None,
            rate_realtime: None,
            rate_ema: None,
        })
    }

    pub async fn get_memory_stats(&self) -> Option<MemoryStatsData> {
        self.get_redis_key(MEMORY_STATS_REDIS_KEY).await
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

fn format_duration_smart(total_secs: f64) -> String {
    let total_secs = total_secs.round() as u64;

    if total_secs < 60 {
        return format!("{}s", total_secs);
    }

    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m {}s", minutes, seconds)
    }
}
