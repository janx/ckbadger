use anyhow::Result;
use ckbadger_common::{MemoryStatsData, SyncProgressData, SyncStatusData, Task, TaskBuilder};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SyncStatusRow {
    pub tip_block: i64,
    pub chain_tip: i64,
    pub is_syncing: bool,
    pub is_bulk_sync: bool,
    pub progress: f64,
    pub indexes_deferred: bool,
    pub elapsed_time: Option<String>,
    pub eta: Option<String>,
    pub rate_realtime: Option<f64>,
    pub rate_ema: Option<f64>,
}

#[derive(Clone, Default)]
pub struct DbPool;

pub struct TaskDb {
    _pool: DbPool,
    _redis_url: Option<String>,
}

#[allow(dead_code)]
impl TaskDb {
    pub async fn new(pool: DbPool, redis_url: Option<&str>) -> Self {
        Self {
            _pool: pool,
            _redis_url: redis_url.map(|s| s.to_string()),
        }
    }

    pub async fn get_sync_status(&self) -> Result<SyncStatusRow> {
        Ok(SyncStatusRow {
            tip_block: 0,
            chain_tip: 0,
            is_syncing: false,
            is_bulk_sync: false,
            progress: 0.0,
            indexes_deferred: false,
            elapsed_time: None,
            eta: None,
            rate_realtime: None,
            rate_ema: None,
        })
    }

    pub async fn get_sync_progress(&self) -> Option<SyncProgressData> {
        None
    }

    pub async fn get_sync_status_data(&self) -> Option<SyncStatusData> {
        None
    }

    pub async fn get_memory_stats(&self) -> Option<MemoryStatsData> {
        None
    }

    pub async fn list_tasks(&self, _limit: i64) -> Result<Vec<Task>> {
        Ok(Vec::new())
    }

    pub async fn get_task(&self, _task_id: Uuid) -> Result<Option<Task>> {
        Ok(None)
    }

    pub async fn create_task(&self, _builder: &TaskBuilder) -> Result<Uuid> {
        Ok(Uuid::nil())
    }

    pub async fn cancel_task(&self, _task_id: Uuid) -> Result<bool> {
        Ok(false)
    }

    pub async fn pause_task(&self, _task_id: Uuid) -> Result<bool> {
        Ok(false)
    }

    pub async fn resume_task(&self, _task_id: Uuid) -> Result<bool> {
        Ok(false)
    }

    pub async fn retry_task(&self, _task_id: Uuid) -> Result<bool> {
        Ok(false)
    }

    pub async fn delete_task(&self, _task_id: Uuid) -> Result<bool> {
        Ok(false)
    }
}
