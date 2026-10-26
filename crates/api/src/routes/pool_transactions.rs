//! Global pool rows from the same immutable mirror used by address activity feeds.
use axum::extract::{Query, State};
use ckbadger_store::types::ScriptInfo;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

use super::activities::{
    LockCallResponse, ParticipantResponse, ProtocolActionResponse, TypeCallResponse,
};
use crate::{
    pool::{InterpretationResponse, PoolStatus},
    response::{ok, ApiError, ApiResult},
    AppState,
};

#[derive(Deserialize)]
pub struct PoolListParams {
    limit: Option<usize>,
    cursor: Option<String>,
    status: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolActivity {
    pub participants: Vec<ParticipantResponse>,
    pub protocol_actions: Vec<ProtocolActionResponse>,
    pub type_calls: Vec<TypeCallResponse>,
    pub lock_calls: Vec<LockCallResponse>,
    pub is_cellbase: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolTransaction {
    pub hash: String,
    pub status: String,
    pub value: Option<String>,
    pub fee: String,
    pub fee_rate: String,
    pub tx_size: u64,
    pub cycles: String,
    pub inputs_count: i16,
    pub outputs_count: i16,
    pub timestamp: String,
    pub interpretation: InterpretationResponse,
    pub activity: Option<PoolActivity>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolTotals {
    pub pending: usize,
    pub proposed: usize,
    pub awaiting_index: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolSummary {
    pub size: String,
    pub cycles: String,
    pub min_fee_rate: Option<String>,
    pub max_fee_rate: Option<String>,
    pub newest_seen_at: Option<String>,
}
fn pool_fee_rate(
    record: &crate::pool::PoolTxRecord,
) -> Result<u128, crate::response::ApiRouteError> {
    let size = i32::try_from(record.entry.size)
        .map_err(|_| ApiError::internal("pool transaction size exceeds i32"))?;
    if size <= 0 {
        return Err(ApiError::internal(format!(
            "pool tx 0x{} has invalid serialized size {size}",
            hex::encode(record.tx_hash)
        )));
    }
    Ok(super::transactions::tx_fee_rate_value(
        u128::from(record.entry.fee),
        size,
    ))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolTransactionsResponse {
    pub summary: PoolSummary,
    pub data: Vec<PoolTransaction>,
    pub total: usize,
    pub limit: usize,
    pub next_cursor: Option<String>,
    pub totals: PoolTotals,
    pub truncated: bool,
    pub last_polled_at: Option<String>,
}

pub async fn list_pool_transactions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PoolListParams>,
) -> ApiResult<PoolTransactionsResponse> {
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let filter = params.status.as_deref().unwrap_or("all");
    if !matches!(
        filter,
        "all" | "pending" | "proposed" | "committed_awaiting_index"
    ) {
        return Err(ApiError::bad_request(
            "pool status must be all, pending, proposed, or committed_awaiting_index",
        ));
    }
    let cursor = params
        .cursor
        .as_deref()
        .map(|raw| {
            let (timestamp, hash) = raw
                .split_once(':')
                .ok_or_else(|| ApiError::bad_request("invalid pool cursor"))?;
            let timestamp = timestamp
                .parse::<u64>()
                .map_err(|_| ApiError::bad_request("invalid pool cursor timestamp"))?;
            let hash = crate::utils::parse_hash32(hash, "pool cursor hash")?;
            Ok::<_, crate::response::ApiRouteError>((timestamp, hash))
        })
        .transpose()?;
    let snapshot = state.pool_mirror.load();
    if !snapshot.status.enabled || !snapshot.status.healthy {
        return Err(ApiError::service_unavailable(format!(
            "pool mirror unavailable: {}",
            snapshot
                .status
                .last_error
                .as_deref()
                .unwrap_or("not enabled or first refresh has not completed")
        )));
    }
    let mut totals = PoolTotals {
        pending: 0,
        proposed: 0,
        awaiting_index: 0,
    };
    let mut records = Vec::new();
    for record in snapshot.records.values() {
        if state
            .store
            .get_tx_location(&record.tx_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some()
        {
            continue;
        }
        match record.pool_status {
            PoolStatus::Pending => totals.pending += 1,
            PoolStatus::Proposed => totals.proposed += 1,
            PoolStatus::CommittedAwaitingIndex { .. } => totals.awaiting_index += 1,
        }
        if filter == "all" || filter == record.pool_status.as_str() {
            records.push(record);
        }
    }
    let total = records.len();
    records.sort_by(|a, b| {
        (b.entry.time_added_to_pool_ms, b.tx_hash).cmp(&(a.entry.time_added_to_pool_ms, a.tx_hash))
    });
    let mut total_size = 0u128;
    let mut total_cycles = 0u128;
    let mut rates = Vec::with_capacity(records.len());
    for record in &records {
        total_size = total_size
            .checked_add(u128::from(record.entry.size))
            .ok_or_else(|| ApiError::internal("pool size sum overflow"))?;
        total_cycles = total_cycles
            .checked_add(u128::from(record.entry.cycles))
            .ok_or_else(|| ApiError::internal("pool cycles sum overflow"))?;
        rates.push(pool_fee_rate(record)?);
    }
    let summary = PoolSummary {
        size: total_size.to_string(),
        cycles: total_cycles.to_string(),
        min_fee_rate: rates.iter().min().map(ToString::to_string),
        max_fee_rate: rates.iter().max().map(ToString::to_string),
        newest_seen_at: records
            .first()
            .map(|record| record.entry.time_added_to_pool_ms.to_string()),
    };
    if let Some((timestamp, hash)) = cursor {
        records.retain(|r| {
            (r.entry.time_added_to_pool_ms, r.tx_hash.as_slice()) < (timestamp, hash.as_slice())
        });
    }
    let has_more = records.len() > limit;
    records.truncate(limit);
    let next_cursor = if has_more {
        records.last().map(|r| {
            format!(
                "{}:0x{}",
                r.entry.time_added_to_pool_ms,
                hex::encode(r.tx_hash)
            )
        })
    } else {
        None
    };
    let mut script_cache: HashMap<Vec<u8>, Option<ScriptInfo>> = HashMap::new();
    let mut addresses = HashMap::new();
    let mut data = Vec::with_capacity(records.len());
    for record in records {
        let activity = record
            .actions
            .as_ref()
            .map(|actions| {
                super::activities::build_global_activity_response_with_pool(
                    &state.store,
                    &state.append_only_store,
                    &state.ckb_network,
                    actions,
                    &mut script_cache,
                    &mut addresses,
                    Some(record),
                )
                .map(|activity| PoolActivity {
                    participants: activity.participants,
                    protocol_actions: activity.protocol_actions,
                    type_calls: activity.type_calls,
                    lock_calls: activity.lock_calls,
                    is_cellbase: activity.is_cellbase,
                })
            })
            .transpose()
            .map_err(|e| ApiError::internal(e.to_string()))?;
        data.push(PoolTransaction {
            hash: format!("0x{}", hex::encode(record.tx_hash)),
            status: record.pool_status.as_str().to_string(),
            value: record
                .actions
                .as_ref()
                .map(|actions| actions.ckb_moved().to_string()),
            fee: record.entry.fee.to_string(),
            fee_rate: pool_fee_rate(record)?.to_string(),
            tx_size: record.entry.size,
            cycles: record.entry.cycles.to_string(),
            inputs_count: record.inputs_count,
            outputs_count: record.outputs_count,
            timestamp: record.entry.time_added_to_pool_ms.to_string(),
            interpretation: InterpretationResponse::from(&record.interpretation),
            activity,
        });
    }
    ok(PoolTransactionsResponse {
        summary,
        data,
        total,
        limit,
        next_cursor,
        totals,
        truncated: snapshot.status.truncated,
        last_polled_at: snapshot.status.last_polled_at_ms.map(|v| v.to_string()),
    })
}
