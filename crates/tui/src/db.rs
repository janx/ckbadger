use anyhow::Result;
use ckbadger_common::{
    format_duration_smart, MemoryStatsData, SyncProgressData, SyncStatusData,
    MEMORY_STATS_REDIS_KEY, SYNC_PROGRESS_REDIS_KEY, SYNC_STATUS_REDIS_KEY,
};
use ckbadger_store::CkbadgerStore;
use redis::AsyncCommands;
use serde::Deserialize;
use std::sync::Arc;

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
    pub rate_realtime: Option<f64>,
    pub rate_ema: Option<f64>,
    pub db_write_ms: Option<f64>,
    pub rpc_fetch_ms: Option<f64>,
    pub is_direct_db_read: bool,
    pub address_balances_deferred: bool,
    pub activities_deferred: bool,
    pub token_deferred: bool,
    pub spore_deferred: bool,
    pub tx_block_map_deferred: bool,
}

#[derive(Debug, Clone)]
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

pub struct TuiDb {
    store: Arc<CkbadgerStore>,
    redis: Option<redis::aio::MultiplexedConnection>,
    api_url: String,
    http: reqwest::Client,
}

impl TuiDb {
    pub async fn new(store: Arc<CkbadgerStore>, redis_url: Option<&str>, api_url: &str) -> Self {
        let redis = if let Some(url) = redis_url {
            match redis::Client::open(url) {
                Ok(client) => match client.get_multiplexed_async_connection().await {
                    Ok(conn) => Some(conn),
                    Err(e) => {
                        eprintln!("Failed to connect to Redis: {e}");
                        None
                    }
                },
                Err(e) => {
                    eprintln!("Failed to create Redis client: {e}");
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
            token: status_data.as_ref().is_some_and(|s| s.token_deferred),
            spore: status_data.as_ref().is_some_and(|s| s.spore_deferred),
            tx_block_map: status_data
                .as_ref()
                .is_some_and(|s| s.tx_block_map_deferred),
        };

        if let Some(ref progress) = progress_data {
            return Ok(self.build_from_progress(progress, &status_data, &deferred));
        }

        if let Some(ref status) = status_data {
            return self.build_from_status(status, &deferred);
        }

        self.build_fallback(&deferred)
    }

    async fn get_redis_key<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let conn = self.redis.as_ref()?;
        let mut conn = conn.clone();
        let result: std::result::Result<Option<String>, _> = conn.get(key).await;
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
            is_direct_db_read: progress.is_direct_db_read,
            address_balances_deferred: deferred.address_balances,
            activities_deferred: deferred.activities,
            token_deferred: deferred.token,
            spore_deferred: deferred.spore,
            tx_block_map_deferred: deferred.tx_block_map,
        }
    }

    fn build_from_status(
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
            is_direct_db_read: false,
            address_balances_deferred: deferred.address_balances,
            activities_deferred: deferred.activities,
            token_deferred: deferred.token,
            spore_deferred: deferred.spore,
            tx_block_map_deferred: deferred.tx_block_map,
        })
    }

    fn build_fallback(&self, deferred: &DeferredFlags) -> Result<SyncStatusRow> {
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
            is_direct_db_read: false,
            address_balances_deferred: deferred.address_balances,
            activities_deferred: deferred.activities,
            token_deferred: deferred.token,
            spore_deferred: deferred.spore,
            tx_block_map_deferred: deferred.tx_block_map,
        })
    }

    pub async fn get_memory_stats(&self) -> Option<MemoryStatsData> {
        let mut mem: MemoryStatsData = self.get_redis_key(MEMORY_STATS_REDIS_KEY).await?;

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
}

#[cfg(test)]
mod tests {
    use super::parse_epoch_string;

    #[test]
    fn parse_epoch_full() {
        assert_eq!(parse_epoch_string("100(800/1800)"), (100, 800, 1800));
    }

    #[test]
    fn parse_epoch_without_details() {
        assert_eq!(parse_epoch_string("101"), (101, 0, 1800));
    }

    #[test]
    fn parse_epoch_invalid() {
        assert_eq!(parse_epoch_string("bad"), (0, 0, 1800));
    }
}
