#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::AppState;

// Row structs for ClickHouse queries
#[derive(clickhouse::Row, serde::Deserialize)]
struct CountRow {
    count: i64,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct ReorgEventRow {
    id: i32,
    detected_at: chrono::DateTime<chrono::Utc>,
    fork_point_number: i64,
    fork_point_hash: Vec<u8>,
    old_tip_number: i64,
    old_tip_hash: Vec<u8>,
    new_tip_number: i64,
    new_tip_hash: Vec<u8>,
    depth: i32,
    orphaned_blocks_count: i32,
    orphaned_txs_count: i32,
    event_type: String,
    resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    resolved_by: Option<String>,
    resolution_action: Option<String>,
    resolution_notes: Option<String>,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct OrphanedBlockRow {
    number: i64,
    hash: Vec<u8>,
    parent_hash: Vec<u8>,
    timestamp: chrono::DateTime<chrono::Utc>,
    transactions_count: i32,
    miner_lock_hash: Option<Vec<u8>>,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct OrphanedTxRow {
    hash: Vec<u8>,
    block_number: i64,
    block_hash: Vec<u8>,
    tx_index: i32,
    inputs_count: Option<i16>,
    outputs_count: Option<i16>,
    total_capacity: Option<i64>,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct DeepForkRow {
    deep_fork_detected: bool,
    deep_fork_at: Option<chrono::DateTime<chrono::Utc>>,
    deep_fork_db_tip: Option<i64>,
    deep_fork_db_tip_hash: Option<Vec<u8>>,
    deep_fork_chain_tip: Option<i64>,
    deep_fork_chain_tip_hash: Option<Vec<u8>>,
    deep_fork_depth: Option<i32>,
    deep_fork_fork_point: Option<i64>,
}

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
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveDeepForkRequest {
    pub action: String,
    pub admin_token: String,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveDeepForkResponse {
    pub success: bool,
    pub action: String,
    pub message: String,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/forks", get(list_forks))
        .route("/forks/recent", get(get_recent_reorg))
        .route("/forks/{id}", get(get_fork_detail))
        .route("/admin/resolve-deep-fork", post(resolve_deep_fork))
}

async fn list_forks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListForksParams>,
) -> ApiResult<CursorPaginatedResponse<ReorgEventResponse>> {
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);

    let total_row = state
        .clickhouse
        .client()
        .query("SELECT COUNT(*) as count FROM reorg_events")
        .fetch_one::<CountRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let query = format!(
        r#"
        SELECT 
            id, detected_at, 
            fork_point_number, fork_point_hash,
            old_tip_number, old_tip_hash,
            new_tip_number, new_tip_hash,
            depth, orphaned_blocks_count, orphaned_txs_count,
            event_type, resolved_at, resolved_by, resolution_action, resolution_notes
        FROM reorg_events
        ORDER BY detected_at DESC
        LIMIT {} OFFSET {}
        "#,
        limit, offset
    );

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<ReorgEventRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let events: Vec<ReorgEventResponse> = rows
        .into_iter()
        .map(|r| ReorgEventResponse {
            id: r.id,
            detected_at: r.detected_at.to_rfc3339(),
            fork_point_number: r.fork_point_number,
            fork_point_hash: format!("0x{}", hex::encode(&r.fork_point_hash)),
            old_tip_number: r.old_tip_number,
            old_tip_hash: format!("0x{}", hex::encode(&r.old_tip_hash)),
            new_tip_number: r.new_tip_number,
            new_tip_hash: format!("0x{}", hex::encode(&r.new_tip_hash)),
            depth: r.depth,
            orphaned_blocks_count: r.orphaned_blocks_count,
            orphaned_txs_count: r.orphaned_txs_count,
            event_type: r.event_type,
            resolved_at: r.resolved_at.map(|t| t.to_rfc3339()),
            resolved_by: r.resolved_by,
            resolution_action: r.resolution_action,
            resolution_notes: r.resolution_notes,
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        events,
        total_row.count,
        limit,
        None,
    ))
}

async fn get_fork_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> ApiResult<ReorgDetailResponse> {
    let query = format!(
        r#"
        SELECT 
            id, detected_at, 
            fork_point_number, fork_point_hash,
            old_tip_number, old_tip_hash,
            new_tip_number, new_tip_hash,
            depth, orphaned_blocks_count, orphaned_txs_count,
            event_type, resolved_at, resolved_by, resolution_action, resolution_notes
        FROM reorg_events
        WHERE id = {}
        "#,
        id
    );

    let event_rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<ReorgEventRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let r = event_rows
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::not_found("Reorg event not found"))?;

    let event = ReorgEventResponse {
        id: r.id,
        detected_at: r.detected_at.to_rfc3339(),
        fork_point_number: r.fork_point_number,
        fork_point_hash: format!("0x{}", hex::encode(&r.fork_point_hash)),
        old_tip_number: r.old_tip_number,
        old_tip_hash: format!("0x{}", hex::encode(&r.old_tip_hash)),
        new_tip_number: r.new_tip_number,
        new_tip_hash: format!("0x{}", hex::encode(&r.new_tip_hash)),
        depth: r.depth,
        orphaned_blocks_count: r.orphaned_blocks_count,
        orphaned_txs_count: r.orphaned_txs_count,
        event_type: r.event_type,
        resolved_at: r.resolved_at.map(|t| t.to_rfc3339()),
        resolved_by: r.resolved_by,
        resolution_action: r.resolution_action,
        resolution_notes: r.resolution_notes,
    };

    let orphaned_blocks_query = format!(
        r#"
        SELECT number, hash, parent_hash, timestamp, transactions_count, miner_lock_hash
        FROM orphaned_blocks
        WHERE reorg_event_id = {}
        ORDER BY number DESC
        "#,
        id
    );

    let orphaned_blocks = state
        .clickhouse
        .client()
        .query(&orphaned_blocks_query)
        .fetch_all::<OrphanedBlockRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let orphaned_txs_query = format!(
        r#"
        SELECT hash, block_number, block_hash, tx_index, inputs_count, outputs_count, total_capacity
        FROM orphaned_transactions
        WHERE reorg_event_id = {}
        ORDER BY block_number DESC, tx_index
        LIMIT 100
        "#,
        id
    );

    let orphaned_txs = state
        .clickhouse
        .client()
        .query(&orphaned_txs_query)
        .fetch_all::<OrphanedTxRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(ReorgDetailResponse {
        event,
        orphaned_blocks: orphaned_blocks
            .into_iter()
            .map(|b| OrphanedBlockResponse {
                number: b.number,
                hash: format!("0x{}", hex::encode(&b.hash)),
                parent_hash: format!("0x{}", hex::encode(&b.parent_hash)),
                timestamp: b.timestamp.to_rfc3339(),
                transactions_count: b.transactions_count,
                miner_lock_hash: b.miner_lock_hash.map(|h| format!("0x{}", hex::encode(&h))),
            })
            .collect(),
        orphaned_transactions: orphaned_txs
            .into_iter()
            .map(|t| OrphanedTransactionResponse {
                hash: format!("0x{}", hex::encode(&t.hash)),
                block_number: t.block_number,
                block_hash: format!("0x{}", hex::encode(&t.block_hash)),
                tx_index: t.tx_index,
                inputs_count: t.inputs_count,
                outputs_count: t.outputs_count,
                total_capacity: t.total_capacity.map(|c| c.to_string()),
            })
            .collect(),
    })
}

async fn get_recent_reorg(State(state): State<Arc<AppState>>) -> ApiResult<RecentReorgResponse> {
    let deep_fork_query = r#"
        SELECT 
            deep_fork_detected,
            deep_fork_at,
            deep_fork_db_tip,
            deep_fork_db_tip_hash,
            deep_fork_chain_tip,
            deep_fork_chain_tip_hash,
            deep_fork_depth,
            deep_fork_fork_point
        FROM sync_status WHERE id = 1
    "#;

    let deep_fork_rows = state
        .clickhouse
        .client()
        .query(deep_fork_query)
        .fetch_all::<DeepForkRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let deep_fork_row = deep_fork_rows
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::internal("No sync_status row found".to_string()))?;

    let deep_fork = DeepForkStatusResponse {
        detected: deep_fork_row.deep_fork_detected,
        detected_at: deep_fork_row.deep_fork_at.map(|t| t.to_rfc3339()),
        db_tip: deep_fork_row.deep_fork_db_tip,
        db_tip_hash: deep_fork_row
            .deep_fork_db_tip_hash
            .map(|h| format!("0x{}", hex::encode(&h))),
        chain_tip: deep_fork_row.deep_fork_chain_tip,
        chain_tip_hash: deep_fork_row
            .deep_fork_chain_tip_hash
            .map(|h| format!("0x{}", hex::encode(&h))),
        depth: deep_fork_row.deep_fork_depth,
        fork_point: deep_fork_row.deep_fork_fork_point,
    };

    let recent_reorg_query = r#"
        SELECT 
            id, detected_at, 
            fork_point_number, fork_point_hash,
            old_tip_number, old_tip_hash,
            new_tip_number, new_tip_hash,
            depth, orphaned_blocks_count, orphaned_txs_count,
            event_type, resolved_at, resolved_by, resolution_action, resolution_notes
        FROM reorg_events
        WHERE detected_at >= now() - INTERVAL 24 HOUR
        ORDER BY detected_at DESC
        LIMIT 1
    "#;

    let recent_reorg_rows = state
        .clickhouse
        .client()
        .query(recent_reorg_query)
        .fetch_all::<ReorgEventRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let reorg = recent_reorg_rows
        .into_iter()
        .next()
        .map(|r| ReorgEventResponse {
            id: r.id,
            detected_at: r.detected_at.to_rfc3339(),
            fork_point_number: r.fork_point_number,
            fork_point_hash: format!("0x{}", hex::encode(&r.fork_point_hash)),
            old_tip_number: r.old_tip_number,
            old_tip_hash: format!("0x{}", hex::encode(&r.old_tip_hash)),
            new_tip_number: r.new_tip_number,
            new_tip_hash: format!("0x{}", hex::encode(&r.new_tip_hash)),
            depth: r.depth,
            orphaned_blocks_count: r.orphaned_blocks_count,
            orphaned_txs_count: r.orphaned_txs_count,
            event_type: r.event_type,
            resolved_at: r.resolved_at.map(|t| t.to_rfc3339()),
            resolved_by: r.resolved_by,
            resolution_action: r.resolution_action,
            resolution_notes: r.resolution_notes,
        });

    ok(RecentReorgResponse {
        has_recent_reorg: reorg.is_some() || deep_fork.detected,
        reorg,
        deep_fork,
    })
}

async fn resolve_deep_fork(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ResolveDeepForkRequest>,
) -> ApiResult<ResolveDeepForkResponse> {
    let expected_token = std::env::var("ADMIN_TOKEN").unwrap_or_default();
    if expected_token.is_empty() || req.admin_token != expected_token {
        return Err(ApiError::unauthorized("Invalid admin token"));
    }

    // Check if deep_fork_detected is true
    let deep_fork_check_query = r#"
        SELECT 
            CAST(deep_fork_detected AS Int64) as count
        FROM sync_status WHERE id = 1
    "#;

    let deep_fork_check = state
        .clickhouse
        .client()
        .query(deep_fork_check_query)
        .fetch_one::<CountRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if deep_fork_check.count == 0 {
        return Err(ApiError::bad_request("No deep fork to resolve"));
    }

    match req.action.as_str() {
        "dismiss" => {
            // Update reorg_events using ClickHouse ALTER TABLE UPDATE syntax
            let notes_escaped = req
                .notes
                .as_ref()
                .map(|n| n.replace("'", "''"))
                .unwrap_or_default();
            let update_reorg_query = format!(
                r#"
                ALTER TABLE reorg_events UPDATE
                    event_type = 'resolved',
                    resolved_at = now(),
                    resolved_by = 'admin',
                    resolution_action = 'dismissed',
                    resolution_notes = '{}'
                WHERE event_type = 'deep' AND resolved_at IS NULL
                "#,
                notes_escaped
            );

            state
                .clickhouse
                .client()
                .query(&update_reorg_query)
                .execute()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            // Update sync_status using ClickHouse ALTER TABLE UPDATE syntax
            let update_sync_query = r#"
                ALTER TABLE sync_status UPDATE
                    deep_fork_detected = false,
                    deep_fork_at = NULL,
                    deep_fork_db_tip = NULL,
                    deep_fork_db_tip_hash = NULL,
                    deep_fork_chain_tip = NULL,
                    deep_fork_chain_tip_hash = NULL,
                    deep_fork_depth = NULL,
                    deep_fork_fork_point = NULL
                WHERE id = 1
            "#;

            state
                .clickhouse
                .client()
                .query(update_sync_query)
                .execute()
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            ok(ResolveDeepForkResponse {
                success: true,
                action: "dismiss".to_string(),
                message: "Deep fork dismissed. Sync will resume but data may be inconsistent."
                    .to_string(),
            })
        }
        _ => Err(ApiError::bad_request(
            "Invalid action. Supported: dismiss. For rollback/reset, use CLI tools.",
        )),
    }
}
