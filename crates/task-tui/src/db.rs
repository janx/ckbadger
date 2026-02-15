use anyhow::Result;
use ckbadger_common::{
    MemoryStatsData, SyncProgressData, SyncStatusData, Task, TaskBuilder, MEMORY_STATS_REDIS_KEY,
    SYNC_PROGRESS_REDIS_KEY, SYNC_STATUS_REDIS_KEY,
};
use ckbadger_store::CkbadgerStore;
use redis::AsyncCommands;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

/// API response for /statistics/network (subset of fields we need).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiNetworkStats {
    pub latest_block: i64,
    pub avg_block_time: String,
    pub hash_rate: String,
    pub difficulty: String,
    pub epoch: String,
    pub tps: String,
    pub transactions_per_day: String,
}

/// Parse epoch string like "100(800/1800)" into (epoch_number, epoch_index, epoch_length).
fn parse_epoch_string(epoch: &str) -> (i64, i32, i32) {
    let parts: Vec<&str> = epoch.splitn(2, '(').collect();
    let epoch_number = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    if let Some(inner) = parts.get(1).and_then(|s| s.strip_suffix(')')) {
        let idx_parts: Vec<&str> = inner.splitn(2, '/').collect();
        let epoch_index = idx_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let epoch_length = idx_parts
            .get(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1800);
        (epoch_number, epoch_index, epoch_length)
    } else {
        (epoch_number, 0, 1800)
    }
}

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
    /// DB write time in ms for the last batch
    pub db_write_ms: Option<f64>,
    /// RPC fetch time in ms for the last batch
    pub rpc_fetch_ms: Option<f64>,
    /// Detailed deferred flags
    pub address_balances_deferred: bool,
    pub activities_deferred: bool,
    pub token_deferred: bool,
    pub spore_deferred: bool,
    pub tx_block_map_deferred: bool,
}

struct DeferredFlags {
    address_balances: bool,
    activities: bool,
    token: bool,
    spore: bool,
    tx_block_map: bool,
}

impl DeferredFlags {
    fn any(&self) -> bool {
        self.address_balances || self.activities || self.token || self.spore || self.tx_block_map
    }
}

#[derive(Debug, Clone)]
pub struct ChainInfoData {
    pub latest_block: i64,
    pub epoch_number: i64,
    pub epoch_index: i32,
    pub epoch_length: i32,
    pub difficulty: String,
    pub hash_rate: String,
    pub avg_block_time: String,
    pub tps: String,
    pub tx_24h: i64,
}

pub struct TaskDb {
    store: Arc<CkbadgerStore>,
    redis: Option<redis::aio::MultiplexedConnection>,
    api_url: String,
    http: reqwest::Client,
}

#[allow(dead_code)]
impl TaskDb {
    pub async fn new(store: Arc<CkbadgerStore>, redis_url: Option<&str>, api_url: &str) -> Self {
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

        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap_or_default();

        Self {
            store,
            redis,
            api_url: api_url.to_string(),
            http,
        }
    }

    pub async fn get_sync_status(&self) -> Result<SyncStatusRow> {
        let progress_data: Option<SyncProgressData> =
            self.get_redis_key(SYNC_PROGRESS_REDIS_KEY).await;
        let status_data: Option<SyncStatusData> = self.get_redis_key(SYNC_STATUS_REDIS_KEY).await;

        let store_status = self.store.get_sync_status()?;
        let deferred = DeferredFlags {
            address_balances: store_status.address_balances_deferred,
            activities: store_status.activities_deferred,
            // token/spore/tx_block_map are only in SyncStatusData (Redis), not in store SyncStatus
            token: status_data.as_ref().is_some_and(|s| s.token_deferred),
            spore: status_data.as_ref().is_some_and(|s| s.spore_deferred),
            tx_block_map: status_data
                .as_ref()
                .is_some_and(|s| s.tx_block_map_deferred),
        };
        let _indexes_deferred = deferred.any();

        if let Some(ref progress) = progress_data {
            return Ok(self.build_from_progress(progress, &status_data, &deferred));
        }

        if let Some(ref status) = status_data {
            return self.build_from_status(status, &deferred).await;
        }

        self.build_fallback(&deferred).await
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
        deferred: &DeferredFlags,
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
            indexes_deferred: deferred.any(),
            elapsed_time,
            eta: Some(progress.eta_formatted.clone()),
            rate_realtime: Some(progress.blocks_per_second),
            rate_ema: Some(progress.ema_blocks_per_second),

            db_write_ms: progress.db_write_ms,
            rpc_fetch_ms: progress.rpc_fetch_ms,
            address_balances_deferred: deferred.address_balances,
            activities_deferred: deferred.activities,
            token_deferred: deferred.token,
            spore_deferred: deferred.spore,
            tx_block_map_deferred: deferred.tx_block_map,
        }
    }

    async fn build_from_status(
        &self,
        status: &SyncStatusData,
        deferred: &DeferredFlags,
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
            indexes_deferred: deferred.any(),
            elapsed_time,
            eta,
            rate_realtime: None,
            rate_ema: status.sync_ema_rate,

            db_write_ms: None,
            rpc_fetch_ms: None,
            address_balances_deferred: deferred.address_balances,
            activities_deferred: deferred.activities,
            token_deferred: deferred.token,
            spore_deferred: deferred.spore,
            tx_block_map_deferred: deferred.tx_block_map,
        })
    }

    async fn build_fallback(&self, deferred: &DeferredFlags) -> Result<SyncStatusRow> {
        let (tip, _) = self.store.get_sync_tip()?;

        Ok(SyncStatusRow {
            tip_block: tip,
            chain_tip: tip,
            is_syncing: false,
            is_bulk_sync: false,
            progress: 100.0,
            indexes_deferred: deferred.any(),
            elapsed_time: None,
            eta: None,
            rate_realtime: None,
            rate_ema: None,

            db_write_ms: None,
            rpc_fetch_ms: None,
            address_balances_deferred: deferred.address_balances,
            activities_deferred: deferred.activities,
            token_deferred: deferred.token,
            spore_deferred: deferred.spore,
            tx_block_map_deferred: deferred.tx_block_map,
        })
    }

    pub async fn get_memory_stats(&self) -> Option<MemoryStatsData> {
        let mut mem: MemoryStatsData = self.get_redis_key(MEMORY_STATS_REDIS_KEY).await?;

        // The memory:stats publisher may not populate chain-level stats.
        // Backfill from sync:status (which always has accurate counters).
        if mem.total_transactions == 0 || mem.total_cells == 0 {
            if let Some(sync) = self
                .get_redis_key::<SyncStatusData>(SYNC_STATUS_REDIS_KEY)
                .await
            {
                mem.total_transactions = sync.total_transactions;
                mem.total_cells = sync.total_cells;
                mem.total_live_cells = sync.total_live_cells;
                mem.total_addresses = sync.total_addresses;
            }
        }

        Some(mem)
    }

    pub async fn get_chain_info(&self) -> Option<ChainInfoData> {
        let url = format!("{}/statistics/network", self.api_url);
        let resp = self.http.get(&url).send().await.ok()?;
        let stats: ApiNetworkStats = resp.json().await.ok()?;

        let tx_24h = stats.transactions_per_day.parse::<i64>().unwrap_or(0);
        let (epoch_number, epoch_index, epoch_length) = parse_epoch_string(&stats.epoch);

        Some(ChainInfoData {
            latest_block: stats.latest_block,
            epoch_number,
            epoch_index,
            epoch_length,
            difficulty: stats.difficulty,
            hash_rate: stats.hash_rate,
            avg_block_time: stats.avg_block_time,
            tps: stats.tps,
            tx_24h,
        })
    }

    // ─── Task operations (via API) ────────────────────────────────────────

    pub async fn list_tasks(&self, limit: i64) -> Result<Vec<Task>> {
        let url = format!("{}/tasks", self.api_url);
        let resp = self.http.get(&url).send().await?;
        let mut tasks: Vec<ApiTaskJson> = resp.json().await?;

        // Sort: running first, then pending, paused, failed, completed, cancelled
        tasks.sort_by(|a, b| {
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
            b.created_at.cmp(&a.created_at)
        });

        tasks.truncate(limit as usize);
        Ok(tasks.into_iter().map(api_task_to_task).collect())
    }

    pub async fn create_task(&self, builder: &TaskBuilder) -> Result<Uuid> {
        let url = format!("{}/tasks", self.api_url);
        let body = serde_json::json!({
            "taskType": builder.task_type().to_string(),
            "config": builder.config(),
        });
        let resp = self.http.post(&url).json(&body).send().await?;
        let result: ApiCreateTaskResponse = resp.json().await?;
        Ok(result.id.parse()?)
    }

    pub async fn cancel_task(&self, task_id: Uuid) -> Result<bool> {
        let url = format!("{}/tasks/{}/cancel", self.api_url, task_id);
        let resp = self.http.post(&url).send().await?;
        Ok(resp.status().is_success())
    }

    pub async fn pause_task(&self, task_id: Uuid) -> Result<bool> {
        let url = format!("{}/tasks/{}/pause", self.api_url, task_id);
        let resp = self.http.post(&url).send().await?;
        Ok(resp.status().is_success())
    }

    pub async fn resume_task(&self, task_id: Uuid) -> Result<bool> {
        let url = format!("{}/tasks/{}/resume", self.api_url, task_id);
        let resp = self.http.post(&url).send().await?;
        Ok(resp.status().is_success())
    }

    pub async fn retry_task(&self, task_id: Uuid) -> Result<bool> {
        let url = format!("{}/tasks/{}/retry", self.api_url, task_id);
        let resp = self.http.post(&url).send().await?;
        Ok(resp.status().is_success())
    }

    pub async fn delete_task(&self, task_id: Uuid) -> Result<bool> {
        let url = format!("{}/tasks/{}", self.api_url, task_id);
        let resp = self.http.delete(&url).send().await?;
        Ok(resp.status().is_success())
    }
}

/// API task JSON (matches the API's TaskJson response).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiTaskJson {
    pub id: String,
    pub task_type: String,
    pub status: String,
    pub priority: i32,
    #[serde(default)]
    pub config: serde_json::Value,
    pub progress_total: Option<i64>,
    pub progress_current: Option<i64>,
    pub progress_message: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error_message: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub heartbeat_at: Option<chrono::DateTime<chrono::Utc>>,
    pub runner_id: Option<String>,
    #[serde(default)]
    pub retry_count: i32,
    #[serde(default)]
    pub max_retries: i32,
    pub rate_ema: Option<f64>,
    pub log_tail: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiCreateTaskResponse {
    pub id: String,
}

fn api_task_to_task(t: ApiTaskJson) -> Task {
    Task {
        id: t.id.parse().unwrap_or_default(),
        task_type: t.task_type,
        status: t.status,
        priority: t.priority,
        config: t.config,
        progress_total: t.progress_total,
        progress_current: t.progress_current,
        progress_message: t.progress_message,
        result: t.result,
        error_message: t.error_message,
        created_at: t.created_at,
        started_at: t.started_at,
        completed_at: t.completed_at,
        heartbeat_at: t.heartbeat_at,
        runner_id: t.runner_id,
        retry_count: t.retry_count,
        max_retries: t.max_retries,
        rate_samples: None,
        rate_ema: t.rate_ema,
        log_tail: t.log_tail,
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
