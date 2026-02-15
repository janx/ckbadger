use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use chrono::NaiveDate;
use ckbadger_common::dao::calculate_estimated_apc;
use ckbadger_store::keys;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::shannon_to_ckb;
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

/// Convert a DAO deposit entry from the store to an API response.
/// The outpoint_key encodes tx_hash(32B) + output_index(2B BE).
fn deposit_to_response(
    outpoint_key: &[u8],
    entry: &ckbadger_store::DaoDepositCacheEntry,
    state: &AppState,
) -> DaoDepositResponse {
    let (tx_hash_bytes, output_index) = keys::decode_outpoint(outpoint_key);

    // Try to resolve the block header for timestamp
    let deposit_timestamp = state
        .store
        .get_block_header(entry.deposit_block_number)
        .ok()
        .flatten()
        .map(|h| {
            chrono::DateTime::from_timestamp_millis(h.timestamp)
                .unwrap_or_default()
                .to_rfc3339()
        })
        .unwrap_or_default();

    let withdraw_request_timestamp = entry.withdraw_request_block.and_then(|bn| {
        state.store.get_block_header(bn).ok()?.map(|h| {
            chrono::DateTime::from_timestamp_millis(h.timestamp)
                .unwrap_or_default()
                .to_rfc3339()
        })
    });

    let withdraw_timestamp = entry.withdraw_block.and_then(|bn| {
        state.store.get_block_header(bn).ok()?.map(|h| {
            chrono::DateTime::from_timestamp_millis(h.timestamp)
                .unwrap_or_default()
                .to_rfc3339()
        })
    });

    DaoDepositResponse {
        tx_hash: format!("0x{}", hex::encode(&tx_hash_bytes)),
        output_index: output_index as i32,
        lock_script_hash: format!("0x{}", hex::encode(&entry.lock_script_hash)),
        address: None,
        lock_code_hash: None,
        capacity: entry.capacity.to_string(),
        deposit_block_number: entry.deposit_block_number,
        deposit_timestamp,
        status: status_to_string(entry.status),
        withdraw_request_block: entry.withdraw_request_block,
        withdraw_request_timestamp,
        withdraw_block: entry.withdraw_block,
        withdraw_timestamp,
        compensation: entry.compensation.map(|c| c.to_string()),
    }
}

async fn list_deposits(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<DaoDepositResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    let all_deposits = state
        .store
        .list_dao_deposits()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Filter by status if requested, sort by deposit_block_number DESC
    let mut filtered: Vec<_> = all_deposits
        .into_iter()
        .filter(|(_, entry)| {
            if let Some(status) = params.status {
                entry.status == status
            } else {
                true
            }
        })
        .collect();

    filtered.sort_by(|a, b| b.1.deposit_block_number.cmp(&a.1.deposit_block_number));

    // Apply cursor: skip entries until we find the cursor block number
    let cursor_block = params.cursor.as_ref().and_then(|c| c.parse::<i64>().ok());

    let start_idx = if let Some(cb) = cursor_block {
        filtered
            .iter()
            .position(|(_, e)| e.deposit_block_number < cb)
            .unwrap_or(filtered.len())
    } else {
        0
    };

    let page: Vec<_> = filtered.iter().skip(start_idx).take(limit + 1).collect();

    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last().map(|(_, e)| e.deposit_block_number.to_string())
    } else {
        None
    };

    let deposits: Vec<DaoDepositResponse> = page
        .into_iter()
        .map(|(key, entry)| deposit_to_response(key, entry, &state))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        deposits,
        limit as i64,
        next_cursor,
    ))
}

async fn get_deposits_by_address(
    State(state): State<Arc<AppState>>,
    Path(lock_hash): Path<String>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<DaoDepositResponse>> {
    let hash = hex::decode(lock_hash.strip_prefix("0x").unwrap_or(&lock_hash))
        .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?;

    let limit = params.limit.clamp(1, 100) as usize;

    let all_deposits = state
        .store
        .list_dao_deposits()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Filter by lock_script_hash
    let mut filtered: Vec<_> = all_deposits
        .into_iter()
        .filter(|(_, entry)| entry.lock_script_hash == hash)
        .collect();

    filtered.sort_by(|a, b| b.1.deposit_block_number.cmp(&a.1.deposit_block_number));

    let cursor_block = params.cursor.as_ref().and_then(|c| c.parse::<i64>().ok());

    let start_idx = if let Some(cb) = cursor_block {
        filtered
            .iter()
            .position(|(_, e)| e.deposit_block_number < cb)
            .unwrap_or(filtered.len())
    } else {
        0
    };

    let page: Vec<_> = filtered.iter().skip(start_idx).take(limit + 1).collect();

    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last().map(|(_, e)| e.deposit_block_number.to_string())
    } else {
        None
    };

    let deposits: Vec<DaoDepositResponse> = page
        .into_iter()
        .map(|(key, entry)| deposit_to_response(key, entry, &state))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        deposits,
        limit as i64,
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

    let all_deposits = state
        .store
        .list_dao_deposits()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let my_deposits: Vec<_> = all_deposits
        .into_iter()
        .filter(|(_, entry)| entry.lock_script_hash == hash)
        .collect();

    let active_count = my_deposits.iter().filter(|(_, e)| e.status == 0).count() as i32;
    let pending_count = my_deposits.iter().filter(|(_, e)| e.status == 1).count() as i32;
    let completed_count = my_deposits.iter().filter(|(_, e)| e.status == 2).count() as i32;

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

    // Total locked = sum of capacity for active (0) and pending (1) deposits
    let total_locked: i128 = my_deposits
        .iter()
        .filter(|(_, e)| e.status == 0 || e.status == 1)
        .map(|(_, e)| e.capacity as i128)
        .sum();

    // Total compensation earned = sum of compensation for completed (2) deposits
    let total_comp_earned: i128 = my_deposits
        .iter()
        .filter(|(_, e)| e.status == 2 && e.compensation.is_some())
        .map(|(_, e)| e.compensation.unwrap_or(0) as i128)
        .sum();

    // Get latest block header for AR
    let latest_block = state.store.get_sync_tip_block().ok().flatten();

    let (latest_block_number, latest_ar, total_issuance) = match &latest_block {
        Some((num, header)) => {
            let ar = extract_ar(&header.dao).unwrap_or(1);
            let issuance = extract_total_issuance(&header.dao).unwrap_or(0);
            (*num, ar, issuance)
        }
        None => (0, 1, 0),
    };

    // Calculate unclaimed compensation for active/pending deposits
    let mut total_unclaimed: u128 = 0;
    for (_, entry) in my_deposits.iter().filter(|(_, e)| {
        (e.status == 0 || e.status == 1) && e.deposit_block_number <= latest_block_number
    }) {
        let capacity = entry.capacity as u128;
        let free_capacity = capacity.saturating_sub(DAO_OCCUPIED_CAPACITY);
        let ar_deposit = entry.deposit_ar as u64;
        if ar_deposit > 0 && latest_ar > ar_deposit {
            let compensation = (free_capacity * latest_ar as u128 / ar_deposit as u128)
                .saturating_sub(free_capacity);
            total_unclaimed += compensation;
        }
    }

    // Get DAO stats for APC (cumulative_burnt)
    let dao_stats = state.store.get_dao_stats(b"global").ok().flatten();

    let secondary_burnt: u128 = 0; // Not tracked separately in RocksDB; derive from issuance data
    let _ = dao_stats; // DaoStats doesn't carry cumulative_burnt

    let apc = calculate_estimated_apc(total_issuance, secondary_burnt);
    let estimated_apc = if apc > 0.0 {
        format!("{:.2}", apc)
    } else {
        String::new()
    };

    let total_locked_str = total_locked.to_string();
    let total_comp_str = total_comp_earned.to_string();

    ok(AddressDaoSummaryResponse {
        has_dao_activity: true,
        active_deposits_count: active_count,
        pending_withdrawals_count: pending_count,
        completed_withdrawals_count: completed_count,
        total_locked_capacity: total_locked_str.clone(),
        total_locked_ckb: shannon_to_ckb(&total_locked_str),
        unclaimed_compensation: total_unclaimed.to_string(),
        unclaimed_compensation_ckb: shannon_to_ckb(&total_unclaimed.to_string()),
        total_compensation_earned: total_comp_str.clone(),
        total_compensation_earned_ckb: shannon_to_ckb(&total_comp_str),
        estimated_apc,
    })
}

async fn get_statistics(State(state): State<Arc<AppState>>) -> ApiResult<DaoStatisticsResponse> {
    let active_deposits = state
        .store
        .list_active_dao_deposits()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let all_deposits = state
        .store
        .list_dao_deposits()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Live stats from active deposits
    let total_deposited: i128 = active_deposits
        .iter()
        .map(|(_, e)| e.capacity as i128)
        .sum();

    let unique_depositors: std::collections::HashSet<&[u8]> = active_deposits
        .iter()
        .map(|(_, e)| e.lock_script_hash.as_slice())
        .collect();
    let total_depositors = unique_depositors.len() as i32;
    let active_count = active_deposits.len() as i32;

    // Compensation paid = sum of compensation for completed deposits
    let total_compensation_paid: i128 = all_deposits
        .iter()
        .filter(|(_, e)| e.status == 2 && e.compensation.is_some())
        .map(|(_, e)| e.compensation.unwrap_or(0) as i128)
        .sum();

    // Get latest block for AR
    let latest_block = state.store.get_sync_tip_block().ok().flatten();

    let (latest_block_number, latest_ar, total_issuance) = match &latest_block {
        Some((num, header)) => {
            let ar = extract_ar(&header.dao).unwrap_or(1);
            let issuance = extract_total_issuance(&header.dao).unwrap_or(0);
            (*num, ar, issuance)
        }
        None => (0, 1, 0),
    };

    // Calculate average deposit time
    let total_blocks_held: f64 = active_deposits
        .iter()
        .filter(|(_, e)| e.deposit_block_number <= latest_block_number)
        .map(|(_, e)| (latest_block_number - e.deposit_block_number) as f64)
        .sum();
    let active_filtered_count = active_deposits
        .iter()
        .filter(|(_, e)| e.deposit_block_number <= latest_block_number)
        .count();
    let avg_epochs = if active_filtered_count > 0 {
        (total_blocks_held / active_filtered_count as f64) / 1800.0
    } else {
        0.0
    };

    // Calculate unclaimed compensation
    let mut total_unclaimed: u128 = 0;
    for (_, entry) in active_deposits
        .iter()
        .filter(|(_, e)| e.deposit_block_number <= latest_block_number)
    {
        let capacity = entry.capacity as u128;
        let free_capacity = capacity.saturating_sub(DAO_OCCUPIED_CAPACITY);
        let ar_deposit = entry.deposit_ar as u64;
        if ar_deposit > 0 && latest_ar > ar_deposit {
            let compensation = (free_capacity * latest_ar as u128 / ar_deposit as u128)
                .saturating_sub(free_capacity);
            total_unclaimed += compensation;
        }
    }

    let secondary_burnt: u128 = 0;
    let apc = calculate_estimated_apc(total_issuance, secondary_burnt);
    let estimated_apc = if apc > 0.0 {
        format!("{:.2}", apc)
    } else {
        String::new()
    };

    let avg_days = epochs_to_days(avg_epochs);

    let total_deposited_str = total_deposited.to_string();
    let total_comp_str = total_compensation_paid.to_string();
    let mining_reward = "0".to_string();
    let deposit_compensation = "0".to_string();
    let burnt = "0".to_string();

    ok(DaoStatisticsResponse {
        total_deposited: total_deposited_str.clone(),
        total_deposited_ckb: shannon_to_ckb(&total_deposited_str),
        total_depositors,
        active_deposits: active_count,
        total_compensation_paid: total_comp_str.clone(),
        total_compensation_paid_ckb: shannon_to_ckb(&total_comp_str),
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

    let latest_block = state
        .store
        .get_sync_tip_block()
        .ok()
        .flatten()
        .map(|(n, _)| n)
        .unwrap_or(0);

    let withdraw_block = params.withdraw_block.unwrap_or(latest_block);

    let deposit_dao = state
        .store
        .get_block_header(params.deposit_block)
        .ok()
        .flatten()
        .map(|h| h.dao);

    let withdraw_dao = state
        .store
        .get_block_header(withdraw_block)
        .ok()
        .flatten()
        .map(|h| h.dao);

    let (ar_deposit, ar_withdraw) = match (deposit_dao, withdraw_dao) {
        (Some(d), Some(w)) => {
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

    let snapshots = state
        .store
        .list_dao_daily_snapshots()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let data: Vec<ChartDataPoint> = snapshots
        .iter()
        .map(|s| ChartDataPoint {
            date: s.date.clone(),
            value: shannon_to_ckb(&s.total_deposited.to_string()),
            value2: Some(s.depositors_count.to_string()),
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

    let snapshots = state
        .store
        .list_dao_daily_snapshots()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Compute daily gross deposits from cumulative deposit amounts.
    // Uses cumulative_deposit_amount (gross, never reduced by withdrawals)
    // to match the official explorer's daily_dao_deposit metric.
    let data: Vec<ChartDataPoint> = snapshots
        .windows(2)
        .map(|w| {
            let daily_deposited =
                (w[1].cumulative_deposit_amount - w[0].cumulative_deposit_amount).max(0);
            let daily_deposits = (w[1].new_deposits - w[0].new_deposits).max(0);
            ChartDataPoint {
                date: w[1].date.clone(),
                value: shannon_to_ckb(&daily_deposited.to_string()),
                value2: Some(daily_deposits.to_string()),
            }
        })
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

    let snapshots = state
        .store
        .list_dao_daily_snapshots()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // 8.4 billion CKB burnt at genesis (in shannons)
    const GENESIS_BURNT: i128 = 8_400_000_000 * 100_000_000;

    let data: Vec<ChartDataPoint> = snapshots
        .iter()
        .filter_map(|s| {
            // Circulating supply = C - S - burnt
            // C = total issuance, S = treasury (secondary pool)
            // Falls back to estimated formula if C/S not available (pre-migration data)
            let circulating = if s.total_issuance > 0 {
                (s.total_issuance - s.secondary_pool - GENESIS_BURNT) as f64
            } else {
                estimated_circulating_supply(&s.date)?
            };
            if circulating <= 0.0 {
                return None;
            }
            let deposited = s.total_deposited as f64;
            let ratio = (deposited / circulating) * 100.0;
            Some(ChartDataPoint {
                date: s.date.clone(),
                value: format!("{:.4}", ratio),
                value2: None,
            })
        })
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

/// Estimate circulating supply (in shannons) at a given date.
///
/// CKB issuance schedule:
/// - Genesis: 33.6B CKB (8.4B burnt)
/// - Primary: 4.2B/year, halving every ~4 years (first halving ~Nov 2023)
/// - Secondary: 1.344B/year (constant)
fn estimated_circulating_supply(date_str: &str) -> Option<f64> {
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    let launch = NaiveDate::from_ymd_opt(2019, 11, 16)?;
    let days = (date - launch).num_days().max(0) as f64;
    let years = days / 365.25;

    const SHANNON: f64 = 100_000_000.0;
    const GENESIS_SUPPLY: f64 = 33_600_000_000.0 * SHANNON;
    const GENESIS_BURNT: f64 = 8_400_000_000.0 * SHANNON;
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
    let total = GENESIS_SUPPLY + primary_issued + secondary_issued;
    Some(total - GENESIS_BURNT)
}
