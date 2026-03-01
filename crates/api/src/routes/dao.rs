use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_common::dao::calculate_estimated_apc;
use ckbadger_store::keys;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::shannon_to_ckb;
use crate::AppState;

const CHART_CACHE_TTL: Duration = Duration::from_secs(3600);
type ApiRouteError = (axum::http::StatusCode, axum::Json<ApiError>);

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
    pub withdraw_request_tx_hash: Option<String>,
    pub withdraw_request_output_index: Option<i32>,
    pub withdraw_block: Option<i64>,
    pub withdraw_timestamp: Option<String>,
    pub withdraw_tx_hash: Option<String>,
    pub withdraw_to_output_index: Option<i32>,
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

    let withdraw_request_tx_hash = entry
        .withdraw_request_tx
        .as_ref()
        .map(|tx| format!("0x{}", hex::encode(tx)));
    let withdraw_tx_hash = entry
        .withdraw_tx
        .as_ref()
        .map(|tx| format!("0x{}", hex::encode(tx)));

    let address = if !entry.lock_code_hash.is_empty() {
        crate::utils::script_to_address(
            &entry.lock_code_hash,
            entry.lock_hash_type,
            &entry.lock_args,
            &state.ckb_network,
        )
        .ok()
    } else {
        None
    };
    let lock_code_hash = if !entry.lock_code_hash.is_empty() {
        Some(format!("0x{}", hex::encode(&entry.lock_code_hash)))
    } else {
        None
    };

    DaoDepositResponse {
        tx_hash: format!("0x{}", hex::encode(&tx_hash_bytes)),
        output_index: output_index as i32,
        lock_script_hash: format!("0x{}", hex::encode(&entry.lock_script_hash)),
        address,
        lock_code_hash,
        capacity: entry.capacity.to_string(),
        deposit_block_number: entry.deposit_block_number,
        deposit_timestamp,
        status: status_to_string(entry.status),
        withdraw_request_block: entry.withdraw_request_block,
        withdraw_request_timestamp,
        withdraw_request_tx_hash,
        withdraw_request_output_index: entry.withdraw_request_output_index.map(i32::from),
        withdraw_block: entry.withdraw_block,
        withdraw_timestamp,
        withdraw_tx_hash,
        withdraw_to_output_index: entry.withdraw_to_output_index.map(i32::from),
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

fn snapshot_secondary_burnt(
    snapshot: &ckbadger_store::DaoDailySnapshot,
) -> Result<u128, ApiRouteError> {
    if snapshot.cum_treasury < 0 {
        return Err(ApiError::internal(format!(
            "negative cum_treasury in dao_daily_snapshots for {}: {}",
            snapshot.date, snapshot.cum_treasury
        )));
    }
    Ok(snapshot.cum_treasury as u128)
}

fn snapshot_estimated_apc(
    snapshot: &ckbadger_store::DaoDailySnapshot,
) -> Result<Option<String>, ApiRouteError> {
    let Ok(total_issuance) = u64::try_from(snapshot.total_issuance) else {
        return Ok(None);
    };
    if total_issuance == 0 {
        return Ok(None);
    }
    let apc = calculate_estimated_apc(total_issuance, snapshot_secondary_burnt(snapshot)?);
    Ok((apc > 0.0).then(|| format!("{:.2}", apc)))
}

fn snapshot_circulating_supply(
    snapshot: &ckbadger_store::DaoDailySnapshot,
) -> Result<Option<i128>, ApiRouteError> {
    const GENESIS_BURNT: i128 = 8_400_000_000 * 100_000_000;
    let total_issuance = snapshot.total_issuance;
    if total_issuance <= 0 {
        return Ok(None);
    }
    if snapshot.cum_treasury < 0 {
        return Err(ApiError::internal(format!(
            "negative cum_treasury in dao_daily_snapshots for {}: {}",
            snapshot.date, snapshot.cum_treasury
        )));
    }
    let circulating = total_issuance - GENESIS_BURNT - snapshot.cum_treasury;
    if circulating < 0 {
        return Err(ApiError::internal(format!(
            "negative circulating supply for {}: total_issuance={}, burnt={}, cum_treasury={}",
            snapshot.date, total_issuance, GENESIS_BURNT, snapshot.cum_treasury
        )));
    }
    Ok(Some(circulating))
}

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

    let (latest_block_number, latest_ar) = match &latest_block {
        Some((num, header)) => {
            let ar = extract_ar(&header.dao).unwrap_or(1);
            (*num, ar)
        }
        None => (0, 1),
    };

    // Calculate unclaimed compensation for active/pending deposits
    let mut total_unclaimed: u128 = 0;
    for (_, entry) in my_deposits.iter().filter(|(_, e)| {
        (e.status == 0 || e.status == 1) && e.deposit_block_number <= latest_block_number
    }) {
        if entry.capacity < 0 {
            return Err(ApiError::internal(format!(
                "negative DAO deposit capacity: deposit_block={}, lock_script_hash=0x{}, capacity={}",
                entry.deposit_block_number,
                hex::encode(&entry.lock_script_hash),
                entry.capacity
            )));
        }
        let capacity = entry.capacity as u128;
        let free_capacity = capacity.checked_sub(DAO_OCCUPIED_CAPACITY).ok_or_else(|| {
            ApiError::internal(format!(
                "DAO deposit capacity below occupied capacity: deposit_block={}, lock_script_hash=0x{}, capacity={}",
                entry.deposit_block_number,
                hex::encode(&entry.lock_script_hash),
                capacity
            ))
        })?;
        let ar_deposit = entry.deposit_ar as u64;
        if ar_deposit > 0 && latest_ar > ar_deposit {
            let gross = free_capacity * latest_ar as u128 / ar_deposit as u128;
            let compensation = gross.checked_sub(free_capacity).ok_or_else(|| {
                ApiError::internal(format!(
                    "DAO compensation underflow: deposit_block={}, lock_script_hash=0x{}, free_capacity={}, ar_deposit={}, latest_ar={}",
                    entry.deposit_block_number,
                    hex::encode(&entry.lock_script_hash),
                    free_capacity,
                    ar_deposit,
                    latest_ar
                ))
            })?;
            total_unclaimed += compensation;
        }
    }

    let latest_snapshot = state
        .store
        .list_dao_daily_snapshots()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .last()
        .cloned();
    let estimated_apc = latest_snapshot
        .as_ref()
        .map(snapshot_estimated_apc)
        .transpose()?
        .flatten()
        .unwrap_or_default();

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

    let (latest_block_number, latest_ar) = match &latest_block {
        Some((num, header)) => {
            let ar = extract_ar(&header.dao).unwrap_or(1);
            (*num, ar)
        }
        None => (0, 1),
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
        if entry.capacity < 0 {
            return Err(ApiError::internal(format!(
                "negative DAO deposit capacity: deposit_block={}, lock_script_hash=0x{}, capacity={}",
                entry.deposit_block_number,
                hex::encode(&entry.lock_script_hash),
                entry.capacity
            )));
        }
        let capacity = entry.capacity as u128;
        let free_capacity = capacity.checked_sub(DAO_OCCUPIED_CAPACITY).ok_or_else(|| {
            ApiError::internal(format!(
                "DAO deposit capacity below occupied capacity: deposit_block={}, lock_script_hash=0x{}, capacity={}",
                entry.deposit_block_number,
                hex::encode(&entry.lock_script_hash),
                capacity
            ))
        })?;
        let ar_deposit = entry.deposit_ar as u64;
        if ar_deposit > 0 && latest_ar > ar_deposit {
            let gross = free_capacity * latest_ar as u128 / ar_deposit as u128;
            let compensation = gross.checked_sub(free_capacity).ok_or_else(|| {
                ApiError::internal(format!(
                    "DAO compensation underflow: deposit_block={}, lock_script_hash=0x{}, free_capacity={}, ar_deposit={}, latest_ar={}",
                    entry.deposit_block_number,
                    hex::encode(&entry.lock_script_hash),
                    free_capacity,
                    ar_deposit,
                    latest_ar
                ))
            })?;
            total_unclaimed += compensation;
        }
    }

    let latest_snapshot = state
        .store
        .list_dao_daily_snapshots()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .last()
        .cloned();
    let estimated_apc = match latest_snapshot.as_ref() {
        Some(snapshot) => snapshot_estimated_apc(snapshot)?.unwrap_or_default(),
        None => String::new(),
    };

    let avg_days = epochs_to_days(avg_epochs);

    let total_deposited_str = total_deposited.to_string();
    let total_comp_str = total_compensation_paid.to_string();
    let (mining_reward, deposit_compensation, burnt) = if let Some(s) = latest_snapshot.as_ref() {
        if s.cum_miner_secondary < 0 {
            return Err(ApiError::internal(format!(
                "negative cum_miner_secondary in dao_daily_snapshots for {}: {}",
                s.date, s.cum_miner_secondary
            )));
        }
        if s.cum_dao_compensation < 0 {
            return Err(ApiError::internal(format!(
                "negative cum_dao_compensation in dao_daily_snapshots for {}: {}",
                s.date, s.cum_dao_compensation
            )));
        }
        if s.cum_treasury < 0 {
            return Err(ApiError::internal(format!(
                "negative cum_treasury in dao_daily_snapshots for {}: {}",
                s.date, s.cum_treasury
            )));
        }
        (
            s.cum_miner_secondary.to_string(),
            s.cum_dao_compensation.to_string(),
            s.cum_treasury.to_string(),
        )
    } else {
        ("0".to_string(), "0".to_string(), "0".to_string())
    };

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
            let ar_d = extract_ar(&d).ok_or_else(|| {
                ApiError::internal(format!(
                    "invalid DAO field in deposit block header: block_number={}",
                    params.deposit_block
                ))
            })?;
            let ar_w = extract_ar(&w).ok_or_else(|| {
                ApiError::internal(format!(
                    "invalid DAO field in withdraw block header: block_number={}",
                    withdraw_block
                ))
            })?;
            (ar_d, ar_w)
        }
        _ => {
            return Err(ApiError::bad_request(
                "Unable to load deposit/withdraw block headers for compensation calculation",
            ));
        }
    };

    let occupied = 102_00000000u128;
    let free = capacity
        .checked_sub(occupied)
        .ok_or_else(|| ApiError::bad_request("Capacity must be at least 102 CKB"))?;
    let compensation = if ar_deposit > 0 {
        let gross = free * ar_withdraw as u128 / ar_deposit as u128;
        gross
            .checked_sub(free)
            .ok_or_else(|| ApiError::internal("DAO compensation underflow"))?
    } else {
        0
    };

    let total = capacity + compensation;

    if withdraw_block < params.deposit_block {
        return Err(ApiError::bad_request(
            "withdraw_block must be greater than or equal to deposit_block",
        ));
    }
    let blocks_held = (withdraw_block - params.deposit_block) as f64;
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
        .map(|w| -> Result<ChartDataPoint, ApiRouteError> {
            let daily_deposited = w[1]
                .cumulative_deposit_amount
                .checked_sub(w[0].cumulative_deposit_amount)
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "cumulative_deposit_amount decreased between {} and {}",
                        w[0].date, w[1].date
                    ))
                })?;
            let daily_deposits = w[1]
                .new_deposits
                .checked_sub(w[0].new_deposits)
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "new_deposits decreased between {} and {}",
                        w[0].date, w[1].date
                    ))
                })?;
            Ok(ChartDataPoint {
                date: w[1].date.clone(),
                value: shannon_to_ckb(&daily_deposited.to_string()),
                value2: Some(daily_deposits.to_string()),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

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

    let mut data = Vec::with_capacity(snapshots.len());
    for s in &snapshots {
        let Some(circulating) = snapshot_circulating_supply(s)? else {
            return Err(ApiError::internal(format!(
                "missing DAO snapshot total_issuance for {}. delete RocksDB and re-sync from genesis",
                s.date
            )));
        };
        if circulating <= 0 {
            continue;
        }
        if s.total_deposited < 0 {
            return Err(ApiError::internal(format!(
                "negative total_deposited in dao_daily_snapshots for {}: {}",
                s.date, s.total_deposited
            )));
        }
        let deposited = s.total_deposited as f64;
        let ratio = (deposited / circulating as f64) * 100.0;
        data.push(ChartDataPoint {
            date: s.date.clone(),
            value: format!("{:.4}", ratio),
            value2: None,
        });
    }

    let response = ChartResponse {
        data,
        title: "Deposit to Circulation Ratio".to_string(),
        y_axis_label: "%".to_string(),
        y2_axis_label: None,
    };

    state.cache.set(cache_key, &response, CHART_CACHE_TTL).await;
    ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(total_issuance: i128, cum_treasury: i128) -> ckbadger_store::DaoDailySnapshot {
        ckbadger_store::DaoDailySnapshot {
            date: "2026-02-18".to_string(),
            total_deposited: 0,
            depositors_count: 0,
            new_deposits: 0,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 0,
            total_issuance,
            secondary_pool: 0,
            occupied_capacity: 0,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury,
        }
    }

    #[test]
    fn test_snapshot_secondary_burnt_errors_on_negative() {
        let err = snapshot_secondary_burnt(&snapshot(1, -10)).unwrap_err();
        assert!(err.1 .0.message.contains("negative cum_treasury"));
    }

    #[test]
    fn test_snapshot_secondary_burnt_returns_value() {
        assert_eq!(snapshot_secondary_burnt(&snapshot(1, 25)).unwrap(), 25);
    }

    #[test]
    fn test_snapshot_estimated_apc_requires_total_issuance() {
        assert!(snapshot_estimated_apc(&snapshot(0, 0)).unwrap().is_none());
    }

    #[test]
    fn test_snapshot_circulating_supply_uses_cum_treasury() {
        let total_issuance = 1_000_000_000_000_000_000i128;
        let cum_treasury = 20_000_000_000_000_000i128;
        let s = snapshot(total_issuance, cum_treasury);
        let expected = total_issuance - (8_400_000_000i128 * 100_000_000i128) - cum_treasury;
        assert_eq!(snapshot_circulating_supply(&s).unwrap(), Some(expected));
    }
}
