//! Sync status operations.

use anyhow::anyhow;

use crate::keys::sync_meta_keys;
use crate::store::CkbadgerStore;
use crate::types::{BulkBuildSessionMarker, DeepForkInfo, ReorgEvent, RuntimeStatus, SyncStatus};

impl CkbadgerStore {
    pub fn get_bulk_build_session_marker(&self) -> anyhow::Result<Option<BulkBuildSessionMarker>> {
        match self.get_cf(
            self.cf_sync_meta(),
            sync_meta_keys::BULK_BUILD_SESSION_IN_PROGRESS,
        )? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn set_bulk_build_session_marker(
        &self,
        marker: Option<&BulkBuildSessionMarker>,
    ) -> anyhow::Result<()> {
        match marker {
            Some(marker) => {
                let value = bincode::serialize(marker)?;
                self.put_cf(
                    self.cf_sync_meta(),
                    sync_meta_keys::BULK_BUILD_SESSION_IN_PROGRESS,
                    &value,
                )
            }
            None => self.clear_bulk_build_session_marker(),
        }
    }

    pub fn clear_bulk_build_session_marker(&self) -> anyhow::Result<()> {
        self.delete_cf(
            self.cf_sync_meta(),
            sync_meta_keys::BULK_BUILD_SESSION_IN_PROGRESS,
        )
    }

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
            Some(value) => Ok(bincode::deserialize::<RuntimeStatus>(&value)?),
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
            status.last_heartbeat_target_block = tip_block;
            status.last_heartbeat_stage = Some("run_start".to_string());
            status.last_heartbeat_oom_events = None;
            status.last_heartbeat_oom_kill_events = None;
            status.last_shutdown_reason = None;
            status.last_exit_code = None;
            status.last_shutdown_at = 0;
        })
    }

    pub fn mark_runtime_heartbeat(&self, run_id: &str, current_block: i64) -> anyhow::Result<()> {
        self.mark_runtime_heartbeat_with_diag(
            run_id,
            current_block,
            current_block,
            None,
            None,
            None,
        )
    }

    pub fn mark_runtime_heartbeat_with_diag(
        &self,
        run_id: &str,
        current_block: i64,
        target_block: i64,
        stage: Option<&str>,
        oom_events: Option<u64>,
        oom_kill_events: Option<u64>,
    ) -> anyhow::Result<()> {
        let now = chrono::Utc::now().timestamp();
        let mut status = self.get_runtime_status()?;
        match status.active_run_id.as_deref() {
            Some(active_run) if active_run == run_id => {
                status.last_heartbeat_at = now;
                status.last_heartbeat_block = current_block;
                status.last_heartbeat_target_block = target_block;
                status.last_heartbeat_stage = stage.map(ToOwned::to_owned);
                status.last_heartbeat_oom_events = oom_events;
                status.last_heartbeat_oom_kill_events = oom_kill_events;
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

    pub fn rollback_sync_status_tip_and_totals(
        &self,
        tip_block_number: i64,
        tip_block_hash: &[u8],
        txs_removed: i64,
        cells_created_removed: i64,
        cells_consumed_removed: i64,
    ) -> anyhow::Result<()> {
        let mut status = self.get_sync_status()?;
        status.tip_block_number = tip_block_number;
        status.tip_block_hash = tip_block_hash.to_vec();
        status.total_transactions = checked_rollback_total(
            "total_transactions",
            status.total_transactions,
            txs_removed,
            tip_block_number,
        )?;
        status.total_cells_created = checked_rollback_total(
            "total_cells_created",
            status.total_cells_created,
            cells_created_removed,
            tip_block_number,
        )?;
        status.total_cells_consumed = checked_rollback_total(
            "total_cells_consumed",
            status.total_cells_consumed,
            cells_consumed_removed,
            tip_block_number,
        )?;
        status.last_synced_at = chrono::Utc::now().timestamp();
        self.set_sync_status(&status)
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

    /// Store sync progress data (JSON serialized, ephemeral monitoring).
    pub fn put_sync_progress(&self, data: &[u8]) -> anyhow::Result<()> {
        self.put_cf(self.cf_sync_meta(), sync_meta_keys::SYNC_PROGRESS, data)
    }

    /// Get sync progress data (JSON bytes).
    pub fn get_sync_progress(&self) -> anyhow::Result<Option<Vec<u8>>> {
        self.get_cf(self.cf_sync_meta(), sync_meta_keys::SYNC_PROGRESS)
    }

    /// Read the chain-network tag persisted at first sync, if any.
    pub fn get_network_identity(&self) -> anyhow::Result<Option<String>> {
        match self.get_cf(self.cf_sync_meta(), sync_meta_keys::NETWORK_IDENTITY)? {
            Some(bytes) => Ok(Some(String::from_utf8(bytes).map_err(|e| {
                anyhow!("network_identity record is not valid UTF-8: {e}")
            })?)),
            None => Ok(None),
        }
    }

    /// Persist the chain-network tag (idempotent overwrite at storage layer).
    pub fn set_network_identity(&self, network: &str) -> anyhow::Result<()> {
        self.put_cf(
            self.cf_sync_meta(),
            sync_meta_keys::NETWORK_IDENTITY,
            network.as_bytes(),
        )
    }

    /// Store memory stats data (JSON serialized, ephemeral monitoring).
    pub fn put_memory_stats(&self, data: &[u8]) -> anyhow::Result<()> {
        self.put_cf(self.cf_sync_meta(), sync_meta_keys::MEMORY_STATS, data)
    }

    /// Get memory stats data (JSON bytes).
    pub fn get_memory_stats(&self) -> anyhow::Result<Option<Vec<u8>>> {
        self.get_cf(self.cf_sync_meta(), sync_meta_keys::MEMORY_STATS)
    }

    pub fn get_latest_reorg_event(&self) -> anyhow::Result<Option<ReorgEvent>> {
        if let Some(value) = self.get_cf(self.cf_sync_meta(), sync_meta_keys::REORG_LATEST_EVENT)? {
            let event: ReorgEvent = bincode::deserialize(&value).map_err(|e| {
                anyhow!(
                    "failed to deserialize latest reorg event marker in sync_meta: key={}, error={}",
                    std::str::from_utf8(sync_meta_keys::REORG_LATEST_EVENT).unwrap_or("reorg_latest_event"),
                    e
                )
            })?;
            return Ok(Some(event));
        }

        Ok(None)
    }
}

pub(crate) fn checked_rollback_total(
    field_name: &str,
    current_total: i64,
    removed_total: i64,
    tip_block_number: i64,
) -> anyhow::Result<i64> {
    if current_total < 0 {
        return Err(anyhow!(
            "invalid negative sync_status total before rollback: field={} current_total={} tip_block_number={}",
            field_name,
            current_total,
            tip_block_number
        ));
    }
    if removed_total < 0 {
        return Err(anyhow!(
            "invalid negative rollback delta for sync_status: field={} removed_total={} tip_block_number={}",
            field_name,
            removed_total,
            tip_block_number
        ));
    }
    if removed_total > current_total {
        return Err(anyhow!(
            "rollback would underflow sync_status total: field={} current_total={} removed_total={} tip_block_number={}",
            field_name,
            current_total,
            removed_total,
            tip_block_number
        ));
    }
    Ok(current_total - removed_total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::sync_meta_keys;

    #[test]
    fn test_runtime_status_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let initial = store.get_runtime_status().unwrap();
        assert!(initial.active_run_id.is_none());

        store.mark_runtime_run_start("run-1", 120).unwrap();
        let running = store.get_runtime_status().unwrap();
        assert_eq!(running.active_run_id.as_deref(), Some("run-1"));
        assert_eq!(running.last_run_id.as_deref(), Some("run-1"));
        assert_eq!(running.last_heartbeat_block, 120);
        assert_eq!(running.last_heartbeat_target_block, 120);
        assert_eq!(running.last_heartbeat_stage.as_deref(), Some("run_start"));
        assert!(running.run_started_at > 0);

        store
            .mark_runtime_heartbeat_with_diag(
                "run-1",
                130,
                180,
                Some("bulk_sync"),
                Some(11),
                Some(2),
            )
            .unwrap();
        let heartbeat = store.get_runtime_status().unwrap();
        assert_eq!(heartbeat.active_run_id.as_deref(), Some("run-1"));
        assert_eq!(heartbeat.last_heartbeat_block, 130);
        assert_eq!(heartbeat.last_heartbeat_target_block, 180);
        assert_eq!(heartbeat.last_heartbeat_stage.as_deref(), Some("bulk_sync"));
        assert_eq!(heartbeat.last_heartbeat_oom_events, Some(11));
        assert_eq!(heartbeat.last_heartbeat_oom_kill_events, Some(2));

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
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

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
    fn test_runtime_status_deserialize_invalid_payload_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        store
            .put_cf(
                store.cf_sync_meta(),
                sync_meta_keys::RUNTIME_STATUS,
                b"invalid-payload",
            )
            .unwrap();

        let err = store.get_runtime_status().unwrap_err();
        assert!(!err.to_string().is_empty(), "unexpected empty error");
    }

    #[test]
    fn test_rollback_cleanup_in_progress_marker_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        assert!(!store.is_rollback_cleanup_in_progress().unwrap());

        store.set_rollback_cleanup_in_progress(true).unwrap();
        assert!(store.is_rollback_cleanup_in_progress().unwrap());

        store.set_rollback_cleanup_in_progress(false).unwrap();
        assert!(!store.is_rollback_cleanup_in_progress().unwrap());
    }

    #[test]
    fn test_bulk_build_session_marker_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        assert!(store.get_bulk_build_session_marker().unwrap().is_none());

        let session = crate::types::BulkBuildSessionMarker {
            run_id: "run-bulk-1".to_string(),
            started_at: 1_710_000_000,
            start_block: 0,
        };
        store.set_bulk_build_session_marker(Some(&session)).unwrap();

        let restored = store
            .get_bulk_build_session_marker()
            .unwrap()
            .expect("bulk build session marker");
        assert_eq!(restored, session);

        store.clear_bulk_build_session_marker().unwrap();
        assert!(store.get_bulk_build_session_marker().unwrap().is_none());
    }

    #[test]
    fn test_get_latest_reorg_event_returns_none_without_latest_marker() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let latest = store.get_latest_reorg_event().unwrap();
        assert!(latest.is_none());
    }

    #[test]
    fn test_get_latest_reorg_event_uses_latest_marker_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let latest_event = ReorgEvent {
            detected_at: 999,
            rollback_from: 21,
            rollback_to: 20,
            depth: 1,
        };

        // Legacy-style keys no longer participate in reads.
        store
            .put_cf(
                store.cf_sync_meta(),
                b"reorg:bad-ts:1",
                &bincode::serialize(&latest_event).unwrap(),
            )
            .unwrap();
        store
            .put_cf(
                store.cf_sync_meta(),
                sync_meta_keys::REORG_LATEST_EVENT,
                &bincode::serialize(&latest_event).unwrap(),
            )
            .unwrap();

        let latest = store.get_latest_reorg_event().unwrap().unwrap();
        assert_eq!(latest.detected_at, 999);
        assert_eq!(latest.rollback_from, 21);
    }

    #[test]
    fn network_identity_roundtrip_and_absent_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        // Absent before first write.
        assert_eq!(store.get_network_identity().unwrap(), None);

        store.set_network_identity("testnet").unwrap();
        assert_eq!(
            store.get_network_identity().unwrap(),
            Some("testnet".to_string())
        );

        // Overwrite is allowed at the storage layer (policy enforced above it).
        store.set_network_identity("mainnet").unwrap();
        assert_eq!(
            store.get_network_identity().unwrap(),
            Some("mainnet".to_string())
        );
    }

    #[test]
    fn network_identity_non_utf8_fails_fast() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        // Write a raw non-UTF-8 payload directly under the identity key.
        store
            .put_cf(
                store.cf_sync_meta(),
                sync_meta_keys::NETWORK_IDENTITY,
                &[0xff, 0xfe],
            )
            .unwrap();

        let err = store.get_network_identity().unwrap_err();
        assert!(
            err.to_string()
                .contains("network_identity record is not valid UTF-8"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_get_latest_reorg_event_fails_on_malformed_latest_marker() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        store
            .put_cf(
                store.cf_sync_meta(),
                sync_meta_keys::REORG_LATEST_EVENT,
                b"invalid-payload",
            )
            .unwrap();

        let err = store.get_latest_reorg_event().unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to deserialize latest reorg event marker"),
            "unexpected error: {err}"
        );
    }
}
