//! Task management API endpoints.

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use ckbadger_common::task::{
    IndexRebuildResult, LabelImportConfig, StatisticsRebuildResult, TaskBuilder,
};
use ckbadger_store::types::TaskEntry;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::response::{ok, ApiError, ApiResult};
use crate::AppState;

// ─── Shared task JSON representation ───────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskJson {
    pub id: String,
    pub task_type: String,
    pub status: String,
    pub priority: i32,
    pub config: serde_json::Value,
    pub progress_total: Option<i64>,
    pub progress_current: Option<i64>,
    pub progress_message: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub runner_id: Option<String>,
    pub retry_count: i32,
    pub max_retries: i32,
    pub rate_ema: Option<f64>,
    pub log_tail: Option<String>,
}

fn entry_to_json(e: TaskEntry) -> TaskJson {
    TaskJson {
        id: e.id.to_string(),
        task_type: e.task_type,
        status: e.status,
        priority: e.priority,
        config: serde_json::from_str(&e.config).unwrap_or_default(),
        progress_total: e.progress_total,
        progress_current: e.progress_current,
        progress_message: e.progress_message,
        result: e.result.and_then(|s| serde_json::from_str(&s).ok()),
        error_message: e.error_message,
        created_at: e.created_at,
        started_at: e.started_at,
        completed_at: e.completed_at,
        heartbeat_at: e.heartbeat_at,
        runner_id: e.runner_id,
        retry_count: e.retry_count,
        max_retries: e.max_retries,
        rate_ema: e.rate_ema,
        log_tail: e.log_tail,
    }
}

// ─── GET /tasks ────────────────────────────────────────────────────────────

async fn list_tasks(State(state): State<Arc<AppState>>) -> ApiResult<Vec<TaskJson>> {
    let entries = state
        .store
        .list_all_tasks()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(entries.into_iter().map(entry_to_json).collect())
}

// ─── POST /tasks ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    pub task_type: String,
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskResponse {
    pub id: String,
}

async fn create_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTaskRequest>,
) -> ApiResult<CreateTaskResponse> {
    let builder = match req.task_type.as_str() {
        "label_import" => {
            let config: LabelImportConfig = serde_json::from_value(req.config).unwrap_or_default();
            TaskBuilder::label_import(config)
        }
        _ => {
            return Err(ApiError::bad_request(format!(
                "Unknown task type: {}",
                req.task_type
            )))
        }
    };

    let id = Uuid::new_v4();
    let entry = TaskEntry {
        id,
        task_type: builder.task_type().to_string(),
        status: "pending".to_string(),
        priority: builder.get_priority(),
        config: serde_json::to_string(builder.config()).unwrap_or_default(),
        progress_total: None,
        progress_current: None,
        progress_message: None,
        result: None,
        error_message: None,
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        heartbeat_at: None,
        runner_id: None,
        retry_count: 0,
        max_retries: builder.get_max_retries(),
        rate_samples: None,
        rate_ema: None,
        log_tail: None,
    };
    state
        .store
        .create_task(&entry)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(CreateTaskResponse { id: id.to_string() })
}

// ─── POST /tasks/:id/cancel ────────────────────────────────────────────────

async fn cancel_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<TaskJson> {
    let task_id = id
        .parse::<Uuid>()
        .map_err(|_| ApiError::bad_request("Invalid task ID"))?;
    let mut task = state
        .store
        .get_task(&task_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Task not found"))?;

    match task.status.as_str() {
        "pending" | "running" | "paused" => {}
        _ => {
            return Err(ApiError::bad_request(format!(
                "Cannot cancel task in '{}' state",
                task.status
            )))
        }
    }

    let old_status = task.status.clone();
    let old_priority = task.priority;
    task.status = "cancelled".to_string();
    task.completed_at = Some(Utc::now());
    state
        .store
        .update_task(&task, &old_status, old_priority)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(entry_to_json(task))
}

// ─── POST /tasks/:id/pause ─────────────────────────────────────────────────

async fn pause_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<TaskJson> {
    let task_id = id
        .parse::<Uuid>()
        .map_err(|_| ApiError::bad_request("Invalid task ID"))?;
    let mut task = state
        .store
        .get_task(&task_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Task not found"))?;

    if task.status != "running" {
        return Err(ApiError::bad_request(format!(
            "Cannot pause task in '{}' state",
            task.status
        )));
    }

    let old_status = task.status.clone();
    let old_priority = task.priority;
    task.status = "paused".to_string();
    state
        .store
        .update_task(&task, &old_status, old_priority)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(entry_to_json(task))
}

// ─── POST /tasks/:id/resume ────────────────────────────────────────────────

async fn resume_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<TaskJson> {
    let task_id = id
        .parse::<Uuid>()
        .map_err(|_| ApiError::bad_request("Invalid task ID"))?;
    let mut task = state
        .store
        .get_task(&task_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Task not found"))?;

    if task.status != "paused" {
        return Err(ApiError::bad_request(format!(
            "Cannot resume task in '{}' state",
            task.status
        )));
    }

    let old_status = task.status.clone();
    let old_priority = task.priority;
    task.status = "pending".to_string();
    task.runner_id = None;
    state
        .store
        .update_task(&task, &old_status, old_priority)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(entry_to_json(task))
}

// ─── POST /tasks/:id/retry ─────────────────────────────────────────────────

async fn retry_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<TaskJson> {
    let task_id = id
        .parse::<Uuid>()
        .map_err(|_| ApiError::bad_request("Invalid task ID"))?;
    let mut task = state
        .store
        .get_task(&task_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Task not found"))?;

    if task.status != "failed" {
        return Err(ApiError::bad_request(format!(
            "Cannot retry task in '{}' state",
            task.status
        )));
    }

    let old_status = task.status.clone();
    let old_priority = task.priority;
    task.status = "pending".to_string();
    task.runner_id = None;
    task.error_message = None;
    task.retry_count = 0;
    state
        .store
        .update_task(&task, &old_status, old_priority)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(entry_to_json(task))
}

// ─── DELETE /tasks/:id ─────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTaskResponse {
    pub deleted: bool,
}

async fn delete_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<DeleteTaskResponse> {
    let task_id = id
        .parse::<Uuid>()
        .map_err(|_| ApiError::bad_request("Invalid task ID"))?;
    let task = state
        .store
        .get_task(&task_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Task not found"))?;

    if task.status == "running" {
        return Err(ApiError::bad_request("Cannot delete a running task"));
    }

    let status_byte = match task.status.as_str() {
        "pending" => 0x01,
        "running" => 0x02,
        "completed" => 0x03,
        "failed" => 0x04,
        "cancelled" => 0x05,
        "paused" => 0x06,
        _ => 0xFF,
    };
    state
        .store
        .delete_cf(state.store.cf_tasks(), task_id.as_bytes())
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let idx_key = ckbadger_store::keys::encode_task_index_key(status_byte, task.priority, &task.id);
    state
        .store
        .delete_cf(state.store.cf_tasks_index(), &idx_key)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(DeleteTaskResponse { deleted: true })
}

// ─── Legacy: GET /tasks/active ─────────────────────────────────────────────

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

// ─── Routes ────────────────────────────────────────────────────────────────

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/active", get(get_active_tasks))
        .route("/tasks/{id}", delete(delete_task))
        .route("/tasks/{id}/cancel", post(cancel_task))
        .route("/tasks/{id}/pause", post(pause_task))
        .route("/tasks/{id}/resume", post(resume_task))
        .route("/tasks/{id}/retry", post(retry_task))
}
