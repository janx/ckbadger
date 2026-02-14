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

async fn get_active_tasks(State(state): State<Arc<AppState>>) -> ApiResult<ActiveTasksResponse> {
    // Fetch pending and running tasks from the store
    let mut all_tasks = state
        .store
        .list_tasks_by_status("running")
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let pending_tasks = state
        .store
        .list_tasks_by_status("pending")
        .map_err(|e| ApiError::internal(e.to_string()))?;

    all_tasks.extend(pending_tasks);

    // Filter to the task types we care about
    let relevant_types = ["index_rebuild", "statistics_rebuild"];
    let rows: Vec<_> = all_tasks
        .into_iter()
        .filter(|t| relevant_types.contains(&t.task_type.as_str()))
        .collect();

    let mut index_rebuild = None;
    let mut statistics_rebuild = None;
    for task in rows {
        let progress_total = task.progress_total.unwrap_or(0);
        let progress_current = task.progress_current.unwrap_or(0);
        let progress = if progress_total > 0 {
            (progress_current as f64 / progress_total as f64) * 100.0
        } else {
            0.0
        };

        match task.task_type.as_str() {
            "index_rebuild" if index_rebuild.is_none() => {
                let is_running = task.status == "running";
                let (current_index, failed) = match task.result {
                    Some(ref json) => match serde_json::from_str::<IndexRebuildResult>(json) {
                        Ok(result) => (
                            result.current_index,
                            result.failed.into_iter().map(|f| f.name).collect(),
                        ),
                        Err(_) => (None, vec![]),
                    },
                    None => (None, vec![]),
                };
                index_rebuild = Some(IndexRebuildStatus {
                    status: task.status,
                    is_rebuilding: is_running,
                    total: progress_total,
                    completed: progress_current,
                    current_index,
                    failed,
                    progress,
                    started_at: task.started_at.map(|t| t.to_rfc3339()),
                });
            }
            "statistics_rebuild" if statistics_rebuild.is_none() => {
                let current_table = match task.result {
                    Some(ref json) => serde_json::from_str::<StatisticsRebuildResult>(json)
                        .ok()
                        .and_then(|r| r.current_table),
                    None => None,
                };
                statistics_rebuild = Some(StatisticsRebuildStatus {
                    status: task.status,
                    total: progress_total,
                    completed: progress_current,
                    current_table,
                    progress,
                    started_at: task.started_at.map(|t| t.to_rfc3339()),
                });
            }
            _ => {}
        }
    }

    ok(ActiveTasksResponse {
        index_rebuild,
        statistics_rebuild,
    })
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/tasks/active", get(get_active_tasks))
}
