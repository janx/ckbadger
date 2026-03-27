use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_common::dao::calculate_estimated_apc;
use ckbadger_store::keys;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use crate::response::{
    chart_response_has_data, default_limit, ok, ApiError, ApiResult, ApiRouteError, ChartDataPoint,
    ChartResponse, CursorPaginatedResponse,
};
use crate::utils::{script_to_address, shannon_to_ckb, shannon_to_ckb_signed};
use crate::AppState;

const CHART_CACHE_TTL: Duration = Duration::from_secs(3600);
const DAO_STATS_CACHE_TTL: Duration = Duration::from_secs(30);
const DAO_ADDRESS_SUMMARY_CACHE_TTL: Duration = Duration::from_secs(30);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/dao/deposits", get(list_deposits))
        .route("/dao/deposits/{lock_hash}", get(get_deposits_by_address))
        .route("/dao/summary/{lock_hash}", get(get_address_dao_summary))
        .route("/dao/statistics", get(get_statistics))
        .route("/dao/top-depositors", get(get_top_depositors))
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

fn parse_dao_cursor_key(
    cursor: Option<&str>,
    expected_len: usize,
    label: &str,
) -> Result<Option<Vec<u8>>, ApiRouteError> {
    let Some(raw) = cursor else {
        return Ok(None);
    };
    let decoded = hex::decode(raw.strip_prefix("0x").unwrap_or(raw))
        .map_err(|_| ApiError::bad_request(format!("Invalid {} cursor", label)))?;
    if decoded.len() != expected_len {
        return Err(ApiError::bad_request(format!(
            "Invalid {} cursor length: expected {} bytes, got {}",
            label,
            expected_len,
            decoded.len()
        )));
    }
    Ok(Some(decoded))
}

fn map_dao_pagination_error(err: anyhow::Error, label: &str) -> ApiRouteError {
    let msg = err.to_string();
    if msg.contains("cursor") {
        ApiError::bad_request(format!("Invalid {} cursor", label))
    } else {
        ApiError::internal(msg)
    }
}

fn resolve_latest_block_and_ar_from_tip(
    tip: Option<(i64, ckbadger_store::CachedBlockHeader)>,
    context: &str,
) -> Result<(i64, u64), ApiRouteError> {
    let (block_number, header) = tip.ok_or_else(|| {
        ApiError::internal(format!(
            "missing sync tip block while computing DAO {}",
            context
        ))
    })?;
    let ar = extract_ar(&header.dao).ok_or_else(|| {
        ApiError::internal(format!(
            "invalid DAO field in sync tip block while computing DAO {}: block_number={}, dao_len={}",
            context,
            block_number,
            header.dao.len()
        ))
    })?;
    Ok((block_number, ar))
}

fn resolve_latest_block_and_ar(
    state: &AppState,
    context: &str,
) -> Result<(i64, u64), ApiRouteError> {
    let tip = state
        .store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(format!("failed to load sync tip block: {}", e)))?;
    resolve_latest_block_and_ar_from_tip(tip, context)
}

fn depositor_series_value(
    depositors_series: &HashMap<String, i64>,
    date: &str,
) -> Result<i64, ApiRouteError> {
    depositors_series.get(date).copied().ok_or_else(|| {
        ApiError::internal(format!(
            "missing dao depositor series point for date={}",
            date
        ))
    })
}

fn dao_deposit_ar_as_u64(
    entry: &ckbadger_store::types::DaoDepositCacheEntry,
    context: &str,
) -> anyhow::Result<u64> {
    u64::try_from(entry.deposit_ar).map_err(|_| {
        anyhow::anyhow!(
            "invalid negative DAO deposit AR while computing {}: deposit_block={}, lock_script_hash=0x{}, deposit_ar={}",
            context,
            entry.deposit_block_number,
            hex::encode(&entry.lock_script_hash),
            entry.deposit_ar
        )
    })
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn dao_address_summary_cache_key(lock_hash: &[u8], latest_block_number: i64) -> String {
    format!(
        "dao:summary:tip:{}:lock:0x{}",
        latest_block_number,
        hex::encode(lock_hash)
    )
}

#[derive(Default)]
struct DaoStatisticsAccumulator {
    total_deposited: i128,
    unique_depositors: HashSet<Vec<u8>>,
    active_count: i32,
    total_compensation_paid: i128,
    total_blocks_held: f64,
    active_filtered_count: usize,
    total_unclaimed: u128,
}

fn accumulate_dao_statistics_entry(
    acc: &mut DaoStatisticsAccumulator,
    entry: &ckbadger_store::DaoDepositCacheEntry,
    latest_block_number: i64,
    latest_ar: u64,
) -> anyhow::Result<()> {
    match entry.status {
        0 => {
            acc.total_deposited += entry.capacity as i128;
            acc.unique_depositors.insert(entry.lock_script_hash.clone());
            acc.active_count += 1;

            if entry.deposit_block_number <= latest_block_number {
                acc.total_blocks_held += (latest_block_number - entry.deposit_block_number) as f64;
                acc.active_filtered_count += 1;

                if entry.capacity < 0 {
                    anyhow::bail!(
                        "negative DAO deposit capacity: deposit_block={}, lock_script_hash=0x{}, capacity={}",
                        entry.deposit_block_number,
                        hex::encode(&entry.lock_script_hash),
                        entry.capacity
                    );
                }
                let capacity = entry.capacity as u128;
                let free_capacity = capacity.checked_sub(DAO_OCCUPIED_CAPACITY).ok_or_else(|| {
                    anyhow::anyhow!(
                        "DAO deposit capacity below occupied capacity: deposit_block={}, lock_script_hash=0x{}, capacity={}",
                        entry.deposit_block_number,
                        hex::encode(&entry.lock_script_hash),
                        capacity
                    )
                })?;
                let ar_deposit = dao_deposit_ar_as_u64(entry, "statistics")?;
                if ar_deposit > 0 && latest_ar > ar_deposit {
                    let gross = free_capacity * latest_ar as u128 / ar_deposit as u128;
                    let compensation = gross.checked_sub(free_capacity).ok_or_else(|| {
                        anyhow::anyhow!(
                            "DAO compensation underflow: deposit_block={}, lock_script_hash=0x{}, free_capacity={}, ar_deposit={}, latest_ar={}",
                            entry.deposit_block_number,
                            hex::encode(&entry.lock_script_hash),
                            free_capacity,
                            ar_deposit,
                            latest_ar
                        )
                    })?;
                    acc.total_unclaimed += compensation;
                }
            }
        }
        2 => {
            if let Some(comp) = entry.compensation {
                acc.total_compensation_paid += comp as i128;
            }
        }
        _ => {}
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deposit_change_24h: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depositors_change_24h: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_compensation_change_24h: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unclaimed_compensation_change_24h: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoTopDepositorResponse {
    pub rank: i32,
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub total_capacity: String,
    pub total_capacity_ckb: String,
    pub deposit_count: i32,
    pub average_deposit_days: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoTopDepositorsResponse {
    pub depositors: Vec<DaoTopDepositorResponse>,
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

    // Resolve address and lock_code_hash from the cell payload in the append-only store.
    // The deposit cell's outpoint key maps to a LiveCellInfo that has the full lock script.
    let (address, lock_code_hash) = match state
        .append_only_store
        .get_cell_by_outpoint_key(outpoint_key)
    {
        Ok(Some(cell_info)) => {
            let addr = script_to_address(
                &cell_info.lock_code_hash,
                cell_info.lock_hash_type,
                &cell_info.lock_args,
                &state.ckb_network,
            )
            .ok();
            let code_hash = format!("0x{}", hex::encode(&cell_info.lock_code_hash));
            (addr, Some(code_hash))
        }
        Ok(None) => (None, None),
        Err(e) => {
            tracing::warn!(
                "failed to resolve address for DAO deposit outpoint=0x{}: {}",
                hex::encode(outpoint_key),
                e
            );
            (None, None)
        }
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
    let status_filter = params.status;
    let cursor_key = parse_dao_cursor_key(
        params.cursor.as_deref(),
        if status_filter.is_some() {
            keys::DAO_BY_STATUS_BLOCK_KEY_SIZE
        } else {
            keys::DAO_BY_BLOCK_KEY_SIZE
        },
        "dao deposits",
    )?;
    let page = if let Some(status) = status_filter {
        state
            .store
            .list_dao_deposits_by_status_paginated(status, limit + 1, cursor_key.as_deref())
            .map_err(|e| map_dao_pagination_error(e, "dao deposits"))?
    } else {
        state
            .store
            .list_dao_deposits_paginated(limit + 1, cursor_key.as_deref())
            .map_err(|e| map_dao_pagination_error(e, "dao deposits"))?
    };

    let mut page = page;

    let has_more = page.len() > limit;
    if has_more {
        page.truncate(limit);
    }

    let next_cursor = if has_more {
        page.last().map(|(outpoint_key, entry)| {
            let cursor_key = if let Some(status) = status_filter {
                keys::encode_dao_by_status_block_key(
                    status,
                    entry.deposit_block_number,
                    outpoint_key,
                )
                .to_vec()
            } else {
                keys::encode_dao_by_block_key(entry.deposit_block_number, outpoint_key).to_vec()
            };
            format!("0x{}", hex::encode(cursor_key))
        })
    } else {
        None
    };

    let deposits: Vec<DaoDepositResponse> = page
        .iter()
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
    if hash.len() != 32 {
        return Err(ApiError::bad_request("Invalid lock script hash"));
    }

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_key = parse_dao_cursor_key(
        params.cursor.as_deref(),
        keys::DAO_BY_LOCK_BLOCK_KEY_SIZE,
        "dao deposits by address",
    )?;
    let mut page = state
        .store
        .list_dao_deposits_by_lock_paginated(&hash, limit + 1, cursor_key.as_deref())
        .map_err(|e| map_dao_pagination_error(e, "dao deposits by address"))?;

    let has_more = page.len() > limit;
    if has_more {
        page.truncate(limit);
    }

    let next_cursor = if has_more {
        page.last().map(|(outpoint_key, entry)| {
            let cursor_key =
                keys::encode_dao_by_lock_block_key(&hash, entry.deposit_block_number, outpoint_key);
            format!("0x{}", hex::encode(cursor_key))
        })
    } else {
        None
    };

    let deposits: Vec<DaoDepositResponse> = page
        .iter()
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

#[derive(Default)]
struct DaoDeltas {
    deposit_change: Option<String>,
    depositors_change: Option<i32>,
    claimed_compensation_change: Option<String>,
    unclaimed_compensation_change: Option<String>,
}

fn compute_dao_24h_deltas(state: &AppState) -> DaoDeltas {
    let Ok(Some(latest)) = state.store.get_latest_dao_daily_snapshot() else {
        return DaoDeltas::default();
    };
    let Ok(latest_date) = chrono::NaiveDate::parse_from_str(&latest.date, "%Y-%m-%d") else {
        return DaoDeltas::default();
    };
    let prev_date = latest_date - chrono::Duration::days(1);
    let prev_key = prev_date.format("%Y%m%d").to_string();
    let Ok(Some(prev)) = state.store.get_dao_daily_snapshot(&prev_key) else {
        return DaoDeltas::default();
    };

    let deposit_delta = latest.total_deposited - prev.total_deposited;
    let depositors_delta = latest.depositors_count - prev.depositors_count;
    let claimed_delta = latest.compensation - prev.compensation;
    let unclaimed_delta =
        latest.unclaimed_compensation as i128 - prev.unclaimed_compensation as i128;

    DaoDeltas {
        deposit_change: Some(shannon_to_ckb_signed(deposit_delta)),
        depositors_change: Some(depositors_delta as i32),
        claimed_compensation_change: Some(shannon_to_ckb_signed(claimed_delta)),
        unclaimed_compensation_change: Some(shannon_to_ckb_signed(unclaimed_delta)),
    }
}

fn dao_latest_to_response(
    latest: &ckbadger_store::DaoLatestStatistics,
    deltas: DaoDeltas,
) -> DaoStatisticsResponse {
    let total_deposited = latest.total_deposited.to_string();
    let total_compensation_paid = latest.total_compensation_paid.to_string();
    let unclaimed_compensation = latest.unclaimed_compensation.to_string();
    let mining_reward = latest.mining_reward.to_string();
    let deposit_compensation = latest.deposit_compensation.to_string();
    let burnt = latest.burnt.to_string();

    DaoStatisticsResponse {
        total_deposited: total_deposited.clone(),
        total_deposited_ckb: shannon_to_ckb(&total_deposited),
        total_depositors: latest.total_depositors,
        active_deposits: latest.active_deposits,
        total_compensation_paid: total_compensation_paid.clone(),
        total_compensation_paid_ckb: shannon_to_ckb(&total_compensation_paid),
        unclaimed_compensation: unclaimed_compensation.clone(),
        unclaimed_compensation_ckb: shannon_to_ckb(&unclaimed_compensation),
        average_deposit_days: latest.average_deposit_days.clone(),
        estimated_apc: latest.estimated_apc.clone(),
        mining_reward: mining_reward.clone(),
        mining_reward_ckb: shannon_to_ckb(&mining_reward),
        deposit_compensation: deposit_compensation.clone(),
        deposit_compensation_ckb: shannon_to_ckb(&deposit_compensation),
        burnt: burnt.clone(),
        burnt_ckb: shannon_to_ckb(&burnt),
        deposit_change_24h: deltas.deposit_change,
        depositors_change_24h: deltas.depositors_change,
        claimed_compensation_change_24h: deltas.claimed_compensation_change,
        unclaimed_compensation_change_24h: deltas.unclaimed_compensation_change,
    }
}

async fn get_address_dao_summary(
    State(state): State<Arc<AppState>>,
    Path(lock_hash): Path<String>,
) -> ApiResult<AddressDaoSummaryResponse> {
    let hash = hex::decode(lock_hash.strip_prefix("0x").unwrap_or(&lock_hash))
        .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?;
    if hash.len() != 32 {
        return Err(ApiError::bad_request("Invalid lock script hash"));
    }

    let (latest_block_number, latest_ar) = resolve_latest_block_and_ar(&state, "summary")?;
    let cache_key = dao_address_summary_cache_key(&hash, latest_block_number);
    if let Some(cached) = state.mem_cache.get::<AddressDaoSummaryResponse>(&cache_key) {
        return ok(cached);
    }

    let mut active_count = 0i32;
    let mut pending_count = 0i32;
    let mut completed_count = 0i32;
    let mut total_locked: i128 = 0;
    let mut total_comp_earned: i128 = 0;
    let mut total_unclaimed: u128 = 0;
    state
        .store
        .scan_dao_deposits_by_lock(&hash, |_, entry| {
            match entry.status {
                0 => active_count += 1,
                1 => pending_count += 1,
                2 => completed_count += 1,
                _ => {}
            }

            if entry.status == 0 || entry.status == 1 {
                total_locked += entry.capacity as i128;
            }
            if entry.status == 2 {
                if let Some(comp) = entry.compensation {
                    total_comp_earned += comp as i128;
                }
            }

            if (entry.status == 0 || entry.status == 1)
                && entry.deposit_block_number <= latest_block_number
            {
                if entry.capacity < 0 {
                    anyhow::bail!(
                        "negative DAO deposit capacity: deposit_block={}, lock_script_hash=0x{}, capacity={}",
                        entry.deposit_block_number,
                        hex::encode(&entry.lock_script_hash),
                        entry.capacity
                    );
                }
                let capacity = entry.capacity as u128;
                let free_capacity = capacity.checked_sub(DAO_OCCUPIED_CAPACITY).ok_or_else(|| {
                    anyhow::anyhow!(
                        "DAO deposit capacity below occupied capacity: deposit_block={}, lock_script_hash=0x{}, capacity={}",
                        entry.deposit_block_number,
                        hex::encode(&entry.lock_script_hash),
                        capacity
                    )
                })?;
                let ar_deposit = dao_deposit_ar_as_u64(entry, "summary")?;
                if ar_deposit > 0 && latest_ar > ar_deposit {
                    let gross = free_capacity * latest_ar as u128 / ar_deposit as u128;
                    let compensation = gross.checked_sub(free_capacity).ok_or_else(|| {
                        anyhow::anyhow!(
                            "DAO compensation underflow: deposit_block={}, lock_script_hash=0x{}, free_capacity={}, ar_deposit={}, latest_ar={}",
                            entry.deposit_block_number,
                            hex::encode(&entry.lock_script_hash),
                            free_capacity,
                            ar_deposit,
                            latest_ar
                        )
                    })?;
                    total_unclaimed += compensation;
                }
            }
            Ok(())
        })
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let response = if active_count == 0 && pending_count == 0 && completed_count == 0 {
        AddressDaoSummaryResponse {
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
        }
    } else {
        let store = state.store.clone();
        let latest_snapshot =
            tokio::task::spawn_blocking(move || store.get_latest_dao_daily_snapshot())
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
                .map_err(|e| ApiError::internal(e.to_string()))?;
        let estimated_apc = latest_snapshot
            .as_ref()
            .map(snapshot_estimated_apc)
            .transpose()?
            .flatten()
            .unwrap_or_default();

        let total_locked_str = total_locked.to_string();
        let total_comp_str = total_comp_earned.to_string();

        AddressDaoSummaryResponse {
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
        }
    };

    state
        .mem_cache
        .set(&cache_key, &response, DAO_ADDRESS_SUMMARY_CACHE_TTL);
    ok(response)
}

async fn get_statistics(State(state): State<Arc<AppState>>) -> ApiResult<DaoStatisticsResponse> {
    let (latest_block_number, latest_ar) = resolve_latest_block_and_ar(&state, "statistics")?;
    if let Some(latest) = state
        .store
        .get_latest_dao_statistics()
        .map_err(|e| ApiError::internal(e.to_string()))?
    {
        if latest.tip_block_number == latest_block_number {
            let deltas = compute_dao_24h_deltas(&state);
            return ok(dao_latest_to_response(&latest, deltas));
        }
    }

    let cache_key = format!("dao:statistics:tip:{}", latest_block_number);
    if let Some(cached) = state.mem_cache.get::<DaoStatisticsResponse>(&cache_key) {
        return ok(cached);
    }

    let mut acc = DaoStatisticsAccumulator::default();
    state
        .store
        .scan_dao_deposits(|_, entry| {
            accumulate_dao_statistics_entry(&mut acc, entry, latest_block_number, latest_ar)
        })
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let total_depositors = acc.unique_depositors.len() as i32;

    let avg_epochs = if acc.active_filtered_count > 0 {
        (acc.total_blocks_held / acc.active_filtered_count as f64) / 1800.0
    } else {
        0.0
    };

    let store = state.store.clone();
    let latest_snapshot =
        tokio::task::spawn_blocking(move || store.get_latest_dao_daily_snapshot())
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let estimated_apc = match latest_snapshot.as_ref() {
        Some(snapshot) => snapshot_estimated_apc(snapshot)?.unwrap_or_default(),
        None => String::new(),
    };

    let avg_days = epochs_to_days(avg_epochs);

    let total_deposited_str = acc.total_deposited.to_string();
    let total_comp_str = acc.total_compensation_paid.to_string();
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

    let deltas = compute_dao_24h_deltas(&state);

    let response = DaoStatisticsResponse {
        total_deposited: total_deposited_str.clone(),
        total_deposited_ckb: shannon_to_ckb(&total_deposited_str),
        total_depositors,
        active_deposits: acc.active_count,
        total_compensation_paid: total_comp_str.clone(),
        total_compensation_paid_ckb: shannon_to_ckb(&total_comp_str),
        unclaimed_compensation: acc.total_unclaimed.to_string(),
        unclaimed_compensation_ckb: shannon_to_ckb(&acc.total_unclaimed.to_string()),
        average_deposit_days: avg_days,
        estimated_apc,
        mining_reward: mining_reward.clone(),
        mining_reward_ckb: shannon_to_ckb(&mining_reward),
        deposit_compensation: deposit_compensation.clone(),
        deposit_compensation_ckb: shannon_to_ckb(&deposit_compensation),
        burnt: burnt.clone(),
        burnt_ckb: shannon_to_ckb(&burnt),
        deposit_change_24h: deltas.deposit_change,
        depositors_change_24h: deltas.depositors_change,
        claimed_compensation_change_24h: deltas.claimed_compensation_change,
        unclaimed_compensation_change_24h: deltas.unclaimed_compensation_change,
    };

    state
        .mem_cache
        .set(&cache_key, &response, DAO_STATS_CACHE_TTL);
    ok(response)
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

async fn get_top_depositors(
    State(state): State<Arc<AppState>>,
) -> ApiResult<DaoTopDepositorsResponse> {
    let cache_key = "dao:top-depositors";
    if let Some(cached) = state.mem_cache.get::<DaoTopDepositorsResponse>(cache_key) {
        return ok(cached);
    }

    let store = state.store.clone();
    let top = tokio::task::spawn_blocking(move || store.get_dao_top_depositors())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .unwrap_or_else(|| ckbadger_store::DaoTopDepositors {
            tip_block_number: 0,
            depositors: vec![],
        });

    let depositors = top
        .depositors
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let capacity_str = d.total_capacity.to_string();
            let avg_epochs = d.average_deposit_blocks / 1800.0;
            let avg_days = avg_epochs * 4.0 / 24.0;
            DaoTopDepositorResponse {
                rank: (i + 1) as i32,
                lock_script_hash: format!("0x{}", hex::encode(&d.lock_script_hash)),
                address: None,
                total_capacity: capacity_str.clone(),
                total_capacity_ckb: shannon_to_ckb(&capacity_str),
                deposit_count: d.deposit_count,
                average_deposit_days: format_deposit_days(avg_days),
            }
        })
        .collect();

    let response = DaoTopDepositorsResponse { depositors };
    state
        .mem_cache
        .set(cache_key, &response, DAO_STATS_CACHE_TTL);
    ok(response)
}

fn format_deposit_days(days: f64) -> String {
    if days >= 1000.0 {
        format!("{:.1}K", days / 1000.0)
    } else if days < 0.1 {
        "0".to_string()
    } else {
        format!("{:.1}", days)
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
    ensure_withdraw_block_not_before_deposit(params.deposit_block, withdraw_block)?;

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
    if ar_deposit == 0 {
        return Err(ApiError::bad_request(
            "Invalid zero deposit AR — corrupt block header data",
        ));
    }
    let gross = free
        .checked_mul(ar_withdraw as u128)
        .ok_or_else(|| ApiError::internal("DAO compensation multiply overflow"))?
        / ar_deposit as u128;
    let compensation = gross
        .checked_sub(free)
        .ok_or_else(|| ApiError::internal("DAO compensation underflow"))?;

    let total = capacity + compensation;

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

fn ensure_withdraw_block_not_before_deposit(
    deposit_block: i64,
    withdraw_block: i64,
) -> Result<(), (axum::http::StatusCode, axum::Json<ApiError>)> {
    if withdraw_block < deposit_block {
        return Err(ApiError::bad_request(
            "withdraw_block must be greater than or equal to deposit_block",
        ));
    }
    Ok(())
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

fn build_total_depositors_series(
    snapshots: &[ckbadger_store::DaoDailySnapshot],
) -> Result<HashMap<String, i64>, ApiRouteError> {
    if snapshots.is_empty() {
        return Ok(HashMap::new());
    }

    Ok(snapshots
        .iter()
        .map(|snapshot| (snapshot.date.clone(), snapshot.depositors_count))
        .collect())
}

async fn get_total_deposit_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:dao-total-deposit";
    if let Some(cached) = state.mem_cache.get::<ChartResponse>(cache_key) {
        if chart_response_has_data(&cached) {
            return ok(cached);
        }
        state.mem_cache.delete(cache_key);
    }
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        if chart_response_has_data(&cached) {
            state.mem_cache.set(cache_key, &cached, CHART_CACHE_TTL);
            return ok(cached);
        }
        state.cache.delete(cache_key).await;
    }

    let store = state.store.clone();
    let snapshots = tokio::task::spawn_blocking(move || store.list_dao_daily_snapshots())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let depositors_series = build_total_depositors_series(&snapshots)?;

    let data: Vec<ChartDataPoint> = snapshots
        .iter()
        .map(|s| -> Result<ChartDataPoint, ApiRouteError> {
            Ok(ChartDataPoint {
                date: s.date.clone(),
                value: shannon_to_ckb(&s.total_deposited.to_string()),
                value2: Some(depositor_series_value(&depositors_series, &s.date)?.to_string()),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let response = ChartResponse {
        data,
        title: "Total Deposit".to_string(),
        y_axis_label: "CKB".to_string(),
        y2_axis_label: Some("Depositors".to_string()),
    };

    if chart_response_has_data(&response) {
        state.cache.set(cache_key, &response, CHART_CACHE_TTL).await;
        state.mem_cache.set(cache_key, &response, CHART_CACHE_TTL);
    }
    ok(response)
}

async fn get_daily_deposit_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let cache_key = "chart:dao-daily-deposit";
    if let Some(cached) = state.mem_cache.get::<ChartResponse>(cache_key) {
        return ok(cached);
    }
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        state.mem_cache.set(cache_key, &cached, CHART_CACHE_TTL);
        return ok(cached);
    }

    let store = state.store.clone();
    let snapshots = tokio::task::spawn_blocking(move || store.list_dao_daily_snapshots())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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
    state.mem_cache.set(cache_key, &response, CHART_CACHE_TTL);
    ok(response)
}

async fn get_circulation_ratio_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let cache_key = "chart:dao-circulation-ratio";
    if let Some(cached) = state.mem_cache.get::<ChartResponse>(cache_key) {
        return ok(cached);
    }
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        state.mem_cache.set(cache_key, &cached, CHART_CACHE_TTL);
        return ok(cached);
    }

    let store = state.store.clone();
    let snapshots = tokio::task::spawn_blocking(move || store.list_dao_daily_snapshots())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
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
    state.mem_cache.set(cache_key, &response, CHART_CACHE_TTL);
    ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::InMemoryCache;
    use ckbadger_store::types::CachedBlockHeader;
    use ckbadger_store::types::DaoDepositCacheEntry;
    use std::time::Duration;

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
            unclaimed_compensation: 0,
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

    fn header_at(ms: i64) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![0u8; 32],
            parent_hash: vec![0u8; 32],
            timestamp: ms,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0u8; 32],
            transactions_count: 1,
        }
    }

    #[test]
    fn test_build_total_depositors_series_tracks_deposit_and_withdraw_request_transitions() {
        let snapshots = vec![
            ckbadger_store::DaoDailySnapshot {
                date: "2026-02-17".to_string(),
                depositors_count: 1,
                ..snapshot(1, 0)
            },
            ckbadger_store::DaoDailySnapshot {
                date: "2026-02-18".to_string(),
                depositors_count: 2,
                ..snapshot(1, 0)
            },
            ckbadger_store::DaoDailySnapshot {
                date: "2026-02-19".to_string(),
                depositors_count: 1,
                ..snapshot(1, 0)
            },
        ];
        let series = build_total_depositors_series(&snapshots).unwrap();
        assert_eq!(series.get("2026-02-17"), Some(&1));
        assert_eq!(series.get("2026-02-18"), Some(&2));
        assert_eq!(series.get("2026-02-19"), Some(&1));
    }

    #[test]
    fn test_resolve_latest_block_and_ar_from_tip_errors_on_missing_tip() {
        let err = resolve_latest_block_and_ar_from_tip(None, "summary").unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("missing sync tip block while computing DAO summary"));
    }

    #[test]
    fn test_resolve_latest_block_and_ar_from_tip_errors_on_invalid_dao() {
        let mut header = header_at(0);
        header.dao = vec![0u8; 8];
        let err =
            resolve_latest_block_and_ar_from_tip(Some((42, header)), "statistics").unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("invalid DAO field in sync tip block while computing DAO statistics"));
    }

    #[test]
    fn test_depositor_series_value_errors_on_missing_date() {
        let mut series = HashMap::new();
        series.insert("2026-02-18".to_string(), 7);

        assert_eq!(depositor_series_value(&series, "2026-02-18").unwrap(), 7);

        let err = depositor_series_value(&series, "2026-02-19").unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("missing dao depositor series point for date=2026-02-19"));
    }

    #[test]
    fn test_ensure_withdraw_block_not_before_deposit_validates_order() {
        let err = ensure_withdraw_block_not_before_deposit(100, 99).unwrap_err();
        assert_eq!(
            err.1 .0.message,
            "withdraw_block must be greater than or equal to deposit_block"
        );
        assert!(ensure_withdraw_block_not_before_deposit(100, 100).is_ok());
    }

    #[test]
    fn test_dao_deposit_ar_as_u64_rejects_negative_values() {
        let entry = DaoDepositCacheEntry {
            capacity: 200_00000000,
            deposit_block_number: 123,
            lock_script_hash: vec![0xAB; 32],
            deposit_ar: -1,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };

        let err = dao_deposit_ar_as_u64(&entry, "summary").unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid negative DAO deposit AR while computing summary"));
        assert!(err.to_string().contains("deposit_block=123"));
    }

    #[test]
    fn test_dao_deposit_ar_as_u64_accepts_non_negative_values() {
        let entry = DaoDepositCacheEntry {
            capacity: 200_00000000,
            deposit_block_number: 456,
            lock_script_hash: vec![0xCD; 32],
            deposit_ar: 42,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };

        assert_eq!(dao_deposit_ar_as_u64(&entry, "statistics").unwrap(), 42);
    }

    #[test]
    fn test_accumulate_dao_statistics_entry_tracks_active_and_completed() {
        let mut acc = DaoStatisticsAccumulator::default();
        let active = DaoDepositCacheEntry {
            capacity: (DAO_OCCUPIED_CAPACITY + 1_000) as i64,
            deposit_block_number: 90,
            lock_script_hash: vec![0xAB; 32],
            deposit_ar: 100,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        let completed = DaoDepositCacheEntry {
            capacity: 0,
            deposit_block_number: 80,
            lock_script_hash: vec![0xCD; 32],
            deposit_ar: 100,
            status: 2,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: Some(500),
        };

        accumulate_dao_statistics_entry(&mut acc, &active, 100, 200).unwrap();
        accumulate_dao_statistics_entry(&mut acc, &completed, 100, 200).unwrap();

        assert_eq!(acc.total_deposited, active.capacity as i128);
        assert_eq!(acc.unique_depositors.len(), 1);
        assert_eq!(acc.active_count, 1);
        assert_eq!(acc.total_compensation_paid, 500);
        assert_eq!(acc.total_blocks_held, 10.0);
        assert_eq!(acc.active_filtered_count, 1);
        assert_eq!(acc.total_unclaimed, 1_000);
    }

    #[test]
    fn test_dao_address_summary_cache_key_contains_tip_and_lock() {
        let lock_hash = [0xAB; 32];
        let key = dao_address_summary_cache_key(&lock_hash, 12345);
        assert_eq!(
            key,
            format!("dao:summary:tip:12345:lock:0x{}", hex::encode(lock_hash))
        );
    }

    #[test]
    fn test_address_dao_summary_response_roundtrips_in_mem_cache() {
        let cache = InMemoryCache::new();
        let key = "dao:summary:test";
        let response = AddressDaoSummaryResponse {
            has_dao_activity: true,
            active_deposits_count: 1,
            pending_withdrawals_count: 2,
            completed_withdrawals_count: 3,
            total_locked_capacity: "100".to_string(),
            total_locked_ckb: "1".to_string(),
            unclaimed_compensation: "10".to_string(),
            unclaimed_compensation_ckb: "0.1".to_string(),
            total_compensation_earned: "20".to_string(),
            total_compensation_earned_ckb: "0.2".to_string(),
            estimated_apc: "3.14".to_string(),
        };

        cache.set(key, &response, Duration::from_secs(30));
        let loaded = cache.get::<AddressDaoSummaryResponse>(key).unwrap();
        assert_eq!(loaded.has_dao_activity, response.has_dao_activity);
        assert_eq!(loaded.active_deposits_count, response.active_deposits_count);
        assert_eq!(
            loaded.completed_withdrawals_count,
            response.completed_withdrawals_count
        );
        assert_eq!(loaded.total_locked_capacity, response.total_locked_capacity);
        assert_eq!(loaded.estimated_apc, response.estimated_apc);
    }

    #[test]
    fn test_chart_response_roundtrips_in_mem_cache() {
        let cache = InMemoryCache::new();
        let key = "dao:chart:test";
        let response = ChartResponse {
            data: vec![ChartDataPoint {
                date: "2026-03-04".to_string(),
                value: "1.23".to_string(),
                value2: Some("42".to_string()),
            }],
            title: "Sample".to_string(),
            y_axis_label: "CKB".to_string(),
            y2_axis_label: Some("Count".to_string()),
        };

        cache.set(key, &response, Duration::from_secs(30));
        let loaded = cache.get::<ChartResponse>(key).unwrap();
        assert_eq!(loaded.title, response.title);
        assert_eq!(loaded.y_axis_label, response.y_axis_label);
        assert_eq!(loaded.data.len(), 1);
        assert_eq!(loaded.data[0].date, "2026-03-04");
        assert_eq!(loaded.data[0].value2.as_deref(), Some("42"));
    }
}
