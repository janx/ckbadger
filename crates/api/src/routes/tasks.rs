//! Task status API endpoints.

use axum::{extract::State, routing::get, Router};
use ckbadger_common::task::IndexRebuildResult;
use serde::Serialize;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveTasksResponse {
    pub index_rebuild: Option<IndexRebuildStatus>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRebuildStatus {
    pub is_rebuilding: bool,
    pub total: i64,
    pub completed: i64,
    pub current_index: Option<String>,
    pub failed: Vec<String>,
    pub progress: f64,
    pub started_at: Option<String>,
}

async fn get_active_tasks(State(state): State<Arc<AppState>>) -> ApiResult<ActiveTasksResponse> {
    let row = sqlx::query_as::<
        _,
        (
            i64,
            i64,
            Option<serde_json::Value>,
            Option<chrono::DateTime<chrono::Utc>>,
        ),
    >(
        r#"
        SELECT 
            COALESCE(progress_total, 0),
            COALESCE(progress_current, 0),
            result,
            started_at
        FROM tasks
        WHERE task_type = 'index_rebuild' AND status = 'running'
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let index_rebuild = match row {
        Some((progress_total, progress_current, result_json, started_at)) => {
            let (current_index, failed) = match result_json {
                Some(json) => match serde_json::from_value::<IndexRebuildResult>(json) {
                    Ok(result) => (
                        result.current_index,
                        result.failed.into_iter().map(|f| f.name).collect(),
                    ),
                    Err(_) => (None, vec![]),
                },
                None => (None, vec![]),
            };

            let progress = if progress_total > 0 {
                (progress_current as f64 / progress_total as f64) * 100.0
            } else {
                0.0
            };

            Some(IndexRebuildStatus {
                is_rebuilding: true,
                total: progress_total,
                completed: progress_current,
                current_index,
                failed,
                progress,
                started_at: started_at.map(|t| t.to_rfc3339()),
            })
        }
        None => None,
    };

    ok(ActiveTasksResponse { index_rebuild })
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/tasks/active", get(get_active_tasks))
}
