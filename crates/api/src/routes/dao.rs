use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_common::dao::{calculate_estimated_apc, GENESIS_BURNT};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::response::{
    decode_cursor_single, encode_cursor_single, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::utils::{script_to_address, shannon_to_ckb};
use crate::AppState;

const CHART_CACHE_TTL: Duration = Duration::from_secs(3600);
const MAX_CHART_POINTS: usize = 600;

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

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    status: Option<i16>,
    cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
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
    pub active_deposits_count: i32,
    pub pending_withdrawals_count: i32,
    pub completed_withdrawals_count: i32,
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
pub struct DaoStatisticsResponse {
    pub total_deposited: String,
    pub total_deposited_ckb: String,
    pub total_depositors: i32,
    pub active_deposits: i32,
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

#[derive(Debug, Deserialize)]
pub struct CalculatorParams {
    capacity: String,
    deposit_block: i64,
    withdraw_block: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculatorResponse {
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

async fn list_deposits(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<DaoDepositResponse>> {
    let limit = params.limit.clamp(1, 100);
    let cursor_id = params
        .cursor
        .as_ref()
        .and_then(|c| decode_cursor_single(c))
        .unwrap_or(i64::MAX);

    type DaoRow = (
        i64,
        Vec<u8>,
        i16,
        Vec<u8>,
        Option<Vec<u8>>,
        Option<i16>,
        Option<Vec<u8>>,
        String,
        i64,
        chrono::DateTime<chrono::Utc>,
        i16,
        Option<i64>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<i64>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
    );

    let (total, rows) = if let Some(status) = params.status {
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM dao_deposits WHERE status = $1")
            .bind(status)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let rows = sqlx::query_as::<_, DaoRow>(
            r#"
            SELECT d.id, d.tx_hash, d.output_index, d.lock_script_hash, 
                   c.lock_code_hash, c.lock_hash_type, c.lock_args,
                   CAST(d.capacity AS TEXT), d.deposit_block_number, d.deposit_timestamp,
                   d.status, d.withdraw_request_block, d.withdraw_request_timestamp, 
                   d.withdraw_block, d.withdraw_timestamp, CAST(d.compensation AS TEXT)
            FROM dao_deposits d
            LEFT JOIN cells c ON d.tx_hash = c.tx_hash AND d.output_index = c.output_index
            WHERE d.status = $1 AND d.id < $2
            ORDER BY d.id DESC
            LIMIT $3
            "#,
        )
        .bind(status)
        .bind(cursor_id)
        .bind(limit + 1)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

        (total.0, rows)
    } else {
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM dao_deposits")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        let rows = sqlx::query_as::<_, DaoRow>(
            r#"
            SELECT d.id, d.tx_hash, d.output_index, d.lock_script_hash,
                   c.lock_code_hash, c.lock_hash_type, c.lock_args,
                   CAST(d.capacity AS TEXT), d.deposit_block_number, d.deposit_timestamp,
                   d.status, d.withdraw_request_block, d.withdraw_request_timestamp, 
                   d.withdraw_block, d.withdraw_timestamp, CAST(d.compensation AS TEXT)
            FROM dao_deposits d
            LEFT JOIN cells c ON d.tx_hash = c.tx_hash AND d.output_index = c.output_index
            WHERE d.id < $1
            ORDER BY d.id DESC
            LIMIT $2
            "#,
        )
        .bind(cursor_id)
        .bind(limit + 1)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

        (total.0, rows)
    };

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(id, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _)| encode_cursor_single(*id))
    } else {
        None
    };

    let network = &state.ckb_network;
    let deposits: Vec<DaoDepositResponse> = rows
        .into_iter()
        .map(
            |(
                _id,
                tx_hash,
                output_index,
                lock_script_hash,
                lock_code_hash,
                lock_hash_type,
                lock_args,
                capacity,
                deposit_block_number,
                deposit_timestamp,
                status,
                withdraw_request_block,
                withdraw_request_timestamp,
                withdraw_block,
                withdraw_timestamp,
                compensation,
            )| {
                let address = lock_code_hash.as_ref().and_then(|code_hash| {
                    let hash_type = lock_hash_type.unwrap_or(0);
                    let args = lock_args.as_deref().unwrap_or(&[]);
                    script_to_address(code_hash, hash_type, args, network).ok()
                });

                DaoDepositResponse {
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    output_index: output_index as i32,
                    lock_script_hash: format!("0x{}", hex::encode(&lock_script_hash)),
                    address,
                    lock_code_hash: lock_code_hash.map(|h| format!("0x{}", hex::encode(&h))),
                    capacity,
                    deposit_block_number,
                    deposit_timestamp: deposit_timestamp.to_rfc3339(),
                    status: status_to_string(status),
                    withdraw_request_block,
                    withdraw_request_timestamp: withdraw_request_timestamp.map(|t| t.to_rfc3339()),
                    withdraw_block,
                    withdraw_timestamp: withdraw_timestamp.map(|t| t.to_rfc3339()),
                    compensation,
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::new(
        deposits,
        total,
        limit,
        next_cursor,
    ))
}

async fn get_deposits_by_address(
    State(state): State<Arc<AppState>>,
    Path(lock_hash): Path<String>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<DaoDepositResponse>> {
    type DaoRow = (
        i64,
        Vec<u8>,
        i16,
        Vec<u8>,
        Option<Vec<u8>>,
        Option<i16>,
        Option<Vec<u8>>,
        String,
        i64,
        chrono::DateTime<chrono::Utc>,
        i16,
        Option<i64>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<i64>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
    );

    let hash = hex::decode(lock_hash.strip_prefix("0x").unwrap_or(&lock_hash))
        .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?;

    let limit = params.limit.clamp(1, 100);
    let cursor_id = params
        .cursor
        .as_ref()
        .and_then(|c| decode_cursor_single(c))
        .unwrap_or(i64::MAX);

    let total: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM dao_deposits WHERE lock_script_hash = $1")
            .bind(&hash)
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let rows = sqlx::query_as::<_, DaoRow>(
        r#"
        SELECT d.id, d.tx_hash, d.output_index, d.lock_script_hash, 
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               CAST(d.capacity AS TEXT), d.deposit_block_number, d.deposit_timestamp,
               d.status, d.withdraw_request_block, d.withdraw_request_timestamp, 
               d.withdraw_block, d.withdraw_timestamp, CAST(d.compensation AS TEXT)
        FROM dao_deposits d
        LEFT JOIN cells c ON d.tx_hash = c.tx_hash AND d.output_index = c.output_index
        WHERE d.lock_script_hash = $1 AND d.id < $2
        ORDER BY d.id DESC
        LIMIT $3
        "#,
    )
    .bind(&hash)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(id, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _)| encode_cursor_single(*id))
    } else {
        None
    };

    let network = &state.ckb_network;
    let deposits: Vec<DaoDepositResponse> = rows
        .into_iter()
        .map(
            |(
                _id,
                tx_hash,
                output_index,
                lock_script_hash,
                lock_code_hash,
                lock_hash_type,
                lock_args,
                capacity,
                deposit_block_number,
                deposit_timestamp,
                status,
                withdraw_request_block,
                withdraw_request_timestamp,
                withdraw_block,
                withdraw_timestamp,
                compensation,
            )| {
                let address = lock_code_hash.as_ref().and_then(|code_hash| {
                    let hash_type = lock_hash_type.unwrap_or(0);
                    let args = lock_args.as_deref().unwrap_or(&[]);
                    script_to_address(code_hash, hash_type, args, network).ok()
                });

                DaoDepositResponse {
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    output_index: output_index as i32,
                    lock_script_hash: format!("0x{}", hex::encode(&lock_script_hash)),
                    address,
                    lock_code_hash: lock_code_hash.map(|h| format!("0x{}", hex::encode(&h))),
                    capacity,
                    deposit_block_number,
                    deposit_timestamp: deposit_timestamp.to_rfc3339(),
                    status: status_to_string(status),
                    withdraw_request_block,
                    withdraw_request_timestamp: withdraw_request_timestamp.map(|t| t.to_rfc3339()),
                    withdraw_block,
                    withdraw_timestamp: withdraw_timestamp.map(|t| t.to_rfc3339()),
                    compensation,
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::new(
        deposits,
        total.0,
        limit,
        next_cursor,
    ))
}

const DAO_OCCUPIED_CAPACITY: u128 = 102_00000000;

async fn get_address_dao_summary(
    State(state): State<Arc<AppState>>,
    Path(lock_hash): Path<String>,
) -> ApiResult<AddressDaoSummaryResponse> {
    let hash = hex::decode(lock_hash.strip_prefix("0x").unwrap_or(&lock_hash))
        .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?;

    let counts = sqlx::query_as::<_, (i64, i64, i64)>(
        r#"SELECT 
            COUNT(*) FILTER (WHERE status = 0) as active,
            COUNT(*) FILTER (WHERE status = 1) as pending,
            COUNT(*) FILTER (WHERE status = 2) as completed
        FROM dao_deposits WHERE lock_script_hash = $1"#,
    )
    .bind(&hash)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (active_count, pending_count, completed_count) = counts;

    if active_count == 0 && pending_count == 0 && completed_count == 0 {
        return ok(AddressDaoSummaryResponse {
            has_dao_activity: false,
            active_deposits_count: 0,
            pending_withdrawals_count: 0,
            completed_withdrawals_count: 0,
            total_locked_capacity: "0".to_string(),
            total_locked_ckb: "0".to_string(),
            unclaimed_compensation: "0".to_string(),
            unclaimed_compensation_ckb: "0".to_string(),
            total_compensation_earned: "0".to_string(),
            total_compensation_earned_ckb: "0".to_string(),
            estimated_apc: "".to_string(),
        });
    }

    let total_locked: (String,) = sqlx::query_as(
        "SELECT CAST(COALESCE(SUM(capacity), 0) AS TEXT) FROM dao_deposits WHERE lock_script_hash = $1 AND status IN (0, 1)",
    )
    .bind(&hash)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let total_comp_earned: (String,) = sqlx::query_as(
        "SELECT CAST(COALESCE(SUM(compensation), 0) AS TEXT) FROM dao_deposits WHERE lock_script_hash = $1 AND status = 2 AND compensation IS NOT NULL",
    )
    .bind(&hash)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let latest_block = sqlx::query_as::<_, (i64, Vec<u8>)>(
        "SELECT number, dao FROM blocks WHERE dao IS NOT NULL ORDER BY number DESC LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (latest_block_number, latest_ar, total_issuance) = match &latest_block {
        Some((num, dao)) => {
            let ar = extract_ar(dao).unwrap_or(1);
            let issuance = extract_total_issuance(dao).unwrap_or(0);
            (*num, ar, issuance)
        }
        None => (0, 1, 0),
    };

    let deposits_with_ar = sqlx::query_as::<_, (String, Vec<u8>)>(
        r#"SELECT CAST(d.capacity AS TEXT), b.dao
        FROM dao_deposits d
        JOIN blocks b ON d.deposit_block_number = b.number
        WHERE d.lock_script_hash = $1 AND d.status IN (0, 1) AND b.dao IS NOT NULL AND d.deposit_block_number <= $2"#,
    )
    .bind(&hash)
    .bind(latest_block_number)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut total_unclaimed: u128 = 0;
    for (capacity_str, deposit_dao) in &deposits_with_ar {
        let capacity: u128 = capacity_str.parse().unwrap_or(0);
        let free_capacity = capacity.saturating_sub(DAO_OCCUPIED_CAPACITY);
        if let Some(ar_deposit) = extract_ar(deposit_dao) {
            if ar_deposit > 0 && latest_ar > ar_deposit {
                let compensation = (free_capacity * latest_ar as u128 / ar_deposit as u128)
                    .saturating_sub(free_capacity);
                total_unclaimed += compensation;
            }
        }
    }

    let secondary_burnt: u128 = sqlx::query_as::<_, (String,)>(
        "SELECT COALESCE(cumulative_burnt, '0') FROM dao_statistics WHERE id = 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map(|(s,)| s.parse().unwrap_or(0))
    .unwrap_or(0);

    let apc = calculate_estimated_apc(total_issuance, secondary_burnt);
    let estimated_apc = if apc > 0.0 {
        format!("{:.2}", apc)
    } else {
        String::new()
    };

    ok(AddressDaoSummaryResponse {
        has_dao_activity: true,
        active_deposits_count: active_count as i32,
        pending_withdrawals_count: pending_count as i32,
        completed_withdrawals_count: completed_count as i32,
        total_locked_capacity: total_locked.0.clone(),
        total_locked_ckb: shannon_to_ckb(&total_locked.0),
        unclaimed_compensation: total_unclaimed.to_string(),
        unclaimed_compensation_ckb: shannon_to_ckb(&total_unclaimed.to_string()),
        total_compensation_earned: total_comp_earned.0.clone(),
        total_compensation_earned_ckb: shannon_to_ckb(&total_comp_earned.0),
        estimated_apc,
    })
}

async fn get_statistics(State(state): State<Arc<AppState>>) -> ApiResult<DaoStatisticsResponse> {
    let live_stats = sqlx::query_as::<_, (String, i64, i64)>(
        r#"SELECT 
            CAST(COALESCE(SUM(capacity), 0) AS TEXT) as total_deposited,
            COUNT(DISTINCT lock_script_hash) as total_depositors,
            COUNT(*) as active_deposits
        FROM dao_deposits WHERE status = 0"#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let compensation_stats = sqlx::query_as::<_, (String,)>(
        r#"SELECT CAST(COALESCE(SUM(compensation), 0) AS TEXT) 
           FROM dao_deposits WHERE status = 2 AND compensation IS NOT NULL"#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let latest_block = sqlx::query_as::<_, (i64, Vec<u8>)>(
        "SELECT number, dao FROM blocks WHERE dao IS NOT NULL ORDER BY number DESC LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (latest_block_number, latest_ar, total_issuance) = match &latest_block {
        Some((num, dao)) => {
            let ar = extract_ar(dao).unwrap_or(1);
            let issuance = extract_total_issuance(dao).unwrap_or(0);
            (*num, ar, issuance)
        }
        None => (0, 1, 0),
    };

    let avg_epochs: (Option<f64>,) = sqlx::query_as(
        r#"SELECT AVG((($1::bigint) - deposit_block_number)::float8 / 1800.0) 
        FROM dao_deposits 
        WHERE status = 0 AND deposit_block_number <= $1"#,
    )
    .bind(latest_block_number)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((None,));

    let deposits_with_ar = sqlx::query_as::<_, (String, Vec<u8>)>(
        r#"SELECT 
            CAST(d.capacity AS TEXT),
            b.dao
        FROM dao_deposits d
        JOIN blocks b ON d.deposit_block_number = b.number
        WHERE d.status = 0 AND b.dao IS NOT NULL AND d.deposit_block_number <= $1"#,
    )
    .bind(latest_block_number)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut total_unclaimed: u128 = 0;
    for (capacity_str, deposit_dao) in &deposits_with_ar {
        let capacity: u128 = capacity_str.parse().unwrap_or(0);
        let free_capacity = capacity.saturating_sub(DAO_OCCUPIED_CAPACITY);
        if let Some(ar_deposit) = extract_ar(deposit_dao) {
            if ar_deposit > 0 && latest_ar > ar_deposit {
                let compensation = (free_capacity * latest_ar as u128 / ar_deposit as u128)
                    .saturating_sub(free_capacity);
                total_unclaimed += compensation;
            }
        }
    }

    let secondary_burnt: u128 = sqlx::query_as::<_, (String,)>(
        "SELECT COALESCE(cumulative_burnt, '0') FROM dao_statistics WHERE id = 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map(|(s,)| s.parse().unwrap_or(0))
    .unwrap_or(0);

    let apc = calculate_estimated_apc(total_issuance, secondary_burnt);
    let estimated_apc = if apc > 0.0 {
        format!("{:.2}", apc)
    } else {
        String::new()
    };

    let cached_row = sqlx::query_as::<_, (String, String, String)>(
        r#"SELECT 
            COALESCE(cumulative_miner_secondary, '0'),
            COALESCE(cumulative_dao_compensation, '0'),
            COALESCE(cumulative_burnt, '0')
        FROM dao_statistics WHERE id = 1"#,
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (mining_reward, deposit_compensation, burnt) =
        cached_row.unwrap_or(("0".to_string(), "0".to_string(), "0".to_string()));

    let total_deposited = live_stats.0;
    let total_depositors = live_stats.1 as i32;
    let active_deposits = live_stats.2 as i32;
    let total_compensation_paid = compensation_stats.0;
    let avg_days = epochs_to_days(avg_epochs.0.unwrap_or(0.0));

    ok(DaoStatisticsResponse {
        total_deposited: total_deposited.clone(),
        total_deposited_ckb: shannon_to_ckb(&total_deposited),
        total_depositors,
        active_deposits,
        total_compensation_paid: total_compensation_paid.clone(),
        total_compensation_paid_ckb: shannon_to_ckb(&total_compensation_paid),
        unclaimed_compensation: total_unclaimed.to_string(),
        unclaimed_compensation_ckb: shannon_to_ckb(&total_unclaimed.to_string()),
        average_deposit_days: avg_days,
        estimated_apc,
        mining_reward: mining_reward.clone(),
        mining_reward_ckb: shannon_to_ckb(&mining_reward),
        deposit_compensation: deposit_compensation.clone(),
        deposit_compensation_ckb: shannon_to_ckb(&deposit_compensation),
        burnt: burnt.clone(),
        burnt_ckb: shannon_to_ckb(&burnt),
    })
}

fn extract_total_issuance(dao: &[u8]) -> Option<u64> {
    if dao.len() < 8 {
        return None;
    }
    let bytes: [u8; 8] = dao[0..8].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn epochs_to_days(epochs: f64) -> String {
    let days = epochs * 4.0 / 24.0;
    if days >= 1000.0 {
        format!("{:.1}K days+", days / 1000.0)
    } else if days < 1.0 && days > 0.0 {
        format!("{:.1} days", days)
    } else {
        format!("{:.0} days", days)
    }
}

async fn calculate_compensation(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CalculatorParams>,
) -> ApiResult<CalculatorResponse> {
    let capacity: u128 = params
        .capacity
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid capacity"))?;

    let latest_block: (i64,) = sqlx::query_as("SELECT COALESCE(MAX(number), 0) FROM blocks")
        .fetch_one(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let withdraw_block = params.withdraw_block.unwrap_or(latest_block.0);

    let deposit_dao: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT dao FROM blocks WHERE number = $1")
            .bind(params.deposit_block)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let withdraw_dao: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT dao FROM blocks WHERE number = $1")
            .bind(withdraw_block)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    let (ar_deposit, ar_withdraw) = match (deposit_dao, withdraw_dao) {
        (Some((d,)), Some((w,))) => {
            let ar_d = extract_ar(&d).unwrap_or(1);
            let ar_w = extract_ar(&w).unwrap_or(1);
            (ar_d, ar_w)
        }
        _ => (1u64, 1u64),
    };

    let occupied = 102_00000000u128;
    let free = capacity.saturating_sub(occupied);
    let compensation = if ar_deposit > 0 {
        (free * ar_withdraw as u128 / ar_deposit as u128).saturating_sub(free)
    } else {
        0
    };

    let total = capacity + compensation;

    let blocks_held = (withdraw_block - params.deposit_block).max(0) as f64;
    let years = blocks_held / (365.25 * 24.0 * 60.0 * 60.0 / 8.0);
    let apc = if years > 0.0 && free > 0 {
        (compensation as f64 / free as f64 / years) * 100.0
    } else {
        0.0
    };

    ok(CalculatorResponse {
        capacity: capacity.to_string(),
        capacity_ckb: shannon_to_ckb(&capacity.to_string()),
        deposit_block: params.deposit_block,
        withdraw_block,
        estimated_compensation: compensation.to_string(),
        estimated_compensation_ckb: shannon_to_ckb(&compensation.to_string()),
        total_withdrawable: total.to_string(),
        total_withdrawable_ckb: shannon_to_ckb(&total.to_string()),
        apc: format!("{:.2}%", apc),
    })
}

fn status_to_string(status: i16) -> String {
    match status {
        0 => "deposited".to_string(),
        1 => "withdrawing".to_string(),
        2 => "withdrawn".to_string(),
        _ => "unknown".to_string(),
    }
}

fn extract_ar(dao: &[u8]) -> Option<u64> {
    if dao.len() < 16 {
        return None;
    }
    let bytes: [u8; 8] = dao[8..16].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

#[allow(clippy::needless_range_loop)]
fn downsample_chart_data(data: Vec<ChartDataPoint>, target_points: usize) -> Vec<ChartDataPoint> {
    if data.len() <= target_points || target_points < 3 {
        return data;
    }

    let mut result = Vec::with_capacity(target_points);
    result.push(data[0].clone());

    let bucket_size = (data.len() - 2) as f64 / (target_points - 2) as f64;

    for i in 0..(target_points - 2) {
        let bucket_start = ((i as f64) * bucket_size).floor() as usize + 1;
        let bucket_end = (((i + 1) as f64) * bucket_size).floor() as usize + 1;
        let bucket_end = bucket_end.min(data.len() - 1);

        let prev_y: f64 = result
            .last()
            .and_then(|p| p.value.parse().ok())
            .unwrap_or(0.0);

        let next_bucket_start = bucket_end;
        let next_bucket_end = (((i + 2) as f64) * bucket_size).floor() as usize + 1;
        let next_bucket_end = next_bucket_end.min(data.len());

        let (next_avg_x, next_avg_y) = if next_bucket_start < next_bucket_end {
            let sum: f64 = data[next_bucket_start..next_bucket_end]
                .iter()
                .filter_map(|p| p.value.parse::<f64>().ok())
                .sum();
            let count = next_bucket_end - next_bucket_start;
            (
                (next_bucket_start + next_bucket_end) as f64 / 2.0,
                sum / count as f64,
            )
        } else {
            ((data.len() - 1) as f64, 0.0)
        };

        let mut max_area = -1.0f64;
        let mut max_idx = bucket_start;
        let prev_x = i as f64;

        for j in bucket_start..bucket_end {
            let curr_y: f64 = data[j].value.parse().unwrap_or(0.0);
            let area = ((prev_x - next_avg_x) * (curr_y - prev_y)
                - (prev_x - j as f64) * (next_avg_y - prev_y))
                .abs();
            if area > max_area {
                max_area = area;
                max_idx = j;
            }
        }

        result.push(data[max_idx].clone());
    }

    result.push(data[data.len() - 1].clone());
    result
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

async fn get_total_deposit_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:dao-total-deposit";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let rows = sqlx::query_as::<_, (chrono::NaiveDate, String, i32)>(
        r#"
        SELECT date, CAST(total_deposit AS TEXT), depositors_count
        FROM dao_daily_snapshots
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(|(date, total_deposit, depositors_count)| ChartDataPoint {
            date: date.format("%Y/%m/%d").to_string(),
            value: shannon_to_ckb(&total_deposit),
            value2: Some(depositors_count.to_string()),
        })
        .collect();

    let data = downsample_chart_data(data, MAX_CHART_POINTS);

    let response = ChartResponse {
        data,
        title: "Total Deposit".to_string(),
        y_axis_label: "CKB".to_string(),
        y2_axis_label: Some("Depositors".to_string()),
    };

    state.cache.set(cache_key, &response, CHART_CACHE_TTL).await;
    ok(response)
}

async fn get_daily_deposit_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:dao-daily-deposit";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let rows = sqlx::query_as::<_, (chrono::NaiveDate, String, i32)>(
        r#"
        SELECT date, CAST(daily_deposit AS TEXT), daily_deposit_count
        FROM dao_daily_snapshots
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .map(
            |(date, daily_deposit, daily_deposit_count)| ChartDataPoint {
                date: date.format("%Y/%m/%d").to_string(),
                value: shannon_to_ckb(&daily_deposit),
                value2: Some(daily_deposit_count.to_string()),
            },
        )
        .collect();

    let data = downsample_chart_data(data, MAX_CHART_POINTS);

    let response = ChartResponse {
        data,
        title: "Daily Deposit".to_string(),
        y_axis_label: "CKB".to_string(),
        y2_axis_label: Some("Count".to_string()),
    };

    state.cache.set(cache_key, &response, CHART_CACHE_TTL).await;
    ok(response)
}

async fn get_circulation_ratio_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:dao-circulation-ratio";
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        return ok(cached);
    }

    let rows = sqlx::query_as::<_, (chrono::NaiveDate, String, String, String)>(
        r#"
        SELECT date, CAST(total_deposit AS TEXT), CAST(total_issuance AS TEXT), COALESCE(cumulative_burnt, '0')
        FROM dao_daily_snapshots
        WHERE total_issuance != 0
        ORDER BY date ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = rows
        .into_iter()
        .filter_map(
            |(date, total_deposit, total_issuance_str, secondary_burnt_str)| {
                let deposit: u128 = total_deposit.parse().ok()?;
                let total_issuance: u128 = total_issuance_str.parse().ok()?;
                let secondary_burnt: u128 = secondary_burnt_str.parse().unwrap_or(0);
                let total_burnt = GENESIS_BURNT + secondary_burnt;
                let circulating = total_issuance.saturating_sub(total_burnt);
                if circulating == 0 {
                    return None;
                }
                let ratio = (deposit as f64 / circulating as f64) * 100.0;
                Some(ChartDataPoint {
                    date: date.format("%Y/%m/%d").to_string(),
                    value: format!("{:.2}", ratio),
                    value2: None,
                })
            },
        )
        .collect();

    let data = downsample_chart_data(data, MAX_CHART_POINTS);

    let response = ChartResponse {
        data,
        title: "Deposit to Circulation Ratio".to_string(),
        y_axis_label: "%".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CHART_CACHE_TTL).await;
    ok(response)
}
