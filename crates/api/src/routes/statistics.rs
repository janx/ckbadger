#![allow(clippy::type_complexity)]

use axum::{extract::State, routing::get, Router};
use chrono::{DateTime, Utc};
use ckbadger_common::dao::GENESIS_BURNT;
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::cache::{CacheKeys, CacheTtl};
use crate::clickhouse::ClickHouseClient;
use crate::response::{ok, ApiError, ApiResult};
use crate::utils::script_to_address;
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

    let stats = fetch_network_stats_clickhouse(&state.clickhouse, &state).await?;

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

    get_tx_stats_clickhouse(&state.clickhouse, &state, cache_key).await
}

async fn get_tx_stats_clickhouse(
    ch_client: &ClickHouseClient,
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<TxStatsResponse> {
    #[derive(Row, Deserialize)]
    struct TimestampRow {
        timestamp: u32,
    }

    let latest_ts: Option<TimestampRow> = ch_client
        .client()
        .query("SELECT timestamp FROM blocks ORDER BY number DESC LIMIT 1")
        .fetch_optional()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_time = latest_ts
        .and_then(|row| DateTime::from_timestamp(row.timestamp as i64, 0))
        .unwrap_or_else(Utc::now);
    let reference_date = reference_time.date_naive();

    #[derive(Row, Deserialize)]
    struct HourlyStatsRow {
        hour: String,
        transactions_count: u32,
    }

    #[derive(Row, Deserialize)]
    struct DailyStatsRow {
        date: String,
        transactions_count: u32,
    }

    let hourly_rows: Vec<HourlyStatsRow> = ch_client
        .client()
        .query(
            r#"
            SELECT toString(hour) as hour, transactions_count
            FROM hourly_statistics
            WHERE hour > subtractHours(toDateTime(?1), 24) AND hour <= toDateTime(?1)
            ORDER BY hour DESC
            LIMIT 24
            "#,
        )
        .bind(reference_time.timestamp())
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let daily_rows: Vec<DailyStatsRow> = ch_client
        .client()
        .query(
            r#"
            SELECT toString(date) as date, transactions_count
            FROM daily_statistics
            WHERE date > subtractDays(toDate(?1), 14) AND date <= toDate(?1)
            ORDER BY date DESC
            LIMIT 14
            "#,
        )
        .bind(reference_date.to_string())
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let txs_this_hour: i64 = hourly_rows
        .first()
        .map(|r| r.transactions_count as i64)
        .unwrap_or(0);
    let txs_in_24_hours: i64 = hourly_rows
        .iter()
        .map(|r| r.transactions_count as i64)
        .sum();

    let hourly_data: Vec<TxStatsDataPoint> = hourly_rows
        .into_iter()
        .rev()
        .map(|row| TxStatsDataPoint {
            label: row.hour.split(' ').next().unwrap_or("").to_string(),
            value: row.transactions_count as i64,
        })
        .collect();

    let daily_data: Vec<TxStatsDataPoint> = daily_rows
        .into_iter()
        .rev()
        .map(|row| TxStatsDataPoint {
            label: row.date,
            value: row.transactions_count as i64,
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
        .set(cache_key, &response, std::time::Duration::from_secs(60))
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

    get_recent_blocks_clickhouse(&state.clickhouse, &state, cache_key).await
}

async fn get_recent_blocks_clickhouse(
    ch_client: &ClickHouseClient,
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<RecentBlocksResponse> {
    #[derive(Row, Deserialize)]
    struct TimestampRow {
        timestamp: u32,
    }

    let latest_ts: Option<TimestampRow> = ch_client
        .client()
        .query("SELECT timestamp FROM blocks ORDER BY number DESC LIMIT 1")
        .fetch_optional()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let reference_time = latest_ts
        .and_then(|row| DateTime::from_timestamp(row.timestamp as i64, 0))
        .unwrap_or_else(Utc::now);

    let cutoff_timestamp = (reference_time.timestamp() - 86400) as u32;

    #[derive(Row, Deserialize)]
    struct BlockRow {
        timestamp: u32,
        transactions_count: u32,
    }

    let query = format!(
        "SELECT timestamp, transactions_count FROM blocks WHERE timestamp > {} ORDER BY number ASC",
        cutoff_timestamp
    );

    let rows: Vec<BlockRow> = ch_client
        .client()
        .query(&query)
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let blocks: Vec<RecentBlockItem> = rows
        .into_iter()
        .map(|row| RecentBlockItem {
            timestamp: (row.timestamp as i64) * 1000,
            transactions_count: row.transactions_count as i32,
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
    get_transaction_count_chart_impl(&state).await
}

async fn get_transaction_count_chart_impl(state: &Arc<AppState>) -> ApiResult<ChartResponse> {
    #[derive(Row, Deserialize)]
    struct DailyStatsRow {
        date: String,
        transactions_count: u32,
    }

    let rows: Vec<DailyStatsRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT toString(date) as date, transactions_count
            FROM daily_statistics
            ORDER BY date ASC
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|row| ChartDataPoint {
            date: row.date,
            value: row.transactions_count.to_string(),
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

async fn get_cell_count_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    get_cell_count_chart_impl(&state).await
}

async fn get_cell_count_chart_impl(state: &Arc<AppState>) -> ApiResult<ChartResponse> {
    #[derive(Row, Deserialize)]
    struct CellCountRow {
        date: String,
        total_live_cells: u64,
    }

    let rows: Vec<CellCountRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT toString(date) as date, total_live_cells
            FROM daily_statistics
            WHERE total_live_cells IS NOT NULL
            ORDER BY date ASC
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|row| ChartDataPoint {
            date: row.date,
            value: row.total_live_cells.to_string(),
            value2: None,
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Live Cell Count".to_string(),
        y_axis_label: "Cells".to_string(),
        y2_axis_label: None,
    })
}

async fn get_knowledge_size_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    get_knowledge_size_chart_impl(&state).await
}

async fn get_knowledge_size_chart_impl(state: &Arc<AppState>) -> ApiResult<ChartResponse> {
    #[derive(Row, Deserialize)]
    struct KnowledgeSizeRow {
        date: String,
        total_data_size: u64,
    }

    let rows: Vec<KnowledgeSizeRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT toString(date) as date, total_data_size
            FROM daily_statistics
            WHERE total_data_size IS NOT NULL
            ORDER BY date ASC
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|row| ChartDataPoint {
            date: row.date,
            value: row.total_data_size.to_string(),
            value2: None,
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Common Knowledge Size".to_string(),
        y_axis_label: "Bytes".to_string(),
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

    get_block_time_distribution_chart_impl(&state, cache_key).await
}

async fn get_block_time_distribution_chart_impl(
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<ChartResponse> {
    #[derive(Row, Deserialize)]
    struct BlockTimeDistRow {
        bucket_seconds: u32,
        block_count: u64,
    }

    let rows: Vec<BlockTimeDistRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT bucket_seconds, block_count
            FROM block_time_distribution
            ORDER BY bucket_seconds
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|row| ChartDataPoint {
            date: format!("{}s", row.bucket_seconds),
            value: row.block_count.to_string(),
            value2: None,
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Block Time Distribution".to_string(),
        y_axis_label: "Blocks".to_string(),
        y2_axis_label: None,
    };

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(3600))
        .await;

    ok(response)
}

async fn get_epoch_time_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:epoch-time-distribution";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    get_epoch_time_distribution_chart_impl(&state, cache_key).await
}

async fn get_epoch_time_distribution_chart_impl(
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<ChartResponse> {
    #[derive(Row, Deserialize)]
    struct EpochTimeDistRow {
        bucket_minutes: u32,
        epoch_count: u64,
    }

    let rows: Vec<EpochTimeDistRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT bucket_minutes, epoch_count
            FROM epoch_time_distribution
            ORDER BY bucket_minutes
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|row| {
            let hours = row.bucket_minutes / 60;
            let mins = row.bucket_minutes % 60;
            ChartDataPoint {
                date: format!("{}:{:02}", hours, mins),
                value: row.epoch_count.to_string(),
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

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(3600))
        .await;

    ok(response)
}

async fn get_epoch_time_length_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:epoch-time-length";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    get_epoch_time_length_chart_impl(&state, cache_key).await
}

async fn get_epoch_time_length_chart_impl(
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<ChartResponse> {
    #[derive(Row, Deserialize)]
    struct EpochTimeLengthRow {
        epoch_number: u64,
        duration_hours: f64,
        blocks_count: u32,
    }

    let rows: Vec<EpochTimeLengthRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT 
                epoch_number,
                (toUnixTimestamp(end_timestamp) - toUnixTimestamp(start_timestamp)) / 3600.0 as duration_hours,
                blocks_count
            FROM epoch_statistics
            WHERE end_timestamp IS NOT NULL
            ORDER BY epoch_number ASC
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|row| ChartDataPoint {
            date: row.epoch_number.to_string(),
            value: format!("{:.2}", row.duration_hours),
            value2: Some(row.blocks_count.to_string()),
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Epoch Time Length".to_string(),
        y_axis_label: "Hours".to_string(),
        y2_axis_label: Some("Blocks".to_string()),
    };

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(3600))
        .await;

    ok(response)
}

async fn get_average_block_time_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:average-block-time";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    get_average_block_time_chart_impl(&state, cache_key).await
}

async fn get_average_block_time_chart_impl(
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<ChartResponse> {
    #[derive(Row, Deserialize)]
    struct AvgBlockTimeRow {
        date: String,
        avg_block_time_ms: u32,
    }

    let rows: Vec<AvgBlockTimeRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT toString(date) as date, avg_block_time_ms
            FROM daily_statistics
            WHERE avg_block_time_ms IS NOT NULL
            ORDER BY date ASC
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|row| ChartDataPoint {
            date: row.date,
            value: format!("{:.2}", row.avg_block_time_ms as f64 / 1000.0),
            value2: None,
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Average Block Time".to_string(),
        y_axis_label: "Seconds".to_string(),
        y2_axis_label: None,
    };

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(3600))
        .await;

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

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    } else {
        let days = seconds / 86400;
        let hours = (seconds % 86400) / 3600;
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
        }
    }
}

async fn fetch_network_stats_clickhouse(
    ch_client: &ClickHouseClient,
    state: &AppState,
) -> Result<
    NetworkStats,
    (
        axum::http::StatusCode,
        axum::Json<crate::response::ApiError>,
    ),
> {
    #[derive(Row, Deserialize)]
    struct LatestBlockRow {
        number: u64,
        epoch_number: u64,
        epoch_index: u32,
        epoch_length: u32,
        compact_target: u64,
        timestamp: u32,
    }

    let latest_query = "SELECT number, epoch_number, epoch_index, epoch_length, compact_target, timestamp FROM blocks ORDER BY number DESC LIMIT 1";
    let latest: Option<LatestBlockRow> = ch_client
        .client()
        .query(latest_query)
        .fetch_optional()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let (latest_block, epoch_number, epoch_index, epoch_length, compact_target, latest_timestamp) =
        if let Some(row) = latest {
            (
                row.number as i64,
                row.epoch_number as i64,
                row.epoch_index as i32,
                row.epoch_length as i32,
                row.compact_target as i64,
                DateTime::from_timestamp(row.timestamp as i64, 0).unwrap_or_else(Utc::now),
            )
        } else {
            (0, 0, 0, 1800, 0, Utc::now())
        };

    let today = latest_timestamp.date_naive();
    let yesterday = today - chrono::Duration::days(1);

    #[derive(Row, Deserialize)]
    struct EpochStatsRow {
        start_timestamp: Option<u32>,
        end_timestamp: Option<u32>,
        blocks_count: u32,
    }

    #[derive(Row, Deserialize)]
    struct TimestampRow {
        timestamp: u32,
    }

    #[derive(Row, Deserialize)]
    struct TxCountRow {
        total: u64,
    }

    #[derive(Row, Deserialize)]
    struct SyncStatusRow {
        tip_block_number: u64,
        sync_started_at: Option<u32>,
        sync_started_block: u64,
        deep_fork_detected: bool,
        deep_fork_at: Option<u32>,
        deep_fork_db_tip: Option<u64>,
        deep_fork_chain_tip: Option<u64>,
        deep_fork_depth: Option<i32>,
        deep_fork_fork_point: Option<u64>,
    }

    let (epoch_avg_result, recent_blocks_result, tx_count_result, tip_block_result) = tokio::join!(
        async {
            ch_client
                .client()
                .query(
                    r#"
                    SELECT start_timestamp, end_timestamp, blocks_count
                    FROM epoch_statistics
                    WHERE epoch_number = ?1
                    "#,
                )
                .bind(epoch_number)
                .fetch_optional::<EpochStatsRow>()
                .await
        },
        async {
            let query = format!(
                "SELECT timestamp FROM blocks WHERE number >= {} - 1 AND number <= {} ORDER BY number ASC",
                latest_block, latest_block
            );
            ch_client
                .client()
                .query(&query)
                .fetch_all::<TimestampRow>()
                .await
        },
        async {
            ch_client
                .client()
                .query(
                    r#"
                    SELECT COALESCE(SUM(transactions_count), 0) as total
                    FROM daily_statistics
                    WHERE date >= toDate(?1)
                    "#,
                )
                .bind(yesterday.to_string())
                .fetch_optional::<TxCountRow>()
                .await
        },
        fetch_tip_block_from_ckb(&state.ckb_rpc_url)
    );

    // Calculate epoch avg time from epoch statistics
    let epoch_avg_time = epoch_avg_result
        .ok()
        .flatten()
        .and_then(|row| {
            if row.blocks_count > 1 {
                if let (Some(s), Some(e)) = (row.start_timestamp, row.end_timestamp) {
                    let duration = (e as i64 - s as i64) as f64;
                    Some(duration / (row.blocks_count - 1) as f64)
                } else if let Some(s) = row.start_timestamp {
                    let duration = (latest_timestamp.timestamp() - s as i64) as f64;
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
                let ts0 = DateTime::from_timestamp(blocks[0].timestamp as i64, 0)?;
                let ts1 = DateTime::from_timestamp(blocks[1].timestamp as i64, 0)?;
                let duration = ts1.signed_duration_since(ts0).num_seconds() as f64;
                Some(duration.max(1.0))
            } else {
                None
            }
        })
        .unwrap_or(10.0);

    let tx_count_24h = tx_count_result
        .ok()
        .flatten()
        .map(|r| r.total as i64)
        .unwrap_or(0);

    let tip_block = tip_block_result.unwrap_or(latest_block as u64) as i64;

    let remaining_blocks = epoch_length - epoch_index;
    let estimated_epoch_seconds = (remaining_blocks as f64 * epoch_avg_time) as i64;

    let tps = tx_count_24h as f64 / 86400.0;
    let tx_per_minute = tps * 60.0;

    let sync_row: Option<SyncStatusRow> = ch_client
        .client()
        .query(
            r#"SELECT 
                tip_block_number, 
                sync_started_at, 
                COALESCE(sync_started_block, 0) as sync_started_block,
                COALESCE(deep_fork_detected, false) as deep_fork_detected,
                deep_fork_at,
                deep_fork_db_tip,
                deep_fork_chain_tip,
                deep_fork_depth,
                deep_fork_fork_point
            FROM sync_status WHERE id = 1"#,
        )
        .fetch_optional()
        .await
        .ok()
        .flatten();

    let (
        synced_block,
        sync_started_at_ts,
        sync_started_block,
        deep_fork_detected,
        deep_fork_at_ts,
        deep_fork_db_tip,
        deep_fork_chain_tip,
        deep_fork_depth,
        deep_fork_fork_point,
    ) = if let Some(row) = sync_row {
        (
            row.tip_block_number as i64,
            row.sync_started_at,
            row.sync_started_block as i64,
            row.deep_fork_detected,
            row.deep_fork_at,
            row.deep_fork_db_tip.map(|v| v as i64),
            row.deep_fork_chain_tip.map(|v| v as i64),
            row.deep_fork_depth,
            row.deep_fork_fork_point.map(|v| v as i64),
        )
    } else {
        (latest_block, None, 0, false, None, None, None, None, None)
    };

    let blocks_behind = tip_block - synced_block;
    let is_syncing = blocks_behind > 100;
    let progress = if tip_block > 0 {
        (synced_block as f64 / tip_block as f64 * 100.0).min(100.0)
    } else {
        0.0
    };

    let estimated_time = if is_syncing && blocks_behind > 0 {
        if let Some(started_at_ts) = sync_started_at_ts {
            if let Some(started_at) = DateTime::from_timestamp(started_at_ts as i64, 0) {
                let elapsed = Utc::now().signed_duration_since(started_at).num_seconds() as u64;
                let blocks_synced = (synced_block - sync_started_block).max(0) as u64;
                if elapsed > 0 && blocks_synced > 0 {
                    let rate = blocks_synced as f64 / elapsed as f64;
                    let seconds_remaining = (blocks_behind as f64 / rate) as u64;
                    Some(format_duration(seconds_remaining))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let sync_status = SyncStatus {
        is_syncing,
        synced_block,
        tip_block,
        progress,
        estimated_time,
        chart_data_may_be_incomplete: blocks_behind > 1000,
    };

    let deep_fork_status = DeepForkStatus {
        detected: deep_fork_detected,
        detected_at: deep_fork_at_ts.and_then(|ts| DateTime::from_timestamp(ts as i64, 0)),
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

    get_hash_rate_chart_impl(&state, cache_key).await
}

async fn get_hash_rate_chart_impl(
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<ChartResponse> {
    #[derive(Row, Deserialize)]
    struct HashRateRow {
        date: String,
        avg_compact_target: u64,
        block_count: u32,
    }

    let rows: Vec<HashRateRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT toString(date) as date, COALESCE(avg_compact_target, 0) as avg_compact_target, block_count
            FROM daily_block_stats
            WHERE avg_compact_target IS NOT NULL AND date < (SELECT MAX(date) FROM daily_block_stats)
            ORDER BY date ASC
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|row| {
            let difficulty = compact_to_difficulty(row.avg_compact_target as i64);
            let avg_block_time = 86400.0 / row.block_count as f64;
            let hash_rate = difficulty as f64 / avg_block_time;
            ChartDataPoint {
                date: row.date,
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

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(3600))
        .await;

    ok(response)
}

async fn get_difficulty_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:difficulty";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    get_difficulty_chart_impl(&state, cache_key).await
}

async fn get_difficulty_chart_impl(
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<ChartResponse> {
    #[derive(Row, Deserialize)]
    struct DifficultyRow {
        date: String,
        avg_compact_target: u64,
    }

    let rows: Vec<DifficultyRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT toString(date) as date, COALESCE(avg_compact_target, 0) as avg_compact_target
            FROM daily_block_stats
            WHERE avg_compact_target IS NOT NULL AND date < (SELECT MAX(date) FROM daily_block_stats)
            ORDER BY date ASC
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|row| {
            let difficulty = compact_to_difficulty(row.avg_compact_target as i64);
            ChartDataPoint {
                date: row.date,
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

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(3600))
        .await;

    ok(response)
}

async fn get_uncle_rate_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:uncle-rate";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    get_uncle_rate_chart_impl(&state, cache_key).await
}

async fn get_uncle_rate_chart_impl(
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<ChartResponse> {
    #[derive(Row, Deserialize)]
    struct UncleRateRow {
        date: String,
        avg_uncle_rate: f64,
    }

    let rows: Vec<UncleRateRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT toString(date) as date, avg_uncle_rate
            FROM daily_block_stats
            WHERE date < (SELECT MAX(date) FROM daily_block_stats)
            ORDER BY date ASC
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|row| ChartDataPoint {
            date: row.date,
            value: format!("{:.6}", row.avg_uncle_rate),
            value2: None,
        })
        .collect();

    let response = ChartResponse {
        data,
        title: "Uncle Rate".to_string(),
        y_axis_label: "Uncle Rate".to_string(),
        y2_axis_label: None,
    };

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(3600))
        .await;

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

    get_miner_address_distribution_chart_impl(&state, cache_key).await
}

async fn get_miner_address_distribution_chart_impl(
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<MinerDistributionResponse> {
    #[derive(Row, Deserialize)]
    struct TotalBlocksRow {
        total: u64,
    }

    let total_row: Option<TotalBlocksRow> = state
        .clickhouse
        .client()
        .query("SELECT COALESCE(SUM(blocks_count), 0) as total FROM miner_statistics")
        .fetch_optional()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let total_blocks = total_row.map(|r| r.total).unwrap_or(0);

    #[derive(Row, Deserialize)]
    struct MinerRow {
        miner_lock_hash: String,
        lock_code_hash: Option<String>,
        lock_hash_type: Option<i16>,
        lock_args: Option<String>,
        blocks_mined: u64,
    }

    let rows: Vec<MinerRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            WITH miner_blocks AS (
                SELECT miner_lock_hash, SUM(blocks_count) as blocks_mined
                FROM miner_statistics
                GROUP BY miner_lock_hash
                ORDER BY blocks_mined DESC
                LIMIT 100
            ),
            miner_scripts AS (
                SELECT DISTINCT ON (lock_script_hash)
                    lock_script_hash, hex_hash(lock_code_hash) as lock_code_hash, lock_hash_type, hex_hash(lock_args) as lock_args
                FROM cells
                WHERE lock_script_hash IN (SELECT miner_lock_hash FROM miner_blocks)
            )
            SELECT mb.miner_lock_hash, ms.lock_code_hash, ms.lock_hash_type, ms.lock_args, mb.blocks_mined
            FROM miner_blocks mb
            LEFT JOIN miner_scripts ms ON mb.miner_lock_hash = ms.lock_script_hash
            ORDER BY mb.blocks_mined DESC
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let total = total_blocks as f64;
    let network = &state.ckb_network;
    let data: Vec<MinerDistributionDataPoint> = rows
        .into_iter()
        .map(|row| {
            let percentage = if total > 0.0 {
                (row.blocks_mined as f64 / total) * 100.0
            } else {
                0.0
            };
            let address = row
                .lock_code_hash
                .as_ref()
                .and_then(|code_hash| {
                    let hash_type = row.lock_hash_type.unwrap_or(0);
                    let args = row.lock_args.as_deref().unwrap_or("");
                    let code_hash_bytes = hex::decode(code_hash).ok()?;
                    let args_bytes = hex::decode(args).ok()?;
                    script_to_address(&code_hash_bytes, hash_type, &args_bytes, network).ok()
                })
                .unwrap_or_else(|| format!("0x{}", row.miner_lock_hash));
            MinerDistributionDataPoint {
                address,
                miner_name: None,
                blocks_mined: row.blocks_mined as i64,
                percentage: format!("{:.4}", percentage),
            }
        })
        .collect();

    let response = MinerDistributionResponse {
        data,
        title: "Miner Address Distribution".to_string(),
        total_blocks: total_blocks as i64,
    };

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(3600))
        .await;

    ok(response)
}

fn shannon_to_ckb(shannon: &str) -> String {
    let num: u128 = shannon.parse().unwrap_or(0);
    let ckb = num / 100_000_000;
    let remainder = num % 100_000_000;
    if remainder == 0 {
        format!("{}", ckb)
    } else {
        format!("{}.{:08}", ckb, remainder)
            .trim_end_matches('0')
            .to_string()
    }
}

async fn get_total_supply_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    let cache_key = "chart:total-supply";
    if let Some(cached) = state.cache.get::<StackedAreaChartResponse>(cache_key).await {
        return ok(cached);
    }

    get_total_supply_chart_impl(&state, cache_key).await
}

async fn get_total_supply_chart_impl(
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<StackedAreaChartResponse> {
    #[derive(Row, Deserialize)]
    struct DaoSnapshotRow {
        date: String,
        total_issuance: String,
        total_deposit: String,
        cumulative_burnt: String,
    }

    let rows: Vec<DaoSnapshotRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT toString(date) as date, toString(total_issuance) as total_issuance, toString(total_deposit) as total_deposit, COALESCE(toString(cumulative_burnt), '0') as cumulative_burnt
            FROM dao_daily_snapshots
            WHERE total_issuance != 0
            ORDER BY date ASC
            "#,
        )
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<StackedAreaDataPoint> = rows
        .into_iter()
        .map(|row| {
            let total_issuance: u128 = row.total_issuance.parse().unwrap_or(0);
            let locked: u128 = row.total_deposit.parse().unwrap_or(0);
            let secondary_burnt: u128 = row.cumulative_burnt.parse().unwrap_or(0);
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
                date: row.date,
                values,
            }
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

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(3600))
        .await;

    ok(response)
}

async fn get_nominal_apc_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    get_nominal_apc_chart_impl().await
}

async fn get_nominal_apc_chart_impl() -> ApiResult<ChartResponse> {
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

    get_secondary_issuance_chart_impl(&state, cache_key).await
}

async fn get_secondary_issuance_chart_impl(
    state: &Arc<AppState>,
    cache_key: &str,
) -> ApiResult<StackedAreaChartResponse> {
    #[derive(Row, Deserialize)]
    struct SecondaryIssuanceRow {
        date: String,
        cumulative_mining_reward: Option<String>,
        cumulative_deposit_compensation: Option<String>,
        cumulative_burnt: Option<String>,
    }

    let rows: Vec<SecondaryIssuanceRow> = state
        .clickhouse
        .client()
        .query(
            r#"
            SELECT 
                toString(date) as date,
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
        .fetch_all()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<StackedAreaDataPoint> = rows
        .into_iter()
        .filter_map(|row| {
            let mining: f64 = row.cumulative_mining_reward?.parse().ok()?;
            let compensation: f64 = row.cumulative_deposit_compensation?.parse().ok()?;
            let burnt: f64 = row.cumulative_burnt?.parse().ok()?;

            let total = mining + compensation + burnt;
            if total <= 0.0 {
                return None;
            }

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
                date: row.date,
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

    state
        .cache
        .set(cache_key, &response, std::time::Duration::from_secs(3600))
        .await;

    ok(response)
}

async fn get_inflation_rate_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    get_inflation_rate_chart_impl().await
}

async fn get_inflation_rate_chart_impl() -> ApiResult<ChartResponse> {
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
