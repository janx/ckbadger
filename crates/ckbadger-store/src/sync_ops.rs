//! Sync status operations.

use anyhow::anyhow;
use serde::Deserialize;

use crate::keys::sync_meta_keys;
use crate::store::CkbadgerStore;
use crate::types::{DeepForkInfo, RuntimeStatus, SyncStatus};

#[derive(Debug, Clone, Deserialize)]
struct LegacyRuntimeStatusV1 {
    pub active_run_id: Option<String>,
    pub last_run_id: Option<String>,
    pub run_started_at: i64,
    pub last_heartbeat_at: i64,
    pub last_heartbeat_block: i64,
    pub last_shutdown_reason: Option<String>,
    pub last_exit_code: Option<i32>,
    pub last_incident_id: Option<String>,
    pub last_incident_at: i64,
    pub last_incident_summary: Option<String>,
}

impl From<LegacyRuntimeStatusV1> for RuntimeStatus {
    fn from(value: LegacyRuntimeStatusV1) -> Self {
        Self {
            active_run_id: value.active_run_id,
            last_run_id: value.last_run_id,
            run_started_at: value.run_started_at,
            last_heartbeat_at: value.last_heartbeat_at,
            last_heartbeat_block: value.last_heartbeat_block,
            last_shutdown_reason: value.last_shutdown_reason,
            last_exit_code: value.last_exit_code,
            last_shutdown_at: 0,
            last_incident_id: value.last_incident_id,
            last_incident_at: value.last_incident_at,
            last_incident_summary: value.last_incident_summary,
        }
    }
}

impl CkbadgerStore {
    pub fn get_sync_status(&self) -> anyhow::Result<SyncStatus> {
        match self.get_cf(self.cf_sync_meta(), sync_meta_keys::SYNC_STATUS)? {
            Some(value) => Ok(bincode::deserialize(&value)?),
            None => Ok(SyncStatus::default()),
        }
    }

    pub fn set_sync_status(&self, status: &SyncStatus) -> anyhow::Result<()> {
        let value = bincode::serialize(status)?;
        self.put_cf(self.cf_sync_meta(), sync_meta_keys::SYNC_STATUS, &value)
    }

    pub fn update_sync_status<F>(&self, update_fn: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut SyncStatus),
    {
        let mut status = self.get_sync_status()?;
        update_fn(&mut status);
        self.set_sync_status(&status)
    }

    pub fn get_runtime_status(&self) -> anyhow::Result<RuntimeStatus> {
        match self.get_cf(self.cf_sync_meta(), sync_meta_keys::RUNTIME_STATUS)? {
            Some(value) => match bincode::deserialize::<RuntimeStatus>(&value) {
                Ok(status) => Ok(status),
                Err(primary_err) => match bincode::deserialize::<LegacyRuntimeStatusV1>(&value) {
                    Ok(legacy) => Ok(legacy.into()),
                    Err(legacy_err) => Err(anyhow!(
                        "failed to deserialize runtime_status as current or legacy format: current={:#} legacy={:#}",
                        primary_err,
                        legacy_err
                    )),
                },
            },
            None => Ok(RuntimeStatus::default()),
        }
    }

    pub fn set_runtime_status(&self, status: &RuntimeStatus) -> anyhow::Result<()> {
        let value = bincode::serialize(status)?;
        self.put_cf(self.cf_sync_meta(), sync_meta_keys::RUNTIME_STATUS, &value)
    }

    pub fn update_runtime_status<F>(&self, update_fn: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut RuntimeStatus),
    {
        let mut status = self.get_runtime_status()?;
        update_fn(&mut status);
        self.set_runtime_status(&status)
    }

    pub fn mark_runtime_run_start(&self, run_id: &str, tip_block: i64) -> anyhow::Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.update_runtime_status(|status| {
            status.active_run_id = Some(run_id.to_string());
            status.last_run_id = Some(run_id.to_string());
            status.run_started_at = now;
            status.last_heartbeat_at = now;
            status.last_heartbeat_block = tip_block;
            status.last_shutdown_reason = None;
            status.last_exit_code = None;
            status.last_shutdown_at = 0;
        })
    }

    pub fn mark_runtime_heartbeat(&self, run_id: &str, current_block: i64) -> anyhow::Result<()> {
        let now = chrono::Utc::now().timestamp();
        let mut status = self.get_runtime_status()?;
        match status.active_run_id.as_deref() {
            Some(active_run) if active_run == run_id => {
                status.last_heartbeat_at = now;
                status.last_heartbeat_block = current_block;
                self.set_runtime_status(&status)
            }
            Some(active_run) => Err(anyhow!(
                "runtime heartbeat run mismatch: run_id={} active_run_id={}",
                run_id,
                active_run
            )),
            None => {
                let shutdown_recorded_for_same_run = status.last_run_id.as_deref() == Some(run_id)
                    && status.last_shutdown_reason.is_some()
                    && status.last_exit_code.is_some()
                    && status.last_shutdown_at >= status.run_started_at;
                if shutdown_recorded_for_same_run {
                    return Ok(());
                }
                Err(anyhow!(
                    "runtime heartbeat without active run marker: run_id={} last_run_id={:?} shutdown_reason={:?} last_exit_code={:?}",
                    run_id,
                    status.last_run_id,
                    status.last_shutdown_reason,
                    status.last_exit_code
                ))
            }
        }
    }

    pub fn mark_runtime_shutdown(
        &self,
        run_id: &str,
        reason: &str,
        exit_code: i32,
    ) -> anyhow::Result<()> {
        let now = chrono::Utc::now().timestamp();
        let mut status = self.get_runtime_status()?;
        let can_shutdown = match status.active_run_id.as_deref() {
            Some(active_run) => active_run == run_id,
            None => status.last_run_id.as_deref() == Some(run_id),
        };
        if !can_shutdown {
            return Err(anyhow!(
                "runtime shutdown run mismatch: run_id={} active_run_id={:?} last_run_id={:?}",
                run_id,
                status.active_run_id,
                status.last_run_id
            ));
        }
        if status.active_run_id.as_deref() == Some(run_id) {
            status.active_run_id = None;
        }
        status.last_run_id = Some(run_id.to_string());
        status.last_shutdown_reason = Some(reason.to_string());
        status.last_exit_code = Some(exit_code);
        status.last_shutdown_at = now;
        status.last_heartbeat_at = now;
        self.set_runtime_status(&status)
    }

    pub fn mark_runtime_incident(
        &self,
        run_id: &str,
        incident_id: &str,
        summary: &str,
    ) -> anyhow::Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.update_runtime_status(|status| {
            status.last_run_id = Some(run_id.to_string());
            status.last_incident_id = Some(incident_id.to_string());
            status.last_incident_at = now;
            status.last_incident_summary = Some(summary.to_string());
        })
    }

    pub fn is_rollback_cleanup_in_progress(&self) -> anyhow::Result<bool> {
        match self.get_cf(
            self.cf_sync_meta(),
            sync_meta_keys::ROLLBACK_CLEANUP_IN_PROGRESS,
        )? {
            Some(value) => Ok(value.first().copied() == Some(1)),
            None => Ok(false),
        }
    }

    pub fn set_rollback_cleanup_in_progress(&self, in_progress: bool) -> anyhow::Result<()> {
        if in_progress {
            self.put_cf(
                self.cf_sync_meta(),
                sync_meta_keys::ROLLBACK_CLEANUP_IN_PROGRESS,
                &[1u8],
            )
        } else {
            self.delete_cf(
                self.cf_sync_meta(),
                sync_meta_keys::ROLLBACK_CLEANUP_IN_PROGRESS,
            )
        }
    }

    /// Get sync tip (block number and hash) from the sync_status.
    pub fn get_sync_tip(&self) -> anyhow::Result<(i64, Option<Vec<u8>>)> {
        let status = self.get_sync_status()?;
        let hash = if status.tip_block_hash.is_empty() {
            None
        } else {
            Some(status.tip_block_hash)
        };
        Ok((status.tip_block_number, hash))
    }

    /// Update sync tip.
    pub fn update_sync_tip(
        &self,
        block_number: i64,
        block_hash: &[u8],
        tx_count_delta: i64,
    ) -> anyhow::Result<()> {
        self.update_sync_status(|status| {
            status.tip_block_number = block_number;
            status.tip_block_hash = block_hash.to_vec();
            status.total_transactions += tx_count_delta;
            status.last_synced_at = chrono::Utc::now().timestamp();
        })
    }

    /// Check if there's an unresolved deep fork.
    pub fn has_unresolved_deep_fork(&self) -> anyhow::Result<bool> {
        let status = self.get_sync_status()?;
        Ok(status.deep_fork_detected)
    }

    /// Get deep fork info.
    pub fn get_deep_fork_info(&self) -> anyhow::Result<Option<DeepForkInfo>> {
        let status = self.get_sync_status()?;
        if status.deep_fork_detected {
            Ok(status.deep_fork_info)
        } else {
            Ok(None)
        }
    }

    /// Set deep fork detected.
    pub fn set_deep_fork(&self, info: DeepForkInfo) -> anyhow::Result<()> {
        self.update_sync_status(|status| {
            status.deep_fork_detected = true;
            status.deep_fork_info = Some(info);
        })
    }

    /// Clear deep fork.
    pub fn clear_deep_fork(&self) -> anyhow::Result<()> {
        self.update_sync_status(|status| {
            status.deep_fork_detected = false;
            status.deep_fork_info = None;
        })
    }

    /// Check if bulk sync is active by looking at block timestamps.
    pub fn is_bulk_sync_active_by_timestamp(&self) -> anyhow::Result<bool> {
        if let Some((_, header)) = self.get_sync_tip_block()? {
            let now = chrono::Utc::now().timestamp();
            let block_time = header.timestamp / 1000; // ms -> s
            Ok(now - block_time > 3600)
        } else {
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::sync_meta_keys;
    use serde::Serialize;

    #[derive(Debug, Clone, Serialize)]
    struct LegacyRuntimeStatusForTest {
        pub active_run_id: Option<String>,
        pub last_run_id: Option<String>,
        pub run_started_at: i64,
        pub last_heartbeat_at: i64,
        pub last_heartbeat_block: i64,
        pub last_shutdown_reason: Option<String>,
        pub last_exit_code: Option<i32>,
        pub last_incident_id: Option<String>,
        pub last_incident_at: i64,
        pub last_incident_summary: Option<String>,
    }

    #[test]
    fn test_runtime_status_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let initial = store.get_runtime_status().unwrap();
        assert!(initial.active_run_id.is_none());

        store.mark_runtime_run_start("run-1", 120).unwrap();
        let running = store.get_runtime_status().unwrap();
        assert_eq!(running.active_run_id.as_deref(), Some("run-1"));
        assert_eq!(running.last_run_id.as_deref(), Some("run-1"));
        assert_eq!(running.last_heartbeat_block, 120);
        assert!(running.run_started_at > 0);

        store.mark_runtime_heartbeat("run-1", 130).unwrap();
        let heartbeat = store.get_runtime_status().unwrap();
        assert_eq!(heartbeat.active_run_id.as_deref(), Some("run-1"));
        assert_eq!(heartbeat.last_heartbeat_block, 130);

        store
            .mark_runtime_incident("run-1", "run-1-inc-000001", "pipeline batch mismatch")
            .unwrap();
        let incident = store.get_runtime_status().unwrap();
        assert_eq!(
            incident.last_incident_id.as_deref(),
            Some("run-1-inc-000001")
        );
        assert_eq!(
            incident.last_incident_summary.as_deref(),
            Some("pipeline batch mismatch")
        );

        store
            .mark_runtime_shutdown("run-1", "graceful_shutdown", 0)
            .unwrap();
        let shutdown = store.get_runtime_status().unwrap();
        assert!(shutdown.active_run_id.is_none());
        assert_eq!(shutdown.last_run_id.as_deref(), Some("run-1"));
        assert_eq!(
            shutdown.last_shutdown_reason.as_deref(),
            Some("graceful_shutdown")
        );
        assert_eq!(shutdown.last_exit_code, Some(0));
        assert!(shutdown.last_shutdown_at > 0);
    }

    #[test]
    fn test_runtime_heartbeat_noop_after_shutdown_marker() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        store.mark_runtime_run_start("run-1", 120).unwrap();
        store
            .mark_runtime_shutdown("run-1", "sigterm_shutdown", 0)
            .unwrap();
        let before = store.get_runtime_status().unwrap();

        store.mark_runtime_heartbeat("run-1", 130).unwrap();
        let after = store.get_runtime_status().unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn test_runtime_heartbeat_fails_on_active_run_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        store.mark_runtime_run_start("run-2", 120).unwrap();
        let err = store.mark_runtime_heartbeat("run-1", 130).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("runtime heartbeat run mismatch"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_runtime_shutdown_fails_on_run_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        store.mark_runtime_run_start("run-2", 120).unwrap();
        let err = store
            .mark_runtime_shutdown("run-1", "sigterm_shutdown", 0)
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("runtime shutdown run mismatch"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_runtime_status_deserialize_legacy_v1() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let legacy = LegacyRuntimeStatusForTest {
            active_run_id: Some("run-legacy".to_string()),
            last_run_id: Some("run-legacy".to_string()),
            run_started_at: 10,
            last_heartbeat_at: 20,
            last_heartbeat_block: 30,
            last_shutdown_reason: Some("sigterm_shutdown".to_string()),
            last_exit_code: Some(0),
            last_incident_id: Some("inc-1".to_string()),
            last_incident_at: 40,
            last_incident_summary: Some("legacy".to_string()),
        };
        let bytes = bincode::serialize(&legacy).unwrap();
        store
            .put_cf(store.cf_sync_meta(), sync_meta_keys::RUNTIME_STATUS, &bytes)
            .unwrap();

        let status = store.get_runtime_status().unwrap();
        assert_eq!(status.active_run_id.as_deref(), Some("run-legacy"));
        assert_eq!(status.last_run_id.as_deref(), Some("run-legacy"));
        assert_eq!(status.run_started_at, 10);
        assert_eq!(status.last_heartbeat_at, 20);
        assert_eq!(status.last_heartbeat_block, 30);
        assert_eq!(
            status.last_shutdown_reason.as_deref(),
            Some("sigterm_shutdown")
        );
        assert_eq!(status.last_exit_code, Some(0));
        assert_eq!(status.last_shutdown_at, 0);
        assert_eq!(status.last_incident_id.as_deref(), Some("inc-1"));
        assert_eq!(status.last_incident_at, 40);
        assert_eq!(status.last_incident_summary.as_deref(), Some("legacy"));
    }

    #[test]
    fn test_rollback_cleanup_in_progress_marker_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        assert!(!store.is_rollback_cleanup_in_progress().unwrap());

        store.set_rollback_cleanup_in_progress(true).unwrap();
        assert!(store.is_rollback_cleanup_in_progress().unwrap());

        store.set_rollback_cleanup_in_progress(false).unwrap();
        assert!(!store.is_rollback_cleanup_in_progress().unwrap());
    }
}
