#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, ApiRouteError, CursorPaginatedResponse};
use crate::AppState;
use ckbadger_store::types::{DeepForkInfo, SyncStatus};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorgEventResponse {
    pub id: i32,
    pub detected_at: String,
    pub fork_point_number: i64,
    pub fork_point_hash: String,
    pub old_tip_number: i64,
    pub old_tip_hash: String,
    pub new_tip_number: i64,
    pub new_tip_hash: String,
    pub depth: i32,
    pub orphaned_blocks_count: i32,
    pub orphaned_txs_count: i32,
    pub event_type: String,
    pub resolved_at: Option<String>,
    pub resolved_by: Option<String>,
    pub resolution_action: Option<String>,
    pub resolution_notes: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanedBlockResponse {
    pub number: i64,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: String,
    pub transactions_count: i32,
    pub miner_lock_hash: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanedTransactionResponse {
    pub hash: String,
    pub block_number: i64,
    pub block_hash: String,
    pub tx_index: i32,
    pub inputs_count: Option<i16>,
    pub outputs_count: Option<i16>,
    pub total_capacity: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorgDetailResponse {
    pub event: ReorgEventResponse,
    pub orphaned_blocks: Vec<OrphanedBlockResponse>,
    pub orphaned_transactions: Vec<OrphanedTransactionResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepForkStatusResponse {
    pub detected: bool,
    pub detected_at: Option<String>,
    pub db_tip: Option<i64>,
    pub db_tip_hash: Option<String>,
    pub chain_tip: Option<i64>,
    pub chain_tip_hash: Option<String>,
    pub depth: Option<i32>,
    pub fork_point: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentReorgResponse {
    pub has_recent_reorg: bool,
    pub reorg: Option<ReorgEventResponse>,
    pub deep_fork: DeepForkStatusResponse,
}

#[derive(Debug, Deserialize)]
pub struct ListForksParams {
    pub limit: Option<i64>,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/forks", get(list_forks))
        .route("/forks/recent", get(get_recent_reorg))
        .route("/forks/{id}", get(get_fork_detail))
}

fn deep_fork_info_if_consistent(
    status: &SyncStatus,
) -> Result<Option<&DeepForkInfo>, ApiRouteError> {
    match (status.deep_fork_detected, status.deep_fork_info.as_ref()) {
        (false, None) => Ok(None),
        (true, Some(info)) => Ok(Some(info)),
        (true, None) => Err(ApiError::internal(format!(
            "sync_status invariant violated: deep_fork_detected=true but deep_fork_info is missing (tip_block={})",
            status.tip_block_number
        ))),
        (false, Some(_)) => Err(ApiError::internal(format!(
            "sync_status invariant violated: deep_fork_detected=false but deep_fork_info exists (tip_block={})",
            status.tip_block_number
        ))),
    }
}

fn build_deep_fork_event(info: &DeepForkInfo, detected_at: String) -> ReorgEventResponse {
    ReorgEventResponse {
        id: 1,
        detected_at,
        fork_point_number: info.fork_point,
        fork_point_hash: String::new(),
        old_tip_number: info.db_tip,
        old_tip_hash: format!("0x{}", hex::encode(&info.db_tip_hash)),
        new_tip_number: info.chain_tip,
        new_tip_hash: format!("0x{}", hex::encode(&info.chain_tip_hash)),
        depth: info.depth,
        orphaned_blocks_count: 0,
        orphaned_txs_count: 0,
        event_type: "deep".to_string(),
        resolved_at: None,
        resolved_by: None,
        resolution_action: None,
        resolution_notes: None,
    }
}

/// List forks.
///
/// With the migration to RocksDB, we no longer have a `reorg_events` table.
/// Reorg data is now derived from the sync status deep_fork_info. We return
/// the deep fork as a single-element list if one is detected, or an empty list.
async fn list_forks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListForksParams>,
) -> ApiResult<CursorPaginatedResponse<ReorgEventResponse>> {
    let limit = params.limit.unwrap_or(20).clamp(1, 100);

    let store = state.store.clone();
    let sync_status = tokio::task::spawn_blocking(move || store.get_sync_status())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut events = Vec::new();
    if let Some(info) = deep_fork_info_if_consistent(&sync_status)? {
        let store = state.store.clone();
        let detected_at = tokio::task::spawn_blocking(move || {
            let latest = store
                .get_latest_reorg_event()?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing persisted reorg event while deep fork is active; check sync_meta reorg records"
                    )
                })?;
            let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(latest.detected_at, 0)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "invalid reorg detected_at timestamp in sync_meta: {}",
                        latest.detected_at
                    )
                })?;
            Ok::<_, anyhow::Error>(dt.to_rfc3339())
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
        events.push(build_deep_fork_event(info, detected_at));
    }

    let total = events.len() as i64;

    ok(CursorPaginatedResponse::new(events, total, limit, None))
}

/// Get fork detail by ID.
///
/// Since reorg_events / orphaned_blocks / orphaned_transactions tables are gone,
/// we derive this from the deep fork info if id == 1 and a deep fork is active.
async fn get_fork_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> ApiResult<ReorgDetailResponse> {
    let store = state.store.clone();
    let sync_status = tokio::task::spawn_blocking(move || store.get_sync_status())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if id != 1 {
        return Err(ApiError::not_found("Reorg event not found"));
    }

    let Some(info) = deep_fork_info_if_consistent(&sync_status)? else {
        return Err(ApiError::not_found("Reorg event not found"));
    };
    let store = state.store.clone();
    let detected_at = tokio::task::spawn_blocking(move || {
        let latest = store.get_latest_reorg_event()?.ok_or_else(|| {
            anyhow::anyhow!("missing persisted reorg event while deep fork is active")
        })?;
        let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(latest.detected_at, 0)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid reorg detected_at timestamp: {}",
                    latest.detected_at
                )
            })?;
        Ok::<_, anyhow::Error>(dt.to_rfc3339())
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let event = build_deep_fork_event(info, detected_at);

    ok(ReorgDetailResponse {
        event,
        orphaned_blocks: Vec::new(),
        orphaned_transactions: Vec::new(),
    })
}

async fn get_recent_reorg(State(state): State<Arc<AppState>>) -> ApiResult<RecentReorgResponse> {
    let store = state.store.clone();
    let sync_status = tokio::task::spawn_blocking(move || store.get_sync_status())
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let deep_fork_info = deep_fork_info_if_consistent(&sync_status)?;
    let deep_fork = if let Some(info) = deep_fork_info {
        DeepForkStatusResponse {
            detected: true,
            detected_at: None,
            db_tip: Some(info.db_tip),
            db_tip_hash: Some(format!("0x{}", hex::encode(&info.db_tip_hash))),
            chain_tip: Some(info.chain_tip),
            chain_tip_hash: Some(format!("0x{}", hex::encode(&info.chain_tip_hash))),
            depth: Some(info.depth),
            fork_point: Some(info.fork_point),
        }
    } else {
        DeepForkStatusResponse {
            detected: false,
            detected_at: None,
            db_tip: None,
            db_tip_hash: None,
            chain_tip: None,
            chain_tip_hash: None,
            depth: None,
            fork_point: None,
        }
    };

    // With RocksDB we don't have a reorg_events table with timestamps, so we
    // only surface the deep fork if one is currently detected.
    let reorg = if let Some(info) = deep_fork_info {
        let store = state.store.clone();
        let detected_at = tokio::task::spawn_blocking(move || {
            let latest = store.get_latest_reorg_event()?.ok_or_else(|| {
                anyhow::anyhow!("missing persisted reorg event while deep fork is active")
            })?;
            let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(latest.detected_at, 0)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "invalid reorg detected_at timestamp: {}",
                        latest.detected_at
                    )
                })?;
            Ok::<_, anyhow::Error>(dt.to_rfc3339())
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
        Some(build_deep_fork_event(info, detected_at))
    } else {
        None
    };

    ok(RecentReorgResponse {
        has_recent_reorg: reorg.is_some() || deep_fork.detected,
        reorg,
        deep_fork,
    })
}
