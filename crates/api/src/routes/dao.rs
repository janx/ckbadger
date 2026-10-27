use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_common::dao::{
    calculate_dao_compensation_from_ar, calculate_estimated_apc, extract_s_from_dao,
};
use ckbadger_store::keys;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::response::{
    chart_response_has_data, default_limit, ok, ApiError, ApiResult, ApiRouteError, ChartDataPoint,
    ChartResponse, CursorPaginatedResponse,
};
use crate::utils::daily_series::consecutive_daily_chart;
use crate::utils::stats_bucket::{complete_days_cache_key, is_complete_iso_day, load_chain_time};
use crate::utils::{
    dao_supply, parse_hash32, script_to_address, shannon_to_ckb, shannon_to_ckb_signed,
    validate_block_number,
};
use crate::AppState;
use tracing::instrument;

#[path = "dao_epoch.rs"]
mod dao_epoch;
#[path = "dao_evidence.rs"]
mod dao_evidence;

const CHART_CACHE_TTL: Duration = Duration::from_secs(3600);
const DAO_STATS_CACHE_TTL: Duration = Duration::from_secs(30);
const DAO_ADDRESS_SUMMARY_CACHE_TTL: Duration = Duration::from_secs(30);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/dao/deposits", get(list_deposits))
        .route("/dao/deposits/{lock_hash}", get(get_deposits_by_address))
        .route("/dao/summary/{lock_hash}", get(get_address_dao_summary))
        .route("/dao/statistics", get(get_statistics))
        .route("/dao/evidence", get(dao_evidence::get_evidence))
        .route("/dao/top-depositors", get(get_top_depositors))
        .route("/dao/calculator", get(calculate_compensation))
        .route("/dao/charts/total-deposit", get(get_total_deposit_chart))
        .route("/dao/charts/daily-deposit", get(get_daily_deposit_chart))
        .route(
            "/dao/charts/daily-depositors",
            get(get_daily_depositors_chart),
        )
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
) -> Result<(i64, u64, i64, u64), ApiRouteError> {
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
    let s = extract_s_from_dao(&header.dao).ok_or_else(|| {
        ApiError::internal(format!(
            "invalid DAO S field in sync tip block while computing DAO {}: block_number={}, dao_len={}",
            context,
            block_number,
            header.dao.len()
        ))
    })?;
    Ok((block_number, ar, header.timestamp, s))
}

pub(super) fn resolve_latest_block_and_ar(
    state: &AppState,
    context: &str,
) -> Result<(i64, u64, i64, u64), ApiRouteError> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoStatisticsResponse {
    /// Block the indexer computed these statistics at. The singleton trails the
    /// sync tip by at most one batch commit, so this makes the as-of block
    /// explicit instead of implying "current".
    pub tip_block_number: i64,
    /// Exact chain-derived block-0 burn policy total, in shannons.
    pub genesis_baseline_burnt: String,
    pub total_deposited: String,
    pub total_deposited_ckb: String,
    pub total_depositors: i32,
    pub active_deposits: i32,
    pub deposit_cells_count: i32,
    pub withdrawal_request_cells_count: i32,
    pub dao_capacity: String,
    pub dao_free_capacity: String,
    /// Raw DAO C and U copied into the same indexer singleton as dao_free_capacity.
    pub tip_total_issuance: String,
    pub tip_protocol_occupied_capacity: String,
    pub rolling_24h_baseline_block_number: Option<i64>,
    pub rolling_24h_cutoff_chain_time_ms: Option<String>,
    pub total_compensation_paid: String,
    pub total_compensation_paid_ckb: String,
    pub unclaimed_compensation: String,
    pub unclaimed_compensation_ckb: String,
    pub average_deposit_days: Option<String>,
    pub estimated_apc: String,
    pub mining_reward: String,
    pub mining_reward_ckb: String,
    pub deposit_compensation: String,
    pub deposit_compensation_ckb: String,
    pub burnt: String,
    pub burnt_ckb: String,
    pub pending_withdrawal_capacity: String,
    pub pending_withdrawal_capacity_ckb: String,
    pub deposit_change_24h: Option<String>,
    pub depositors_change_24h: Option<i32>,
    pub claimed_compensation_change_24h: Option<String>,
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
    /// Block the indexer built this leaderboard at.
    pub tip_block_number: i64,
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
    /// Realized annualized return; absent for a zero chain-time span or no free capacity.
    pub apc: Option<String>,
}

/// Convert a DAO deposit entry from the store to an API response.
/// The outpoint_key encodes tx_hash(32B) + output_index(2B BE).
pub(super) fn deposit_to_response(
    outpoint_key: &[u8],
    entry: &ckbadger_store::DaoDepositCacheEntry,
    state: &AppState,
) -> Result<DaoDepositResponse, ApiRouteError> {
    let (tx_hash_bytes, output_index) = keys::decode_outpoint(outpoint_key);

    let timestamp_at = |block: i64, event: &str| -> Result<String, ApiRouteError> {
        let header = state
            .store
            .get_block_header(block)
            .map_err(|e| {
                ApiError::internal(format!(
                    "failed to read {event} header for DAO deposit 0x{} at block {block}: {e}",
                    hex::encode(outpoint_key)
                ))
            })?
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "missing {event} header for DAO deposit 0x{} at block {block}",
                    hex::encode(outpoint_key)
                ))
            })?;
        chrono::DateTime::from_timestamp_millis(header.timestamp)
            .map(|time| time.to_rfc3339())
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "invalid {event} timestamp for DAO deposit 0x{} at block {block}: {} ms",
                    hex::encode(outpoint_key),
                    header.timestamp
                ))
            })
    };
    let deposit_timestamp = timestamp_at(entry.deposit_block_number, "deposit")?;
    let withdraw_request_timestamp = entry
        .withdraw_request_block
        .map(|block| timestamp_at(block, "withdraw request"))
        .transpose()?;
    let withdraw_timestamp = entry
        .withdraw_block
        .map(|block| timestamp_at(block, "withdraw completion"))
        .transpose()?;

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
            .map_err(|e| {
                ApiError::internal(format!(
                    "failed to encode address for DAO deposit 0x{}: {e}",
                    hex::encode(outpoint_key)
                ))
            })?;
            let code_hash = format!("0x{}", hex::encode(&cell_info.lock_code_hash));
            (Some(addr), Some(code_hash))
        }
        Ok(None) => (None, None),
        Err(e) => {
            return Err(ApiError::internal(format!(
                "failed to read append-only cell for DAO deposit 0x{}: {e}",
                hex::encode(outpoint_key)
            )))
        }
    };

    // Compensation is known the moment a withdraw request exists: it freezes at
    // the request AR. Derived through the same helper the DAO aggregates use, so
    // the listing and `/dao/statistics` can never disagree. Still accruing
    // (status 0) has no settled value to report.
    let compensation = if entry.withdraw_request_block.is_some() {
        Some(
            ckbadger_store::dao_frozen_request_compensation(entry).map_err(|e| {
                ApiError::internal(format!(
                    "failed to derive DAO compensation for outpoint 0x{}: {e:#}",
                    hex::encode(outpoint_key)
                ))
            })?,
        )
    } else {
        None
    };

    Ok(DaoDepositResponse {
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
        compensation: compensation.map(|c| c.to_string()),
    })
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
        .collect::<Result<_, _>>()?;

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
    let hash = parse_hash32(&lock_hash, "lock_hash")?;

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
        .collect::<Result<_, _>>()?;

    ok(CursorPaginatedResponse::without_total(
        deposits,
        limit as i64,
        next_cursor,
    ))
}

/// Compute estimated APC from tip block epoch info using the CKB Explorer model.
///
/// Takes the whole state (not just the store) so the genesis baseline comes from
/// `AppState::genesis_baseline()` — the single read path for it, including its
/// "still initializing" reporting — instead of a second, divergent copy here.
pub(super) fn estimated_apc_from_state(state: &AppState) -> Result<String, ApiRouteError> {
    let tip = state
        .store
        .get_sync_tip_block()
        .map_err(|e: anyhow::Error| ApiError::internal(e.to_string()))?;
    match tip {
        Some((_, header)) if header.epoch_length > 0 => {
            let genesis_issuance = state.genesis_baseline()?.total_issuance;
            let apc = calculate_estimated_apc(
                header.epoch_number,
                header.epoch_index,
                header.epoch_length,
                genesis_issuance,
            );
            Ok(if apc > 0.0 {
                format!("{:.2}", apc)
            } else {
                String::new()
            })
        }
        _ => Ok(String::new()),
    }
}

#[derive(Default)]
struct DaoDeltas {
    deposit_change: Option<String>,
    depositors_change: Option<i32>,
    claimed_compensation_change: Option<String>,
    unclaimed_compensation_change: Option<String>,
}

fn compute_dao_24h_deltas(latest: &ckbadger_store::DaoLatestStatistics) -> DaoDeltas {
    let Some(rolling) = &latest.rolling_24h else {
        return DaoDeltas::default();
    };
    DaoDeltas {
        deposit_change: Some(shannon_to_ckb_signed(rolling.deposited_change)),
        depositors_change: Some(rolling.depositors_change),
        claimed_compensation_change: Some(shannon_to_ckb_signed(
            rolling.claimed_compensation_change,
        )),
        unclaimed_compensation_change: Some(shannon_to_ckb_signed(
            rolling.unclaimed_compensation_change,
        )),
    }
}

fn dao_latest_to_response(
    latest: &ckbadger_store::DaoLatestStatistics,
    deltas: DaoDeltas,
    genesis_baseline_burnt: i128,
) -> DaoStatisticsResponse {
    let tip_block_number = latest.tip_block_number;
    let total_deposited = latest.total_deposited.to_string();
    let total_compensation_paid = latest.total_compensation_paid.to_string();
    let unclaimed_compensation = latest.unclaimed_compensation.to_string();
    let mining_reward = latest.mining_reward.to_string();
    let deposit_compensation = latest.deposit_compensation.to_string();
    let burnt = latest.burnt.to_string();

    DaoStatisticsResponse {
        tip_block_number,
        genesis_baseline_burnt: genesis_baseline_burnt.to_string(),
        total_deposited: total_deposited.clone(),
        total_deposited_ckb: shannon_to_ckb(&total_deposited),
        total_depositors: latest.total_depositors,
        active_deposits: latest.active_deposits,
        deposit_cells_count: latest.deposit_cells_count,
        withdrawal_request_cells_count: latest.withdrawal_request_cells_count,
        dao_capacity: latest.dao_capacity.to_string(),
        dao_free_capacity: latest.dao_free_capacity.to_string(),
        tip_total_issuance: latest.tip_total_issuance.to_string(),
        tip_protocol_occupied_capacity: latest.tip_occupied_capacity.to_string(),
        rolling_24h_baseline_block_number: latest
            .rolling_24h
            .as_ref()
            .map(|r| r.baseline_block_number),
        rolling_24h_cutoff_chain_time_ms: latest
            .rolling_24h
            .as_ref()
            .map(|r| r.cutoff_chain_time_ms.to_string()),
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
        pending_withdrawal_capacity: latest.pending_withdrawal_capacity.to_string(),
        pending_withdrawal_capacity_ckb: shannon_to_ckb(
            &latest.pending_withdrawal_capacity.to_string(),
        ),
        deposit_change_24h: deltas.deposit_change,
        depositors_change_24h: deltas.depositors_change,
        claimed_compensation_change_24h: deltas.claimed_compensation_change,
        unclaimed_compensation_change_24h: deltas.unclaimed_compensation_change,
    }
}

#[instrument(skip(state), level = "debug")]
async fn get_address_dao_summary(
    State(state): State<Arc<AppState>>,
    Path(lock_hash): Path<String>,
) -> ApiResult<AddressDaoSummaryResponse> {
    let hash = parse_hash32(&lock_hash, "lock_hash")?;

    let (latest_block_number, latest_ar, _tip_timestamp, _s) =
        resolve_latest_block_and_ar(&state, "summary")?;
    let cache_key = dao_address_summary_cache_key(&hash, latest_block_number);
    if let Some(cached) = state.mem_cache.get::<AddressDaoSummaryResponse>(&cache_key) {
        return ok(cached);
    }

    let mut active_count = 0i32;
    let mut pending_count = 0i32;
    let mut completed_count = 0i32;
    let mut total_locked: i128 = 0;
    let mut total_comp_earned: i128 = 0;
    let mut total_unclaimed: i128 = 0;
    state
        .store
        .scan_dao_deposits_by_lock(&hash, |_, entry| {
            match entry.status {
                0 => {
                    active_count = active_count.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!(
                            "DAO active deposit count overflow for lock_hash=0x{}",
                            hex::encode(&hash)
                        )
                    })?;
                    total_locked = total_locked
                        .checked_add(i128::from(entry.capacity))
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "DAO locked capacity overflow for lock_hash=0x{}",
                                hex::encode(&hash)
                            )
                        })?;
                }
                1 => {
                    pending_count = pending_count.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!(
                            "DAO pending withdrawal count overflow for lock_hash=0x{}",
                            hex::encode(&hash)
                        )
                    })?;
                    total_locked = total_locked
                        .checked_add(i128::from(entry.capacity))
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "DAO locked capacity overflow for lock_hash=0x{}",
                                hex::encode(&hash)
                            )
                        })?;
                }
                2 => {
                    completed_count = completed_count.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!(
                            "DAO completed withdrawal count overflow for lock_hash=0x{}",
                            hex::encode(&hash)
                        )
                    })?;
                }
                status => anyhow::bail!(
                    "unknown DAO deposit status {} for deposit_block={}, lock_hash=0x{}",
                    status,
                    entry.deposit_block_number,
                    hex::encode(&hash)
                ),
            }

            let contribution = ckbadger_store::dao_compensation_for_entry_at(
                entry,
                latest_block_number,
                latest_ar,
            )?;
            total_comp_earned = total_comp_earned
                .checked_add(contribution.claimed)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "DAO claimed compensation overflow for lock_hash=0x{}",
                        hex::encode(&hash)
                    )
                })?;
            total_unclaimed = total_unclaimed
                .checked_add(contribution.unclaimed)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "DAO unclaimed compensation overflow for lock_hash=0x{}",
                        hex::encode(&hash)
                    )
                })?;
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
        let estimated_apc = estimated_apc_from_state(&state)?;

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

/// Report a missing DAO singleton with the state that explains it.
///
/// The indexer writes both singletons after every committed batch and advances
/// them again after every reorg rollback (rollback no longer deletes them), so
/// they are absent only before the first post-genesis batch commits. That
/// startup window is the explicit `initializing` state; absence at a synced tip
/// is an invariant violation and must say so, with the tip it was observed at,
/// instead of being papered over with an empty default.
pub(crate) fn dao_singleton_missing(state: &AppState, singleton: &str) -> ApiRouteError {
    match state.store.get_sync_tip_block() {
        Err(e) => ApiError::internal(format!(
            "failed to load sync tip block while reporting missing {}: {}",
            singleton, e
        )),
        Ok(None) => ApiError::initializing(format!(
            "{} not written yet: the indexer has not committed a block",
            singleton
        )),
        Ok(Some((0, _))) => ApiError::initializing(format!(
            "{} not written yet: only genesis is committed",
            singleton
        )),
        Ok(Some((tip_block_number, _))) => ApiError::internal(format!(
            "missing {} at sync tip block {}: the indexer writes it after every batch commit \
             and after every reorg rollback",
            singleton, tip_block_number
        )),
    }
}

/// DAO statistics come from the indexer-maintained `dao_latest_stats`
/// singleton and from nowhere else.
///
/// There is deliberately no read-side recomputation: a second derivation of the
/// same numbers drifted from the indexer's (different treasury and compensation
/// formulas) and, worse, it silently stood in for a missing singleton, which is
/// how the post-reorg singleton gap stayed invisible. The singleton lags the
/// sync tip by at most one batch commit, so the response reports the block it
/// was computed at rather than pretending to be current.
#[instrument(skip(state), level = "debug")]
async fn get_statistics(State(state): State<Arc<AppState>>) -> ApiResult<DaoStatisticsResponse> {
    let store = state.store.clone();
    let latest = tokio::task::spawn_blocking(move || store.get_latest_dao_statistics())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let Some(latest) = latest else {
        return Err(dao_singleton_missing(&state, "dao_latest_stats"));
    };

    let deltas = compute_dao_24h_deltas(&latest);
    let genesis_baseline_burnt = state.genesis_baseline()?.burnt;
    ok(dao_latest_to_response(
        &latest,
        deltas,
        genesis_baseline_burnt,
    ))
}

async fn get_top_depositors(
    State(state): State<Arc<AppState>>,
) -> ApiResult<DaoTopDepositorsResponse> {
    let store = state.store.clone();
    let top = tokio::task::spawn_blocking(move || store.get_dao_top_depositors())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let Some(top) = top else {
        return Err(dao_singleton_missing(&state, "dao_top_depositors"));
    };

    // Keyed by the singleton's own tip: a refreshed leaderboard is served the
    // moment the indexer writes it instead of waiting out a TTL, and a failed
    // or absent read never reaches the cache at all.
    let cache_key = format!("dao:top-depositors:tip:{}", top.tip_block_number);
    if let Some(cached) = state.mem_cache.get::<DaoTopDepositorsResponse>(&cache_key) {
        return ok(cached);
    }

    let store = state.store.clone();
    let network = state.ckb_network.clone();
    let lock_hashes: Vec<Vec<u8>> = top
        .depositors
        .iter()
        .map(|d| d.lock_script_hash.clone())
        .collect();
    let addresses = tokio::task::spawn_blocking(move || {
        lock_hashes
            .iter()
            .map(|lock_hash| {
                Ok(match store.get_lock_script(lock_hash)? {
                    Some(entry) => Some(
                        script_to_address(&entry.code_hash, entry.hash_type, &entry.args, &network)
                            .map_err(|e| {
                                anyhow::anyhow!(
                                    "failed to encode address for top depositor lock_hash=0x{}: {}",
                                    hex::encode(lock_hash),
                                    e
                                )
                            })?,
                    ),
                    None => None,
                })
            })
            .collect::<anyhow::Result<Vec<Option<String>>>>()
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let depositors = top
        .depositors
        .iter()
        .zip(addresses)
        .enumerate()
        .map(
            |(i, (d, address))| -> Result<DaoTopDepositorResponse, ApiRouteError> {
                let capacity_str = d.total_capacity.to_string();
                Ok(DaoTopDepositorResponse {
                    rank: (i + 1) as i32,
                    lock_script_hash: format!("0x{}", hex::encode(&d.lock_script_hash)),
                    address,
                    total_capacity: capacity_str.clone(),
                    total_capacity_ckb: shannon_to_ckb(&capacity_str),
                    deposit_count: d.deposit_count,
                    average_deposit_days: format_deposit_days_exact(
                        d.total_deposit_ms,
                        d.deposit_count,
                    )?,
                })
            },
        )
        .collect::<Result<Vec<_>, _>>()?;

    let response = DaoTopDepositorsResponse {
        tip_block_number: top.tip_block_number,
        depositors,
    };
    state
        .mem_cache
        .set(&cache_key, &response, DAO_STATS_CACHE_TTL);
    ok(response)
}

fn format_deposit_days_exact(total_ms: i128, deposit_count: i32) -> Result<String, ApiRouteError> {
    if total_ms < 0 || deposit_count <= 0 {
        return Err(ApiError::internal(format!(
            "invalid top DAO depositor duration: total_ms={total_ms}, deposit_count={deposit_count}"
        )));
    }
    let denominator = i128::from(deposit_count) * 86_400_000;
    let rounded_tenths = |denominator: i128| -> Result<i128, ApiRouteError> {
        let scaled = total_ms
            .checked_mul(10)
            .and_then(|v| v.checked_add(denominator / 2))
            .ok_or_else(|| ApiError::internal("top DAO duration formatting overflow"))?;
        Ok(scaled / denominator)
    };
    if total_ms >= denominator * 1000 {
        let tenths = rounded_tenths(denominator * 1000)?;
        Ok(format!("{}.{}K", tenths / 10, tenths % 10))
    } else if total_ms * 10 < denominator {
        Ok("0".to_string())
    } else {
        let tenths = rounded_tenths(denominator)?;
        Ok(format!("{}.{}", tenths / 10, tenths % 10))
    }
}

#[instrument(skip(state), level = "debug")]
async fn calculate_compensation(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CalculatorParams>,
) -> ApiResult<CalculatorResponse> {
    let capacity: u128 = params
        .capacity
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid capacity"))?;

    // Validate before any store access: `get_block_header` encodes the number
    // into a store key, and a negative one trips an assert that aborts the
    // process under `panic = "abort"`. This handler runs on the axum task
    // itself, so the abort is immediate.
    let deposit_block = validate_block_number(params.deposit_block, "deposit_block")?;
    let requested_withdraw_block = params
        .withdraw_block
        .map(|bn| validate_block_number(bn, "withdraw_block"))
        .transpose()?;

    let latest_block = match state
        .store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(format!("sync tip unavailable: {}", e)))?
    {
        Some((number, _)) => number,
        None => {
            return Err(ApiError::initializing(
                "no synced block yet: the DAO calculator needs a chain tip",
            ))
        }
    };

    let withdraw_block = requested_withdraw_block.unwrap_or(latest_block);
    ensure_withdraw_block_not_before_deposit(deposit_block, withdraw_block)?;

    let deposit_header = state.store.get_block_header(deposit_block).map_err(|e| {
        ApiError::internal(format!(
            "block header unavailable for deposit_block={}: {}",
            deposit_block, e
        ))
    })?;

    let withdraw_header = state.store.get_block_header(withdraw_block).map_err(|e| {
        ApiError::internal(format!(
            "block header unavailable for withdraw_block={}: {}",
            withdraw_block, e
        ))
    })?;

    let (ar_deposit, ar_withdraw, duration_ms) = match (deposit_header, withdraw_header) {
        (Some(d), Some(w)) => {
            let ar_d = extract_ar(&d.dao).ok_or_else(|| {
                ApiError::internal(format!(
                    "invalid DAO field in deposit block header: block_number={}",
                    deposit_block
                ))
            })?;
            let ar_w = extract_ar(&w.dao).ok_or_else(|| {
                ApiError::internal(format!(
                    "invalid DAO field in withdraw block header: block_number={}",
                    withdraw_block
                ))
            })?;
            let duration_ms = i128::from(w.chain_time_ms) - i128::from(d.chain_time_ms);
            (ar_d, ar_w, duration_ms)
        }
        _ => {
            return Err(ApiError::bad_request(
                "Unable to load deposit/withdraw block headers for compensation calculation",
            ));
        }
    };

    let capacity_i64 = i64::try_from(capacity)
        .map_err(|_| ApiError::bad_request("Capacity exceeds valid range"))?;
    let compensation_i64 = calculate_dao_compensation_from_ar(
        capacity_i64,
        i64::try_from(ckbadger_common::dao::DAO_OCCUPIED_CAPACITY)
            .map_err(|_| ApiError::internal("DAO occupied capacity exceeds i64"))?,
        ar_deposit,
        ar_withdraw,
    )
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let compensation = compensation_i64 as u128;
    let free = capacity - ckbadger_common::dao::DAO_OCCUPIED_CAPACITY as u128;

    let total = capacity + compensation;

    let apc = realized_apc(compensation, free, duration_ms).map_err(|error| {
        ApiError::internal(format!(
            "DAO calculator deposit_block={deposit_block} withdraw_block={withdraw_block}: {error}"
        ))
    })?;

    ok(CalculatorResponse {
        capacity: capacity.to_string(),
        capacity_ckb: shannon_to_ckb(&capacity.to_string()),
        deposit_block,
        withdraw_block,
        estimated_compensation: compensation.to_string(),
        estimated_compensation_ckb: shannon_to_ckb(&compensation.to_string()),
        total_withdrawable: total.to_string(),
        total_withdrawable_ckb: shannon_to_ckb(&total.to_string()),
        apc,
    })
}

fn realized_apc(
    compensation: u128,
    free: u128,
    duration_ms: i128,
) -> Result<Option<String>, String> {
    // 365.25 days, expressed exactly in milliseconds.  This is an annualized
    // historical return, so the actual effective chain-time span is required.
    const YEAR_MS: i128 = 31_557_600_000;
    if duration_ms < 0 {
        return Err(format!(
            "DAO calculator chain time regressed: duration_ms={duration_ms}"
        ));
    }
    if duration_ms == 0 || free == 0 {
        return Ok(None);
    }
    let compensation = i128::try_from(compensation).map_err(|_| "DAO compensation exceeds i128")?;
    let free = i128::try_from(free).map_err(|_| "DAO free capacity exceeds i128")?;
    let numerator = compensation
        .checked_mul(YEAR_MS)
        .ok_or("DAO APC numerator overflow")?;
    let denominator = free
        .checked_mul(duration_ms)
        .ok_or("DAO APC denominator overflow")?;
    crate::utils::exact_ratio::format_percent(numerator, denominator, 2)
        .map(|value| Some(format!("{value}%")))
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

/// The DAO daily snapshots, read off the async runtime under the request's
/// pinned view.
async fn load_dao_daily_snapshots(
    state: &AppState,
) -> Result<Vec<ckbadger_store::DaoDailySnapshot>, ApiRouteError> {
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.list_dao_daily_snapshots())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))
}

async fn get_total_deposit_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let chain = load_chain_time(&state.store).await?;
    let cache_key = complete_days_cache_key("chart:dao-total-deposit:v2", chain)?;
    let cache_key = cache_key.as_str();
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

    let snapshots = load_dao_daily_snapshots(&state).await?;
    let depositors_series = build_total_depositors_series(&snapshots)?;

    // Complete days only: the tip block's snapshot is a running value, and
    // consumers derive daily changes from consecutive points.
    let mut data: Vec<ChartDataPoint> = Vec::with_capacity(snapshots.len());
    for s in &snapshots {
        if !is_complete_iso_day(chain, "DAO daily snapshots", &s.date)? {
            continue;
        }
        data.push(ChartDataPoint {
            date: s.date.clone(),
            value: shannon_to_ckb(&s.total_deposited.to_string()),
            value2: Some(depositor_series_value(&depositors_series, &s.date)?.to_string()),
        });
    }

    let data = consecutive_daily_chart(state.store.clone(), chain, "DAO snapshot", data).await?;
    let response = ChartResponse {
        data,
        title: "Total Deposit".to_string(),
        y_axis_label: "CKB".to_string(),
        y2_axis_label: Some("Current depositors".to_string()),
    };

    if chart_response_has_data(&response) {
        state.cache.set(cache_key, &response, CHART_CACHE_TTL).await;
        state.mem_cache.set(cache_key, &response, CHART_CACHE_TTL);
    }
    ok(response)
}

async fn get_daily_deposit_chart(State(state): State<Arc<AppState>>) -> ApiResult<ChartResponse> {
    let chain = load_chain_time(&state.store).await?;
    let cache_key = complete_days_cache_key("chart:dao-daily-deposit", chain)?;
    let cache_key = cache_key.as_str();
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

    let snapshots = load_dao_daily_snapshots(&state).await?;
    // Deltas use every snapshot; only complete days are charted (they precede
    // every in-progress day, so no charted delta spans one).
    let mut data = Vec::with_capacity(snapshots.len());
    for point in build_daily_deposit_series(&snapshots)? {
        if is_complete_iso_day(chain, "DAO daily snapshots", &point.date)? {
            data.push(point);
        }
    }

    let response = ChartResponse {
        data,
        title: "Daily Deposit".to_string(),
        y_axis_label: "CKB".to_string(),
        y2_axis_label: Some("Count".to_string()),
    };

    if chart_response_has_data(&response) {
        state.cache.set(cache_key, &response, CHART_CACHE_TTL).await;
        state.mem_cache.set(cache_key, &response, CHART_CACHE_TTL);
    }
    ok(response)
}

/// Daily gross deposits, derived as day-over-day deltas of the cumulative
/// series (`cumulative_deposit_amount` is gross and never reduced by
/// withdrawals, matching the official explorer's `daily_dao_deposit`).
///
/// The first snapshot day is measured against a zero baseline: nothing was
/// deposited before the chain's first DAO day, so its delta is its own
/// cumulative total. Pairing snapshots off against each other instead dropped
/// that day entirely and hid launch day's deposits from the chart.
fn build_daily_deposit_series(
    snapshots: &[ckbadger_store::DaoDailySnapshot],
) -> Result<Vec<ChartDataPoint>, ApiRouteError> {
    let mut data = Vec::with_capacity(snapshots.len());
    let mut previous: Option<&ckbadger_store::DaoDailySnapshot> = None;

    for snapshot in snapshots {
        let (prev_amount, prev_deposits, prev_label) = match previous {
            Some(prev) => (
                prev.cumulative_deposit_amount,
                prev.new_deposits,
                prev.date.as_str(),
            ),
            None => (0, 0, "the zero baseline before the first snapshot day"),
        };
        if snapshot.cumulative_deposit_amount < prev_amount {
            return Err(ApiError::internal(format!(
                "cumulative_deposit_amount decreased between {} and {}",
                prev_label, snapshot.date
            )));
        }
        if snapshot.new_deposits < prev_deposits {
            return Err(ApiError::internal(format!(
                "new_deposits decreased between {} and {}",
                prev_label, snapshot.date
            )));
        }
        let daily_deposited = snapshot.cumulative_deposit_amount - prev_amount;
        let daily_deposits = snapshot.new_deposits - prev_deposits;

        data.push(ChartDataPoint {
            date: snapshot.date.clone(),
            value: shannon_to_ckb(&daily_deposited.to_string()),
            value2: Some(daily_deposits.to_string()),
        });
        previous = Some(snapshot);
    }

    Ok(data)
}

async fn get_daily_depositors_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let chain = load_chain_time(&state.store).await?;
    let cache_key = complete_days_cache_key("chart:dao-daily-depositors", chain)?;
    let cache_key = cache_key.as_str();
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        if chart_response_has_data(&cached) {
            return ok(cached);
        }
        state.cache.delete(cache_key).await;
    }

    let snapshots = load_dao_daily_snapshots(&state).await?;

    let mut data: Vec<ChartDataPoint> = Vec::with_capacity(snapshots.len());
    for s in &snapshots {
        if !is_complete_iso_day(chain, "DAO daily snapshots", &s.date)? {
            continue;
        }
        data.push(ChartDataPoint {
            date: s.date.clone(),
            value: s.daily_depositor_addresses.to_string(),
            value2: None,
        });
    }

    let response = ChartResponse {
        data,
        title: "Daily DAO Depositors".to_string(),
        y_axis_label: "Addresses".to_string(),
        y2_axis_label: None,
    };

    if chart_response_has_data(&response) {
        state.cache.set(cache_key, &response, CHART_CACHE_TTL).await;
    }
    ok(response)
}

async fn get_circulation_ratio_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    let chain = load_chain_time(&state.store).await?;
    let cache_key = complete_days_cache_key("chart:dao-circulation-ratio", chain)?;
    let cache_key = cache_key.as_str();
    if let Some(cached) = state.mem_cache.get::<ChartResponse>(cache_key) {
        return ok(cached);
    }
    if let Some(cached) = state.cache.get::<ChartResponse>(cache_key).await {
        state.mem_cache.set(cache_key, &cached, CHART_CACHE_TTL);
        return ok(cached);
    }

    let snapshots = load_dao_daily_snapshots(&state).await?;

    let burnt = state.genesis_baseline()?.burnt;
    let mut data = Vec::with_capacity(snapshots.len());
    for s in &snapshots {
        if !is_complete_iso_day(chain, "DAO daily snapshots", &s.date)? {
            continue;
        }
        let Some(supply) = dao_supply(s, burnt).map_err(|e| ApiError::internal(e.to_string()))?
        else {
            return Err(ApiError::internal(format!(
                "missing DAO snapshot total_issuance for {}. delete RocksDB and re-sync from genesis",
                s.date
            )));
        };
        let circulating = supply.circulating;
        if circulating <= 0 {
            return Err(ApiError::internal(format!(
                "DAO daily snapshot {} has nonpositive circulating supply: {circulating}",
                s.date
            )));
        }
        if s.total_deposited < 0 {
            return Err(ApiError::internal(format!(
                "negative total_deposited in dao_daily_snapshots for {}: {}",
                s.date, s.total_deposited
            )));
        }
        let ratio = crate::utils::exact_ratio::format_ratio(
            s.total_deposited.checked_mul(100).ok_or_else(|| {
                ApiError::internal(format!("DAO deposited percentage overflows for {}", s.date))
            })?,
            circulating,
            4,
        )
        .map_err(|e| ApiError::internal(format!("DAO circulation ratio for {}: {e}", s.date)))?;
        data.push(ChartDataPoint {
            date: s.date.clone(),
            value: ratio,
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
    use std::time::Duration;

    #[test]
    fn realized_apc_uses_actual_chain_duration_and_marks_zero_span_undefined() {
        const YEAR_MS: i128 = 31_557_600_000;
        assert_eq!(
            realized_apc(1, 10, YEAR_MS).unwrap().as_deref(),
            Some("10.00%")
        );
        assert_eq!(
            realized_apc(1, 10, YEAR_MS / 2).unwrap().as_deref(),
            Some("20.00%")
        );
        assert_eq!(realized_apc(1, 10, 0).unwrap(), None);
        assert!(realized_apc(1, 10, -1).unwrap_err().contains("regressed"));
    }

    #[test]
    fn top_depositor_duration_uses_exact_ms_and_rejects_zero_count() {
        let beyond_js_integer = 9_007_199_254_740_993i128;
        assert_eq!(
            format_deposit_days_exact(beyond_js_integer, 1).unwrap(),
            "104250.0K"
        );
        assert_eq!(format_deposit_days_exact(129_600_000, 1).unwrap(), "1.5");
        assert_eq!(format_deposit_days_exact(0, 1).unwrap(), "0");
        assert!(format_deposit_days_exact(1, 0).is_err());
        assert!(format_deposit_days_exact(-1, 1).is_err());
    }

    fn snapshot(
        total_issuance: i128,
        secondary_pool: i128,
        unclaimed_compensation: i128,
    ) -> ckbadger_store::DaoDailySnapshot {
        ckbadger_store::DaoDailySnapshot {
            date: "2026-02-18".to_string(),
            total_deposited: 0,
            depositors_count: 0,
            new_deposits: 0,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 0,
            total_issuance,
            secondary_pool,
            occupied_capacity: 0,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury: 999,
            unclaimed_compensation,
            frozen_phase1_compensation: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: None,
        }
    }

    #[test]
    fn test_circulation_ratio_supply_subtracts_full_unissued_secondary_pool() {
        // Genesis burnt (8.4B CKB) is now supplied from the derived baseline.
        let burnt = 840_000_000_000_000_000i128;
        let total_issuance = 1_000_000_000_000_000_000i128;
        let secondary_pool = 20_000_000_000_000_000i128;
        let s = snapshot(total_issuance, secondary_pool, 5_000_000_000_000_000);
        let expected = total_issuance - burnt - secondary_pool;
        assert_eq!(
            dao_supply(&s, burnt).unwrap().unwrap().circulating,
            expected
        );
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
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: ms,
        }
    }

    #[test]
    fn test_build_total_depositors_series_uses_current_not_cumulative_depositors() {
        let snapshots = vec![
            ckbadger_store::DaoDailySnapshot {
                date: "2026-02-17".to_string(),
                depositors_count: 10,
                cumulative_depositors: 10,
                ..snapshot(1, 0, 0)
            },
            ckbadger_store::DaoDailySnapshot {
                date: "2026-02-18".to_string(),
                depositors_count: 8,
                cumulative_depositors: 12,
                ..snapshot(1, 0, 0)
            },
            ckbadger_store::DaoDailySnapshot {
                date: "2026-02-19".to_string(),
                depositors_count: 9,
                cumulative_depositors: 13,
                ..snapshot(1, 0, 0)
            },
        ];
        let series = build_total_depositors_series(&snapshots).unwrap();
        // The chart's secondary series is current depositors. It may fall as
        // people complete withdrawals even while all-time unique grows.
        assert_eq!(series.get("2026-02-17"), Some(&10));
        assert_eq!(series.get("2026-02-18"), Some(&8));
        assert_eq!(series.get("2026-02-19"), Some(&9));
        assert_eq!(snapshots[2].cumulative_depositors, 13);
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

    fn daily_snapshot(
        date: &str,
        cumulative: i128,
        deposits: i64,
    ) -> ckbadger_store::DaoDailySnapshot {
        ckbadger_store::DaoDailySnapshot {
            date: date.to_string(),
            cumulative_deposit_amount: cumulative,
            new_deposits: deposits,
            ..snapshot(1, 0, 0)
        }
    }

    /// The first snapshot day is a real data point measured against a zero
    /// baseline; pairing snapshots off against each other dropped it.
    #[test]
    fn test_build_daily_deposit_series_includes_the_first_snapshot_day() {
        let snapshots = vec![
            daily_snapshot("2019-11-16", 3_715_755_618_324_833, 33),
            daily_snapshot("2019-11-17", 3_815_755_618_324_833, 77),
        ];

        let series = build_daily_deposit_series(&snapshots).unwrap();
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].date, "2019-11-16");
        assert_eq!(series[0].value, "37157556.18324833");
        assert_eq!(series[0].value2.as_deref(), Some("33"));
        assert_eq!(series[1].date, "2019-11-17");
        assert_eq!(series[1].value, "1000000");
        assert_eq!(series[1].value2.as_deref(), Some("44"));
    }

    #[test]
    fn test_build_daily_deposit_series_empty_input() {
        assert!(build_daily_deposit_series(&[]).unwrap().is_empty());
    }

    #[test]
    fn test_build_daily_deposit_series_rejects_decreasing_cumulative_series() {
        let snapshots = vec![
            daily_snapshot("2019-11-16", 500, 5),
            daily_snapshot("2019-11-17", 400, 5),
        ];
        let err = build_daily_deposit_series(&snapshots).unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("cumulative_deposit_amount decreased between 2019-11-16 and 2019-11-17"));
    }

    /// A negative cumulative on day one is caught by the same zero baseline
    /// that makes day one a data point at all.
    #[test]
    fn test_build_daily_deposit_series_rejects_negative_first_day() {
        let snapshots = vec![daily_snapshot("2019-11-16", -1, 0)];
        let err = build_daily_deposit_series(&snapshots).unwrap_err();
        assert!(err
            .1
             .0
            .message
            .contains("the zero baseline before the first snapshot day"));
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
