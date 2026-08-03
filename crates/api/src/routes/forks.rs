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
use ckbadger_store::types::{DeepForkInfo, ReorgEventKind, ReorgEventRecord, SyncStatus};

/// How long after detection a persisted reorg still counts as "recent".
///
/// `/forks/recent` answers "did the chain just reorganize?", so the horizon has
/// to be stated explicitly rather than implied by whatever happens to be the
/// newest row in a history that is never pruned.
const RECENT_REORG_WINDOW_SECS: i64 = 24 * 60 * 60;

const DEFAULT_FORKS_LIMIT: i64 = 20;
const MAX_FORKS_LIMIT: i64 = 100;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorgEventResponse {
    /// Detection time in milliseconds; the stable id of the event.
    pub id: i64,
    pub detected_at: String,
    pub fork_point_number: i64,
    pub fork_point_hash: String,
    pub old_tip_number: i64,
    pub old_tip_hash: String,
    pub new_tip_number: i64,
    pub new_tip_hash: String,
    pub depth: i32,
    pub orphaned_blocks_count: i64,
    pub orphaned_txs_count: i64,
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
    /// Rollback deletes the orphaned blocks and transactions, so only their
    /// counts on the event survive; the bodies are intentionally not retained.
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
    /// True while the newest persisted reorg is inside
    /// `recentWindowSeconds`, or while a deep fork is active.
    pub has_recent_reorg: bool,
    /// The newest persisted reorg event, recent or not.
    pub reorg: Option<ReorgEventResponse>,
    pub recent_window_seconds: i64,
    pub deep_fork: DeepForkStatusResponse,
}

#[derive(Debug, Deserialize)]
pub struct ListForksParams {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
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

fn hex0x(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn rfc3339_from_unix_seconds(seconds: i64, context: &str) -> Result<String, ApiRouteError> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, 0)
        .map(|dt| dt.to_rfc3339())
        .ok_or_else(|| {
            ApiError::internal(format!(
                "invalid reorg detected_at timestamp in {}: {}",
                context, seconds
            ))
        })
}

/// Render one persisted reorg event. Every field comes from the record the
/// writer captured at detection time, so nothing here is re-derived from chain
/// state that may have moved since.
fn reorg_event_response(record: &ReorgEventRecord) -> Result<ReorgEventResponse, ApiRouteError> {
    let detected_at = rfc3339_from_unix_seconds(record.event.detected_at, &record.key)?;
    Ok(ReorgEventResponse {
        id: record.detected_at_ms,
        detected_at,
        fork_point_number: record.event.fork_point,
        fork_point_hash: hex0x(&record.event.fork_point_hash),
        old_tip_number: record.event.old_tip,
        old_tip_hash: hex0x(&record.event.old_tip_hash),
        new_tip_number: record.event.new_tip,
        new_tip_hash: hex0x(&record.event.new_tip_hash),
        depth: record.event.depth,
        orphaned_blocks_count: record.event.orphaned_blocks,
        orphaned_txs_count: record.event.orphaned_txs,
        event_type: match record.event.kind {
            ReorgEventKind::Automatic => "reorg".to_string(),
            ReorgEventKind::Deep => "deep".to_string(),
        },
        resolved_at: None,
        resolved_by: None,
        resolution_action: None,
        resolution_notes: None,
    })
}

/// Reject anything that is not a well-formed history key before it reaches the
/// store, so a bad cursor is a 400 and never a silently shifted page.
fn parse_forks_cursor(cursor: Option<&str>) -> Result<Option<Vec<u8>>, ApiRouteError> {
    let Some(raw) = cursor else {
        return Ok(None);
    };
    let bytes = raw.as_bytes().to_vec();
    ckbadger_store::keys::decode_reorg_event_key(&bytes)
        .map_err(|_| ApiError::bad_request("Invalid forks cursor"))?;
    Ok(Some(bytes))
}

/// List the persisted reorg-event history, newest first.
///
/// Both automatic reorgs and deep-fork detections are persisted by the indexer,
/// so both appear here, tagged by `eventType`.
async fn list_forks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListForksParams>,
) -> ApiResult<CursorPaginatedResponse<ReorgEventResponse>> {
    let limit = params
        .limit
        .unwrap_or(DEFAULT_FORKS_LIMIT)
        .clamp(1, MAX_FORKS_LIMIT);
    let cursor = parse_forks_cursor(params.cursor.as_deref())?;

    let store = state.store.clone();
    let page_size = limit as usize;
    let (mut records, total) = tokio::task::spawn_blocking(move || {
        let records = store.list_reorg_events(page_size + 1, cursor.as_deref())?;
        let total = store.count_reorg_events()?;
        Ok::<_, anyhow::Error>((records, total))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = records.len() > page_size;
    if has_more {
        records.truncate(page_size);
    }
    let next_cursor = if has_more {
        records.last().map(|record| record.key.clone())
    } else {
        None
    };

    let events = records
        .iter()
        .map(reorg_event_response)
        .collect::<Result<Vec<_>, _>>()?;

    ok(CursorPaginatedResponse::new(
        events,
        total,
        limit,
        next_cursor,
    ))
}

/// Get one persisted reorg event by its id (its detection millisecond).
async fn get_fork_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<ReorgDetailResponse> {
    let store = state.store.clone();
    let record = tokio::task::spawn_blocking(move || store.get_reorg_event(id))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let Some(record) = record else {
        return Err(ApiError::not_found("Reorg event not found"));
    };

    ok(ReorgDetailResponse {
        event: reorg_event_response(&record)?,
        orphaned_blocks: Vec::new(),
        orphaned_transactions: Vec::new(),
    })
}

async fn get_recent_reorg(State(state): State<Arc<AppState>>) -> ApiResult<RecentReorgResponse> {
    let store = state.store.clone();
    let (sync_status, latest) = tokio::task::spawn_blocking(move || {
        let sync_status = store.get_sync_status()?;
        let latest = store.get_latest_reorg_event()?;
        Ok::<_, anyhow::Error>((sync_status, latest))
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let deep_fork_info = deep_fork_info_if_consistent(&sync_status)?;
    let reorg = latest.as_ref().map(reorg_event_response).transpose()?;

    let deep_fork = match deep_fork_info {
        Some(info) => {
            // A deep fork is only ever flagged together with its persisted
            // event, so a missing or mismatched record is an invariant
            // violation rather than a reason to report an unknown time.
            let record = latest.as_ref().ok_or_else(|| {
                ApiError::internal(
                    "sync_status reports an active deep fork but no reorg event is persisted; \
                     check the sync_meta reorg history",
                )
            })?;
            if record.event.kind != ReorgEventKind::Deep {
                return Err(ApiError::internal(format!(
                    "sync_status reports an active deep fork but the newest persisted reorg event {} is not a deep-fork record",
                    record.key
                )));
            }
            DeepForkStatusResponse {
                detected: true,
                detected_at: Some(rfc3339_from_unix_seconds(
                    record.event.detected_at,
                    &record.key,
                )?),
                db_tip: Some(info.db_tip),
                db_tip_hash: Some(hex0x(&info.db_tip_hash)),
                chain_tip: Some(info.chain_tip),
                chain_tip_hash: Some(hex0x(&info.chain_tip_hash)),
                depth: Some(info.depth),
                fork_point: Some(info.fork_point),
            }
        }
        None => DeepForkStatusResponse {
            detected: false,
            detected_at: None,
            db_tip: None,
            db_tip_hash: None,
            chain_tip: None,
            chain_tip_hash: None,
            depth: None,
            fork_point: None,
        },
    };

    let now = chrono::Utc::now().timestamp();
    let within_window = latest
        .as_ref()
        .is_some_and(|record| now - record.event.detected_at <= RECENT_REORG_WINDOW_SECS);

    ok(RecentReorgResponse {
        has_recent_reorg: deep_fork.detected || within_window,
        reorg,
        recent_window_seconds: RECENT_REORG_WINDOW_SECS,
        deep_fork,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::types::ReorgEvent;

    fn record(detected_at_ms: i64, kind: ReorgEventKind) -> ReorgEventRecord {
        ReorgEventRecord {
            key: format!("reorg:{:013}:{}", detected_at_ms, "ab".repeat(16)),
            detected_at_ms,
            event: ReorgEvent {
                detected_at: detected_at_ms / 1000,
                kind,
                fork_point: 17_100,
                fork_point_hash: vec![0x01; 32],
                old_tip: 17_103,
                old_tip_hash: vec![0x02; 32],
                new_tip: 17_104,
                new_tip_hash: vec![0x03; 32],
                depth: 3,
                orphaned_blocks: 3,
                orphaned_txs: 7,
            },
        }
    }

    #[test]
    fn test_reorg_event_response_reports_persisted_context() {
        let response = reorg_event_response(&record(1_700_000_000_000, ReorgEventKind::Automatic))
            .expect("response");
        assert_eq!(response.id, 1_700_000_000_000);
        assert_eq!(response.event_type, "reorg");
        assert_eq!(response.fork_point_number, 17_100);
        assert_eq!(response.old_tip_number, 17_103);
        assert_eq!(response.new_tip_number, 17_104);
        assert_eq!(response.orphaned_blocks_count, 3);
        assert_eq!(response.orphaned_txs_count, 7);
        assert_eq!(response.fork_point_hash, format!("0x{}", "01".repeat(32)));
    }

    #[test]
    fn test_reorg_event_response_tags_deep_forks() {
        let response =
            reorg_event_response(&record(1_700_000_000_000, ReorgEventKind::Deep)).expect("resp");
        assert_eq!(response.event_type, "deep");
    }

    #[test]
    fn test_parse_forks_cursor_rejects_malformed_keys() {
        assert!(parse_forks_cursor(None).unwrap().is_none());
        let valid = format!("reorg:{:013}:{}", 1_700_000_000_000i64, "ab".repeat(16));
        assert_eq!(
            parse_forks_cursor(Some(&valid)).unwrap(),
            Some(valid.as_bytes().to_vec())
        );

        for bad in ["-1", "reorg:1:2", "1700000000000", ""] {
            let err = parse_forks_cursor(Some(bad)).unwrap_err();
            assert_eq!(err.1 .0.message, "Invalid forks cursor", "input: {bad}");
        }
    }

    #[test]
    fn test_rfc3339_from_unix_seconds_rejects_unrepresentable_timestamps() {
        assert!(rfc3339_from_unix_seconds(1_700_000_000, "ctx").is_ok());
        let err = rfc3339_from_unix_seconds(i64::MAX, "reorg:key").unwrap_err();
        assert!(err.1 .0.message.contains("invalid reorg detected_at"));
    }
}
