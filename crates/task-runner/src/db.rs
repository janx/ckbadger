use anyhow::Result;
use ckbadger_common::Task;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct DbPool;

#[derive(Clone)]
pub struct TaskDb {
    _pool: DbPool,
}

impl TaskDb {
    pub fn new(pool: DbPool) -> Self {
        Self { _pool: pool }
    }

    pub async fn is_bulk_sync_active(&self) -> Result<bool> {
        Ok(false)
    }

    pub async fn mark_task_pending(&self, _task_id: Uuid, _reason: &str) -> Result<()> {
        Ok(())
    }

    pub async fn get_task(&self, _task_id: Uuid) -> Result<Option<Task>> {
        Ok(None)
    }

    pub async fn update_task_progress(
        &self,
        _task_id: Uuid,
        _current: i64,
        _total: i64,
    ) -> Result<()> {
        Ok(())
    }

    pub async fn update_task_status(
        &self,
        _task_id: Uuid,
        _status: &str,
        _message: Option<&str>,
    ) -> Result<()> {
        Ok(())
    }
}
