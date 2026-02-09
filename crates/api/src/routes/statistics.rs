#![allow(clippy::type_complexity)]

use axum::{extract::State, routing::get, Router};
use chrono::{DateTime, Utc};
use ckbadger_common::dao::GENESIS_BURNT;
use ckbadger_common::sync::{
    format_duration_smart, SyncProgressData, SyncStatusData, SYNC_PROGRESS_REDIS_KEY,
    SYNC_STATUS_REDIS_KEY,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::cache::{CacheKeys, CacheTtl};
use crate::response::{ok, ApiError, ApiResult};
use crate::utils::{format_duration, script_to_address, shannon_to_ckb};
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

    // Use latest synced block timestamp as reference, not current time
    let latest_ts: Option<(DateTime<Utc>,)> =
        sqlx::query_as("SELECT timestamp FROM blocks_index ORDER BY number DESC LIMIT 1")
            .fetch_optional(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_time = latest_ts.map(|(ts,)| ts).unwrap_or_else(Utc::now);
    let reference_date = reference_time.date_naive();

    let hourly_rows = sqlx::query_as::<_, (DateTime<Utc>, i32)>(
        r#"
        SELECT hour, transactions_count
        FROM hourly_statistics
        WHERE hour > $1 - INTERVAL '24 hours' AND hour <= $1
        ORDER BY hour DESC
        LIMIT 24
        "#,
    )
    .bind(reference_time)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let daily_rows = sqlx::query_as::<_, (chrono::NaiveDate, i32)>(
        r#"
        SELECT date, transactions_count
        FROM daily_statistics
        WHERE date > $1 - INTERVAL '14 days' AND date <= $1
        ORDER BY date DESC
        LIMIT 14
        "#,
    )
    .bind(reference_date)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let txs_this_hour: i64 = hourly_rows.first().map(|(_, c)| *c as i64).unwrap_or(0);
    let txs_in_24_hours: i64 = hourly_rows.iter().map(|(_, c)| *c as i64).sum();

    let hourly_data: Vec<TxStatsDataPoint> = hourly_rows
        .into_iter()
        .rev()
        .map(|(hour, count)| TxStatsDataPoint {
            label: hour.format("%H:00").to_string(),
            value: count as i64,
        })
        .collect();

    let daily_data: Vec<TxStatsDataPoint> = daily_rows
        .into_iter()
        .rev()
        .map(|(date, count)| TxStatsDataPoint {
            label: date.format("%m/%d").to_string(),
            value: count as i64,
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

    let latest_ts: Option<(DateTime<Utc>,)> =
        sqlx::query_as("SELECT timestamp FROM blocks_index ORDER BY number DESC LIMIT 1")
            .fetch_optional(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_time = latest_ts.map(|(ts,)| ts).unwrap_or_else(Utc::now);

    let rows = sqlx::query_as::<_, (DateTime<Utc>, i32)>(
        r#"
        SELECT timestamp, tx_count
        FROM blocks_index
        WHERE timestamp > $1 - INTERVAL '24 hours'
        ORDER BY number ASC
        "#,
    )
    .bind(reference_time)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let blocks: Vec<RecentBlockItem> = rows
        .into_iter()
        .map(|(ts, tx_count)| RecentBlockItem {
            timestamp: ts.timestamp_millis(),
            transactions_count: tx_count,
        })
        .collect();

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
    let rows = sqlx::query_as::<_, (chrono::NaiveDate, i32)>(
        r#"
        SELECT date, transactions_count
        FROM daily_statistics
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|(date, tx_count)| ChartDataPoint {
            date: date.format("%Y/%m/%d").to_string(),
            value: tx_count.to_string(),
            value2: None,
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
    let rows = sqlx::query_as::<_, (chrono::NaiveDate, i64, i64, i64)>(
        r#"
        SELECT date, total_all_cells, total_live_cells, total_dead_cells
        FROM daily_statistics
        WHERE total_all_cells IS NOT NULL
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<StackedAreaDataPoint> = rows
        .into_iter()
        .map(|(date, all_cells, live_cells, dead_cells)| {
            let mut values = std::collections::HashMap::new();
            values.insert("allCells".to_string(), all_cells.to_string());
            values.insert("liveCells".to_string(), live_cells.to_string());
            values.insert("deadCells".to_string(), dead_cells.to_string());
            StackedAreaDataPoint {
                date: date.format("%Y/%m/%d").to_string(),
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
    let rows = sqlx::query_as::<_, (chrono::NaiveDate, String)>(
        r#"
        SELECT date, knowledge_size::text
        FROM daily_statistics
        WHERE knowledge_size IS NOT NULL
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|(date, knowledge_size)| ChartDataPoint {
            date: date.format("%Y/%m/%d").to_string(),
            value: knowledge_size,
            value2: None,
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

    let rows = sqlx::query_as::<_, (i32, i64)>(
        r#"
        SELECT bucket_ms, block_count
        FROM block_time_distribution
        ORDER BY bucket_ms
        "#,
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let total_blocks: i64 = rows.iter().map(|(_, count)| count).sum();

    let data: Vec<ChartDataPoint> = rows
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

    let rows = sqlx::query_as::<_, (i32, i64)>(
        r#"
        SELECT bucket_minutes, epoch_count
        FROM epoch_time_distribution
        ORDER BY bucket_minutes
        "#,
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
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

    let rows = sqlx::query_as::<_, (i64, f64, i32)>(
        r#"
        SELECT 
            epoch_number,
            (EXTRACT(EPOCH FROM (end_timestamp - start_timestamp)) / 3600.0)::float8 as duration_hours,
            blocks_count
        FROM epoch_statistics
        WHERE end_timestamp IS NOT NULL
        ORDER BY epoch_number ASC
        "#,
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(
            |(epoch_number, duration_hours, block_count)| ChartDataPoint {
                date: epoch_number.to_string(),
                value: format!("{:.2}", duration_hours),
                value2: Some(block_count.to_string()),
            },
        )
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

    let rows = sqlx::query_as::<_, (chrono::NaiveDate, i32)>(
        r#"
        SELECT date, avg_block_time_ms
        FROM daily_statistics
        WHERE avg_block_time_ms IS NOT NULL
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|(date, avg_time_ms)| ChartDataPoint {
            date: date.format("%Y/%m/%d").to_string(),
            value: format!("{:.2}", avg_time_ms as f64 / 1000.0),
            value2: None,
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
    let latest: Option<(i64, i64, i32, i32, i64, DateTime<Utc>)> = sqlx::query_as(
        "SELECT number, epoch_number, epoch_index, epoch_length, compact_target, timestamp FROM blocks_index ORDER BY number DESC LIMIT 1",
    )
    .fetch_optional(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (latest_block, epoch_number, epoch_index, epoch_length, compact_target, latest_timestamp) =
        latest.unwrap_or((0, 0, 0, 1800, 0, Utc::now()));

    // Optimization: Use simpler queries to avoid expensive self-joins and full scans
    // 1. For epoch avg time: calculate from epoch start block to latest (using timestamps from epoch stats)
    // 2. For recent avg time: use last 2 blocks instead of self-join on 100 blocks
    // 3. For 24h tx count: use daily_statistics sum instead of COUNT across partitions
    let today = latest_timestamp.date_naive();
    let yesterday = today - chrono::Duration::days(1);

    let (epoch_avg_result, recent_blocks_result, tx_count_result, tip_block_result) = tokio::join!(
        // Get epoch avg block time from epoch_statistics (pre-computed)
        sqlx::query_as::<_, (Option<DateTime<Utc>>, Option<DateTime<Utc>>, i32)>(
            r#"
            SELECT start_timestamp, end_timestamp, blocks_count
            FROM epoch_statistics
            WHERE epoch_number = $1
            "#,
        )
        .bind(epoch_number)
        .fetch_optional(&state.read_pool),
        // Get timestamps of last 2 blocks for recent avg (much faster than self-join)
        sqlx::query_as::<_, (DateTime<Utc>,)>(
            r#"
            SELECT timestamp FROM blocks_index
            WHERE number >= $1 - 1 AND number <= $1
            ORDER BY number ASC
            "#,
        )
        .bind(latest_block)
        .fetch_all(&state.read_pool),
        // Get 24h transaction count from daily_statistics (pre-computed)
        sqlx::query_as::<_, (i64,)>(
            r#"
            SELECT COALESCE(SUM(transactions_count), 0)
            FROM daily_statistics
            WHERE date >= $1
            "#,
        )
        .bind(yesterday)
        .fetch_one(&state.read_pool),
        fetch_tip_block_from_ckb(&state.ckb_rpc_url)
    );

    // Calculate epoch avg time from epoch statistics
    let epoch_avg_time = epoch_avg_result
        .ok()
        .flatten()
        .and_then(|(start, end, blocks_count)| {
            if blocks_count > 1 {
                if let (Some(s), Some(e)) = (start, end) {
                    let duration = e.signed_duration_since(s).num_seconds() as f64;
                    Some(duration / (blocks_count - 1) as f64)
                } else if let Some(s) = start {
                    // Epoch in progress: use time from start to latest block
                    let duration = latest_timestamp.signed_duration_since(s).num_seconds() as f64;
                    Some(duration / epoch_index.max(1) as f64)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or(10.0);

    // Calculate recent avg time from last 2 blocks
    let avg_time = recent_blocks_result
        .ok()
        .and_then(|blocks| {
            if blocks.len() == 2 {
                let duration = blocks[1].0.signed_duration_since(blocks[0].0).num_seconds() as f64;
                Some(duration.max(1.0))
            } else {
                None
            }
        })
        .unwrap_or(10.0);

    let tx_count_24h = tx_count_result
        .map_err(|e| ApiError::internal(e.to_string()))?
        .0;

    let tip_block = tip_block_result.unwrap_or(latest_block as u64) as i64;

    let remaining_blocks = epoch_length - epoch_index;
    let estimated_epoch_seconds = (remaining_blocks as f64 * epoch_avg_time) as i64;

    let tps = tx_count_24h as f64 / 86400.0;
    let tx_per_minute = tps * 60.0;

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

    let deep_fork_row: Option<(
        bool,
        Option<DateTime<Utc>>,
        Option<i64>,
        Option<i64>,
        Option<i32>,
        Option<i64>,
    )> = sqlx::query_as(
        r#"SELECT 
            COALESCE(deep_fork_detected, FALSE),
            deep_fork_at,
            deep_fork_db_tip,
            deep_fork_chain_tip,
            deep_fork_depth,
            deep_fork_fork_point
        FROM sync_status WHERE id = 1"#,
    )
    .fetch_optional(&state.read_pool)
    .await
    .ok()
    .flatten();

    let (
        deep_fork_detected,
        deep_fork_at,
        deep_fork_db_tip,
        deep_fork_chain_tip,
        deep_fork_depth,
        deep_fork_fork_point,
    ) = deep_fork_row.unwrap_or((false, None, None, None, None, None));

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

    let rows = sqlx::query_as::<_, (chrono::NaiveDate, i64, i32)>(
        "SELECT date, COALESCE(avg_compact_target, 0), block_count FROM daily_block_stats WHERE avg_compact_target IS NOT NULL AND date < (SELECT MAX(date) FROM daily_block_stats) ORDER BY date ASC",
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|(date, compact_target, block_count)| {
            let difficulty = compact_to_difficulty(compact_target);
            let avg_block_time = 86400.0 / block_count as f64;
            let hash_rate = difficulty as f64 / avg_block_time;
            ChartDataPoint {
                date: date.format("%Y/%m/%d").to_string(),
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

    let rows = sqlx::query_as::<_, (chrono::NaiveDate, i64)>(
        "SELECT date, COALESCE(avg_compact_target, 0) FROM daily_block_stats WHERE avg_compact_target IS NOT NULL AND date < (SELECT MAX(date) FROM daily_block_stats) ORDER BY date ASC",
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|(date, compact_target)| {
            let difficulty = compact_to_difficulty(compact_target);
            ChartDataPoint {
                date: date.format("%Y/%m/%d").to_string(),
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

    let rows = sqlx::query_as::<_, (chrono::NaiveDate, f64)>(
        "SELECT date, avg_uncle_rate FROM daily_block_stats WHERE date < (SELECT MAX(date) FROM daily_block_stats) ORDER BY date ASC",
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|(date, uncle_rate)| ChartDataPoint {
            date: date.format("%Y/%m/%d").to_string(),
            value: format!("{:.6}", uncle_rate),
            value2: None,
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

    let total_blocks: (i64,) =
        sqlx::query_as("SELECT COALESCE(SUM(blocks_count), 0)::bigint FROM miner_statistics")
            .fetch_one(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    type MinerRow = (Vec<u8>, Option<Vec<u8>>, Option<i16>, Option<Vec<u8>>, i64);

    let rows = sqlx::query_as::<_, MinerRow>(
        r#"
        WITH miner_blocks AS (
            SELECT miner_lock_hash, SUM(blocks_count)::bigint as blocks_mined
            FROM miner_statistics
            GROUP BY miner_lock_hash
            ORDER BY blocks_mined DESC
            LIMIT 100
        ),
        miner_scripts AS (
            SELECT DISTINCT ON (lock_script_hash)
                lock_script_hash, lock_code_hash, lock_hash_type, lock_args
            FROM cells
            WHERE lock_script_hash IN (SELECT miner_lock_hash FROM miner_blocks)
        )
        SELECT mb.miner_lock_hash, ms.lock_code_hash, ms.lock_hash_type, ms.lock_args, mb.blocks_mined
        FROM miner_blocks mb
        LEFT JOIN miner_scripts ms ON mb.miner_lock_hash = ms.lock_script_hash
        ORDER BY mb.blocks_mined DESC
        "#,
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let total = total_blocks.0 as f64;
    let network = &state.ckb_network;
    let data: Vec<MinerDistributionDataPoint> = rows
        .into_iter()
        .map(
            |(hash, lock_code_hash, lock_hash_type, lock_args, blocks_mined)| {
                let percentage = if total > 0.0 {
                    (blocks_mined as f64 / total) * 100.0
                } else {
                    0.0
                };
                let address = lock_code_hash
                    .as_ref()
                    .and_then(|code_hash| {
                        let hash_type = lock_hash_type.unwrap_or(0);
                        let args = lock_args.as_deref().unwrap_or(&[]);
                        script_to_address(code_hash, hash_type, args, network).ok()
                    })
                    .unwrap_or_else(|| format!("0x{}", hex::encode(&hash)));
                MinerDistributionDataPoint {
                    address,
                    miner_name: None,
                    blocks_mined,
                    percentage: format!("{:.4}", percentage),
                }
            },
        )
        .collect();

    let response = MinerDistributionResponse {
        data,
        title: "Miner Address Distribution".to_string(),
        total_blocks: total_blocks.0,
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

    let rows = sqlx::query_as::<_, (chrono::NaiveDate, String, String, String)>(
        r#"
        SELECT date, CAST(total_issuance AS TEXT), CAST(total_deposit AS TEXT), COALESCE(cumulative_burnt, '0')
        FROM dao_daily_snapshots
        WHERE total_issuance != 0
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<StackedAreaDataPoint> = rows
        .into_iter()
        .map(
            |(date, total_issuance_str, locked_capacity, cumulative_burnt_str)| {
                let total_issuance: u128 = total_issuance_str.parse().unwrap_or(0);
                let locked: u128 = locked_capacity.parse().unwrap_or(0);
                let secondary_burnt: u128 = cumulative_burnt_str.parse().unwrap_or(0);
                let total_burnt = GENESIS_BURNT + secondary_burnt;
                let circulating = total_issuance.saturating_sub(total_burnt);
                let liquid = circulating.saturating_sub(locked);

                let mut values = std::collections::HashMap::new();
                values.insert(
                    "circulating".to_string(),
                    shannon_to_ckb(&liquid.to_string()),
                );
                values.insert("locked".to_string(), shannon_to_ckb(&locked.to_string()));
                values.insert(
                    "burnt".to_string(),
                    shannon_to_ckb(&total_burnt.to_string()),
                );

                StackedAreaDataPoint {
                    date: date.format("%Y/%m/%d").to_string(),
                    values,
                }
            },
        )
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

    // Use cumulative secondary issuance values from snapshots (same data source as /dao page)
    // This ensures the chart matches the pie chart on the DAO page
    let rows = sqlx::query_as::<
        _,
        (
            chrono::NaiveDate,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(
        r#"
        SELECT 
            date,
            cumulative_mining_reward,
            cumulative_deposit_compensation,
            cumulative_burnt
        FROM dao_daily_snapshots
        WHERE cumulative_burnt IS NOT NULL 
          AND cumulative_mining_reward IS NOT NULL
          AND cumulative_deposit_compensation IS NOT NULL
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<StackedAreaDataPoint> = rows
        .into_iter()
        .filter_map(|(date, mining_str, compensation_str, burnt_str)| {
            let mining: f64 = mining_str?.parse().ok()?;
            let compensation: f64 = compensation_str?.parse().ok()?;
            let burnt: f64 = burnt_str?.parse().ok()?;

            let total = mining + compensation + burnt;
            if total <= 0.0 {
                return None;
            }

            // Calculate percentages from actual cumulative values
            let mining_pct = mining / total * 100.0;
            let compensation_pct = compensation / total * 100.0;
            let burnt_pct = burnt / total * 100.0;

            let mut values = std::collections::HashMap::new();
            values.insert("burnt".to_string(), format!("{:.2}", burnt_pct));
            values.insert("mining".to_string(), format!("{:.2}", mining_pct));
            values.insert(
                "compensation".to_string(),
                format!("{:.2}", compensation_pct),
            );

            Some(StackedAreaDataPoint {
                date: date.format("%Y/%m/%d").to_string(),
                values,
            })
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
