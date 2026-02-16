//! Task management API endpoints.
//!
//! Mutation endpoints (create, cancel, pause, resume, retry, delete) use a Redis
//! command queue because the API opens RocksDB in secondary (read-only) mode.
//! The task-runner (with primary write access) consumes and executes commands.

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use ckbadger_common::task::{
    IndexRebuildResult, LabelImportConfig, StatisticsRebuildResult, TaskBuilder,
};
use ckbadger_common::task_cmd::{
    task_cmd_key, task_cmd_result_key, TaskCommand, TaskCommandAction, TaskCommandResult,
    TASK_CMD_QUEUE_KEY, TASK_CMD_TTL_SECS,
};
use ckbadger_store::types::TaskEntry;
use redis::AsyncCommands;
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

// ─── Redis command queue helper ────────────────────────────────────────────

type ApiErr = (axum::http::StatusCode, axum::Json<ApiError>);

/// Send a task command via Redis and wait for the result (up to 5s).
async fn send_task_command(
    state: &AppState,
    action: TaskCommandAction,
) -> Result<TaskCommandResult, ApiErr> {
    let mut conn = state
        .redis_conn
        .clone()
        .ok_or_else(|| ApiError::internal("Redis not available — task mutations disabled"))?;

    let cmd = TaskCommand {
        id: Uuid::new_v4(),
        action,
    };
    let cmd_json = serde_json::to_string(&cmd).map_err(|e| ApiError::internal(e.to_string()))?;

    // SET command payload with TTL
    let cmd_key = task_cmd_key(&cmd.id);
    conn.set_ex::<_, _, ()>(&cmd_key, &cmd_json, TASK_CMD_TTL_SECS)
        .await
        .map_err(|e| ApiError::internal(format!("Redis SET failed: {}", e)))?;

    // RPUSH command ID to queue
    conn.rpush::<_, _, ()>(TASK_CMD_QUEUE_KEY, cmd.id.to_string())
        .await
        .map_err(|e| ApiError::internal(format!("Redis RPUSH failed: {}", e)))?;

    // Poll for result (5s timeout, 50ms intervals)
    let result_key = task_cmd_result_key(&cmd.id);
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);

    loop {
        let result: Option<String> = conn
            .get(&result_key)
            .await
            .map_err(|e| ApiError::internal(format!("Redis GET failed: {}", e)))?;

        if let Some(json) = result {
            let cmd_result: TaskCommandResult = serde_json::from_str(&json)
                .map_err(|e| ApiError::internal(format!("Invalid command result: {}", e)))?;
            return Ok(cmd_result);
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(ApiError::internal(
                "Task command timed out — task-runner may not be running",
            ));
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
}

/// After the task-runner processes a command, refresh the store and re-read the task.
fn refresh_and_get_task(state: &AppState, task_id: &Uuid) -> Result<TaskEntry, ApiErr> {
    // Refresh secondary RocksDB instance to pick up the write
    let _ = state.store.refresh();
    state
        .store
        .get_task(task_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Task not found after command execution"))
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
    // Validate task type before sending to queue
    match req.task_type.as_str() {
        "label_import" => {
            let _config: LabelImportConfig =
                serde_json::from_value(req.config.clone()).unwrap_or_default();
            let _ = TaskBuilder::label_import(_config);
        }
        _ => {
            return Err(ApiError::bad_request(format!(
                "Unknown task type: {}",
                req.task_type
            )))
        }
    };

    let result = send_task_command(
        &state,
        TaskCommandAction::Create {
            task_type: req.task_type,
            config: req.config,
        },
    )
    .await?;

    if result.success {
        let task_id = result
            .task_id
            .ok_or_else(|| ApiError::internal("No task ID in create result"))?;
        ok(CreateTaskResponse {
            id: task_id.to_string(),
        })
    } else {
        Err(ApiError::internal(result.error.unwrap_or_else(|| {
            "Unknown error creating task".to_string()
        })))
    }
}

// ─── POST /tasks/:id/cancel ────────────────────────────────────────────────

async fn cancel_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<TaskJson> {
    let task_id = id
        .parse::<Uuid>()
        .map_err(|_| ApiError::bad_request("Invalid task ID"))?;

    // Validate task exists and is in cancellable state (read-only check)
    let task = state
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

    let result = send_task_command(&state, TaskCommandAction::Cancel { task_id }).await?;

    if result.success {
        let entry = refresh_and_get_task(&state, &task_id)?;
        ok(entry_to_json(entry))
    } else {
        Err(ApiError::internal(
            result
                .error
                .unwrap_or_else(|| "Failed to cancel task".to_string()),
        ))
    }
}

// ─── POST /tasks/:id/pause ─────────────────────────────────────────────────

async fn pause_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<TaskJson> {
    let task_id = id
        .parse::<Uuid>()
        .map_err(|_| ApiError::bad_request("Invalid task ID"))?;

    let task = state
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

    let result = send_task_command(&state, TaskCommandAction::Pause { task_id }).await?;

    if result.success {
        let entry = refresh_and_get_task(&state, &task_id)?;
        ok(entry_to_json(entry))
    } else {
        Err(ApiError::internal(
            result
                .error
                .unwrap_or_else(|| "Failed to pause task".to_string()),
        ))
    }
}

// ─── POST /tasks/:id/resume ────────────────────────────────────────────────

async fn resume_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<TaskJson> {
    let task_id = id
        .parse::<Uuid>()
        .map_err(|_| ApiError::bad_request("Invalid task ID"))?;

    let task = state
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

    let result = send_task_command(&state, TaskCommandAction::Resume { task_id }).await?;

    if result.success {
        let entry = refresh_and_get_task(&state, &task_id)?;
        ok(entry_to_json(entry))
    } else {
        Err(ApiError::internal(
            result
                .error
                .unwrap_or_else(|| "Failed to resume task".to_string()),
        ))
    }
}

// ─── POST /tasks/:id/retry ─────────────────────────────────────────────────

async fn retry_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<TaskJson> {
    let task_id = id
        .parse::<Uuid>()
        .map_err(|_| ApiError::bad_request("Invalid task ID"))?;

    let task = state
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

    let result = send_task_command(&state, TaskCommandAction::Retry { task_id }).await?;

    if result.success {
        let entry = refresh_and_get_task(&state, &task_id)?;
        ok(entry_to_json(entry))
    } else {
        Err(ApiError::internal(
            result
                .error
                .unwrap_or_else(|| "Failed to retry task".to_string()),
        ))
    }
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

    let result = send_task_command(&state, TaskCommandAction::Delete { task_id }).await?;

    if result.success {
        ok(DeleteTaskResponse { deleted: true })
    } else {
        Err(ApiError::internal(
            result
                .error
                .unwrap_or_else(|| "Failed to delete task".to_string()),
        ))
    }
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
