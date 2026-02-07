use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::response::{
    decode_cursor, encode_cursor, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::AppState;

// CKB constants
const SHANNONS_PER_CKB: f64 = 100_000_000.0;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/dao/deposits", get(list_deposits))
        .route("/dao/deposits/{lock_hash}", get(get_deposits_by_address))
        .route("/dao/summary/{lock_hash}", get(get_address_dao_summary))
        .route("/dao/statistics", get(get_statistics))
        .route("/dao/calculator", get(calculate_compensation))
        .route("/dao/charts/total-deposit", get(get_total_deposit_chart))
        .route("/dao/charts/daily-deposit", get(get_daily_deposit_chart))
        .route(
            "/dao/charts/circulation-ratio",
            get(get_circulation_ratio_chart),
        )
}

// ============================================
// Request/Response Types
// ============================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListDepositsParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    status: Option<i32>, // 0=active, 1=withdrawing, 2=withdrawn
}

fn default_limit() -> i64 {
    20
}

// Response types matching frontend interfaces
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoStatisticsResponse {
    pub total_deposited: String,
    pub total_deposited_ckb: String,
    pub total_depositors: i64,
    pub active_deposits: i64,
    pub total_compensation_paid: String,
    pub total_compensation_paid_ckb: String,
    pub unclaimed_compensation: String,
    pub unclaimed_compensation_ckb: String,
    pub average_deposit_days: String,
    pub estimated_apc: String,
    pub mining_reward: String,
    pub mining_reward_ckb: String,
    pub deposit_compensation: String,
    pub deposit_compensation_ckb: String,
    pub burnt: String,
    pub burnt_ckb: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoDepositResponse {
    pub tx_hash: String,
    pub output_index: i32,
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub lock_code_hash: Option<String>,
    pub capacity: String,
    pub deposit_block_number: i64,
    pub deposit_timestamp: String,
    pub status: String,
    pub withdraw_request_block: Option<i64>,
    pub withdraw_request_timestamp: Option<String>,
    pub withdraw_block: Option<i64>,
    pub withdraw_timestamp: Option<String>,
    pub compensation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressDaoSummaryResponse {
    pub has_dao_activity: bool,
    pub active_deposits_count: i64,
    pub pending_withdrawals_count: i64,
    pub completed_withdrawals_count: i64,
    pub total_locked_capacity: String,
    pub total_locked_ckb: String,
    pub unclaimed_compensation: String,
    pub unclaimed_compensation_ckb: String,
    pub total_compensation_earned: String,
    pub total_compensation_earned_ckb: String,
    pub estimated_apc: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoCalculatorResponse {
    pub capacity: String,
    pub capacity_ckb: String,
    pub deposit_block: i64,
    pub withdraw_block: i64,
    pub estimated_compensation: String,
    pub estimated_compensation_ckb: String,
    pub total_withdrawable: String,
    pub total_withdrawable_ckb: String,
    pub apc: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartDataPoint {
    pub date: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value2: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartResponse {
    pub data: Vec<ChartDataPoint>,
}

// ============================================
// ClickHouse Row Types
// ============================================

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct DaoDepositRow {
    tx_hash: Vec<u8>,
    output_index: u16,
    lock_script_hash: Vec<u8>,
    capacity: u64,
    deposit_block_number: u64,
    deposit_timestamp: i64, // DateTime64(3) as milliseconds
    status: u8,
    withdraw_request_block: u64,
    withdraw_request_timestamp: i64,
    withdraw_block: u64,
    withdraw_timestamp: i64,
    compensation: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct DaoDepositCountRow {
    count: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct AddressDaoSummaryRow {
    active_count: u64,
    pending_count: u64,
    completed_count: u64,
    total_locked: u64,
    unclaimed_comp: u64,
    total_comp_earned: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct BlockArRow {
    ar: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct ChartRow {
    date: String,
    value: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct ChartRow2 {
    date: String,
    value1: u64,
    value2: u64,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct ApcRow {
    estimated_apc: String,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct LatestBlockRow {
    number: u64,
    ar: u64,
}

// ============================================
// Helper Functions
// ============================================

fn format_shannons(shannons: u64) -> String {
    shannons.to_string()
}

fn format_ckb(shannons: u64) -> String {
    let ckb = shannons as f64 / SHANNONS_PER_CKB;
    format!("{:.8}", ckb)
}

fn status_to_string(status: u8) -> String {
    match status {
        0 => "deposited".to_string(),
        1 => "withdrawing".to_string(),
        2 => "withdrawn".to_string(),
        _ => "unknown".to_string(),
    }
}

fn timestamp_to_iso(ts_millis: i64) -> Option<String> {
    if ts_millis == 0 {
        None
    } else {
        // Convert milliseconds to ISO 8601 string
        let secs = ts_millis / 1000;
        let nsecs = ((ts_millis % 1000) * 1_000_000) as u32;
        chrono::DateTime::from_timestamp(secs, nsecs).map(|dt| dt.to_rfc3339())
    }
}

fn parse_lock_hash(
    lock_hash: &str,
) -> Result<Vec<u8>, (axum::http::StatusCode, axum::Json<ApiError>)> {
    let hash = lock_hash.trim_start_matches("0x");
    hex::decode(hash).map_err(|_| ApiError::bad_request("Invalid lock hash format"))
}

// ============================================
// Route Handlers
// ============================================

async fn get_statistics(State(state): State<Arc<AppState>>) -> ApiResult<DaoStatisticsResponse> {
    let stats_query = "SELECT \
        sum(capacity) as total_deposited, \
        uniqExact(lock_script_hash) as total_depositors, \
        countIf(status = 0) as active_deposits, \
        sumIf(compensation, status = 2) as total_compensation_paid, \
        sumIf(compensation, status IN (0, 1)) as unclaimed_compensation \
        FROM dao_deposits FINAL";

    #[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
    struct ComputedStatsRow {
        total_deposited: u64,
        total_depositors: u64,
        active_deposits: u64,
        total_compensation_paid: u64,
        unclaimed_compensation: u64,
    }

    let row: Option<ComputedStatsRow> = state
        .pool
        .query_one(stats_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query DAO statistics: {}", e)))?;

    match row {
        Some(r) if r.total_deposited > 0 => {
            let response = DaoStatisticsResponse {
                total_deposited: format_shannons(r.total_deposited),
                total_deposited_ckb: format_ckb(r.total_deposited),
                total_depositors: r.total_depositors as i64,
                active_deposits: r.active_deposits as i64,
                total_compensation_paid: format_shannons(r.total_compensation_paid),
                total_compensation_paid_ckb: format_ckb(r.total_compensation_paid),
                unclaimed_compensation: format_shannons(r.unclaimed_compensation),
                unclaimed_compensation_ckb: format_ckb(r.unclaimed_compensation),
                average_deposit_days: "0.0".to_string(),
                estimated_apc: "2.87".to_string(),
                mining_reward: "0".to_string(),
                mining_reward_ckb: "0.00000000".to_string(),
                deposit_compensation: "0".to_string(),
                deposit_compensation_ckb: "0.00000000".to_string(),
                burnt: "0".to_string(),
                burnt_ckb: "0.00000000".to_string(),
            };
            ok(response)
        }
        _ => ok(DaoStatisticsResponse {
            total_deposited: "0".to_string(),
            total_deposited_ckb: "0.00000000".to_string(),
            total_depositors: 0,
            active_deposits: 0,
            total_compensation_paid: "0".to_string(),
            total_compensation_paid_ckb: "0.00000000".to_string(),
            unclaimed_compensation: "0".to_string(),
            unclaimed_compensation_ckb: "0.00000000".to_string(),
            average_deposit_days: "0.0".to_string(),
            estimated_apc: "0.00".to_string(),
            mining_reward: "0".to_string(),
            mining_reward_ckb: "0.00000000".to_string(),
            deposit_compensation: "0".to_string(),
            deposit_compensation_ckb: "0.00000000".to_string(),
            burnt: "0".to_string(),
            burnt_ckb: "0.00000000".to_string(),
        }),
    }
}

async fn list_deposits(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListDepositsParams>,
) -> ApiResult<CursorPaginatedResponse<DaoDepositResponse>> {
    let limit = params.limit.min(100);

    // Build cursor filter
    let cursor_filter = match params.cursor.as_ref().and_then(|c| decode_cursor(c)) {
        Some((block_num, output_idx)) => format!(
            "AND (deposit_block_number < {} OR (deposit_block_number = {} AND output_index < {}))",
            block_num, block_num, output_idx
        ),
        None => String::new(),
    };

    // Build status filter
    let status_filter = match params.status {
        Some(s) => format!("AND status = {}", s),
        None => String::new(),
    };

    // Count total deposits matching status filter
    let count_query = format!(
        "SELECT count() as count FROM dao_deposits FINAL WHERE 1=1 {}",
        status_filter
    );
    let count_row: Option<DaoDepositCountRow> = state
        .pool
        .query_one(&count_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to count deposits: {}", e)))?;
    let total = count_row.map(|r| r.count as i64).unwrap_or(0);

    // Query deposits
    let query = format!(
        "SELECT \
            tx_hash, \
            output_index, \
            lock_script_hash, \
            capacity, \
            deposit_block_number, \
            toInt64(deposit_timestamp) as deposit_timestamp, \
            status, \
            withdraw_request_block, \
            toInt64(withdraw_request_timestamp) as withdraw_request_timestamp, \
            withdraw_block, \
            toInt64(withdraw_timestamp) as withdraw_timestamp, \
            compensation \
        FROM dao_deposits FINAL \
        WHERE 1=1 {} {} \
        ORDER BY deposit_block_number DESC, output_index DESC \
        LIMIT {}",
        status_filter,
        cursor_filter,
        limit + 1
    );

    let rows: Vec<DaoDepositRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query deposits: {}", e)))?;

    let has_more = rows.len() as i64 > limit;
    let data: Vec<DaoDepositResponse> = rows
        .into_iter()
        .take(limit as usize)
        .map(|r| {
            let lock_hash_hex = format!("0x{}", hex::encode(&r.lock_script_hash));

            DaoDepositResponse {
                tx_hash: format!("0x{}", hex::encode(&r.tx_hash)),
                output_index: r.output_index as i32,
                lock_script_hash: lock_hash_hex,
                address: None,
                lock_code_hash: None,
                capacity: format_shannons(r.capacity),
                deposit_block_number: r.deposit_block_number as i64,
                deposit_timestamp: timestamp_to_iso(r.deposit_timestamp).unwrap_or_default(),
                status: status_to_string(r.status),
                withdraw_request_block: if r.withdraw_request_block > 0 {
                    Some(r.withdraw_request_block as i64)
                } else {
                    None
                },
                withdraw_request_timestamp: timestamp_to_iso(r.withdraw_request_timestamp),
                withdraw_block: if r.withdraw_block > 0 {
                    Some(r.withdraw_block as i64)
                } else {
                    None
                },
                withdraw_timestamp: timestamp_to_iso(r.withdraw_timestamp),
                compensation: if r.compensation > 0 {
                    Some(format_shannons(r.compensation))
                } else {
                    None
                },
            }
        })
        .collect();

    let next_cursor = if has_more {
        data.last()
            .map(|d| encode_cursor(d.deposit_block_number, d.output_index))
    } else {
        None
    };

    let response = CursorPaginatedResponse::new(data, total, limit, next_cursor);
    ok(response)
}

async fn get_deposits_by_address(
    State(state): State<Arc<AppState>>,
    Path(lock_hash): Path<String>,
    Query(params): Query<ListDepositsParams>,
) -> ApiResult<CursorPaginatedResponse<DaoDepositResponse>> {
    let lock_hash_bytes = parse_lock_hash(&lock_hash)?;
    let limit = params.limit.min(100);

    // Build cursor filter
    let cursor_filter = match params.cursor.as_ref().and_then(|c| decode_cursor(c)) {
        Some((block_num, output_idx)) => format!(
            "AND (deposit_block_number < {} OR (deposit_block_number = {} AND output_index < {}))",
            block_num, block_num, output_idx
        ),
        None => String::new(),
    };

    // Build status filter
    let status_filter = match params.status {
        Some(s) => format!("AND status = {}", s),
        None => String::new(),
    };

    let lock_hash_hex = hex::encode(&lock_hash_bytes);

    // Count total for this address
    let count_query = format!(
        "SELECT count() as count FROM dao_deposits FINAL WHERE lock_script_hash = unhex('{}') {} ",
        lock_hash_hex, status_filter
    );
    let count_row: Option<DaoDepositCountRow> = state
        .pool
        .query_one(&count_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to count deposits: {}", e)))?;
    let total = count_row.map(|r| r.count as i64).unwrap_or(0);

    // Query deposits for address
    let query = format!(
        "SELECT \
            tx_hash, \
            output_index, \
            lock_script_hash, \
            capacity, \
            deposit_block_number, \
            toInt64(deposit_timestamp) as deposit_timestamp, \
            status, \
            withdraw_request_block, \
            toInt64(withdraw_request_timestamp) as withdraw_request_timestamp, \
            withdraw_block, \
            toInt64(withdraw_timestamp) as withdraw_timestamp, \
            compensation \
        FROM dao_deposits FINAL \
        WHERE lock_script_hash = unhex('{}') {} {} \
        ORDER BY deposit_block_number DESC, output_index DESC \
        LIMIT {}",
        lock_hash_hex,
        status_filter,
        cursor_filter,
        limit + 1
    );

    let rows: Vec<DaoDepositRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query deposits: {}", e)))?;

    let has_more = rows.len() as i64 > limit;
    let data: Vec<DaoDepositResponse> = rows
        .into_iter()
        .take(limit as usize)
        .map(|r| {
            let lock_hash_hex = format!("0x{}", hex::encode(&r.lock_script_hash));

            DaoDepositResponse {
                tx_hash: format!("0x{}", hex::encode(&r.tx_hash)),
                output_index: r.output_index as i32,
                lock_script_hash: lock_hash_hex,
                address: None,
                lock_code_hash: None,
                capacity: format_shannons(r.capacity),
                deposit_block_number: r.deposit_block_number as i64,
                deposit_timestamp: timestamp_to_iso(r.deposit_timestamp).unwrap_or_default(),
                status: status_to_string(r.status),
                withdraw_request_block: if r.withdraw_request_block > 0 {
                    Some(r.withdraw_request_block as i64)
                } else {
                    None
                },
                withdraw_request_timestamp: timestamp_to_iso(r.withdraw_request_timestamp),
                withdraw_block: if r.withdraw_block > 0 {
                    Some(r.withdraw_block as i64)
                } else {
                    None
                },
                withdraw_timestamp: timestamp_to_iso(r.withdraw_timestamp),
                compensation: if r.compensation > 0 {
                    Some(format_shannons(r.compensation))
                } else {
                    None
                },
            }
        })
        .collect();

    let next_cursor = if has_more {
        data.last()
            .map(|d| encode_cursor(d.deposit_block_number, d.output_index))
    } else {
        None
    };

    let response = CursorPaginatedResponse::new(data, total, limit, next_cursor);
    ok(response)
}

async fn get_address_dao_summary(
    State(state): State<Arc<AppState>>,
    Path(lock_hash): Path<String>,
) -> ApiResult<AddressDaoSummaryResponse> {
    let lock_hash_bytes = parse_lock_hash(&lock_hash)?;
    let lock_hash_hex = hex::encode(&lock_hash_bytes);

    // Aggregate stats for this address
    let query = format!(
        "SELECT \
            countIf(status = 0) as active_count, \
            countIf(status = 1) as pending_count, \
            countIf(status = 2) as completed_count, \
            sumIf(capacity, status IN (0, 1)) as total_locked, \
            sumIf(compensation, status IN (0, 1)) as unclaimed_comp, \
            sumIf(compensation, status = 2) as total_comp_earned \
        FROM dao_deposits FINAL \
        WHERE lock_script_hash = unhex('{}')",
        lock_hash_hex
    );

    let row: Option<AddressDaoSummaryRow> =
        state.pool.query_one(&query).await.map_err(|e| {
            ApiError::internal(format!("Failed to query address DAO summary: {}", e))
        })?;

    match row {
        Some(r) => {
            let has_activity = r.active_count > 0 || r.pending_count > 0 || r.completed_count > 0;

            let apc_query = "SELECT estimated_apc FROM dao_statistics FINAL WHERE id = 1 LIMIT 1";
            let apc_row: Option<ApcRow> = state.pool.query_one(apc_query).await.ok().flatten();
            let estimated_apc = apc_row
                .map(|r| r.estimated_apc)
                .unwrap_or_else(|| "0.00".to_string());

            ok(AddressDaoSummaryResponse {
                has_dao_activity: has_activity,
                active_deposits_count: r.active_count as i64,
                pending_withdrawals_count: r.pending_count as i64,
                completed_withdrawals_count: r.completed_count as i64,
                total_locked_capacity: format_shannons(r.total_locked),
                total_locked_ckb: format_ckb(r.total_locked),
                unclaimed_compensation: format_shannons(r.unclaimed_comp),
                unclaimed_compensation_ckb: format_ckb(r.unclaimed_comp),
                total_compensation_earned: format_shannons(r.total_comp_earned),
                total_compensation_earned_ckb: format_ckb(r.total_comp_earned),
                estimated_apc,
            })
        }
        None => ok(AddressDaoSummaryResponse {
            has_dao_activity: false,
            active_deposits_count: 0,
            pending_withdrawals_count: 0,
            completed_withdrawals_count: 0,
            total_locked_capacity: "0".to_string(),
            total_locked_ckb: "0.00000000".to_string(),
            unclaimed_compensation: "0".to_string(),
            unclaimed_compensation_ckb: "0.00000000".to_string(),
            total_compensation_earned: "0".to_string(),
            total_compensation_earned_ckb: "0.00000000".to_string(),
            estimated_apc: "0.00".to_string(),
        }),
    }
}

async fn calculate_compensation(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<DaoCalculatorResponse> {
    // Parse parameters
    let capacity_str = params
        .get("capacity")
        .ok_or_else(|| ApiError::bad_request("Missing 'capacity' parameter"))?;
    let deposit_block: i64 = params
        .get("depositBlock")
        .ok_or_else(|| ApiError::bad_request("Missing 'depositBlock' parameter"))?
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid 'depositBlock' format"))?;
    let withdraw_block: Option<i64> = params.get("withdrawBlock").and_then(|s| s.parse().ok());

    // Parse capacity (can be in CKB or shannons)
    let capacity_shannons: u64 = if capacity_str.contains('.') {
        // Assume CKB format
        let ckb: f64 = capacity_str
            .parse()
            .map_err(|_| ApiError::bad_request("Invalid 'capacity' format"))?;
        (ckb * SHANNONS_PER_CKB) as u64
    } else {
        capacity_str
            .parse()
            .map_err(|_| ApiError::bad_request("Invalid 'capacity' format"))?
    };

    // DAO occupied capacity is 102 CKB
    const DAO_OCCUPIED_CAPACITY: u64 = 102_00000000;
    if capacity_shannons <= DAO_OCCUPIED_CAPACITY {
        return Err(ApiError::bad_request(
            "Capacity must be greater than 102 CKB (DAO minimum)",
        ));
    }
    let free_capacity = capacity_shannons - DAO_OCCUPIED_CAPACITY;

    // Get AR at deposit block
    let deposit_ar_query = format!(
        "SELECT reinterpretAsUInt64(substring(dao, 9, 8)) as ar \
        FROM blocks_all \
        WHERE number = {} \
        LIMIT 1",
        deposit_block
    );
    let deposit_ar_row: Option<BlockArRow> = state
        .pool
        .query_one(&deposit_ar_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query deposit block AR: {}", e)))?;

    let deposit_ar = deposit_ar_row
        .map(|r| r.ar)
        .ok_or_else(|| ApiError::bad_request("Deposit block not found"))?;

    let (actual_withdraw_block, withdraw_ar) = if let Some(wb) = withdraw_block {
        let withdraw_ar_query = format!(
            "SELECT reinterpretAsUInt64(substring(dao, 9, 8)) as ar \
            FROM blocks_all \
            WHERE number = {} \
            LIMIT 1",
            wb
        );
        let withdraw_ar_row: Option<BlockArRow> = state
            .pool
            .query_one(&withdraw_ar_query)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to query withdraw block AR: {}", e)))?;

        let ar = withdraw_ar_row
            .map(|r| r.ar)
            .ok_or_else(|| ApiError::bad_request("Withdraw block not found"))?;
        (wb, ar)
    } else {
        let latest_query = "SELECT number, reinterpretAsUInt64(substring(dao, 9, 8)) as ar \
            FROM blocks_all \
            ORDER BY number DESC \
            LIMIT 1";
        let latest_row: Option<LatestBlockRow> = state
            .pool
            .query_one(latest_query)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to query latest block: {}", e)))?;

        let row = latest_row.ok_or_else(|| ApiError::internal("No blocks found"))?;
        (row.number as i64, row.ar)
    };

    // Calculate compensation: free_capacity * (ar_withdraw / ar_deposit) - free_capacity
    // Using integer math to avoid precision issues
    let compensation = if withdraw_ar > deposit_ar {
        // compensation = free_capacity * (withdraw_ar - deposit_ar) / deposit_ar
        let diff = withdraw_ar - deposit_ar;
        (free_capacity as u128 * diff as u128 / deposit_ar as u128) as u64
    } else {
        0
    };

    let total_withdrawable = capacity_shannons + compensation;

    // Calculate APC (approximate)
    // APC = (withdraw_ar / deposit_ar - 1) * 365 / deposit_days * 100
    let deposit_days = (actual_withdraw_block - deposit_block) as f64 / 8640.0; // ~8640 blocks per day
    let apc = if deposit_days > 0.0 && deposit_ar > 0 {
        let growth = (withdraw_ar as f64 / deposit_ar as f64) - 1.0;
        growth * 365.0 / deposit_days * 100.0
    } else {
        0.0
    };

    ok(DaoCalculatorResponse {
        capacity: format_shannons(capacity_shannons),
        capacity_ckb: format_ckb(capacity_shannons),
        deposit_block,
        withdraw_block: actual_withdraw_block,
        estimated_compensation: format_shannons(compensation),
        estimated_compensation_ckb: format_ckb(compensation),
        total_withdrawable: format_shannons(total_withdrawable),
        total_withdrawable_ckb: format_ckb(total_withdrawable),
        apc: format!("{:.2}", apc),
    })
}

async fn get_total_deposit_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    // Query daily total deposit over time
    let query = "SELECT \
        formatDateTime(deposit_timestamp, '%Y-%m-%d') as date, \
        sum(capacity) as value \
        FROM dao_deposits FINAL \
        WHERE status IN (0, 1) \
        GROUP BY date \
        ORDER BY date \
        LIMIT 365";

    let rows: Vec<ChartRow> =
        state.pool.query_all(query).await.map_err(|e| {
            ApiError::internal(format!("Failed to query total deposit chart: {}", e))
        })?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|r| ChartDataPoint {
            date: r.date,
            value: format_ckb(r.value),
            value2: None,
        })
        .collect();

    ok(ChartResponse { data })
}

async fn get_daily_deposit_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    // Query daily new deposits
    let query = "SELECT \
        formatDateTime(deposit_timestamp, '%Y-%m-%d') as date, \
        sum(capacity) as value \
        FROM dao_deposits FINAL \
        GROUP BY date \
        ORDER BY date DESC \
        LIMIT 30";

    let rows: Vec<ChartRow> =
        state.pool.query_all(query).await.map_err(|e| {
            ApiError::internal(format!("Failed to query daily deposit chart: {}", e))
        })?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .rev() // Reverse to chronological order
        .map(|r| ChartDataPoint {
            date: r.date,
            value: format_ckb(r.value),
            value2: None,
        })
        .collect();

    ok(ChartResponse { data })
}

async fn get_circulation_ratio_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    // Query DAO deposit vs circulating supply ratio over time
    // This is a simplified version - would need proper supply calculation
    let query = "SELECT \
        formatDateTime(deposit_timestamp, '%Y-%m-%d') as date, \
        sum(capacity) as value1, \
        0 as value2 \
        FROM dao_deposits FINAL \
        WHERE status IN (0, 1) \
        GROUP BY date \
        ORDER BY date \
        LIMIT 365";

    let rows: Vec<ChartRow2> = state.pool.query_all(query).await.map_err(|e| {
        ApiError::internal(format!("Failed to query circulation ratio chart: {}", e))
    })?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|r| ChartDataPoint {
            date: r.date,
            value: format_ckb(r.value1),
            value2: Some(format_ckb(r.value2)),
        })
        .collect();

    ok(ChartResponse { data })
}
