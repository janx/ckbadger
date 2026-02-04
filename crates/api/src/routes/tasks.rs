//! Task status API endpoints.

use axum::{extract::State, routing::get, Router};
use ckbadger_common::task::{IndexRebuildResult, StatisticsRebuildResult};
use serde::Serialize;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveTasksResponse {
    pub index_rebuild: Option<IndexRebuildStatus>,
    pub statistics_rebuild: Option<StatisticsRebuildStatus>,
    pub live_cells_populate: Option<LiveCellsPopulateStatus>,
    pub activities_rebuild: Option<ActivitiesRebuildStatus>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRebuildStatus {
    pub status: String,
    pub is_rebuilding: bool,
    pub total: i64,
    pub completed: i64,
    pub current_index: Option<String>,
    pub failed: Vec<String>,
    pub progress: f64,
    pub started_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsRebuildStatus {
    pub status: String,
    pub total: i64,
    pub completed: i64,
    pub current_table: Option<String>,
    pub progress: f64,
    pub started_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCellsPopulateStatus {
    pub status: String,
    pub total: i64,
    pub populated: i64,
    pub progress: f64,
    pub started_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitiesRebuildStatus {
    pub status: String,
    pub total: i64,
    pub processed: i64,
    pub progress: f64,
    pub started_at: Option<String>,
}

async fn get_active_tasks(State(state): State<Arc<AppState>>) -> ApiResult<ActiveTasksResponse> {
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            i64,
            i64,
            Option<serde_json::Value>,
            Option<chrono::DateTime<chrono::Utc>>,
        ),
    >(
        r#"
        SELECT 
            task_type,
            status,
            COALESCE(progress_total, 0),
            COALESCE(progress_current, 0),
            result,
            started_at
        FROM tasks
        WHERE task_type IN ('index_rebuild', 'statistics_rebuild', 'live_cells_populate', 'activities_rebuild') 
          AND status IN ('pending', 'running')
        ORDER BY 
            CASE status WHEN 'running' THEN 0 ELSE 1 END,
            created_at DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut index_rebuild = None;
    let mut statistics_rebuild = None;
    let mut live_cells_populate = None;
    let mut activities_rebuild = None;

    for (task_type, status, progress_total, progress_current, result_json, started_at) in rows {
        let progress = if progress_total > 0 {
            (progress_current as f64 / progress_total as f64) * 100.0
        } else {
            0.0
        };

        match task_type.as_str() {
            "index_rebuild" if index_rebuild.is_none() => {
                let is_running = status == "running";
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
                index_rebuild = Some(IndexRebuildStatus {
                    status,
                    is_rebuilding: is_running,
                    total: progress_total,
                    completed: progress_current,
                    current_index,
                    failed,
                    progress,
                    started_at: started_at.map(|t| t.to_rfc3339()),
                });
            }
            "statistics_rebuild" if statistics_rebuild.is_none() => {
                let current_table = match result_json {
                    Some(json) => serde_json::from_value::<StatisticsRebuildResult>(json)
                        .ok()
                        .and_then(|r| r.current_table),
                    None => None,
                };
                statistics_rebuild = Some(StatisticsRebuildStatus {
                    status,
                    total: progress_total,
                    completed: progress_current,
                    current_table,
                    progress,
                    started_at: started_at.map(|t| t.to_rfc3339()),
                });
            }
            "live_cells_populate" if live_cells_populate.is_none() => {
                live_cells_populate = Some(LiveCellsPopulateStatus {
                    status,
                    total: progress_total,
                    populated: progress_current,
                    progress,
                    started_at: started_at.map(|t| t.to_rfc3339()),
                });
            }
            "activities_rebuild" if activities_rebuild.is_none() => {
                activities_rebuild = Some(ActivitiesRebuildStatus {
                    status,
                    total: progress_total,
                    processed: progress_current,
                    progress,
                    started_at: started_at.map(|t| t.to_rfc3339()),
                });
            }
            _ => {}
        }
    }

    ok(ActiveTasksResponse {
        index_rebuild,
        statistics_rebuild,
        live_cells_populate,
        activities_rebuild,
    })
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/tasks/active", get(get_active_tasks))
}
