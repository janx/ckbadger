use axum::{extract::State, routing::get, Router};
use serde::Serialize;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::rpc::{parse_hex_u64, CkbRpcClient};
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
        .route("/charts/total-supply", get(get_total_supply_chart))
        .route("/charts/nominal-apc", get(get_nominal_apc_chart))
        .route(
            "/charts/secondary-issuance",
            get(get_secondary_issuance_chart),
        )
        .route("/charts/inflation-rate", get(get_inflation_rate_chart))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatsResponse {
    pub latest_block: u64,
    pub avg_block_time: String,
    pub hash_rate: String,
    pub difficulty: String,
    pub epoch: String,
    pub tps: String,
    pub estimated_epoch_time: String,
    pub transactions_per_minute: String,
    pub transactions_per_day: String,
    pub sync_status: SyncStatusResponse,
    pub deep_fork_status: DeepForkStatusResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusResponse {
    pub is_syncing: bool,
    pub synced_block: u64,
    pub tip_block: u64,
    pub progress: f64,
    pub estimated_time: Option<String>,
    pub ema_blocks_per_second: Option<f64>,
    pub sync_mode: String,
    pub started_at: Option<i64>,
    pub elapsed_time: Option<String>,
    pub total_time: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepForkStatusResponse {
    pub detected: bool,
    pub detected_at: Option<String>,
    pub depth: Option<i64>,
    pub db_tip: Option<u64>,
    pub chain_tip: Option<u64>,
    pub fork_point: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxStatsResponse {
    pub current_hour: u64,
    pub current_day: u64,
    pub hourly_data: Vec<TxStatsDataPoint>,
    pub daily_data: Vec<TxStatsDataPoint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxStatsDataPoint {
    pub label: String,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentBlockResponse {
    pub number: u64,
    pub hash: String,
    pub timestamp: i64,
    pub transactions_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartDataPoint {
    pub date: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartResponse {
    pub data: Vec<ChartDataPoint>,
    pub title: String,
    pub y_axis_label: String,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct TipBlockRow {
    tip_block: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct RecentBlockRow {
    number: u64,
    hash: String,
    timestamp: i64,
    transactions_count: u32,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct DailyCountRow {
    date: String,
    count: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct HourlyTxRow {
    hour_label: String,
    tx_count: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct DailyTxRow {
    day_label: String,
    tx_count: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct DailyAvgBlockTimeRow {
    date: String,
    avg_time: f64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct DailyHashRateRow {
    date: String,
    hash_rate: f64,
}

fn format_hash_rate(hash_rate: f64) -> String {
    if hash_rate >= 1e18 {
        format!("{:.2} EH/s", hash_rate / 1e18)
    } else if hash_rate >= 1e15 {
        format!("{:.2} PH/s", hash_rate / 1e15)
    } else if hash_rate >= 1e12 {
        format!("{:.2} TH/s", hash_rate / 1e12)
    } else if hash_rate >= 1e9 {
        format!("{:.2} GH/s", hash_rate / 1e9)
    } else if hash_rate >= 1e6 {
        format!("{:.2} MH/s", hash_rate / 1e6)
    } else if hash_rate >= 1e3 {
        format!("{:.2} KH/s", hash_rate / 1e3)
    } else {
        format!("{:.2} H/s", hash_rate)
    }
}

fn format_difficulty(difficulty: u64) -> String {
    let diff = difficulty as f64;
    if diff >= 1e18 {
        format!("{:.2} E", diff / 1e18)
    } else if diff >= 1e15 {
        format!("{:.2} P", diff / 1e15)
    } else if diff >= 1e12 {
        format!("{:.2} T", diff / 1e12)
    } else if diff >= 1e9 {
        format!("{:.2} G", diff / 1e9)
    } else if diff >= 1e6 {
        format!("{:.2} M", diff / 1e6)
    } else if diff >= 1e3 {
        format!("{:.2} K", diff / 1e3)
    } else {
        format!("{}", difficulty)
    }
}

fn compact_target_to_difficulty(compact_target: u64) -> u64 {
    let exponent = (compact_target >> 24) & 0xff;
    let mantissa = compact_target & 0x00ffffff;

    if exponent <= 3 {
        mantissa >> (8 * (3 - exponent))
    } else {
        let shift = 8 * (exponent - 3);
        if shift >= 64 {
            return u64::MAX;
        }
        mantissa.saturating_mul(1u64 << shift)
    }
}

fn difficulty_to_hash_rate(difficulty: u64) -> f64 {
    (difficulty as f64) * 2.0 / 1.4
}

fn parse_epoch(epoch_hex: &str) -> (u64, u64, u64) {
    let epoch = parse_hex_u64(epoch_hex).unwrap_or(0);
    let length = (epoch >> 40) & 0xFFFF;
    let index = (epoch >> 24) & 0xFFFF;
    let number = epoch & 0xFFFFFF;
    (number, index, length)
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

async fn get_network_stats(State(state): State<Arc<AppState>>) -> ApiResult<NetworkStatsResponse> {
    let rpc = CkbRpcClient::new(&state.ckb_rpc_url);

    let tip_header = rpc
        .get_tip_header()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get tip header: {}", e)))?;

    let tip_number = parse_hex_u64(&tip_header.number).unwrap_or(0);
    let compact_target = parse_hex_u64(&tip_header.compact_target).unwrap_or(0);
    let difficulty = compact_target_to_difficulty(compact_target);
    let hash_rate = difficulty_to_hash_rate(difficulty);

    let (epoch_number, epoch_index, epoch_length) = parse_epoch(&tip_header.epoch);
    let epoch_str = format!("{}({}/{})", epoch_number, epoch_index, epoch_length);

    let blocks_remaining = epoch_length.saturating_sub(epoch_index);
    let estimated_epoch_seconds = blocks_remaining * 8;
    let estimated_epoch_time = format_duration(estimated_epoch_seconds);

    let db_tip_query = "SELECT max(number) as tip_block FROM canonical_blocks FINAL";
    let db_tip_row: Option<TipBlockRow> = state.pool.query_one(db_tip_query).await.ok().flatten();
    let db_tip = db_tip_row.map(|r| r.tip_block).unwrap_or(0);

    let is_syncing = tip_number > db_tip + 10;
    let progress = if tip_number > 0 {
        (db_tip as f64 / tip_number as f64) * 100.0
    } else {
        100.0
    };

    ok(NetworkStatsResponse {
        latest_block: tip_number,
        avg_block_time: "~8s".to_string(),
        hash_rate: format_hash_rate(hash_rate),
        difficulty: format_difficulty(difficulty),
        epoch: epoch_str,
        tps: "0.24".to_string(),
        estimated_epoch_time,
        transactions_per_minute: "14".to_string(),
        transactions_per_day: "20000".to_string(),
        sync_status: SyncStatusResponse {
            is_syncing,
            synced_block: db_tip,
            tip_block: tip_number,
            progress,
            estimated_time: None,
            ema_blocks_per_second: None,
            sync_mode: if is_syncing {
                "bulk".to_string()
            } else {
                "live".to_string()
            },
            started_at: None,
            elapsed_time: None,
            total_time: None,
        },
        deep_fork_status: DeepForkStatusResponse {
            detected: false,
            detected_at: None,
            depth: None,
            db_tip: None,
            chain_tip: None,
            fork_point: None,
        },
    })
}

async fn get_tx_stats(State(state): State<Arc<AppState>>) -> ApiResult<TxStatsResponse> {
    let hourly_query = r#"
        SELECT 
            formatDateTime(fromUnixTimestamp64Milli(b.timestamp), '%H:%M') as hour_label,
            count() as tx_count
        FROM transactions_all t
        INNER JOIN canonical_blocks c ON t.block_number = c.number AND t.block_hash = c.block_hash
        INNER JOIN blocks_all b ON c.number = b.number AND c.block_hash = b.hash
        WHERE b.timestamp >= (toUnixTimestamp64Milli(now64(3)) - 3600000)
        GROUP BY toStartOfFiveMinutes(fromUnixTimestamp64Milli(b.timestamp)), hour_label
        ORDER BY toStartOfFiveMinutes(fromUnixTimestamp64Milli(b.timestamp))
    "#;

    let hourly_rows: Vec<HourlyTxRow> =
        state.pool.query_all(hourly_query).await.unwrap_or_default();

    let daily_query = r#"
        SELECT 
            formatDateTime(fromUnixTimestamp64Milli(b.timestamp), '%m/%d') as day_label,
            count() as tx_count
        FROM transactions_all t
        INNER JOIN canonical_blocks c ON t.block_number = c.number AND t.block_hash = c.block_hash
        INNER JOIN blocks_all b ON c.number = b.number AND c.block_hash = b.hash
        WHERE b.timestamp >= (toUnixTimestamp64Milli(now64(3)) - 86400000)
        GROUP BY toStartOfHour(fromUnixTimestamp64Milli(b.timestamp)), day_label
        ORDER BY toStartOfHour(fromUnixTimestamp64Milli(b.timestamp))
    "#;

    let daily_rows: Vec<DailyTxRow> = state.pool.query_all(daily_query).await.unwrap_or_default();

    let current_hour: u64 = hourly_rows.iter().map(|r| r.tx_count).sum();
    let current_day: u64 = daily_rows.iter().map(|r| r.tx_count).sum();

    let hourly_data: Vec<TxStatsDataPoint> = hourly_rows
        .into_iter()
        .map(|r| TxStatsDataPoint {
            label: r.hour_label,
            value: r.tx_count,
        })
        .collect();

    let daily_data: Vec<TxStatsDataPoint> = daily_rows
        .into_iter()
        .map(|r| TxStatsDataPoint {
            label: r.day_label,
            value: r.tx_count,
        })
        .collect();

    ok(TxStatsResponse {
        current_hour,
        current_day,
        hourly_data,
        daily_data,
    })
}

async fn get_recent_blocks(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Vec<RecentBlockResponse>> {
    let query = r#"
        SELECT 
            b.number as number,
            hex(b.hash) as hash,
            b.timestamp as timestamp,
            b.transactions_count as transactions_count
        FROM blocks_all b
        INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash
        ORDER BY b.number DESC
        LIMIT 10
    "#;

    let rows: Vec<RecentBlockRow> = state
        .pool
        .query_all(query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query recent blocks: {}", e)))?;

    let blocks: Vec<RecentBlockResponse> = rows
        .into_iter()
        .map(|r| RecentBlockResponse {
            number: r.number,
            hash: format!("0x{}", r.hash.to_lowercase()),
            timestamp: r.timestamp,
            transactions_count: r.transactions_count,
        })
        .collect();

    ok(blocks)
}

async fn get_transaction_count_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Vec<ChartDataPoint>> {
    let query = r#"
        SELECT 
            toString(toDate(fromUnixTimestamp64Milli(b.timestamp))) as date,
            count() as count
        FROM transactions_all t
        INNER JOIN canonical_blocks c ON t.block_number = c.number AND t.block_hash = c.block_hash
        INNER JOIN blocks_all b ON c.number = b.number AND c.block_hash = b.hash
        GROUP BY date
        ORDER BY date DESC
        LIMIT 30
    "#;

    let rows: Vec<DailyCountRow> = state
        .pool
        .query_all(query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query transaction count: {}", e)))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|r| ChartDataPoint {
            date: r.date,
            value: r.count.to_string(),
        })
        .collect();

    ok(data)
}

async fn get_cell_count_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Vec<ChartDataPoint>> {
    let query = r#"
        SELECT 
            toString(toDate(fromUnixTimestamp64Milli(b.timestamp))) as date,
            count() as count
        FROM cell_outputs_all co
        INNER JOIN canonical_blocks c ON co.block_number = c.number AND co.block_hash = c.block_hash
        INNER JOIN blocks_all b ON c.number = b.number AND c.block_hash = b.hash
        GROUP BY date
        ORDER BY date DESC
        LIMIT 30
    "#;

    let rows: Vec<DailyCountRow> = state
        .pool
        .query_all(query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query cell count: {}", e)))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|r| ChartDataPoint {
            date: r.date,
            value: r.count.to_string(),
        })
        .collect();

    ok(data)
}

async fn get_knowledge_size_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Knowledge Size".to_string(),
        y_axis_label: "Bytes".to_string(),
    })
}

async fn get_block_time_distribution_chart(
    State(_state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Block Time Distribution".to_string(),
        y_axis_label: "Count".to_string(),
    })
}

async fn get_epoch_time_distribution_chart(
    State(_state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Epoch Time Distribution".to_string(),
        y_axis_label: "Count".to_string(),
    })
}

async fn get_epoch_time_length_chart(
    State(_state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Epoch Length".to_string(),
        y_axis_label: "Blocks".to_string(),
    })
}

async fn get_average_block_time_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let query = r#"
        SELECT 
            toString(toDate(fromUnixTimestamp64Milli(b2.timestamp))) as date,
            avg(b2.timestamp - b1.timestamp) / 1000.0 as avg_time
        FROM blocks_all b1
        INNER JOIN canonical_blocks c1 ON b1.number = c1.number AND b1.hash = c1.block_hash
        INNER JOIN blocks_all b2 ON b2.number = b1.number + 1
        INNER JOIN canonical_blocks c2 ON b2.number = c2.number AND b2.hash = c2.block_hash
        GROUP BY date
        ORDER BY date DESC
        LIMIT 30
    "#;

    let rows: Vec<DailyAvgBlockTimeRow> = state.pool.query_all(query).await.unwrap_or_default();

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .rev()
        .map(|r| ChartDataPoint {
            date: r.date,
            value: format!("{:.2}", r.avg_time),
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Average Block Time".to_string(),
        y_axis_label: "Seconds".to_string(),
    })
}

async fn get_hash_rate_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let query = r#"
        SELECT 
            toString(toDate(fromUnixTimestamp64Milli(b.timestamp))) as date,
            avg(b.difficulty) * 2.0 / 1.4 as hash_rate
        FROM blocks_all b
        INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash
        GROUP BY date
        ORDER BY date DESC
        LIMIT 30
    "#;

    let rows: Vec<DailyHashRateRow> = state.pool.query_all(query).await.unwrap_or_default();

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .rev()
        .map(|r| ChartDataPoint {
            date: r.date,
            value: format!("{:.0}", r.hash_rate),
        })
        .collect();

    ok(ChartResponse {
        data,
        title: "Hash Rate".to_string(),
        y_axis_label: "H/s".to_string(),
    })
}

async fn get_difficulty_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Difficulty".to_string(),
        y_axis_label: "Difficulty".to_string(),
    })
}

async fn get_uncle_rate_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Uncle Rate".to_string(),
        y_axis_label: "Percent".to_string(),
    })
}

async fn get_miner_address_distribution_chart(
    State(_state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Miner Address Distribution".to_string(),
        y_axis_label: "Blocks".to_string(),
    })
}

async fn get_total_supply_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Total Supply".to_string(),
        y_axis_label: "CKB".to_string(),
    })
}

async fn get_nominal_apc_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Nominal APC".to_string(),
        y_axis_label: "Percent".to_string(),
    })
}

async fn get_secondary_issuance_chart(
    State(_state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Secondary Issuance".to_string(),
        y_axis_label: "CKB".to_string(),
    })
}

async fn get_inflation_rate_chart(State(_state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    ok(ChartResponse {
        data: vec![],
        title: "Inflation Rate".to_string(),
        y_axis_label: "Percent".to_string(),
    })
}
