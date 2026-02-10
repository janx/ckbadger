use anyhow::Result;
use ckbadger_common::{
    MemoryStatsData, SyncProgressData, SyncStatusData, Task, TaskBuilder, MEMORY_STATS_REDIS_KEY,
    SYNC_PROGRESS_REDIS_KEY, SYNC_STATUS_REDIS_KEY,
};
use ckbadger_store::CkbadgerStore;
use redis::AsyncCommands;
use std::sync::Arc;
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
    /// True when indexer reads blocks directly from CKB's RocksDB.
    pub is_direct_db_read: bool,
}

pub struct TaskDb {
    store: Arc<CkbadgerStore>,
    redis: Option<redis::aio::MultiplexedConnection>,
}

#[allow(dead_code)]
impl TaskDb {
    pub async fn new(store: Arc<CkbadgerStore>, redis_url: Option<&str>) -> Self {
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

        Self { store, redis }
    }

    pub async fn get_sync_status(&self) -> Result<SyncStatusRow> {
        let progress_data: Option<SyncProgressData> =
            self.get_redis_key(SYNC_PROGRESS_REDIS_KEY).await;
        let status_data: Option<SyncStatusData> = self.get_redis_key(SYNC_STATUS_REDIS_KEY).await;

        let indexes_deferred: bool = self.store.get_sync_status()?.activities_deferred;

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
            is_direct_db_read: progress.is_direct_db_read,
        }
    }

    async fn build_from_status(
        &self,
        status: &SyncStatusData,
        indexes_deferred: bool,
    ) -> Result<SyncStatusRow> {
        let tip_block = status.tip_block_number;
        let (chain_tip, _) = self.store.get_sync_tip()?;

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
            is_direct_db_read: false,
        })
    }

    async fn build_fallback(&self, indexes_deferred: bool) -> Result<SyncStatusRow> {
        let (tip, _) = self.store.get_sync_tip()?;

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
            is_direct_db_read: false,
        })
    }

    pub async fn get_memory_stats(&self) -> Option<MemoryStatsData> {
        self.get_redis_key(MEMORY_STATS_REDIS_KEY).await
    }

    pub async fn list_tasks(&self, limit: i64) -> Result<Vec<Task>> {
        let mut all = self.store.list_all_tasks()?;

        // Sort: running first, then pending, paused, failed, completed, cancelled
        all.sort_by(|a, b| {
            let status_order = |s: &str| -> u8 {
                match s {
                    "running" => 1,
                    "pending" => 2,
                    "paused" => 3,
                    "failed" => 4,
                    "completed" => 5,
                    "cancelled" => 6,
                    _ => 7,
                }
            };
            let ord = status_order(&a.status).cmp(&status_order(&b.status));
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
            // Within same status, newest first
            b.created_at.cmp(&a.created_at)
        });

        all.truncate(limit as usize);
        Ok(all.into_iter().map(task_entry_to_task).collect())
    }

    pub async fn get_task(&self, task_id: Uuid) -> Result<Option<Task>> {
        Ok(self.store.get_task(&task_id)?.map(task_entry_to_task))
    }

    pub async fn create_task(&self, builder: &TaskBuilder) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let entry = ckbadger_store::types::TaskEntry {
            id,
            task_type: builder.task_type().to_string(),
            status: "pending".to_string(),
            priority: builder.get_priority(),
            config: builder.config().clone(),
            progress_total: None,
            progress_current: None,
            progress_message: None,
            result: None,
            error_message: None,
            created_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            heartbeat_at: None,
            runner_id: None,
            retry_count: 0,
            max_retries: builder.get_max_retries(),
            rate_samples: None,
            rate_ema: None,
            log_tail: None,
        };
        self.store.create_task(&entry)?;
        Ok(id)
    }

    pub async fn cancel_task(&self, task_id: Uuid) -> Result<bool> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            if task.status == "pending" || task.status == "running" || task.status == "paused" {
                let old_status = task.status.clone();
                let old_priority = task.priority;
                task.status = "cancelled".to_string();
                self.store.update_task(&task, &old_status, old_priority)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn pause_task(&self, task_id: Uuid) -> Result<bool> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            if task.status == "running" {
                let old_status = task.status.clone();
                let old_priority = task.priority;
                task.status = "paused".to_string();
                self.store.update_task(&task, &old_status, old_priority)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn resume_task(&self, task_id: Uuid) -> Result<bool> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            if task.status == "paused" {
                let old_status = task.status.clone();
                let old_priority = task.priority;
                task.status = "pending".to_string();
                task.runner_id = None;
                self.store.update_task(&task, &old_status, old_priority)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn retry_task(&self, task_id: Uuid) -> Result<bool> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            if task.status == "failed" {
                let old_status = task.status.clone();
                let old_priority = task.priority;
                task.status = "pending".to_string();
                task.runner_id = None;
                task.error_message = None;
                task.retry_count = 0;
                self.store.update_task(&task, &old_status, old_priority)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn delete_task(&self, task_id: Uuid) -> Result<bool> {
        if let Some(task) = self.store.get_task(&task_id)? {
            if task.status == "completed" || task.status == "failed" || task.status == "cancelled" {
                // Delete from both CFs
                self.store
                    .delete_cf(self.store.cf_tasks(), task_id.as_bytes())?;
                let idx_key = ckbadger_store::keys::encode_task_index_key(
                    match task.status.as_str() {
                        "completed" => 0x03,
                        "failed" => 0x04,
                        "cancelled" => 0x05,
                        _ => 0xFF,
                    },
                    task.priority,
                    &task.id,
                );
                self.store
                    .delete_cf(self.store.cf_tasks_index(), &idx_key)?;
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Convert TaskEntry from ckbadger-store to common Task.
fn task_entry_to_task(entry: ckbadger_store::types::TaskEntry) -> Task {
    Task {
        id: entry.id,
        task_type: entry.task_type,
        status: entry.status,
        priority: entry.priority,
        config: entry.config,
        progress_total: entry.progress_total,
        progress_current: entry.progress_current,
        progress_message: entry.progress_message,
        result: entry.result,
        error_message: entry.error_message,
        created_at: entry.created_at,
        started_at: entry.started_at,
        completed_at: entry.completed_at,
        heartbeat_at: entry.heartbeat_at,
        runner_id: entry.runner_id,
        retry_count: entry.retry_count,
        max_retries: entry.max_retries,
        rate_samples: None,
        rate_ema: entry.rate_ema,
        log_tail: entry.log_tail,
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
