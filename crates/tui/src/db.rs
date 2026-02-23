use anyhow::Result;
use ckbadger_common::{
    format_duration_smart, MemoryStatsData, PipelineProgressData, SyncProgressData, SyncStatusData,
    MEMORY_STATS_REDIS_KEY, SYNC_PROGRESS_REDIS_KEY, SYNC_STATUS_REDIS_KEY,
};
use ckbadger_store::CkbadgerStore;
use redis::AsyncCommands;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

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
    pub pipeline: Option<PipelineProgressData>,
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

#[derive(Debug, Clone, Default)]
pub struct RedisServiceInfo {
    pub enabled: bool,
    pub reachable: bool,
    pub latency_ms: Option<f64>,
    pub db_keys_total: Option<u64>,
    pub db_keys_expiring: Option<u64>,
    pub db_keys_persistent: Option<u64>,
    pub used_memory_bytes: Option<u64>,
    pub used_memory_peak_bytes: Option<u64>,
    pub used_memory_rss_bytes: Option<u64>,
    pub mem_fragmentation_ratio: Option<f64>,
    pub keyspace_hits: Option<u64>,
    pub keyspace_misses: Option<u64>,
    pub evicted_keys: Option<u64>,
    pub sync_status_age_secs: Option<i64>,
    pub sync_progress_age_secs: Option<i64>,
    pub memory_stats_age_secs: Option<i64>,
    pub sync_status_type: Option<String>,
    pub sync_status_ttl_secs: Option<i64>,
    pub sync_status_value_bytes: Option<u64>,
    pub sync_progress_type: Option<String>,
    pub sync_progress_ttl_secs: Option<i64>,
    pub sync_progress_value_bytes: Option<u64>,
    pub memory_stats_type: Option<String>,
    pub memory_stats_ttl_secs: Option<i64>,
    pub memory_stats_value_bytes: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ApiServiceInfo {
    pub reachable: bool,
    pub latency_ms: Option<f64>,
    pub status_code: Option<u16>,
    pub latest_block: Option<i64>,
    pub tps: Option<String>,
    pub avg_block_time: Option<String>,
    pub error: Option<String>,
}

fn age_since(unix_ts: i64, now: i64) -> Option<i64> {
    if unix_ts <= 0 {
        None
    } else {
        Some((now - unix_ts).max(0))
    }
}

fn chain_info_from_api_stats(stats: &ApiNetworkStats) -> ChainInfoData {
    let tx_24h = stats.transactions_per_day.parse::<i64>().unwrap_or(0);
    let (epoch_number, epoch_index, epoch_length) = parse_epoch_string(&stats.epoch);

    ChainInfoData {
        latest_block: stats.latest_block,
        epoch_number,
        epoch_index,
        epoch_length,
        difficulty: stats.difficulty.clone(),
        hash_rate: stats.hash_rate.clone(),
        avg_block_time: stats.avg_block_time.clone(),
        tps: stats.tps.clone(),
        tx_24h,
    }
}

fn parse_info_map(info_text: &str) -> HashMap<String, String> {
    info_text
        .lines()
        .filter_map(|line| {
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (k, v) = line.split_once(':')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

fn parse_keyspace_db0(keyspace_info: &str) -> (Option<u64>, Option<u64>) {
    for line in keyspace_info.lines() {
        if let Some((db, payload)) = line.split_once(':') {
            if db.trim() != "db0" {
                continue;
            }
            let mut keys = None;
            let mut expires = None;
            for part in payload.split(',') {
                if let Some((name, value)) = part.split_once('=') {
                    match name.trim() {
                        "keys" => keys = value.trim().parse::<u64>().ok(),
                        "expires" => expires = value.trim().parse::<u64>().ok(),
                        _ => {}
                    }
                }
            }
            return (keys, expires);
        }
    }
    (None, None)
}

fn ttl_or_none(ttl_secs: i64) -> Option<i64> {
    if ttl_secs == -2 {
        None
    } else {
        Some(ttl_secs)
    }
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
            pipeline: progress.pipeline.clone(),
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
            pipeline: None,
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
            pipeline: None,
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

    pub async fn get_chain_info_and_api_service_info(
        &self,
    ) -> (Option<ChainInfoData>, ApiServiceInfo) {
        let mut api_info = ApiServiceInfo::default();
        let url = format!("{}/statistics/network", self.api_url);
        let started = Instant::now();

        let response = self.http.get(&url).send().await;
        api_info.latency_ms = Some(started.elapsed().as_secs_f64() * 1000.0);

        let response = match response {
            Ok(resp) => resp,
            Err(e) => {
                api_info.error = Some(format!("request failed: {e}"));
                return (None, api_info);
            }
        };

        api_info.status_code = Some(response.status().as_u16());
        if !response.status().is_success() {
            api_info.error = Some(format!("http {}", response.status()));
            return (None, api_info);
        }

        match response.json::<ApiNetworkStats>().await {
            Ok(stats) => {
                api_info.reachable = true;
                api_info.latest_block = Some(stats.latest_block);
                api_info.tps = Some(stats.tps.clone());
                api_info.avg_block_time = Some(stats.avg_block_time.clone());
                (Some(chain_info_from_api_stats(&stats)), api_info)
            }
            Err(e) => {
                api_info.error = Some(format!("decode failed: {e}"));
                (None, api_info)
            }
        }
    }

    pub async fn get_redis_service_info(&self) -> RedisServiceInfo {
        let now = chrono::Utc::now().timestamp();
        let mut info = RedisServiceInfo {
            enabled: self.redis.is_some(),
            ..Default::default()
        };

        let Some(conn) = self.redis.as_ref() else {
            info.error = Some("redis not configured".to_string());
            return info;
        };

        let mut conn = conn.clone();
        let started = Instant::now();
        let ping_result: std::result::Result<String, _> =
            redis::cmd("PING").query_async(&mut conn).await;
        info.latency_ms = Some(started.elapsed().as_secs_f64() * 1000.0);

        if let Err(e) = ping_result {
            info.error = Some(format!("ping failed: {e}"));
            return info;
        }

        info.reachable = true;

        let dbsize_result: std::result::Result<u64, _> =
            redis::cmd("DBSIZE").query_async(&mut conn).await;
        info.db_keys_total = dbsize_result.ok();

        let info_memory: std::result::Result<String, _> = redis::cmd("INFO")
            .arg("memory")
            .query_async(&mut conn)
            .await;
        if let Ok(text) = info_memory {
            let map = parse_info_map(&text);
            info.used_memory_bytes = map.get("used_memory").and_then(|v| v.parse::<u64>().ok());
            info.used_memory_peak_bytes = map
                .get("used_memory_peak")
                .and_then(|v| v.parse::<u64>().ok());
            info.used_memory_rss_bytes = map
                .get("used_memory_rss")
                .and_then(|v| v.parse::<u64>().ok());
            info.mem_fragmentation_ratio = map
                .get("mem_fragmentation_ratio")
                .and_then(|v| v.parse::<f64>().ok());
        }

        let info_stats: std::result::Result<String, _> =
            redis::cmd("INFO").arg("stats").query_async(&mut conn).await;
        if let Ok(text) = info_stats {
            let map = parse_info_map(&text);
            info.keyspace_hits = map.get("keyspace_hits").and_then(|v| v.parse::<u64>().ok());
            info.keyspace_misses = map
                .get("keyspace_misses")
                .and_then(|v| v.parse::<u64>().ok());
            info.evicted_keys = map.get("evicted_keys").and_then(|v| v.parse::<u64>().ok());
        }

        let info_keyspace: std::result::Result<String, _> = redis::cmd("INFO")
            .arg("keyspace")
            .query_async(&mut conn)
            .await;
        if let Ok(text) = info_keyspace {
            let (keys, expires) = parse_keyspace_db0(&text);
            if keys.is_some() {
                info.db_keys_total = keys;
            }
            info.db_keys_expiring = expires;
            info.db_keys_persistent = match (info.db_keys_total, info.db_keys_expiring) {
                (Some(total), Some(exp)) => Some(total.saturating_sub(exp)),
                _ => None,
            };
        }

        let sync_status_type: std::result::Result<String, _> = redis::cmd("TYPE")
            .arg(SYNC_STATUS_REDIS_KEY)
            .query_async(&mut conn)
            .await;
        info.sync_status_type = sync_status_type.ok();
        let sync_status_ttl: std::result::Result<i64, _> = redis::cmd("TTL")
            .arg(SYNC_STATUS_REDIS_KEY)
            .query_async(&mut conn)
            .await;
        info.sync_status_ttl_secs = sync_status_ttl.ok().and_then(ttl_or_none);
        let sync_status_strlen: std::result::Result<u64, _> = redis::cmd("STRLEN")
            .arg(SYNC_STATUS_REDIS_KEY)
            .query_async(&mut conn)
            .await;
        info.sync_status_value_bytes = sync_status_strlen.ok();

        let sync_progress_type: std::result::Result<String, _> = redis::cmd("TYPE")
            .arg(SYNC_PROGRESS_REDIS_KEY)
            .query_async(&mut conn)
            .await;
        info.sync_progress_type = sync_progress_type.ok();
        let sync_progress_ttl: std::result::Result<i64, _> = redis::cmd("TTL")
            .arg(SYNC_PROGRESS_REDIS_KEY)
            .query_async(&mut conn)
            .await;
        info.sync_progress_ttl_secs = sync_progress_ttl.ok().and_then(ttl_or_none);
        let sync_progress_strlen: std::result::Result<u64, _> = redis::cmd("STRLEN")
            .arg(SYNC_PROGRESS_REDIS_KEY)
            .query_async(&mut conn)
            .await;
        info.sync_progress_value_bytes = sync_progress_strlen.ok();

        let memory_stats_type: std::result::Result<String, _> = redis::cmd("TYPE")
            .arg(MEMORY_STATS_REDIS_KEY)
            .query_async(&mut conn)
            .await;
        info.memory_stats_type = memory_stats_type.ok();
        let memory_stats_ttl: std::result::Result<i64, _> = redis::cmd("TTL")
            .arg(MEMORY_STATS_REDIS_KEY)
            .query_async(&mut conn)
            .await;
        info.memory_stats_ttl_secs = memory_stats_ttl.ok().and_then(ttl_or_none);
        let memory_stats_strlen: std::result::Result<u64, _> = redis::cmd("STRLEN")
            .arg(MEMORY_STATS_REDIS_KEY)
            .query_async(&mut conn)
            .await;
        info.memory_stats_value_bytes = memory_stats_strlen.ok();

        let status: Option<SyncStatusData> = self.get_redis_key(SYNC_STATUS_REDIS_KEY).await;
        let progress: Option<SyncProgressData> = self.get_redis_key(SYNC_PROGRESS_REDIS_KEY).await;
        let memory: Option<MemoryStatsData> = self.get_redis_key(MEMORY_STATS_REDIS_KEY).await;

        info.sync_status_age_secs = status.and_then(|s| age_since(s.last_synced_at, now));
        info.sync_progress_age_secs = progress.and_then(|p| age_since(p.updated_at, now));
        info.memory_stats_age_secs = memory.and_then(|m| age_since(m.updated_at, now));

        info
    }
}

#[cfg(test)]
mod tests {
    use super::{age_since, parse_epoch_string, parse_info_map, parse_keyspace_db0, ttl_or_none};

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

    #[test]
    fn age_since_handles_zero_and_future() {
        assert_eq!(age_since(0, 100), None);
        assert_eq!(age_since(120, 100), Some(0));
        assert_eq!(age_since(80, 100), Some(20));
    }

    #[test]
    fn parse_info_map_works() {
        let text = "# Memory\r\nused_memory:123\r\nmem_fragmentation_ratio:1.23\r\n";
        let map = parse_info_map(text);
        assert_eq!(map.get("used_memory"), Some(&"123".to_string()));
        assert_eq!(
            map.get("mem_fragmentation_ratio"),
            Some(&"1.23".to_string())
        );
    }

    #[test]
    fn parse_keyspace_db0_works() {
        let text = "# Keyspace\r\ndb0:keys=10,expires=7,avg_ttl=123\r\n";
        let (keys, expires) = parse_keyspace_db0(text);
        assert_eq!(keys, Some(10));
        assert_eq!(expires, Some(7));
    }

    #[test]
    fn ttl_or_none_works() {
        assert_eq!(ttl_or_none(-2), None);
        assert_eq!(ttl_or_none(-1), Some(-1));
        assert_eq!(ttl_or_none(0), Some(0));
        assert_eq!(ttl_or_none(15), Some(15));
    }
}
