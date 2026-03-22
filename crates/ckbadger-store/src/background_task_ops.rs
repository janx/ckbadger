//! Background task status operations.

use ckbadger_common::{BackgroundTaskEntry, BackgroundTaskState, BackgroundTasksData};

use crate::keys::sync_meta_keys;
use crate::store::CkbadgerStore;

impl CkbadgerStore {
    /// Read current background tasks state from domain store.
    pub fn get_background_tasks(&self) -> anyhow::Result<BackgroundTasksData> {
        match self.get_cf(self.cf_sync_meta(), sync_meta_keys::BACKGROUND_TASKS)? {
            Some(value) => Ok(bincode::deserialize(&value)?),
            None => Ok(BackgroundTasksData::default()),
        }
    }

    /// Write background tasks state (full replace).
    pub fn set_background_tasks(&self, data: &BackgroundTasksData) -> anyhow::Result<()> {
        let value = bincode::serialize(data)?;
        self.put_cf(
            self.cf_sync_meta(),
            sync_meta_keys::BACKGROUND_TASKS,
            &value,
        )
    }

    /// Update a single task entry by name, inserting if absent.
    /// Each task name has a single writer — no concurrent updates on the same name.
    pub fn update_background_task<F>(&self, task_name: &str, update_fn: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut BackgroundTaskEntry),
    {
        let mut data = self.get_background_tasks()?;
        let entry = match data.tasks.iter_mut().find(|t| t.name == task_name) {
            Some(existing) => existing,
            None => {
                data.tasks.push(BackgroundTaskEntry {
                    name: task_name.to_string(),
                    state: BackgroundTaskState::Waiting,
                    message: None,
                    progress_current: None,
                    progress_total: None,
                    rate: None,
                    eta_seconds: None,
                    started_at: None,
                    elapsed_ms: None,
                    error: None,
                });
                data.tasks.last_mut().unwrap()
            }
        };
        update_fn(entry);
        data.updated_at = chrono::Utc::now().timestamp();
        self.set_background_tasks(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_background_tasks_empty_store_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let data = store.get_background_tasks().unwrap();
        assert!(data.tasks.is_empty());
        assert_eq!(data.updated_at, 0);
    }

    #[test]
    fn test_background_tasks_set_and_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let data = BackgroundTasksData {
            tasks: vec![BackgroundTaskEntry {
                name: "test_task".to_string(),
                state: BackgroundTaskState::Running,
                message: Some("hello".to_string()),
                progress_current: Some(10),
                progress_total: Some(100),
                rate: Some(5.0),
                eta_seconds: Some(18.0),
                started_at: Some(1711100000),
                elapsed_ms: Some(2000.0),
                error: None,
            }],
            updated_at: 1711100000,
        };
        store.set_background_tasks(&data).unwrap();

        let restored = store.get_background_tasks().unwrap();
        assert_eq!(restored.tasks.len(), 1);
        assert_eq!(restored.tasks[0].name, "test_task");
        assert_eq!(restored.tasks[0].state, BackgroundTaskState::Running);
        assert_eq!(restored.tasks[0].progress_current, Some(10));
    }

    #[test]
    fn test_update_background_task_inserts_new() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        store
            .update_background_task("dob_decode", |entry| {
                entry.state = BackgroundTaskState::Waiting;
                entry.message = Some("Waiting for sync".to_string());
            })
            .unwrap();

        let data = store.get_background_tasks().unwrap();
        assert_eq!(data.tasks.len(), 1);
        assert_eq!(data.tasks[0].name, "dob_decode");
        assert_eq!(data.tasks[0].state, BackgroundTaskState::Waiting);
        assert!(data.updated_at > 0);
    }

    #[test]
    fn test_update_background_task_modifies_existing() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        store
            .update_background_task("dob_decode", |entry| {
                entry.state = BackgroundTaskState::Waiting;
            })
            .unwrap();

        store
            .update_background_task("dob_decode", |entry| {
                entry.state = BackgroundTaskState::Running;
                entry.progress_current = Some(42);
                entry.progress_total = Some(500);
            })
            .unwrap();

        let data = store.get_background_tasks().unwrap();
        assert_eq!(data.tasks.len(), 1);
        assert_eq!(data.tasks[0].state, BackgroundTaskState::Running);
        assert_eq!(data.tasks[0].progress_current, Some(42));
    }

    #[test]
    fn test_update_background_task_isolates_different_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        store
            .update_background_task("dob_decode", |entry| {
                entry.state = BackgroundTaskState::Running;
                entry.progress_current = Some(10);
            })
            .unwrap();

        store
            .update_background_task("cache_warmup", |entry| {
                entry.state = BackgroundTaskState::Completed;
                entry.elapsed_ms = Some(820.0);
            })
            .unwrap();

        let data = store.get_background_tasks().unwrap();
        assert_eq!(data.tasks.len(), 2);

        let dob = data.tasks.iter().find(|t| t.name == "dob_decode").unwrap();
        assert_eq!(dob.state, BackgroundTaskState::Running);
        assert_eq!(dob.progress_current, Some(10));

        let warmup = data
            .tasks
            .iter()
            .find(|t| t.name == "cache_warmup")
            .unwrap();
        assert_eq!(warmup.state, BackgroundTaskState::Completed);
        assert_eq!(warmup.elapsed_ms, Some(820.0));
    }
}
