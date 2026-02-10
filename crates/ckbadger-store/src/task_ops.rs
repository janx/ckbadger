//! Task system operations.

use chrono::Utc;
use uuid::Uuid;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::TaskEntry;

/// Status byte encoding for task index keys
pub mod task_status_byte {
    pub const PENDING: u8 = 0x01;
    pub const RUNNING: u8 = 0x02;
    pub const COMPLETED: u8 = 0x03;
    pub const FAILED: u8 = 0x04;
    pub const CANCELLED: u8 = 0x05;
    pub const PAUSED: u8 = 0x06;
}

fn status_to_byte(status: &str) -> u8 {
    match status {
        "pending" => task_status_byte::PENDING,
        "running" => task_status_byte::RUNNING,
        "completed" => task_status_byte::COMPLETED,
        "failed" => task_status_byte::FAILED,
        "cancelled" => task_status_byte::CANCELLED,
        "paused" => task_status_byte::PAUSED,
        _ => 0xFF,
    }
}

impl CkbadgerStore {
    pub fn get_task(&self, id: &Uuid) -> anyhow::Result<Option<TaskEntry>> {
        match self.get_cf(self.cf_tasks(), id.as_bytes())? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn create_task(&self, entry: &TaskEntry) -> anyhow::Result<()> {
        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_tasks(), entry.id.as_bytes(), &value)?;

        // Add index entry
        let idx_key =
            keys::encode_task_index_key(status_to_byte(&entry.status), entry.priority, &entry.id);
        self.put_cf(self.cf_tasks_index(), &idx_key, &[])?;
        Ok(())
    }

    pub fn update_task(
        &self,
        entry: &TaskEntry,
        old_status: &str,
        old_priority: i32,
    ) -> anyhow::Result<()> {
        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_tasks(), entry.id.as_bytes(), &value)?;

        // Remove old index entry
        let old_idx_key =
            keys::encode_task_index_key(status_to_byte(old_status), old_priority, &entry.id);
        self.delete_cf(self.cf_tasks_index(), &old_idx_key)?;

        // Add new index entry
        let new_idx_key =
            keys::encode_task_index_key(status_to_byte(&entry.status), entry.priority, &entry.id);
        self.put_cf(self.cf_tasks_index(), &new_idx_key, &[])?;
        Ok(())
    }

    /// Claim the next pending task with highest priority.
    pub fn claim_next_task(&self, runner_id: &str) -> anyhow::Result<Option<TaskEntry>> {
        let prefix = [task_status_byte::PENDING];
        let iter = self.prefix_iterator_cf(self.cf_tasks_index(), &prefix);

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            // Key: status(1) + priority_desc(2) + id(16) = 19
            if key.len() == 19 {
                let id = Uuid::from_slice(&key[3..19])?;
                if let Some(mut task) = self.get_task(&id)? {
                    let old_status = task.status.clone();
                    let old_priority = task.priority;
                    task.status = "running".to_string();
                    task.runner_id = Some(runner_id.to_string());
                    task.started_at = Some(Utc::now());
                    task.heartbeat_at = Some(Utc::now());
                    self.update_task(&task, &old_status, old_priority)?;
                    return Ok(Some(task));
                }
            }
        }
        Ok(None)
    }

    /// Complete a task.
    pub fn complete_task(
        &self,
        id: &Uuid,
        result: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        if let Some(mut task) = self.get_task(id)? {
            let old_status = task.status.clone();
            let old_priority = task.priority;
            task.status = "completed".to_string();
            task.completed_at = Some(Utc::now());
            task.result = result.map(|v| serde_json::to_string(&v).unwrap_or_default());
            self.update_task(&task, &old_status, old_priority)?;
        }
        Ok(())
    }

    /// Fail a task.
    pub fn fail_task(&self, id: &Uuid, error: &str) -> anyhow::Result<()> {
        if let Some(mut task) = self.get_task(id)? {
            let old_status = task.status.clone();
            let old_priority = task.priority;
            task.status = "failed".to_string();
            task.completed_at = Some(Utc::now());
            task.error_message = Some(error.to_string());
            self.update_task(&task, &old_status, old_priority)?;
        }
        Ok(())
    }

    /// List tasks by status.
    pub fn list_tasks_by_status(&self, status: &str) -> anyhow::Result<Vec<TaskEntry>> {
        let prefix = [status_to_byte(status)];
        let iter = self.prefix_iterator_cf(self.cf_tasks_index(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() == 19 {
                let id = Uuid::from_slice(&key[3..19])?;
                if let Some(task) = self.get_task(&id)? {
                    results.push(task);
                }
            }
        }
        Ok(results)
    }

    /// List all tasks.
    pub fn list_all_tasks(&self) -> anyhow::Result<Vec<TaskEntry>> {
        let iter = self.iterator_cf(self.cf_tasks(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (_, value) = item;
            if let Ok(task) = bincode::deserialize::<TaskEntry>(&value) {
                results.push(task);
            }
        }
        Ok(results)
    }

    /// Update task heartbeat.
    pub fn heartbeat_task(&self, id: &Uuid) -> anyhow::Result<()> {
        if let Some(mut task) = self.get_task(id)? {
            task.heartbeat_at = Some(Utc::now());
            let value = bincode::serialize(&task)?;
            self.put_cf(self.cf_tasks(), id.as_bytes(), &value)?;
        }
        Ok(())
    }

    /// Update task progress.
    pub fn update_task_progress(
        &self,
        id: &Uuid,
        current: i64,
        total: i64,
        message: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(mut task) = self.get_task(id)? {
            task.progress_current = Some(current);
            task.progress_total = Some(total);
            task.progress_message = message.map(String::from);
            task.heartbeat_at = Some(Utc::now());
            let value = bincode::serialize(&task)?;
            self.put_cf(self.cf_tasks(), id.as_bytes(), &value)?;
        }
        Ok(())
    }

    /// Recover orphaned running tasks.
    pub fn recover_orphaned_tasks(&self, timeout_secs: i64) -> anyhow::Result<u64> {
        let cutoff = Utc::now().timestamp() - timeout_secs;
        let mut recovered = 0u64;

        let running_tasks = self.list_tasks_by_status("running")?;
        for task in running_tasks {
            if let Some(heartbeat) = task.heartbeat_at {
                if heartbeat.timestamp() < cutoff {
                    let old_status = task.status.clone();
                    let old_priority = task.priority;
                    let mut task = task;
                    task.status = "pending".to_string();
                    task.runner_id = None;
                    let err_msg = task.error_message.unwrap_or_default();
                    task.error_message = Some(format!(
                        "{}{}Recovered: runner died (stale heartbeat)",
                        err_msg,
                        if err_msg.is_empty() { "" } else { "\n" }
                    ));
                    self.update_task(&task, &old_status, old_priority)?;
                    recovered += 1;
                }
            }
        }
        Ok(recovered)
    }

    /// Check if a task type is running.
    pub fn is_task_type_running(&self, task_type: &str) -> anyhow::Result<bool> {
        let running_tasks = self.list_tasks_by_status("running")?;
        Ok(running_tasks.iter().any(|t| t.task_type == task_type))
    }
}
