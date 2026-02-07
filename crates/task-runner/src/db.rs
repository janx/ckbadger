use anyhow::Result;
use chrono::{DateTime, Utc};
use ckbadger_common::{ClickHouseClient, Task};
use tracing::{debug, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct DbPool {
    client: ClickHouseClient,
}

impl DbPool {
    pub fn new(client: ClickHouseClient) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &ClickHouseClient {
        &self.client
    }
}

impl Default for DbPool {
    fn default() -> Self {
        Self {
            client: ClickHouseClient::from_env().expect("Failed to create ClickHouse client"),
        }
    }
}

#[derive(Clone)]
pub struct TaskDb {
    pool: DbPool,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct TaskRow {
    id: String,
    task_type: String,
    status: String,
    priority: i32,
    config: String,
    progress_total: u64,
    progress_current: u64,
    progress_message: String,
    result: String,
    error_message: String,
    created_at: i64,
    started_at: i64,
    completed_at: i64,
    heartbeat_at: i64,
    runner_id: String,
    retry_count: u32,
    max_retries: u32,
    rate_samples: String,
    rate_ema: f64,
    log_tail: String,
}

impl TaskRow {
    fn into_task(self) -> Result<Task> {
        let id = Uuid::parse_str(&self.id)?;
        let config: serde_json::Value =
            serde_json::from_str(&self.config).unwrap_or(serde_json::Value::Null);
        let result: Option<serde_json::Value> = if self.result.is_empty() {
            None
        } else {
            serde_json::from_str(&self.result).ok()
        };
        let rate_samples: Option<serde_json::Value> = if self.rate_samples.is_empty() {
            None
        } else {
            serde_json::from_str(&self.rate_samples).ok()
        };

        fn millis_to_datetime(ms: i64) -> Option<DateTime<Utc>> {
            if ms == 0 {
                None
            } else {
                DateTime::from_timestamp_millis(ms)
            }
        }

        Ok(Task {
            id,
            task_type: self.task_type,
            status: self.status,
            priority: self.priority,
            config,
            progress_total: if self.progress_total > 0 {
                Some(self.progress_total as i64)
            } else {
                None
            },
            progress_current: if self.progress_current > 0 {
                Some(self.progress_current as i64)
            } else {
                None
            },
            progress_message: if self.progress_message.is_empty() {
                None
            } else {
                Some(self.progress_message)
            },
            result,
            error_message: if self.error_message.is_empty() {
                None
            } else {
                Some(self.error_message)
            },
            created_at: DateTime::from_timestamp_millis(self.created_at).unwrap_or_else(Utc::now),
            started_at: millis_to_datetime(self.started_at),
            completed_at: millis_to_datetime(self.completed_at),
            heartbeat_at: millis_to_datetime(self.heartbeat_at),
            runner_id: if self.runner_id.is_empty() {
                None
            } else {
                Some(self.runner_id)
            },
            retry_count: self.retry_count as i32,
            max_retries: self.max_retries as i32,
            rate_samples,
            rate_ema: if self.rate_ema > 0.0 {
                Some(self.rate_ema)
            } else {
                None
            },
            log_tail: if self.log_tail.is_empty() {
                None
            } else {
                Some(self.log_tail)
            },
        })
    }
}

impl TaskDb {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn claim_next_task(&self, runner_id: &str) -> Result<Option<Task>> {
        let query = r#"
            SELECT 
                toString(id) as id,
                task_type,
                status,
                priority,
                config,
                progress_total,
                progress_current,
                progress_message,
                result,
                error_message,
                toInt64(toUnixTimestamp64Milli(created_at)) as created_at,
                toInt64(toUnixTimestamp64Milli(started_at)) as started_at,
                toInt64(toUnixTimestamp64Milli(completed_at)) as completed_at,
                toInt64(toUnixTimestamp64Milli(heartbeat_at)) as heartbeat_at,
                runner_id,
                retry_count,
                max_retries,
                rate_samples,
                rate_ema,
                log_tail
            FROM tasks
            WHERE status = 'pending'
            ORDER BY priority DESC, created_at ASC
            LIMIT 1
        "#;

        let rows: Vec<TaskRow> = self.pool.client.query_all(query).await?;

        if let Some(row) = rows.into_iter().next() {
            let task = row.into_task()?;
            let now_ms = Utc::now().timestamp_millis();

            let update_query = format!(
                "ALTER TABLE tasks UPDATE status = 'running', runner_id = '{}', started_at = fromUnixTimestamp64Milli({}), heartbeat_at = fromUnixTimestamp64Milli({}) WHERE id = '{}'",
                runner_id, now_ms, now_ms, task.id
            );

            if let Err(e) = self.pool.client.execute(&update_query).await {
                warn!("Failed to claim task {}: {}", task.id, e);
                return Ok(None);
            }

            debug!("Claimed task {} ({})", task.id, task.task_type);
            Ok(Some(task))
        } else {
            Ok(None)
        }
    }

    pub async fn update_task_progress(
        &self,
        task_id: Uuid,
        current: i64,
        total: i64,
    ) -> Result<()> {
        let now_ms = Utc::now().timestamp_millis();
        let query = format!(
            "ALTER TABLE tasks UPDATE progress_current = {}, progress_total = {}, heartbeat_at = fromUnixTimestamp64Milli({}) WHERE id = '{}'",
            current, total, now_ms, task_id
        );
        self.pool.client.execute(&query).await?;
        Ok(())
    }

    pub async fn update_task_progress_message(&self, task_id: Uuid, message: &str) -> Result<()> {
        let now_ms = Utc::now().timestamp_millis();
        let escaped_message = message.replace('\'', "''");
        let query = format!(
            "ALTER TABLE tasks UPDATE progress_message = '{}', heartbeat_at = fromUnixTimestamp64Milli({}) WHERE id = '{}'",
            escaped_message, now_ms, task_id
        );
        self.pool.client.execute(&query).await?;
        Ok(())
    }

    pub async fn complete_task(
        &self,
        task_id: Uuid,
        result: Option<serde_json::Value>,
    ) -> Result<()> {
        let now_ms = Utc::now().timestamp_millis();
        let result_json = result
            .map(|r| serde_json::to_string(&r).unwrap_or_default())
            .unwrap_or_default()
            .replace('\'', "''");

        let query = format!(
            "ALTER TABLE tasks UPDATE status = 'completed', completed_at = fromUnixTimestamp64Milli({}), result = '{}' WHERE id = '{}'",
            now_ms, result_json, task_id
        );
        self.pool.client.execute(&query).await?;
        Ok(())
    }

    pub async fn fail_task(&self, task_id: Uuid, error: &str) -> Result<()> {
        let now_ms = Utc::now().timestamp_millis();
        let escaped_error = error.replace('\'', "''");

        let query = format!(
            "ALTER TABLE tasks UPDATE status = 'failed', completed_at = fromUnixTimestamp64Milli({}), error_message = '{}' WHERE id = '{}'",
            now_ms, escaped_error, task_id
        );
        self.pool.client.execute(&query).await?;
        Ok(())
    }

    pub async fn heartbeat(&self, task_id: Uuid) -> Result<()> {
        let now_ms = Utc::now().timestamp_millis();
        let query = format!(
            "ALTER TABLE tasks UPDATE heartbeat_at = fromUnixTimestamp64Milli({}) WHERE id = '{}'",
            now_ms, task_id
        );
        self.pool.client.execute(&query).await?;
        Ok(())
    }

    pub async fn is_bulk_sync_active(&self) -> Result<bool> {
        Ok(false)
    }

    pub async fn mark_task_pending(&self, task_id: Uuid, reason: &str) -> Result<()> {
        let escaped_reason = reason.replace('\'', "''");
        let query = format!(
            "ALTER TABLE tasks UPDATE status = 'pending', error_message = '{}' WHERE id = '{}'",
            escaped_reason, task_id
        );
        self.pool.client.execute(&query).await?;
        Ok(())
    }

    pub async fn get_task(&self, task_id: Uuid) -> Result<Option<Task>> {
        let query = format!(
            r#"
            SELECT 
                toString(id) as id,
                task_type,
                status,
                priority,
                config,
                progress_total,
                progress_current,
                progress_message,
                result,
                error_message,
                toInt64(toUnixTimestamp64Milli(created_at)) as created_at,
                toInt64(toUnixTimestamp64Milli(started_at)) as started_at,
                toInt64(toUnixTimestamp64Milli(completed_at)) as completed_at,
                toInt64(toUnixTimestamp64Milli(heartbeat_at)) as heartbeat_at,
                runner_id,
                retry_count,
                max_retries,
                rate_samples,
                rate_ema,
                log_tail
            FROM tasks
            WHERE id = '{}'
            LIMIT 1
            "#,
            task_id
        );

        let rows: Vec<TaskRow> = self.pool.client.query_all(&query).await?;
        if let Some(row) = rows.into_iter().next() {
            Ok(Some(row.into_task()?))
        } else {
            Ok(None)
        }
    }

    pub async fn update_task_status(
        &self,
        task_id: Uuid,
        status: &str,
        message: Option<&str>,
    ) -> Result<()> {
        let now_ms = Utc::now().timestamp_millis();
        let message_part = message
            .map(|m| format!(", error_message = '{}'", m.replace('\'', "''")))
            .unwrap_or_default();

        let query = format!(
            "ALTER TABLE tasks UPDATE status = '{}', heartbeat_at = fromUnixTimestamp64Milli({}) {} WHERE id = '{}'",
            status, now_ms, message_part, task_id
        );
        self.pool.client.execute(&query).await?;
        Ok(())
    }
}
