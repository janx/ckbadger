use axum::{extract::State, routing::get, Router};
use serde::Serialize;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    pub sync: SyncStatus,
    pub integrity: IntegrityStatus,
    pub label_import: LabelImportStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelImportStatus {
    pub is_running: bool,
    pub token_total_count: i64,
    pub token_imported_count: i64,
    pub script_total_count: i64,
    pub script_imported_count: i64,
    pub progress: f64,
    pub started_at: Option<String>,
    pub last_check_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub is_syncing: bool,
    pub synced_block: i64,
    pub tip_block: i64,
    pub progress: f64,
    pub estimated_time: Option<String>,
    pub last_synced_at: Option<String>,
    pub chart_data_may_be_incomplete: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityStatus {
    pub is_running: bool,
    pub pending_count: i64,
    pub total_count: i64,
    pub processed_count: i64,
    pub progress: f64,
    pub estimated_time: Option<String>,
    pub started_at: Option<String>,
    pub last_check_at: Option<String>,
    pub missing_cycles_count: i64,
    pub recent_fixes: Vec<RecentFix>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFix {
    pub tx_hash: String,
    pub cycles: i64,
    pub fixed_at: String,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/status", get(get_system_status))
}

const HEARTBEAT_TIMEOUT_SECS: i64 = 30;

#[derive(sqlx::FromRow)]
struct SyncStatusRow {
    tip_block_number: i64,
    last_synced_at: Option<chrono::DateTime<chrono::Utc>>,
    sync_started_at: Option<chrono::DateTime<chrono::Utc>>,
    sync_started_block: i64,
    integrity_heartbeat: Option<chrono::DateTime<chrono::Utc>>,
    integrity_pending_count: i64,
    integrity_total_count: i64,
    integrity_processed_count: i64,
    integrity_started_at: Option<chrono::DateTime<chrono::Utc>>,
    udt_info_running: bool,
    udt_info_total_count: i64,
    udt_info_processed_count: i64,
    udt_info_started_at: Option<chrono::DateTime<chrono::Utc>>,
    udt_info_last_check_at: Option<chrono::DateTime<chrono::Utc>>,
    script_info_running: bool,
    script_info_total_count: i64,
    script_info_processed_count: i64,
    script_info_started_at: Option<chrono::DateTime<chrono::Utc>>,
    script_info_last_check_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn get_system_status(State(state): State<Arc<AppState>>) -> ApiResult<SystemStatus> {
    let row = sqlx::query_as::<_, SyncStatusRow>(
        r#"
        SELECT 
            tip_block_number,
            last_synced_at,
            sync_started_at,
            COALESCE(sync_started_block, 0) as sync_started_block,
            integrity_heartbeat,
            COALESCE(integrity_pending_count, 0) as integrity_pending_count,
            COALESCE(integrity_total_count, 0) as integrity_total_count,
            COALESCE(integrity_processed_count, 0) as integrity_processed_count,
            integrity_started_at,
            COALESCE(udt_info_running, false) as udt_info_running,
            COALESCE(udt_info_total_count, 0) as udt_info_total_count,
            COALESCE(udt_info_processed_count, 0) as udt_info_processed_count,
            udt_info_started_at,
            udt_info_last_check_at,
            COALESCE(script_info_running, false) as script_info_running,
            COALESCE(script_info_total_count, 0) as script_info_total_count,
            COALESCE(script_info_processed_count, 0) as script_info_processed_count,
            script_info_started_at,
            script_info_last_check_at
        FROM sync_status WHERE id = 1
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let synced_block = row.tip_block_number;
    let last_synced_at = row.last_synced_at;
    let integrity_heartbeat = row.integrity_heartbeat;
    let integrity_pending = row.integrity_pending_count;
    let integrity_total = row.integrity_total_count;
    let integrity_processed = row.integrity_processed_count;
    let integrity_started_at = row.integrity_started_at;
    let udt_info_running = row.udt_info_running;
    let udt_info_total = row.udt_info_total_count;
    let udt_info_processed = row.udt_info_processed_count;
    let udt_info_started_at = row.udt_info_started_at;
    let udt_info_last_check = row.udt_info_last_check_at;
    let script_info_running = row.script_info_running;
    let script_info_total = row.script_info_total_count;
    let script_info_processed = row.script_info_processed_count;
    let script_info_started_at = row.script_info_started_at;
    let script_info_last_check = row.script_info_last_check_at;

    let integrity_running = match integrity_heartbeat {
        Some(heartbeat) => {
            let elapsed = chrono::Utc::now()
                .signed_duration_since(heartbeat)
                .num_seconds();
            elapsed < HEARTBEAT_TIMEOUT_SECS
        }
        None => false,
    };

    let tip_block = get_tip_block_number(&state.ckb_rpc_url)
        .await
        .unwrap_or(synced_block);

    let is_syncing = synced_block < tip_block.saturating_sub(10);
    let progress = if tip_block > 0 {
        (synced_block as f64 / tip_block as f64 * 100.0).min(100.0)
    } else {
        0.0
    };

    let blocks_behind = tip_block - synced_block;
    let estimated_time = if is_syncing && blocks_behind > 0 {
        if let Some(started_at) = row.sync_started_at {
            let elapsed = chrono::Utc::now()
                .signed_duration_since(started_at)
                .num_seconds() as u64;
            let blocks_synced = (synced_block - row.sync_started_block).max(0) as u64;
            if elapsed > 0 && blocks_synced > 0 {
                let rate = blocks_synced as f64 / elapsed as f64;
                let seconds_remaining = (blocks_behind as f64 / rate) as u64;
                Some(format_duration(seconds_remaining))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let missing_cycles: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM transactions WHERE NOT is_cellbase AND (cycles IS NULL OR cycles = 0)",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or((0,));

    let recent_fixes: Vec<(Vec<u8>, i64, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT tx_hash, cycles, fixed_at FROM integrity_recent_fixes ORDER BY fixed_at DESC LIMIT 10",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let missing_count = missing_cycles.0;
    let total_for_progress = integrity_processed + missing_count;
    let integrity_progress = if total_for_progress > 0 {
        (integrity_processed as f64 / total_for_progress as f64 * 100.0).min(100.0)
    } else {
        100.0
    };

    let integrity_eta = if integrity_running && missing_count > 0 && integrity_processed > 0 {
        if let Some(started) = integrity_started_at {
            let elapsed = chrono::Utc::now()
                .signed_duration_since(started)
                .num_seconds() as u64;
            let rate = integrity_processed as f64 / elapsed.max(1) as f64;
            let remaining_seconds = (missing_count as f64 / rate) as u64;
            Some(format_duration(remaining_seconds))
        } else {
            None
        }
    } else {
        None
    };

    let label_total = udt_info_total + script_info_total;
    let label_processed = udt_info_processed + script_info_processed;
    let label_progress = if label_total > 0 {
        (label_processed as f64 / label_total as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    let label_running = udt_info_running || script_info_running;
    let label_last_check = match (udt_info_last_check, script_info_last_check) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    let label_started_at = match (udt_info_started_at, script_info_started_at) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    ok(SystemStatus {
        sync: SyncStatus {
            is_syncing,
            synced_block,
            tip_block,
            progress,
            estimated_time,
            last_synced_at: last_synced_at.map(|t| t.to_rfc3339()),
            chart_data_may_be_incomplete: blocks_behind > 1000,
        },
        integrity: IntegrityStatus {
            is_running: integrity_running,
            pending_count: integrity_pending.max(0),
            total_count: integrity_total,
            processed_count: integrity_processed,
            progress: integrity_progress,
            estimated_time: integrity_eta,
            started_at: integrity_started_at.map(|t| t.to_rfc3339()),
            last_check_at: integrity_heartbeat.map(|t| t.to_rfc3339()),
            missing_cycles_count: missing_cycles.0,
            recent_fixes: recent_fixes
                .into_iter()
                .map(|(hash, cycles, fixed_at)| RecentFix {
                    tx_hash: format!("0x{}", hex::encode(hash)),
                    cycles,
                    fixed_at: fixed_at.to_rfc3339(),
                })
                .collect(),
        },
        label_import: LabelImportStatus {
            is_running: label_running,
            token_total_count: udt_info_total,
            token_imported_count: udt_info_processed,
            script_total_count: script_info_total,
            script_imported_count: script_info_processed,
            progress: label_progress,
            started_at: label_started_at.map(|t| t.to_rfc3339()),
            last_check_at: label_last_check.map(|t| t.to_rfc3339()),
        },
    })
}

fn format_duration(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes.max(1))
    }
}

async fn get_tip_block_number(rpc_url: &str) -> Option<i64> {
    #[derive(serde::Deserialize)]
    struct RpcResponse {
        result: Option<String>,
    }

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_tip_block_number",
        "params": []
    });

    let resp: RpcResponse = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    resp.result.and_then(|s| {
        let s = s.strip_prefix("0x").unwrap_or(&s);
        i64::from_str_radix(s, 16).ok()
    })
}
