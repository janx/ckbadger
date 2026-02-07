use anyhow::Result;
use chrono::{DateTime, Utc};
use ckbadger_common::{
    ClickHouseClient, MemoryStatsData, SyncProgressData, SyncStatusData, Task, TaskBuilder,
};
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
    pub rate_realtime: Option<f64>,
    pub rate_ema: Option<f64>,
}

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

pub struct TaskDb {
    pool: DbPool,
    redis_client: Option<redis::Client>,
}

impl TaskDb {
    pub async fn new(pool: DbPool, redis_url: Option<&str>) -> Self {
        let redis_client = redis_url.and_then(|url| redis::Client::open(url).ok());
        Self { pool, redis_client }
    }

    fn get_redis_key(&self, key: &str) -> Option<String> {
        let client = self.redis_client.as_ref()?;
        let mut conn = client.get_connection().ok()?;
        redis::cmd("GET").arg(key).query(&mut conn).ok()
    }

    pub async fn get_sync_status(&self) -> Result<SyncStatusRow> {
        use ckbadger_common::sync::{SYNC_PROGRESS_REDIS_KEY, SYNC_STATUS_REDIS_KEY};

        let status_data: Option<SyncStatusData> = self
            .get_redis_key(SYNC_STATUS_REDIS_KEY)
            .and_then(|s| serde_json::from_str(&s).ok());

        let progress_data: Option<SyncProgressData> = self
            .get_redis_key(SYNC_PROGRESS_REDIS_KEY)
            .and_then(|s| serde_json::from_str(&s).ok());

        if let (Some(status), Some(progress)) = (status_data, progress_data) {
            let elapsed = status
                .bulk_sync_elapsed_seconds()
                .map(|s| ckbadger_common::sync::format_duration_smart(s as f64));

            Ok(SyncStatusRow {
                tip_block: status.tip_block_number,
                chain_tip: progress.target_block as i64,
                is_syncing: progress.current_block < progress.target_block,
                is_bulk_sync: status.bulk_sync_completed_at.is_none()
                    && progress.current_block + 1000 < progress.target_block,
                progress: progress.progress_percentage,
                indexes_deferred: status.indexes_deferred,
                elapsed_time: elapsed,
                eta: if progress.eta_formatted.is_empty() {
                    None
                } else {
                    Some(progress.eta_formatted)
                },
                rate_realtime: Some(progress.blocks_per_second),
                rate_ema: Some(progress.ema_blocks_per_second),
            })
        } else {
            Ok(SyncStatusRow {
                tip_block: 0,
                chain_tip: 0,
                is_syncing: false,
                is_bulk_sync: false,
                progress: 0.0,
                indexes_deferred: false,
                elapsed_time: None,
                eta: None,
                rate_realtime: None,
                rate_ema: None,
            })
        }
    }

    #[allow(dead_code)]
    pub async fn get_sync_progress(&self) -> Option<SyncProgressData> {
        use ckbadger_common::sync::SYNC_PROGRESS_REDIS_KEY;
        self.get_redis_key(SYNC_PROGRESS_REDIS_KEY)
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    #[allow(dead_code)]
    pub async fn get_sync_status_data(&self) -> Option<SyncStatusData> {
        use ckbadger_common::sync::SYNC_STATUS_REDIS_KEY;
        self.get_redis_key(SYNC_STATUS_REDIS_KEY)
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    pub async fn get_memory_stats(&self) -> Option<MemoryStatsData> {
        use ckbadger_common::sync::MEMORY_STATS_REDIS_KEY;
        self.get_redis_key(MEMORY_STATS_REDIS_KEY)
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    pub async fn list_tasks(&self, limit: i64) -> Result<Vec<Task>> {
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
            ORDER BY 
                CASE status 
                    WHEN 'running' THEN 0 
                    WHEN 'pending' THEN 1 
                    WHEN 'paused' THEN 2 
                    ELSE 3 
                END,
                priority DESC, 
                created_at DESC
            LIMIT {}
            "#,
            limit
        );

        let rows: Vec<TaskRow> = self.pool.client().query_all(&query).await?;
        rows.into_iter().map(|r| r.into_task()).collect()
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

        let rows: Vec<TaskRow> = self.pool.client().query_all(&query).await?;
        if let Some(row) = rows.into_iter().next() {
            Ok(Some(row.into_task()?))
        } else {
            Ok(None)
        }
    }

    pub async fn create_task(&self, builder: &TaskBuilder) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let task_type = builder.task_type().to_string();
        let priority = builder.get_priority();
        let config_json = serde_json::to_string(builder.config())?.replace('\'', "''");
        let max_retries = builder.get_max_retries();
        let now_ms = Utc::now().timestamp_millis();

        let query = format!(
            r#"INSERT INTO tasks (id, task_type, status, priority, config, max_retries, created_at) 
               VALUES ('{}', '{}', 'pending', {}, '{}', {}, fromUnixTimestamp64Milli({}))"#,
            id, task_type, priority, config_json, max_retries, now_ms
        );

        self.pool.client().execute(&query).await?;
        Ok(id)
    }

    pub async fn cancel_task(&self, task_id: Uuid) -> Result<bool> {
        let task = self.get_task(task_id).await?;
        if let Some(t) = task {
            match t.status.as_str() {
                "pending" | "running" | "paused" => {
                    let now_ms = Utc::now().timestamp_millis();
                    let query = format!(
                        "ALTER TABLE tasks UPDATE status = 'cancelled', completed_at = fromUnixTimestamp64Milli({}) WHERE id = '{}'",
                        now_ms, task_id
                    );
                    self.pool.client().execute(&query).await?;
                    Ok(true)
                }
                _ => Ok(false),
            }
        } else {
            Ok(false)
        }
    }

    pub async fn pause_task(&self, task_id: Uuid) -> Result<bool> {
        let task = self.get_task(task_id).await?;
        if let Some(t) = task {
            if t.status == "running" {
                let query = format!(
                    "ALTER TABLE tasks UPDATE status = 'paused' WHERE id = '{}'",
                    task_id
                );
                self.pool.client().execute(&query).await?;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    pub async fn resume_task(&self, task_id: Uuid) -> Result<bool> {
        let task = self.get_task(task_id).await?;
        if let Some(t) = task {
            if t.status == "paused" {
                let query = format!(
                    "ALTER TABLE tasks UPDATE status = 'pending' WHERE id = '{}'",
                    task_id
                );
                self.pool.client().execute(&query).await?;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    pub async fn retry_task(&self, task_id: Uuid) -> Result<bool> {
        let task = self.get_task(task_id).await?;
        if let Some(t) = task {
            if t.status == "failed" {
                let new_retry_count = t.retry_count + 1;
                let query = format!(
                    "ALTER TABLE tasks UPDATE status = 'pending', retry_count = {} WHERE id = '{}'",
                    new_retry_count, task_id
                );
                self.pool.client().execute(&query).await?;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    pub async fn delete_task(&self, task_id: Uuid) -> Result<bool> {
        let query = format!("ALTER TABLE tasks DELETE WHERE id = '{}'", task_id);
        self.pool.client().execute(&query).await?;
        Ok(true)
    }
}
