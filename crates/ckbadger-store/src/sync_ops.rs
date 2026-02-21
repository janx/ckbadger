//! Sync status operations.

use crate::keys::sync_meta_keys;
use crate::store::CkbadgerStore;
use crate::types::{DeepForkInfo, RuntimeStatus, SyncStatus};

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
            Some(value) => Ok(bincode::deserialize(&value)?),
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
        })
    }

    pub fn mark_runtime_heartbeat(&self, run_id: &str, current_block: i64) -> anyhow::Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.update_runtime_status(|status| {
            if status.active_run_id.as_deref() != Some(run_id) {
                status.active_run_id = Some(run_id.to_string());
                status.last_run_id = Some(run_id.to_string());
            }
            status.last_heartbeat_at = now;
            status.last_heartbeat_block = current_block;
        })
    }

    pub fn mark_runtime_shutdown(
        &self,
        run_id: &str,
        reason: &str,
        exit_code: i32,
    ) -> anyhow::Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.update_runtime_status(|status| {
            if status.active_run_id.as_deref() == Some(run_id) {
                status.active_run_id = None;
            }
            status.last_run_id = Some(run_id.to_string());
            status.last_shutdown_reason = Some(reason.to_string());
            status.last_exit_code = Some(exit_code);
            status.last_heartbeat_at = now;
        })
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
    }
}
