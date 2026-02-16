use anyhow::Result;
use ckbadger_common::task_cmd::{
    task_cmd_key, task_cmd_result_key, TaskCommand, TaskCommandAction, TaskCommandResult,
    TASK_CMD_QUEUE_KEY, TASK_CMD_RESULT_TTL_SECS,
};
use ckbadger_common::{LabelImportConfig, Task, TaskBuilder};
use ckbadger_store::CkbadgerStore;
use redis::AsyncCommands;
use std::sync::Arc;
use tracing::{debug, error, warn};
use uuid::Uuid;

pub struct TaskDb {
    store: Arc<CkbadgerStore>,
    redis_conn: Option<redis::aio::ConnectionManager>,
}

#[allow(dead_code)]
impl TaskDb {
    pub fn new(
        store: Arc<CkbadgerStore>,
        redis_conn: Option<redis::aio::ConnectionManager>,
    ) -> Self {
        Self { store, redis_conn }
    }

    /// Check if bulk sync is still in progress by looking at actual block data.
    pub async fn is_bulk_sync_active(&self) -> Result<bool> {
        self.store.is_bulk_sync_active_by_timestamp()
    }

    /// Check if a specific task type is currently running.
    pub async fn is_task_type_running(&self, task_type: &str) -> Result<bool> {
        self.store.is_task_type_running(task_type)
    }

    pub async fn defer_task(&self, task_id: Uuid, reason: &str) -> Result<()> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            let old_status = task.status.clone();
            let old_priority = task.priority;
            task.status = "pending".to_string();
            task.runner_id = None;
            task.error_message = Some(reason.to_string());
            task.heartbeat_at = Some(chrono::Utc::now());
            self.store.update_task(&task, &old_status, old_priority)?;
        }
        Ok(())
    }

    pub async fn recover_orphaned_tasks(&self, timeout_secs: i64) -> Result<u64> {
        self.store.recover_orphaned_tasks(timeout_secs)
    }

    pub async fn claim_next_task(&self, runner_id: &str) -> Result<Option<Task>> {
        let entry = self.store.claim_next_task(runner_id)?;
        Ok(entry.map(task_entry_to_task))
    }

    /// Claim all pending tasks at the highest available priority level.
    pub async fn claim_tasks_at_same_priority(&self, runner_id: &str) -> Result<Vec<Task>> {
        // Get all pending tasks, find the highest priority, claim all at that level
        let pending = self.store.list_tasks_by_status("pending")?;
        if pending.is_empty() {
            return Ok(vec![]);
        }

        let max_priority = pending.iter().map(|t| t.priority).max().unwrap_or(0);

        let mut claimed = Vec::new();
        for entry in pending {
            if entry.priority == max_priority {
                let id = entry.id;
                let old_status = entry.status.clone();
                let old_priority = entry.priority;
                let mut entry = entry;
                entry.status = "running".to_string();
                entry.runner_id = Some(runner_id.to_string());
                entry.started_at = Some(chrono::Utc::now());
                entry.heartbeat_at = Some(chrono::Utc::now());
                self.store.update_task(&entry, &old_status, old_priority)?;
                claimed.push(task_entry_to_task(entry));

                // Verify it was claimed (re-read)
                if let Some(task) = self.store.get_task(&id)? {
                    if task.status == "running" && task.runner_id.as_deref() == Some(runner_id) {
                        continue;
                    }
                }
            }
        }
        Ok(claimed)
    }

    pub async fn update_progress(
        &self,
        task_id: Uuid,
        current: i64,
        total: i64,
        message: Option<&str>,
        rate_ema: Option<f64>,
    ) -> Result<()> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            task.progress_current = Some(current);
            task.progress_total = Some(total);
            task.progress_message = message.map(String::from);
            if let Some(ema) = rate_ema {
                task.rate_ema = Some(ema);
            }
            task.heartbeat_at = Some(chrono::Utc::now());
            let value = bincode::serialize(&task)?;
            self.store
                .put_cf(self.store.cf_tasks(), task_id.as_bytes(), &value)?;
        }
        Ok(())
    }

    pub async fn update_result(&self, task_id: Uuid, result: &serde_json::Value) -> Result<()> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            task.result = Some(serde_json::to_string(result)?);
            task.heartbeat_at = Some(chrono::Utc::now());
            let value = bincode::serialize(&task)?;
            self.store
                .put_cf(self.store.cf_tasks(), task_id.as_bytes(), &value)?;
        }
        Ok(())
    }

    pub async fn append_log(&self, task_id: Uuid, line: &str) -> Result<()> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            let new_log = match &task.log_tail {
                Some(existing) => {
                    // Keep last 100 lines
                    let mut lines: Vec<&str> = existing.lines().collect();
                    lines.push(line);
                    if lines.len() > 100 {
                        lines = lines[lines.len() - 100..].to_vec();
                    }
                    lines.join("\n")
                }
                None => line.to_string(),
            };
            task.log_tail = Some(new_log);
            task.heartbeat_at = Some(chrono::Utc::now());
            let value = bincode::serialize(&task)?;
            self.store
                .put_cf(self.store.cf_tasks(), task_id.as_bytes(), &value)?;
        }
        Ok(())
    }

    pub async fn complete_task(
        &self,
        task_id: Uuid,
        result: Option<serde_json::Value>,
    ) -> Result<()> {
        let completion_message = result.as_ref().and_then(|r| r.as_object()).and_then(|obj| {
            let mut parts = Vec::new();
            for (key, label) in [
                ("udt_labels_imported", "UDT labels"),
                ("script_labels_imported", "scripts"),
                ("cells_populated", "cells"),
                ("blocks_processed", "blocks"),
                ("transactions_updated", "txs"),
            ] {
                if let Some(count) = obj.get(key).and_then(|v| v.as_i64()) {
                    if count > 0 {
                        parts.push(format!("{} {}", count, label));
                    }
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(format!("Completed: {}", parts.join(", ")))
            }
        });

        if let Some(mut task) = self.store.get_task(&task_id)? {
            let old_status = task.status.clone();
            let old_priority = task.priority;
            task.status = "completed".to_string();
            task.completed_at = Some(chrono::Utc::now());
            if let Some(total) = task.progress_total {
                task.progress_current = Some(total);
            }
            if let Some(msg) = completion_message {
                task.progress_message = Some(msg);
            }
            if let Some(r) = result {
                task.result = Some(serde_json::to_string(&r)?);
            }
            task.heartbeat_at = Some(chrono::Utc::now());
            self.store.update_task(&task, &old_status, old_priority)?;
        }
        Ok(())
    }

    pub async fn fail_task(&self, task_id: Uuid, error: &str) -> Result<()> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            let old_status = task.status.clone();
            let old_priority = task.priority;
            if task.retry_count < task.max_retries {
                task.status = "pending".to_string();
            } else {
                task.status = "failed".to_string();
            }
            task.error_message = Some(error.to_string());
            task.retry_count += 1;
            task.runner_id = None;
            task.heartbeat_at = Some(chrono::Utc::now());
            self.store.update_task(&task, &old_status, old_priority)?;
        }
        Ok(())
    }

    pub async fn heartbeat(&self, task_id: Uuid) -> Result<()> {
        self.store.heartbeat_task(&task_id)?;
        Ok(())
    }

    pub async fn check_cancelled(&self, task_id: Uuid) -> Result<bool> {
        // Process any pending Redis commands first so cancel requests are picked up promptly.
        self.process_redis_commands().await;

        if let Some(task) = self.store.get_task(&task_id)? {
            Ok(task.status == "cancelled")
        } else {
            Ok(false)
        }
    }

    pub async fn create_task(&self, builder: &TaskBuilder) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let entry = ckbadger_store::types::TaskEntry {
            id,
            task_type: builder.task_type().to_string(),
            status: "pending".to_string(),
            priority: builder.get_priority(),
            config: serde_json::to_string(builder.config())?,
            progress_total: None,
            progress_current: None,
            progress_message: None,
            result: None,
            error_message: None,
            created_at: chrono::Utc::now(),
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
        self.store.create_task(&entry)?;
        Ok(id)
    }

    pub async fn list_tasks(&self, limit: i64) -> Result<Vec<Task>> {
        let mut all = self.store.list_all_tasks()?;

        // Sort: running first, then pending, paused, failed, completed, cancelled
        all.sort_by(|a, b| {
            let status_order = |s: &str| -> u8 {
                match s {
                    "running" => 1,
                    "pending" => 2,
                    "paused" => 3,
                    "failed" => 4,
                    "completed" => 5,
                    "cancelled" => 6,
                    _ => 7,
                }
            };
            let ord = status_order(&a.status).cmp(&status_order(&b.status));
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
            // Within same status, newest first
            b.created_at.cmp(&a.created_at)
        });

        all.truncate(limit as usize);
        Ok(all.into_iter().map(task_entry_to_task).collect())
    }

    pub async fn get_task(&self, task_id: Uuid) -> Result<Option<Task>> {
        Ok(self.store.get_task(&task_id)?.map(task_entry_to_task))
    }

    pub async fn cancel_task(&self, task_id: Uuid) -> Result<bool> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            if task.status == "pending" || task.status == "running" || task.status == "paused" {
                let old_status = task.status.clone();
                let old_priority = task.priority;
                task.status = "cancelled".to_string();
                self.store.update_task(&task, &old_status, old_priority)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn pause_task(&self, task_id: Uuid) -> Result<bool> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            if task.status == "running" {
                let old_status = task.status.clone();
                let old_priority = task.priority;
                task.status = "paused".to_string();
                self.store.update_task(&task, &old_status, old_priority)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn resume_task(&self, task_id: Uuid) -> Result<bool> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            if task.status == "paused" {
                let old_status = task.status.clone();
                let old_priority = task.priority;
                task.status = "pending".to_string();
                task.runner_id = None;
                self.store.update_task(&task, &old_status, old_priority)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn retry_task(&self, task_id: Uuid) -> Result<bool> {
        if let Some(mut task) = self.store.get_task(&task_id)? {
            if task.status == "failed" {
                let old_status = task.status.clone();
                let old_priority = task.priority;
                task.status = "pending".to_string();
                task.runner_id = None;
                task.error_message = None;
                task.retry_count = 0;
                self.store.update_task(&task, &old_status, old_priority)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn delete_task(&self, task_id: Uuid) -> Result<bool> {
        if let Some(task) = self.store.get_task(&task_id)? {
            if task.status == "completed" || task.status == "failed" || task.status == "cancelled" {
                // Delete from both CFs
                self.store
                    .delete_cf(self.store.cf_tasks(), task_id.as_bytes())?;
                let idx_key = ckbadger_store::keys::encode_task_index_key(
                    match task.status.as_str() {
                        "completed" => 0x03,
                        "failed" => 0x04,
                        "cancelled" => 0x05,
                        _ => 0xFF,
                    },
                    task.priority,
                    &task.id,
                );
                self.store
                    .delete_cf(self.store.cf_tasks_index(), &idx_key)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ─── Redis command queue processing ────────────────────────────────────

    /// Process all pending commands from the Redis queue.
    /// Returns the number of commands processed.
    pub async fn process_redis_commands(&self) -> u64 {
        let Some(ref redis_conn) = self.redis_conn else {
            return 0;
        };

        let mut conn = redis_conn.clone();
        let mut processed = 0u64;

        loop {
            // LPOP one command ID from the queue
            let cmd_id_str: Option<String> = match conn.lpop(TASK_CMD_QUEUE_KEY, None).await {
                Ok(v) => v,
                Err(e) => {
                    warn!("Redis LPOP failed: {}", e);
                    break;
                }
            };

            let Some(cmd_id_str) = cmd_id_str else {
                break; // Queue empty
            };

            let cmd_id = match cmd_id_str.parse::<Uuid>() {
                Ok(id) => id,
                Err(e) => {
                    warn!("Invalid command UUID in queue: {} ({})", cmd_id_str, e);
                    continue;
                }
            };

            // GET command payload
            let cmd_key = task_cmd_key(&cmd_id);
            let cmd_json: Option<String> = match conn.get(&cmd_key).await {
                Ok(v) => v,
                Err(e) => {
                    warn!("Redis GET command failed for {}: {}", cmd_id, e);
                    continue;
                }
            };

            let Some(cmd_json) = cmd_json else {
                warn!("Command {} expired before processing", cmd_id);
                continue;
            };

            let cmd: TaskCommand = match serde_json::from_str(&cmd_json) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Invalid command JSON for {}: {}", cmd_id, e);
                    continue;
                }
            };

            // Execute the command
            let result = self.execute_command(&cmd).await;

            // Publish result
            let result_key = task_cmd_result_key(&cmd.id);
            match serde_json::to_string(&result) {
                Ok(result_json) => {
                    if let Err(e) = conn
                        .set_ex::<_, _, ()>(&result_key, &result_json, TASK_CMD_RESULT_TTL_SECS)
                        .await
                    {
                        error!("Redis SET result failed for {}: {}", cmd.id, e);
                    }
                }
                Err(e) => {
                    error!("Failed to serialize command result for {}: {}", cmd.id, e);
                }
            }

            // Clean up command key
            let _: Result<(), _> = conn.del(&cmd_key).await;

            processed += 1;
            debug!("Processed task command {} ({:?})", cmd.id, result);
        }

        if processed > 0 {
            debug!("Processed {} Redis task command(s)", processed);
        }

        processed
    }

    /// Execute a single task command and return the result.
    async fn execute_command(&self, cmd: &TaskCommand) -> TaskCommandResult {
        match &cmd.action {
            TaskCommandAction::Create { task_type, config } => {
                self.execute_create_command(cmd.id, task_type, config).await
            }
            TaskCommandAction::Cancel { task_id } => {
                self.execute_cancel_command(cmd.id, *task_id).await
            }
            TaskCommandAction::Pause { task_id } => {
                self.execute_pause_command(cmd.id, *task_id).await
            }
            TaskCommandAction::Resume { task_id } => {
                self.execute_resume_command(cmd.id, *task_id).await
            }
            TaskCommandAction::Retry { task_id } => {
                self.execute_retry_command(cmd.id, *task_id).await
            }
            TaskCommandAction::Delete { task_id } => {
                self.execute_delete_command(cmd.id, *task_id).await
            }
        }
    }

    async fn execute_create_command(
        &self,
        cmd_id: Uuid,
        task_type: &str,
        config: &serde_json::Value,
    ) -> TaskCommandResult {
        let builder = match task_type {
            "label_import" => {
                let cfg: LabelImportConfig =
                    serde_json::from_value(config.clone()).unwrap_or_default();
                TaskBuilder::label_import(cfg)
            }
            _ => {
                return TaskCommandResult {
                    cmd_id,
                    success: false,
                    task_id: None,
                    error: Some(format!("Unknown task type: {}", task_type)),
                }
            }
        };

        match self.create_task(&builder).await {
            Ok(id) => TaskCommandResult {
                cmd_id,
                success: true,
                task_id: Some(id),
                error: None,
            },
            Err(e) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: None,
                error: Some(e.to_string()),
            },
        }
    }

    async fn execute_cancel_command(&self, cmd_id: Uuid, task_id: Uuid) -> TaskCommandResult {
        match self.cancel_task(task_id).await {
            Ok(true) => TaskCommandResult {
                cmd_id,
                success: true,
                task_id: Some(task_id),
                error: None,
            },
            Ok(false) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: Some(task_id),
                error: Some("Task not found or not in cancellable state".to_string()),
            },
            Err(e) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: Some(task_id),
                error: Some(e.to_string()),
            },
        }
    }

    async fn execute_pause_command(&self, cmd_id: Uuid, task_id: Uuid) -> TaskCommandResult {
        match self.pause_task(task_id).await {
            Ok(true) => TaskCommandResult {
                cmd_id,
                success: true,
                task_id: Some(task_id),
                error: None,
            },
            Ok(false) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: Some(task_id),
                error: Some("Task not found or not in pausable state".to_string()),
            },
            Err(e) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: Some(task_id),
                error: Some(e.to_string()),
            },
        }
    }

    async fn execute_resume_command(&self, cmd_id: Uuid, task_id: Uuid) -> TaskCommandResult {
        match self.resume_task(task_id).await {
            Ok(true) => TaskCommandResult {
                cmd_id,
                success: true,
                task_id: Some(task_id),
                error: None,
            },
            Ok(false) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: Some(task_id),
                error: Some("Task not found or not in paused state".to_string()),
            },
            Err(e) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: Some(task_id),
                error: Some(e.to_string()),
            },
        }
    }

    async fn execute_retry_command(&self, cmd_id: Uuid, task_id: Uuid) -> TaskCommandResult {
        match self.retry_task(task_id).await {
            Ok(true) => TaskCommandResult {
                cmd_id,
                success: true,
                task_id: Some(task_id),
                error: None,
            },
            Ok(false) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: Some(task_id),
                error: Some("Task not found or not in failed state".to_string()),
            },
            Err(e) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: Some(task_id),
                error: Some(e.to_string()),
            },
        }
    }

    async fn execute_delete_command(&self, cmd_id: Uuid, task_id: Uuid) -> TaskCommandResult {
        match self.delete_task(task_id).await {
            Ok(true) => TaskCommandResult {
                cmd_id,
                success: true,
                task_id: Some(task_id),
                error: None,
            },
            Ok(false) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: Some(task_id),
                error: Some("Task not found or not in deletable state".to_string()),
            },
            Err(e) => TaskCommandResult {
                cmd_id,
                success: false,
                task_id: Some(task_id),
                error: Some(e.to_string()),
            },
        }
    }
}

/// Convert TaskEntry from ckbadger-store to common Task.
fn task_entry_to_task(entry: ckbadger_store::types::TaskEntry) -> Task {
    Task {
        id: entry.id,
        task_type: entry.task_type,
        status: entry.status,
        priority: entry.priority,
        config: serde_json::from_str(&entry.config).unwrap_or_default(),
        progress_total: entry.progress_total,
        progress_current: entry.progress_current,
        progress_message: entry.progress_message,
        result: entry.result.and_then(|s| serde_json::from_str(&s).ok()),
        error_message: entry.error_message,
        created_at: entry.created_at,
        started_at: entry.started_at,
        completed_at: entry.completed_at,
        heartbeat_at: entry.heartbeat_at,
        runner_id: entry.runner_id,
        retry_count: entry.retry_count,
        max_retries: entry.max_retries,
        rate_samples: None,
        rate_ema: entry.rate_ema,
        log_tail: entry.log_tail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::CkbadgerStore;

    fn temp_db() -> (TaskDb, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap());
        (TaskDb::new(store, None), dir)
    }

    #[tokio::test]
    async fn test_create_and_cancel_task() {
        let (db, _dir) = temp_db();
        let builder = TaskBuilder::label_import(LabelImportConfig::default());
        let id = db.create_task(&builder).await.unwrap();

        let task = db.get_task(id).await.unwrap().unwrap();
        assert_eq!(task.status, "pending");

        let cancelled = db.cancel_task(id).await.unwrap();
        assert!(cancelled);

        let task = db.get_task(id).await.unwrap().unwrap();
        assert_eq!(task.status, "cancelled");
    }

    #[tokio::test]
    async fn test_execute_create_command() {
        let (db, _dir) = temp_db();
        let cmd_id = Uuid::new_v4();
        let result = db
            .execute_create_command(cmd_id, "label_import", &serde_json::json!({}))
            .await;
        assert!(result.success);
        assert!(result.task_id.is_some());

        // Verify task was created
        let task = db.get_task(result.task_id.unwrap()).await.unwrap();
        assert!(task.is_some());
        assert_eq!(task.unwrap().task_type, "label_import");
    }

    #[tokio::test]
    async fn test_execute_cancel_command() {
        let (db, _dir) = temp_db();
        let builder = TaskBuilder::label_import(LabelImportConfig::default());
        let task_id = db.create_task(&builder).await.unwrap();

        let cmd_id = Uuid::new_v4();
        let result = db.execute_cancel_command(cmd_id, task_id).await;
        assert!(result.success);

        let task = db.get_task(task_id).await.unwrap().unwrap();
        assert_eq!(task.status, "cancelled");
    }

    #[tokio::test]
    async fn test_execute_cancel_nonexistent() {
        let (db, _dir) = temp_db();
        let cmd_id = Uuid::new_v4();
        let result = db.execute_cancel_command(cmd_id, Uuid::new_v4()).await;
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn test_execute_create_unknown_type() {
        let (db, _dir) = temp_db();
        let cmd_id = Uuid::new_v4();
        let result = db
            .execute_create_command(cmd_id, "nonexistent", &serde_json::json!({}))
            .await;
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Unknown task type"));
    }

    #[tokio::test]
    async fn test_process_redis_commands_no_redis() {
        let (db, _dir) = temp_db();
        // With no Redis connection, should return 0 and not panic
        let processed = db.process_redis_commands().await;
        assert_eq!(processed, 0);
    }

    #[tokio::test]
    async fn test_execute_delete_command() {
        let (db, _dir) = temp_db();
        let builder = TaskBuilder::label_import(LabelImportConfig::default());
        let task_id = db.create_task(&builder).await.unwrap();

        // Cancel first (delete only works on completed/failed/cancelled)
        db.cancel_task(task_id).await.unwrap();

        let cmd_id = Uuid::new_v4();
        let result = db.execute_delete_command(cmd_id, task_id).await;
        assert!(result.success);

        // Verify task is gone
        let task = db.get_task(task_id).await.unwrap();
        assert!(task.is_none());
    }
}
