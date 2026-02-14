#![allow(clippy::type_complexity)]

use axum::{extract::State, routing::get, Router};
use chrono::{DateTime, NaiveDate, Utc};
use ckbadger_common::dao::GENESIS_BURNT;
use ckbadger_common::sync::{
    format_duration_smart, SyncProgressData, SyncStatusData, SYNC_PROGRESS_REDIS_KEY,
    SYNC_STATUS_REDIS_KEY,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::cache::{CacheKeys, CacheTtl};
use crate::response::{ok, ApiError, ApiResult};
use crate::utils::{format_duration, shannon_to_ckb, shannon_to_ckb_u128};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/statistics/network", get(get_network_stats))
        .route("/statistics/tx-stats", get(get_tx_stats))
        .route("/statistics/recent-blocks", get(get_recent_blocks))
        .route(
            "/charts/transaction-count",
            get(get_transaction_count_chart),
        )
        .route("/charts/cell-count", get(get_cell_count_chart))
        .route("/charts/knowledge-size", get(get_knowledge_size_chart))
        .route(
            "/charts/block-time-distribution",
            get(get_block_time_distribution_chart),
        )
        .route(
            "/charts/epoch-time-distribution",
            get(get_epoch_time_distribution_chart),
        )
        .route(
            "/charts/epoch-time-length",
            get(get_epoch_time_length_chart),
        )
        .route(
            "/charts/average-block-time",
            get(get_average_block_time_chart),
        )
        .route("/charts/hash-rate", get(get_hash_rate_chart))
        .route("/charts/difficulty", get(get_difficulty_chart))
        .route("/charts/uncle-rate", get(get_uncle_rate_chart))
        .route(
            "/charts/miner-address-distribution",
            get(get_miner_address_distribution_chart),
        )
        // Economics charts
        .route("/charts/total-supply", get(get_total_supply_chart))
        .route("/charts/nominal-apc", get(get_nominal_apc_chart))
        .route(
            "/charts/secondary-issuance",
            get(get_secondary_issuance_chart),
        )
        .route("/charts/inflation-rate", get(get_inflation_rate_chart))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub is_syncing: bool,
    pub synced_block: i64,
    pub tip_block: i64,
    pub progress: f64,
    pub estimated_time: Option<String>,
    pub chart_data_may_be_incomplete: bool,
    pub blocks_per_second: Option<f64>,
    pub ema_blocks_per_second: Option<f64>,
    pub sync_mode: String,
    pub started_at: Option<i64>,
    pub elapsed_time: Option<String>,
    pub total_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepForkStatus {
    pub detected: bool,
    pub detected_at: Option<DateTime<Utc>>,
    pub depth: Option<i32>,
    pub db_tip: Option<i64>,
    pub chain_tip: Option<i64>,
    pub fork_point: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStats {
    pub latest_block: i64,
    pub avg_block_time: String,
    pub hash_rate: String,
    pub difficulty: String,
    pub epoch: String,
    pub tps: String,
    pub estimated_epoch_time: String,
    pub transactions_per_minute: String,
    pub transactions_per_day: String,
    pub sync_status: SyncStatus,
    pub deep_fork_status: DeepForkStatus,
}

async fn get_network_stats(State(state): State<Arc<AppState>>) -> ApiResult<NetworkStats> {
    if let Some(cached) = state
        .cache
        .get::<NetworkStats>(CacheKeys::NETWORK_STATS)
        .await
    {
        return ok(cached);
    }

    let stats = fetch_network_stats_from_db(&state).await?;

    state
        .cache
        .set(CacheKeys::NETWORK_STATS, &stats, CacheTtl::NETWORK_STATS)
        .await;

    ok(stats)
}

async fn get_tx_stats(State(state): State<Arc<AppState>>) -> ApiResult<TxStatsResponse> {
    let cache_key = "statistics:tx-stats";
    if let Some(cached) = state.cache.get::<TxStatsResponse>(cache_key).await {
        return ok(cached);
    }

    // Get the latest block header to determine reference time
    let store = state.store.clone();
    let latest_header = store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_time = latest_header
        .as_ref()
        .and_then(|(_, h)| DateTime::from_timestamp_millis(h.timestamp))
        .unwrap_or_else(Utc::now);
    let reference_ts = reference_time.timestamp() * 1000; // ms

    // Get hourly stats (last 24 hours)
    let hourly_stats = store
        .list_hourly_stats_with_keys()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let cutoff_24h = reference_ts - 24 * 3600 * 1000;

    // Filter hourly stats to last 24 hours
    let mut recent_hourly: Vec<(String, ckbadger_store::HourlyStats)> = hourly_stats
        .into_iter()
        .filter(|(_, h)| h.hour * 1000 > cutoff_24h && h.hour * 1000 <= reference_ts)
        .collect();
    recent_hourly.sort_by(|a, b| b.1.hour.cmp(&a.1.hour)); // desc
    recent_hourly.truncate(24);

    // Get daily stats (last 14 days)
    let daily_stats = store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_date = reference_time.date_naive();
    let cutoff_date = reference_date - chrono::Duration::days(14);

    let mut recent_daily: Vec<(String, ckbadger_store::DailyStats)> = daily_stats
        .into_iter()
        .filter(|(date_str, _)| {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y%m%d") {
                date > cutoff_date && date <= reference_date
            } else {
                false
            }
        })
        .collect();
    recent_daily.sort_by(|a, b| b.0.cmp(&a.0)); // desc
    recent_daily.truncate(14);

    let txs_this_hour: i64 = recent_hourly
        .first()
        .map(|(_, h)| h.transactions_count as i64)
        .unwrap_or(0);
    let txs_in_24_hours: i64 = recent_hourly
        .iter()
        .map(|(_, h)| h.transactions_count as i64)
        .sum();

    let hourly_data: Vec<TxStatsDataPoint> = recent_hourly
        .into_iter()
        .rev()
        .map(|(_, h)| {
            let dt = DateTime::from_timestamp(h.hour, 0).unwrap_or_default();
            TxStatsDataPoint {
                label: dt.format("%H:00").to_string(),
                value: h.transactions_count as i64,
            }
        })
        .collect();

    let daily_data: Vec<TxStatsDataPoint> = recent_daily
        .into_iter()
        .rev()
        .map(|(date_str, stats)| {
            let label = if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y%m%d") {
                date.format("%m/%d").to_string()
            } else {
                date_str
            };
            TxStatsDataPoint {
                label,
                value: stats.transactions_count as i64,
            }
        })
        .collect();

    let response = TxStatsResponse {
        current_hour: txs_this_hour,
        current_day: txs_in_24_hours,
        hourly_data,
        daily_data,
    };

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(10))
        .await;

    ok(response)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentBlockItem {
    pub timestamp: i64,
    pub transactions_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentBlocksResponse {
    pub blocks: Vec<RecentBlockItem>,
}

async fn get_recent_blocks(State(state): State<Arc<AppState>>) -> ApiResult<RecentBlocksResponse> {
    let cache_key = "statistics:recent-blocks";
    if let Some(cached) = state.cache.get::<RecentBlocksResponse>(cache_key).await {
        return ok(cached);
    }

    let store = state.store.clone();

    // Get the latest block to find the reference time
    let latest = store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_ts = latest
        .as_ref()
        .map(|(_, h)| h.timestamp)
        .unwrap_or_else(|| Utc::now().timestamp_millis());

    let cutoff_ts = reference_ts - 24 * 3600 * 1000; // 24 hours ago in ms

    // Get blocks for last 24 hours
    // Estimate: ~8640 blocks in 24h at ~10s/block
    let blocks_desc = store
        .list_blocks_desc(None, 10000)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut blocks: Vec<RecentBlockItem> = blocks_desc
        .into_iter()
        .filter(|(_, h)| h.timestamp > cutoff_ts)
        .map(|(_, h)| RecentBlockItem {
            timestamp: h.timestamp,
            transactions_count: h.transactions_count,
        })
        .collect();

    // Reverse to ascending order
    blocks.reverse();

    let response = RecentBlocksResponse { blocks };

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(10))
        .await;

    ok(response)
}

fn compact_to_difficulty(compact: i64) -> u64 {
    let compact = compact as u32;
    let exponent = ((compact >> 24) & 0xFF) as i32;
    let mantissa = (compact & 0x00FFFFFF) as f64;

    if mantissa == 0.0 {
        return 0;
    }

    const GENESIS_EXP: i32 = 0x20;
    const GENESIS_MAN: f64 = 0x01_0000 as f64;

    let exp_diff = GENESIS_EXP - exponent;
    let difficulty = if exp_diff >= 0 {
        (GENESIS_MAN / mantissa) * (256.0_f64).powi(exp_diff)
    } else {
        (GENESIS_MAN / mantissa) / (256.0_f64).powi(-exp_diff)
    };

    difficulty as u64
}

fn format_hash_rate(hash_rate: f64) -> String {
    const KILO: f64 = 1_000.0;
    const MEGA: f64 = 1_000_000.0;
    const GIGA: f64 = 1_000_000_000.0;
    const TERA: f64 = 1_000_000_000_000.0;
    const PETA: f64 = 1_000_000_000_000_000.0;
    const EXA: f64 = 1_000_000_000_000_000_000.0;

    if hash_rate >= EXA {
        format!("{:.2} EH/s", hash_rate / EXA)
    } else if hash_rate >= PETA {
        format!("{:.2} PH/s", hash_rate / PETA)
    } else if hash_rate >= TERA {
        format!("{:.2} TH/s", hash_rate / TERA)
    } else if hash_rate >= GIGA {
        format!("{:.2} GH/s", hash_rate / GIGA)
    } else if hash_rate >= MEGA {
        format!("{:.2} MH/s", hash_rate / MEGA)
    } else if hash_rate >= KILO {
        format!("{:.2} KH/s", hash_rate / KILO)
    } else {
        format!("{:.2} H/s", hash_rate)
    }
}

fn format_difficulty(difficulty: u64) -> String {
    const KILO: u64 = 1_000;
    const MEGA: u64 = 1_000_000;
    const GIGA: u64 = 1_000_000_000;
    const TERA: u64 = 1_000_000_000_000;
    const PETA: u64 = 1_000_000_000_000_000;
    const EXA: u64 = 1_000_000_000_000_000_000;

    if difficulty >= EXA {
        format!("{:.2} E", difficulty as f64 / EXA as f64)
    } else if difficulty >= PETA {
        format!("{:.2} P", difficulty as f64 / PETA as f64)
    } else if difficulty >= TERA {
        format!("{:.2} T", difficulty as f64 / TERA as f64)
    } else if difficulty >= GIGA {
        format!("{:.2} G", difficulty as f64 / GIGA as f64)
    } else if difficulty >= MEGA {
        format!("{:.2} M", difficulty as f64 / MEGA as f64)
    } else if difficulty >= KILO {
        format!("{:.2} K", difficulty as f64 / KILO as f64)
    } else {
        format!("{}", difficulty)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxStatsDataPoint {
    pub label: String,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxStatsResponse {
    pub current_hour: i64,
    pub current_day: i64,
    pub hourly_data: Vec<TxStatsDataPoint>,
    pub daily_data: Vec<TxStatsDataPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartDataPoint {
    pub date: String,
    pub value: String,
    pub value2: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartResponse {
    pub data: Vec<ChartDataPoint>,
    pub title: String,
    pub y_axis_label: String,
    pub y2_axis_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackedAreaDataPoint {
    pub date: String,
    pub values: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackedAreaSeries {
    pub key: String,
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackedAreaChartResponse {
    pub data: Vec<StackedAreaDataPoint>,
    pub series: Vec<StackedAreaSeries>,
    pub title: String,
}

async fn get_transaction_count_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let daily_stats = state
        .store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = daily_stats
        .into_iter()
        .map(|(date_str, stats)| {
            let formatted_date = format_date_for_chart(&date_str);
            ChartDataPoint {
                date: formatted_date,
                value: stats.transactions_count.to_string(),
                value2: None,
            }
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Transaction Count".to_string(),
        y_axis_label: "Transactions".to_string(),
        y2_axis_label: None,
    })
}

async fn get_cell_count_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    let daily_stats = state
        .store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<StackedAreaDataPoint> = daily_stats
        .into_iter()
        .filter(|(_, stats)| stats.total_all_cells > 0)
        .map(|(date_str, stats)| {
            let mut values = std::collections::HashMap::new();
            values.insert("allCells".to_string(), stats.total_all_cells.to_string());
            values.insert("liveCells".to_string(), stats.total_live_cells.to_string());
            values.insert("deadCells".to_string(), stats.total_dead_cells.to_string());
            StackedAreaDataPoint {
                date: format_date_for_chart(&date_str),
                values,
            }
        })
        .collect();

    let series = vec![
        StackedAreaSeries {
            key: "allCells".to_string(),
            label: "All Cells".to_string(),
            color: "#6b7280".to_string(),
        },
        StackedAreaSeries {
            key: "liveCells".to_string(),
            label: "Live Cells".to_string(),
            color: "#00c389".to_string(),
        },
        StackedAreaSeries {
            key: "deadCells".to_string(),
            label: "Dead Cells".to_string(),
            color: "#ef4444".to_string(),
        },
    ];

    ok(StackedAreaChartResponse {
        data,
        series,
        title: "Cell Count".to_string(),
    })
}

async fn get_knowledge_size_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let daily_stats = state
        .store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = daily_stats
        .into_iter()
        .filter_map(|(date_str, stats)| {
            stats.knowledge_size.map(|ks| ChartDataPoint {
                date: format_date_for_chart(&date_str),
                value: ks.to_string(),
                value2: None,
            })
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Common Knowledge Size".to_string(),
        y_axis_label: "CKB".to_string(),
        y2_axis_label: None,
    })
}

async fn get_block_time_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:block-time-distribution";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let dist = state
        .store
        .list_block_time_dist()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let total_blocks: i64 = dist.iter().map(|(_, count)| *count as i64).sum();

    let data: Vec<ChartDataPoint> = dist
        .into_iter()
        .map(|(bucket_ms, count)| {
            let time_seconds = bucket_ms as f64 / 1000.0;
            let ratio = if total_blocks > 0 {
                (count as f64 / total_blocks as f64 * 100.0 * 1000.0).round() / 1000.0
            } else {
                0.0
            };
            ChartDataPoint {
                date: format!("{:.1}", time_seconds),
                value: format!("{:.3}", ratio),
                value2: None,
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Block Time Distribution (Recent 50000 blocks)".to_string(),
        y_axis_label: "Block Ratio (%)".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_epoch_time_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:epoch-time-distribution";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let dist = state
        .store
        .list_epoch_time_dist()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = dist
        .into_iter()
        .map(|(bucket_minutes, count)| {
            let hours_decimal = bucket_minutes as f64 / 60.0;
            ChartDataPoint {
                date: format!("{:.2}", hours_decimal),
                value: count.to_string(),
                value2: None,
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Epoch Time Distribution".to_string(),
        y_axis_label: "Epochs".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_epoch_time_length_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:epoch-time-length";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let epochs = state
        .store
        .list_epoch_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = epochs
        .into_iter()
        .filter_map(|e| {
            let end = e.end_timestamp?;
            let duration_secs = end.signed_duration_since(e.start_timestamp).num_seconds() as f64;
            let duration_hours = duration_secs / 3600.0;
            Some(ChartDataPoint {
                date: e.epoch_number.to_string(),
                value: format!("{:.2}", duration_hours),
                value2: Some(e.blocks_count.to_string()),
            })
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Epoch Time Length".to_string(),
        y_axis_label: "Hours".to_string(),
        y2_axis_label: Some("Blocks".to_string()),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_average_block_time_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:average-block-time";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_stats = state
        .store
        .list_daily_stats_with_dates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = daily_stats
        .into_iter()
        .filter_map(|(date_str, stats)| {
            stats.avg_block_time_ms.map(|avg_time_ms| ChartDataPoint {
                date: format_date_for_chart(&date_str),
                value: format!("{:.2}", avg_time_ms as f64 / 1000.0),
                value2: None,
            })
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Average Block Time".to_string(),
        y_axis_label: "Seconds".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn fetch_tip_block_from_ckb(ckb_rpc_url: &str) -> Result<u64, String> {
    #[derive(Serialize)]
    struct RpcRequest {
        jsonrpc: &'static str,
        method: &'static str,
        params: Vec<()>,
        id: u64,
    }

    #[derive(Deserialize)]
    struct RpcResponse {
        result: Option<String>,
    }

    let client = reqwest::Client::new();
    let request = RpcRequest {
        jsonrpc: "2.0",
        method: "get_tip_block_number",
        params: vec![],
        id: 1,
    };

    let response = client
        .post(ckb_rpc_url)
        .json(&request)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<RpcResponse>()
        .await
        .map_err(|e| e.to_string())?;

    let hex = response.result.ok_or("Empty RPC response")?;
    let hex = hex.strip_prefix("0x").unwrap_or(&hex);
    u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
}

async fn fetch_network_stats_from_db(
    state: &AppState,
) -> Result<
    NetworkStats,
    (
        axum::http::StatusCode,
        axum::Json<crate::response::ApiError>,
    ),
> {
    let store = &state.store;

    // Get latest block header from store
    let latest = store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (latest_block, epoch_number, epoch_index, epoch_length, latest_timestamp) = match latest {
        Some((block_num, header)) => {
            let ts = DateTime::from_timestamp_millis(header.timestamp).unwrap_or_else(Utc::now);
            (
                block_num,
                header.epoch_number,
                header.epoch_index,
                header.epoch_length,
                ts,
            )
        }
        None => (0i64, 0i64, 0i32, 1800i32, Utc::now()),
    };

    // Get compact_target from the latest block header's DAO or from block header directly
    // The CachedBlockHeader doesn't store compact_target, so we compute difficulty
    // from the latest DailyBlockStats instead. For now use the latest daily block stats.
    let today = latest_timestamp.date_naive();
    let today_str = today.format("%Y%m%d").to_string();
    let yesterday = today - chrono::Duration::days(1);
    let yesterday_str = yesterday.format("%Y%m%d").to_string();

    // Fetch epoch stats for avg block time
    let epoch_stats = store
        .get_epoch_stats(epoch_number)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Get recent block for avg block time
    let recent_blocks = store
        .list_blocks_desc(Some(latest_block), 2)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Get 24h tx count from daily stats
    let today_stats = store
        .get_daily_stats(&today_str)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let yesterday_stats = store
        .get_daily_stats(&yesterday_str)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let tx_count_24h: i64 = today_stats
        .as_ref()
        .map(|s| s.transactions_count as i64)
        .unwrap_or(0)
        + yesterday_stats
            .as_ref()
            .map(|s| s.transactions_count as i64)
            .unwrap_or(0);

    // Fetch tip block from CKB node
    let tip_block_result = fetch_tip_block_from_ckb(&state.ckb_rpc_url).await;
    let tip_block = tip_block_result.unwrap_or(latest_block as u64) as i64;

    // Calculate epoch avg block time
    let epoch_avg_time = epoch_stats
        .and_then(|es| {
            if es.blocks_count > 1 {
                if let Some(end) = es.end_timestamp {
                    let duration =
                        end.signed_duration_since(es.start_timestamp).num_seconds() as f64;
                    Some(duration / (es.blocks_count - 1) as f64)
                } else {
                    // Epoch in progress
                    let duration = latest_timestamp
                        .signed_duration_since(es.start_timestamp)
                        .num_seconds() as f64;
                    Some(duration / epoch_index.max(1) as f64)
                }
            } else {
                None
            }
        })
        .unwrap_or(10.0);

    // Calculate recent avg block time from last 2 blocks
    let avg_time = if recent_blocks.len() == 2 {
        let ts0 = DateTime::from_timestamp_millis(recent_blocks[1].1.timestamp).unwrap_or_default();
        let ts1 = DateTime::from_timestamp_millis(recent_blocks[0].1.timestamp).unwrap_or_default();
        let duration = ts0.signed_duration_since(ts1).num_seconds() as f64;
        duration.abs().max(1.0)
    } else {
        10.0
    };

    // Get compact_target from daily block stats for difficulty/hash rate
    let daily_block_stats = store
        .get_daily_block_stats(&today_str)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .or_else(|| store.get_daily_block_stats(&yesterday_str).ok().flatten());

    let compact_target = daily_block_stats
        .as_ref()
        .map(|s| s.avg_compact_target as i64)
        .unwrap_or(0);

    let remaining_blocks = epoch_length - epoch_index;
    let estimated_epoch_seconds = (remaining_blocks as f64 * epoch_avg_time) as i64;

    let tps = tx_count_24h as f64 / 86400.0;
    let tx_per_minute = tps * 60.0;

    // Get sync status from Redis cache or from store
    let sync_status_from_redis: Option<SyncStatusData> =
        state.cache.get(SYNC_STATUS_REDIS_KEY).await;
    let (synced_block, db_ema_rate, sync_started_at, bulk_sync_completed_at) =
        sync_status_from_redis
            .as_ref()
            .map(|s| {
                (
                    s.tip_block_number,
                    s.sync_ema_rate,
                    s.sync_started_at,
                    s.bulk_sync_completed_at,
                )
            })
            .unwrap_or((latest_block, None, None, None));

    // Get deep fork status from store
    let store_sync = store
        .get_sync_status()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (
        deep_fork_detected,
        deep_fork_at,
        deep_fork_db_tip,
        deep_fork_chain_tip,
        deep_fork_depth,
        deep_fork_fork_point,
    ) = if store_sync.deep_fork_detected {
        if let Some(ref info) = store_sync.deep_fork_info {
            (
                true,
                None::<DateTime<Utc>>, // deep fork timestamp not stored in store
                Some(info.db_tip),
                Some(info.chain_tip),
                Some(info.depth),
                Some(info.fork_point),
            )
        } else {
            (true, None, None, None, None, None)
        }
    } else {
        (false, None, None, None, None, None)
    };

    let blocks_behind = tip_block - synced_block;
    let is_syncing = blocks_behind > 100;
    let is_bulk_syncing = blocks_behind > 1000;

    let sync_mode = if bulk_sync_completed_at.is_some() && !is_syncing {
        "synced".to_string()
    } else if is_bulk_syncing {
        "bulk".to_string()
    } else if is_syncing {
        "normal".to_string()
    } else {
        "synced".to_string()
    };

    let now = Utc::now().timestamp();
    let elapsed_time = sync_started_at.map(|started| {
        let end = bulk_sync_completed_at.unwrap_or(now);
        format_duration_smart((end - started) as f64)
    });

    let total_time =
        if let (Some(started), Some(completed)) = (sync_started_at, bulk_sync_completed_at) {
            Some(format_duration_smart((completed - started) as f64))
        } else {
            None
        };

    let sync_progress_from_redis: Option<SyncProgressData> =
        state.cache.get(SYNC_PROGRESS_REDIS_KEY).await;

    let (progress, estimated_time, blocks_per_second, ema_blocks_per_second) =
        if let Some(ref sp) = sync_progress_from_redis {
            let stale = Utc::now().timestamp() - sp.updated_at > 60;
            if !stale && is_syncing {
                (
                    sp.progress_percentage,
                    Some(sp.eta_formatted.clone()),
                    Some(sp.blocks_per_second),
                    Some(sp.ema_blocks_per_second),
                )
            } else {
                let p = if tip_block > 0 {
                    (synced_block as f64 / tip_block as f64 * 100.0).min(100.0)
                } else {
                    0.0
                };
                let (ema, eta) = if is_syncing {
                    if let Some(rate) = db_ema_rate {
                        if rate > 0.0 {
                            let remaining = blocks_behind as f64;
                            let eta_secs = remaining / rate;
                            let eta_str = format_duration_smart(eta_secs);
                            (Some(rate), Some(eta_str))
                        } else {
                            (Some(rate), None)
                        }
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };
                (p, eta, ema, ema)
            }
        } else {
            let p = if tip_block > 0 {
                (synced_block as f64 / tip_block as f64 * 100.0).min(100.0)
            } else {
                0.0
            };
            let (ema, eta) = if is_syncing {
                if let Some(rate) = db_ema_rate {
                    if rate > 0.0 {
                        let remaining = blocks_behind as f64;
                        let eta_secs = remaining / rate;
                        let eta_str = format_duration_smart(eta_secs);
                        (Some(rate), Some(eta_str))
                    } else {
                        (Some(rate), None)
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            };
            (p, eta, ema, ema)
        };

    let sync_status = SyncStatus {
        is_syncing,
        synced_block,
        tip_block,
        progress,
        estimated_time,
        chart_data_may_be_incomplete: blocks_behind > 1000,
        blocks_per_second,
        ema_blocks_per_second,
        sync_mode,
        started_at: sync_started_at,
        elapsed_time,
        total_time,
    };

    let deep_fork_status = DeepForkStatus {
        detected: deep_fork_detected,
        detected_at: deep_fork_at,
        depth: deep_fork_depth,
        db_tip: deep_fork_db_tip,
        chain_tip: deep_fork_chain_tip,
        fork_point: deep_fork_fork_point,
    };

    let difficulty = compact_to_difficulty(compact_target);
    let hash_rate = if avg_time > 0.0 {
        difficulty as f64 / avg_time
    } else {
        0.0
    };

    Ok(NetworkStats {
        latest_block,
        avg_block_time: format!("{:.2}s", avg_time),
        hash_rate: format_hash_rate(hash_rate),
        difficulty: format_difficulty(difficulty),
        epoch: format!("{}({}/{})", epoch_number, epoch_index, epoch_length),
        tps: format!("{:.2}", tps),
        estimated_epoch_time: format_duration(estimated_epoch_seconds as u64),
        transactions_per_minute: format!("{:.1}", tx_per_minute),
        transactions_per_day: tx_count_24h.to_string(),
        sync_status,
        deep_fork_status,
    })
}

async fn get_hash_rate_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:hash-rate";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_block_stats = state
        .store
        .list_daily_block_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Exclude the last day (incomplete) like the SQL version did
    let max_date = daily_block_stats
        .iter()
        .map(|(d, _)| d.as_str())
        .max()
        .map(|s| s.to_string());

    let data: Vec<ChartDataPoint> = daily_block_stats
        .into_iter()
        .filter(|(date, stats)| {
            stats.avg_compact_target > 0.0
                && max_date.as_ref().is_none_or(|m| date.as_str() < m.as_str())
        })
        .map(|(date_str, stats)| {
            let difficulty = compact_to_difficulty(stats.avg_compact_target as i64);
            let avg_block_time = 86400.0 / stats.block_count as f64;
            let hash_rate = difficulty as f64 / avg_block_time;
            ChartDataPoint {
                date: format_date_for_chart(&date_str),
                value: format!("{:.0}", hash_rate),
                value2: None,
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Hash Rate".to_string(),
        y_axis_label: "Hash Rate (H/s)".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_difficulty_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:difficulty";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_block_stats = state
        .store
        .list_daily_block_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let max_date = daily_block_stats
        .iter()
        .map(|(d, _)| d.as_str())
        .max()
        .map(|s| s.to_string());

    let data: Vec<ChartDataPoint> = daily_block_stats
        .into_iter()
        .filter(|(date, stats)| {
            stats.avg_compact_target > 0.0
                && max_date.as_ref().is_none_or(|m| date.as_str() < m.as_str())
        })
        .map(|(date_str, stats)| {
            let difficulty = compact_to_difficulty(stats.avg_compact_target as i64);
            ChartDataPoint {
                date: format_date_for_chart(&date_str),
                value: difficulty.to_string(),
                value2: None,
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Difficulty".to_string(),
        y_axis_label: "Difficulty".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_uncle_rate_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:uncle-rate";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_block_stats = state
        .store
        .list_daily_block_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let max_date = daily_block_stats
        .iter()
        .map(|(d, _)| d.as_str())
        .max()
        .map(|s| s.to_string());

    let data: Vec<ChartDataPoint> = daily_block_stats
        .into_iter()
        .filter(|(date, _)| max_date.as_ref().is_none_or(|m| date.as_str() < m.as_str()))
        .map(|(date_str, stats)| {
            let uncle_rate = if stats.block_count > 0 {
                stats.total_uncles as f64 / stats.block_count as f64
            } else {
                0.0
            };
            ChartDataPoint {
                date: format_date_for_chart(&date_str),
                value: format!("{:.6}", uncle_rate),
                value2: None,
            }
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Uncle Rate".to_string(),
        y_axis_label: "Uncle Rate".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinerDistributionDataPoint {
    pub address: String,
    pub miner_name: Option<String>,
    pub blocks_mined: i64,
    pub percentage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinerDistributionResponse {
    pub data: Vec<MinerDistributionDataPoint>,
    pub title: String,
    pub total_blocks: i64,
}

async fn get_miner_address_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<MinerDistributionResponse> {
    let cache_key = "chart:miner-address-distribution";
    if let Some(cached) = state
        .cache
        .get::<MinerDistributionResponse>(cache_key)
        .await
    {
        return ok(cached);
    }

    let miner_stats = state
        .store
        .list_miner_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Aggregate by miner_lock_hash across all dates
    let mut aggregated: std::collections::HashMap<Vec<u8>, i64> = std::collections::HashMap::new();
    for ms in &miner_stats {
        *aggregated.entry(ms.miner_lock_hash.clone()).or_insert(0) += ms.blocks_count as i64;
    }

    let total_blocks: i64 = aggregated.values().sum();
    let total = total_blocks as f64;

    // Sort by blocks descending and take top 100
    let mut sorted: Vec<(Vec<u8>, i64)> = aggregated.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted.truncate(100);

    let data: Vec<MinerDistributionDataPoint> = sorted
        .into_iter()
        .map(|(hash, blocks_mined)| {
            let percentage = if total > 0.0 {
                (blocks_mined as f64 / total) * 100.0
            } else {
                0.0
            };
            let address = format!("0x{}", hex::encode(&hash));
            MinerDistributionDataPoint {
                address,
                miner_name: None,
                blocks_mined,
                percentage: format!("{:.4}", percentage),
            }
        })
        .collect();

    let response = MinerDistributionResponse {
        data,
        title: "Miner Address Distribution".to_string(),
        total_blocks,
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_total_supply_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    let cache_key = "chart:total-supply";
    if let Some(cached) = state.cache.get::<StackedAreaChartResponse>(cache_key).await {
        return ok(cached);
    }

    let snapshots = state
        .store
        .list_dao_daily_snapshots()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    const BURNT_CKB: &str = "84000000";
    const SHANNON: f64 = 100_000_000.0;

    let data: Vec<StackedAreaDataPoint> = snapshots
        .iter()
        .filter_map(|s| {
            let total_supply = estimated_total_supply(&s.date)?;
            let locked_shannon = s.total_deposited.max(0) as u128;
            let circulating_shannon =
                (total_supply - locked_shannon as f64 - GENESIS_BURNT as f64).max(0.0);
            let mut values = std::collections::HashMap::new();
            values.insert(
                "circulating".to_string(),
                format!("{:.0}", circulating_shannon / SHANNON),
            );
            values.insert("locked".to_string(), shannon_to_ckb_u128(locked_shannon));
            values.insert("burnt".to_string(), BURNT_CKB.to_string());
            Some(StackedAreaDataPoint {
                date: s.date.clone(),
                values,
            })
        })
        .collect();

    let series = vec![
        StackedAreaSeries {
            key: "circulating".to_string(),
            label: "Circulating".to_string(),
            color: "#00c389".to_string(),
        },
        StackedAreaSeries {
            key: "locked".to_string(),
            label: "Locked in DAO".to_string(),
            color: "#8b5cf6".to_string(),
        },
        StackedAreaSeries {
            key: "burnt".to_string(),
            label: "Burnt".to_string(),
            color: "#6b7280".to_string(),
        },
    ];

    let response = StackedAreaChartResponse {
        data,
        series,
        title: "Total Supply".to_string(),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_nominal_apc_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let data: Vec<ChartDataPoint> = (0..=80)
        .map(|i| {
            let year = i as f64 * 0.25;
            let apc = calculate_nominal_apc(year);
            ChartDataPoint {
                date: format!("{}", year),
                value: format!("{:.4}", apc),
                value2: None,
            }
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Nominal DAO Compensation Rate".to_string(),
        y_axis_label: "APC".to_string(),
        y2_axis_label: None,
    })
}

fn calculate_nominal_apc(year: f64) -> f64 {
    // Genesis actual supply is 25.2B (33.6B - 8.4B burnt at genesis)
    const GENESIS_SUPPLY: f64 = 25_200_000_000.0;
    const SECONDARY_ISSUANCE_PER_YEAR: f64 = 1_344_000_000.0;

    let halving_count = (year / 4.0).floor() as u32;

    let mut total_primary_issued = 0.0;
    for h in 0..halving_count {
        let rate = 4_200_000_000.0 / 2.0_f64.powi(h as i32);
        total_primary_issued += rate * 4.0;
    }

    let years_in_current_era = year - (halving_count as f64 * 4.0);
    let current_era_rate = 4_200_000_000.0 / 2.0_f64.powi(halving_count as i32);
    total_primary_issued += current_era_rate * years_in_current_era;

    let total_secondary_issued = SECONDARY_ISSUANCE_PER_YEAR * year;
    let total_supply = GENESIS_SUPPLY + total_primary_issued + total_secondary_issued;

    (SECONDARY_ISSUANCE_PER_YEAR / total_supply) * 100.0
}

async fn get_secondary_issuance_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    let cache_key = "chart:secondary-issuance";
    if let Some(cached) = state.cache.get::<StackedAreaChartResponse>(cache_key).await {
        return ok(cached);
    }

    let daily_issuance = state
        .store
        .list_daily_secondary_issuance()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<StackedAreaDataPoint> = daily_issuance
        .iter()
        .map(|(date, cum_dao, cum_miner, cum_treasury)| {
            let mut values = std::collections::HashMap::new();
            values.insert(
                "compensation".to_string(),
                shannon_to_ckb(&cum_dao.to_string()),
            );
            values.insert("mining".to_string(), shannon_to_ckb(&cum_miner.to_string()));
            values.insert(
                "burnt".to_string(),
                shannon_to_ckb(&cum_treasury.to_string()),
            );
            StackedAreaDataPoint {
                date: date.clone(),
                values,
            }
        })
        .collect();

    let series = vec![
        StackedAreaSeries {
            key: "compensation".to_string(),
            label: "Deposit Compensation".to_string(),
            color: "#00c389".to_string(),
        },
        StackedAreaSeries {
            key: "mining".to_string(),
            label: "Mining Reward".to_string(),
            color: "#8b5cf6".to_string(),
        },
        StackedAreaSeries {
            key: "burnt".to_string(),
            label: "Burnt".to_string(),
            color: "#6b7280".to_string(),
        },
    ];

    let response = StackedAreaChartResponse {
        data,
        series,
        title: "Secondary Issuance".to_string(),
    };

    state.cache.set(cache_key, &response, CacheTtl::CHART).await;

    ok(response)
}

async fn get_inflation_rate_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let data: Vec<ChartDataPoint> = (0..=100)
        .map(|i| {
            let year = i as f64 * 0.5;
            let (nominal, real) = calculate_inflation_rates(year);
            ChartDataPoint {
                date: format!("{:.1}", year),
                value: format!("{:.4}", nominal),
                value2: Some(format!("{:.4}", real)),
            }
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Inflation Rate".to_string(),
        y_axis_label: "Nominal Inflation (%)".to_string(),
        y2_axis_label: Some("Real Inflation (%)".to_string()),
    })
}

fn calculate_inflation_rates(year: f64) -> (f64, f64) {
    const INITIAL_PRIMARY_RATE: f64 = 0.125;
    const SECONDARY_RATE: f64 = 0.0134;

    let halving_era = (year / 4.0).floor() as u32;
    let primary_rate = INITIAL_PRIMARY_RATE / 2.0_f64.powi(halving_era as i32);

    let nominal = (primary_rate + SECONDARY_RATE) * 100.0;

    let effective_locked_ratio = 0.5;
    let real = (primary_rate + SECONDARY_RATE * (1.0 - effective_locked_ratio)) * 100.0;

    (nominal, real)
}

/// Convert a date string from "YYYY-MM-DD" to "YYYY/MM/DD" for chart display.
fn format_date_for_chart(date_str: &str) -> String {
    date_str.replace('-', "/")
}

/// Estimate total supply (in shannons) at a given date.
///
/// CKB issuance schedule:
/// - Genesis: 33.6B CKB
/// - Primary: 4.2B/year, halving every ~4 years (first halving ~Nov 2023)
/// - Secondary: 1.344B/year (constant)
fn estimated_total_supply(date_str: &str) -> Option<f64> {
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    let launch = NaiveDate::from_ymd_opt(2019, 11, 16)?;
    let days = (date - launch).num_days().max(0) as f64;
    let years = days / 365.25;

    const SHANNON: f64 = 100_000_000.0;
    const GENESIS_SUPPLY: f64 = 33_600_000_000.0 * SHANNON;
    const SECONDARY_PER_YEAR: f64 = 1_344_000_000.0 * SHANNON;
    const PRIMARY_PER_YEAR: f64 = 4_200_000_000.0 * SHANNON;

    let mut primary_issued = 0.0;
    let mut remaining = years;
    let mut era = 0u32;
    while remaining > 0.0 {
        let era_years = remaining.min(4.0);
        let rate = PRIMARY_PER_YEAR / 2.0_f64.powi(era as i32);
        primary_issued += rate * era_years;
        remaining -= 4.0;
        era += 1;
    }

    let secondary_issued = SECONDARY_PER_YEAR * years;
    Some(GENESIS_SUPPLY + primary_issued + secondary_issued)
}
