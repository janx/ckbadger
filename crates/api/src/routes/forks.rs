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

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM reorg_events")
        .fetch_one(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let rows: Vec<(
        i32,
        chrono::DateTime<chrono::Utc>,
        i64,
        Vec<u8>,
        i64,
        Vec<u8>,
        i64,
        Vec<u8>,
        i32,
        i32,
        i32,
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
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
        LIMIT $1 OFFSET $2
        "#,
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let events: Vec<ReorgEventResponse> = rows
        .into_iter()
        .map(|r| ReorgEventResponse {
            id: r.0,
            detected_at: r.1.to_rfc3339(),
            fork_point_number: r.2,
            fork_point_hash: format!("0x{}", hex::encode(&r.3)),
            old_tip_number: r.4,
            old_tip_hash: format!("0x{}", hex::encode(&r.5)),
            new_tip_number: r.6,
            new_tip_hash: format!("0x{}", hex::encode(&r.7)),
            depth: r.8,
            orphaned_blocks_count: r.9,
            orphaned_txs_count: r.10,
            event_type: r.11,
            resolved_at: r.12.map(|t| t.to_rfc3339()),
            resolved_by: r.13,
            resolution_action: r.14,
            resolution_notes: r.15,
        })
        .collect();

    ok(CursorPaginatedResponse::new(events, total.0, limit, None))
}

async fn get_fork_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> ApiResult<ReorgDetailResponse> {
    let event_row: Option<(
        i32,
        chrono::DateTime<chrono::Utc>,
        i64,
        Vec<u8>,
        i64,
        Vec<u8>,
        i64,
        Vec<u8>,
        i32,
        i32,
        i32,
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT 
            id, detected_at, 
            fork_point_number, fork_point_hash,
            old_tip_number, old_tip_hash,
            new_tip_number, new_tip_hash,
            depth, orphaned_blocks_count, orphaned_txs_count,
            event_type, resolved_at, resolved_by, resolution_action, resolution_notes
        FROM reorg_events
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let r = event_row.ok_or_else(|| ApiError::not_found("Reorg event not found"))?;

    let event = ReorgEventResponse {
        id: r.0,
        detected_at: r.1.to_rfc3339(),
        fork_point_number: r.2,
        fork_point_hash: format!("0x{}", hex::encode(&r.3)),
        old_tip_number: r.4,
        old_tip_hash: format!("0x{}", hex::encode(&r.5)),
        new_tip_number: r.6,
        new_tip_hash: format!("0x{}", hex::encode(&r.7)),
        depth: r.8,
        orphaned_blocks_count: r.9,
        orphaned_txs_count: r.10,
        event_type: r.11,
        resolved_at: r.12.map(|t| t.to_rfc3339()),
        resolved_by: r.13,
        resolution_action: r.14,
        resolution_notes: r.15,
    };

    let orphaned_blocks: Vec<(
        i64,
        Vec<u8>,
        Vec<u8>,
        chrono::DateTime<chrono::Utc>,
        i32,
        Option<Vec<u8>>,
    )> = sqlx::query_as(
        r#"
            SELECT number, hash, parent_hash, timestamp, transactions_count, miner_lock_hash
            FROM orphaned_blocks
            WHERE reorg_event_id = $1
            ORDER BY number DESC
            "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let orphaned_txs: Vec<(Vec<u8>, i64, Vec<u8>, i32, Option<i16>, Option<i16>, Option<i64>)> =
        sqlx::query_as(
            r#"
            SELECT hash, block_number, block_hash, tx_index, inputs_count, outputs_count, total_capacity::bigint
            FROM orphaned_transactions
            WHERE reorg_event_id = $1
            ORDER BY block_number DESC, tx_index
            LIMIT 100
            "#,
        )
        .bind(id)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(ReorgDetailResponse {
        event,
        orphaned_blocks: orphaned_blocks
            .into_iter()
            .map(|b| OrphanedBlockResponse {
                number: b.0,
                hash: format!("0x{}", hex::encode(&b.1)),
                parent_hash: format!("0x{}", hex::encode(&b.2)),
                timestamp: b.3.to_rfc3339(),
                transactions_count: b.4,
                miner_lock_hash: b.5.map(|h| format!("0x{}", hex::encode(&h))),
            })
            .collect(),
        orphaned_transactions: orphaned_txs
            .into_iter()
            .map(|t| OrphanedTransactionResponse {
                hash: format!("0x{}", hex::encode(&t.0)),
                block_number: t.1,
                block_hash: format!("0x{}", hex::encode(&t.2)),
                tx_index: t.3,
                inputs_count: t.4,
                outputs_count: t.5,
                total_capacity: t.6.map(|c| c.to_string()),
            })
            .collect(),
    })
}

async fn get_recent_reorg(State(state): State<Arc<AppState>>) -> ApiResult<RecentReorgResponse> {
    let deep_fork_row: (
        bool,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<i64>,
        Option<Vec<u8>>,
        Option<i64>,
        Option<Vec<u8>>,
        Option<i32>,
        Option<i64>,
    ) = sqlx::query_as(
        r#"
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
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let deep_fork = DeepForkStatusResponse {
        detected: deep_fork_row.0,
        detected_at: deep_fork_row.1.map(|t| t.to_rfc3339()),
        db_tip: deep_fork_row.2,
        db_tip_hash: deep_fork_row.3.map(|h| format!("0x{}", hex::encode(&h))),
        chain_tip: deep_fork_row.4,
        chain_tip_hash: deep_fork_row.5.map(|h| format!("0x{}", hex::encode(&h))),
        depth: deep_fork_row.6,
        fork_point: deep_fork_row.7,
    };

    let recent_reorg: Option<(
        i32,
        chrono::DateTime<chrono::Utc>,
        i64,
        Vec<u8>,
        i64,
        Vec<u8>,
        i64,
        Vec<u8>,
        i32,
        i32,
        i32,
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT 
            id, detected_at, 
            fork_point_number, fork_point_hash,
            old_tip_number, old_tip_hash,
            new_tip_number, new_tip_hash,
            depth, orphaned_blocks_count, orphaned_txs_count,
            event_type, resolved_at, resolved_by, resolution_action, resolution_notes
        FROM reorg_events
        WHERE detected_at >= NOW() - INTERVAL '24 hours'
        ORDER BY detected_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let reorg = recent_reorg.map(|r| ReorgEventResponse {
        id: r.0,
        detected_at: r.1.to_rfc3339(),
        fork_point_number: r.2,
        fork_point_hash: format!("0x{}", hex::encode(&r.3)),
        old_tip_number: r.4,
        old_tip_hash: format!("0x{}", hex::encode(&r.5)),
        new_tip_number: r.6,
        new_tip_hash: format!("0x{}", hex::encode(&r.7)),
        depth: r.8,
        orphaned_blocks_count: r.9,
        orphaned_txs_count: r.10,
        event_type: r.11,
        resolved_at: r.12.map(|t| t.to_rfc3339()),
        resolved_by: r.13,
        resolution_action: r.14,
        resolution_notes: r.15,
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

    let deep_fork_detected: (bool,) =
        sqlx::query_as("SELECT deep_fork_detected FROM sync_status WHERE id = 1")
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

    if !deep_fork_detected.0 {
        return Err(ApiError::bad_request("No deep fork to resolve"));
    }

    match req.action.as_str() {
        "dismiss" => {
            sqlx::query(
                r#"
                UPDATE reorg_events SET
                    event_type = 'resolved',
                    resolved_at = NOW(),
                    resolved_by = 'admin',
                    resolution_action = 'dismissed',
                    resolution_notes = $1
                WHERE event_type = 'deep' AND resolved_at IS NULL
                "#,
            )
            .bind(&req.notes)
            .execute(&state.pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            sqlx::query(
                r#"
                UPDATE sync_status SET
                    deep_fork_detected = FALSE,
                    deep_fork_at = NULL,
                    deep_fork_db_tip = NULL,
                    deep_fork_db_tip_hash = NULL,
                    deep_fork_chain_tip = NULL,
                    deep_fork_chain_tip_hash = NULL,
                    deep_fork_depth = NULL,
                    deep_fork_fork_point = NULL
                WHERE id = 1
                "#,
            )
            .execute(&state.pool)
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
