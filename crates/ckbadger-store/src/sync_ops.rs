//! Sync status operations.

use anyhow::anyhow;
use rocksdb::{Direction, IteratorMode};

use crate::batch::StoreBatch;
use crate::keys::{self, sync_meta_keys};
use crate::store::CkbadgerStore;
use crate::types::{
    BulkBuildSessionMarker, DeepForkInfo, GenesisBaseline, LiveCellSummary, ReorgEvent,
    ReorgEventRecord, RuntimeStatus, SyncStatus,
};

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

    /// Read the one API-visible live-cell summary. This is a single RocksDB
    /// point lookup; no cell or index scan is involved.
    pub fn get_live_cell_summary(&self) -> anyhow::Result<Option<LiveCellSummary>> {
        let value = self.get_cf(
            self.cf_sync_meta(),
            sync_meta_keys::LIVE_CELL_SUMMARY_CURRENT,
        )?;
        let summary = value
            .map(|value| decode_live_cell_summary(&value, "current", None))
            .transpose()?;
        if summary.is_some() && !self.is_live_cell_summary_initialized()? {
            anyhow::bail!("current live-cell summary exists without initialized marker");
        }
        Ok(summary)
    }

    pub fn is_live_cell_summary_initialized(&self) -> anyhow::Result<bool> {
        match self.get_cf(
            self.cf_sync_meta(),
            sync_meta_keys::LIVE_CELL_SUMMARY_INITIALIZED,
        )? {
            None => Ok(false),
            Some(value) if value.as_slice() == [1u8] => Ok(true),
            Some(value) => Err(anyhow!(
                "invalid live-cell summary initialized marker: expected=0x01 actual=0x{}",
                crate::bytes_to_hex(&value),
            )),
        }
    }

    /// Read one block-end history snapshot used by shallow reorg rollback.
    pub fn get_live_cell_summary_at(
        &self,
        block_number: i64,
    ) -> anyhow::Result<Option<LiveCellSummary>> {
        if block_number < 0 {
            return Err(anyhow!(
                "cannot read live-cell summary history at negative block: block={}",
                block_number
            ));
        }
        let key = keys::encode_live_cell_summary_history_key(block_number);
        let value = self.get_cf(self.cf_sync_meta(), &key)?;
        value
            .map(|value| decode_live_cell_summary(&value, "history", Some(block_number)))
            .transpose()
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

    /// Restore the public live-cell summary after an interrupted rollback that
    /// committed only the visibility marker, but no canonical data beyond the
    /// persisted sync tip. The exact block-end history record is the sole
    /// recovery source; no cell scan or recalculation is performed.
    pub fn restore_live_cell_summary_visibility_after_interrupted_rollback(
        &self,
    ) -> anyhow::Result<()> {
        if !self.is_rollback_cleanup_in_progress()? {
            anyhow::bail!(
                "cannot restore live-cell summary visibility without a rollback cleanup marker"
            );
        }

        let status = self.get_sync_status()?;
        let initialized = self.is_live_cell_summary_initialized()?;
        let current = self.get_live_cell_summary()?;
        let history = if status.tip_block_hash.is_empty() {
            None
        } else {
            self.get_live_cell_summary_at(status.tip_block_number)?
        };

        if !initialized && (current.is_some() || history.is_some()) {
            anyhow::bail!(
                "live-cell summary records exist without initialized marker during interrupted rollback recovery: current={:?} history={:?}",
                current,
                history,
            );
        }

        let mut batch = StoreBatch::new(self);
        match (initialized, current, history) {
            (_, None, None) if status.tip_block_hash.is_empty() => {}
            (false, None, None) => {}
            (true, None, None) => {
                anyhow::bail!(
                    "initialized live-cell summary lost both current and tip history during interrupted rollback recovery: tip_block={}",
                    status.tip_block_number,
                );
            }
            (_, Some(_), None) => {
                anyhow::bail!(
                    "current live-cell summary exists without tip history during interrupted rollback recovery: tip_block={}",
                    status.tip_block_number,
                );
            }
            (false, _, Some(_)) => {
                anyhow::bail!(
                    "live-cell summary history exists without initialized marker during interrupted rollback recovery"
                );
            }
            (true, current, Some(history)) => {
                history.validate_against_sync_totals(
                    status.total_cells_created,
                    status.total_cells_consumed,
                )?;
                if history.tip_block_number != status.tip_block_number
                    || history.tip_block_hash.as_slice() != status.tip_block_hash.as_slice()
                {
                    anyhow::bail!(
                        "live-cell summary history/status mismatch during interrupted rollback recovery: summary_block={} summary_hash=0x{} status_block={} status_hash=0x{}",
                        history.tip_block_number,
                        hex::encode(history.tip_block_hash),
                        status.tip_block_number,
                        hex::encode(&status.tip_block_hash),
                    );
                }
                if let Some(current) = current {
                    if current != history {
                        anyhow::bail!(
                            "current/history live-cell summary mismatch during interrupted rollback recovery: current={:?} history={:?}",
                            current,
                            history,
                        );
                    }
                }
                let header = self
                    .get_block_header(status.tip_block_number)?
                    .ok_or_else(|| {
                        anyhow!(
                            "missing canonical tip header during interrupted rollback summary recovery: block={}",
                            status.tip_block_number,
                        )
                    })?;
                if header.hash.as_slice() != history.tip_block_hash.as_slice() {
                    anyhow::bail!(
                        "live-cell summary history/header mismatch during interrupted rollback recovery: block={} summary_hash=0x{} header_hash=0x{}",
                        status.tip_block_number,
                        hex::encode(history.tip_block_hash),
                        hex::encode(header.hash),
                    );
                }
                batch.put_live_cell_summary_snapshots(&[history])?;
            }
        }
        batch.delete_sync_meta(sync_meta_keys::ROLLBACK_CLEANUP_IN_PROGRESS);
        batch.commit()
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

    /// Read the genesis economic baseline, if it has been derived+persisted.
    pub fn get_genesis_baseline(&self) -> anyhow::Result<Option<GenesisBaseline>> {
        match self.get_cf(self.cf_sync_meta(), sync_meta_keys::GENESIS_BASELINE)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// Persist the genesis economic baseline (write-once at first sync).
    pub fn set_genesis_baseline(&self, baseline: &GenesisBaseline) -> anyhow::Result<()> {
        let value = bincode::serialize(baseline)?;
        self.put_cf(
            self.cf_sync_meta(),
            sync_meta_keys::GENESIS_BASELINE,
            &value,
        )
    }

    /// Read the consensus secondary issuance per epoch (shannons), persisted
    /// from the node's `get_consensus` at indexer startup.
    pub fn get_secondary_epoch_reward(&self) -> anyhow::Result<Option<u64>> {
        match self.get_cf(self.cf_sync_meta(), sync_meta_keys::SECONDARY_EPOCH_REWARD)? {
            Some(value) => {
                let bytes: [u8; 8] = value.as_slice().try_into().map_err(|_| {
                    anyhow::anyhow!(
                        "corrupt secondary_epoch_reward value in sync meta: expected 8 bytes, got {}",
                        value.len()
                    )
                })?;
                Ok(Some(u64::from_le_bytes(bytes)))
            }
            None => Ok(None),
        }
    }

    /// Persist the consensus secondary issuance per epoch (write-once at
    /// indexer startup, verified against the node on every restart).
    pub fn set_secondary_epoch_reward(&self, shannons: u64) -> anyhow::Result<()> {
        self.put_cf(
            self.cf_sync_meta(),
            sync_meta_keys::SECONDARY_EPOCH_REWARD,
            &shannons.to_le_bytes(),
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

    /// Page through the persisted reorg-event history, newest first.
    ///
    /// This is the only read path for reorg events: "the latest event" is the
    /// first row of this scan, not a separately maintained marker that could
    /// disagree with the history. `cursor_key_exclusive` is a full history key
    /// and the page continues strictly below it.
    pub fn list_reorg_events(
        &self,
        limit: usize,
        cursor_key_exclusive: Option<&[u8]>,
    ) -> anyhow::Result<Vec<ReorgEventRecord>> {
        if let Some(cursor_key) = cursor_key_exclusive {
            // Rejects a cursor that is not a well-formed history key, so a
            // bad cursor cannot silently start the page somewhere else.
            keys::decode_reorg_event_key(cursor_key)?;
        }
        let mut rows = Vec::with_capacity(limit.min(1024));
        if limit == 0 {
            return Ok(rows);
        }
        let start = cursor_key_exclusive.unwrap_or(keys::REORG_EVENT_KEY_PREFIX_END);
        let iter = self.iterator_cf(
            self.cf_sync_meta(),
            IteratorMode::From(start, Direction::Reverse),
        );

        for item in iter {
            let (key, value) = item
                .map_err(|e| anyhow!("failed to iterate sync_meta in list_reorg_events: {}", e))?;
            if !key.starts_with(keys::REORG_EVENT_KEY_PREFIX) {
                break;
            }
            if Some(key.as_ref()) == cursor_key_exclusive {
                continue;
            }
            rows.push(decode_reorg_event_record(&key, &value)?);
            if rows.len() == limit {
                break;
            }
        }
        Ok(rows)
    }

    /// Exact number of persisted reorg events.
    pub fn count_reorg_events(&self) -> anyhow::Result<i64> {
        let iter = self.iterator_cf(
            self.cf_sync_meta(),
            IteratorMode::From(keys::REORG_EVENT_KEY_PREFIX, Direction::Forward),
        );
        let mut total = 0i64;
        for item in iter {
            let (key, _) = item
                .map_err(|e| anyhow!("failed to iterate sync_meta in count_reorg_events: {}", e))?;
            if !key.starts_with(keys::REORG_EVENT_KEY_PREFIX) {
                break;
            }
            keys::decode_reorg_event_key(&key)?;
            total += 1;
        }
        Ok(total)
    }

    /// Look one reorg event up by its detection millisecond, which is the
    /// public event id.
    pub fn get_reorg_event(&self, detected_at_ms: i64) -> anyhow::Result<Option<ReorgEventRecord>> {
        let prefix = keys::reorg_event_key_ms_prefix(detected_at_ms)?;
        let iter = self.iterator_cf(
            self.cf_sync_meta(),
            IteratorMode::From(prefix.as_bytes(), Direction::Forward),
        );
        let mut found: Option<ReorgEventRecord> = None;
        for item in iter {
            let (key, value) =
                item.map_err(|e| anyhow!("failed to iterate sync_meta in get_reorg_event: {}", e))?;
            if !key.starts_with(prefix.as_bytes()) {
                break;
            }
            let record = decode_reorg_event_record(&key, &value)?;
            if let Some(previous) = &found {
                // The uuid segment exists to keep same-millisecond events
                // distinct in the history; it also means the millisecond alone
                // stops being a unique id, so report the collision instead of
                // picking one arbitrarily.
                return Err(anyhow!(
                    "ambiguous reorg event id {}: keys {} and {} share the same millisecond",
                    detected_at_ms,
                    previous.key,
                    record.key
                ));
            }
            found = Some(record);
        }
        Ok(found)
    }

    /// The most recent persisted reorg event, if any.
    pub fn get_latest_reorg_event(&self) -> anyhow::Result<Option<ReorgEventRecord>> {
        Ok(self.list_reorg_events(1, None)?.into_iter().next())
    }
}

fn decode_reorg_event_record(key: &[u8], value: &[u8]) -> anyhow::Result<ReorgEventRecord> {
    let detected_at_ms = keys::decode_reorg_event_key(key)?;
    let event: ReorgEvent = bincode::deserialize(value).map_err(|e| {
        anyhow!(
            "failed to deserialize reorg event in sync_meta: key=0x{}, error={}",
            crate::bytes_to_hex(key),
            e
        )
    })?;
    Ok(ReorgEventRecord {
        key: String::from_utf8(key.to_vec()).map_err(|e| {
            anyhow!(
                "reorg event key is not valid UTF-8: key=0x{}, error={}",
                crate::bytes_to_hex(key),
                e
            )
        })?,
        detected_at_ms,
        event,
    })
}

fn decode_live_cell_summary(
    value: &[u8],
    source: &str,
    expected_block: Option<i64>,
) -> anyhow::Result<LiveCellSummary> {
    // All fields are fixed width: i64 + [u8; 32] + 4*u64.
    const ENCODED_LEN: usize = 72;
    if value.len() != ENCODED_LEN {
        return Err(anyhow!(
            "corrupt live-cell summary value length: source={} expected_bytes={} actual_bytes={} expected_block={:?}",
            source,
            ENCODED_LEN,
            value.len(),
            expected_block,
        ));
    }
    let summary: LiveCellSummary = bincode::deserialize(value).map_err(|error| {
        anyhow!(
            "failed to deserialize live-cell summary: source={} expected_block={:?} error={}",
            source,
            expected_block,
            error,
        )
    })?;
    summary.validate()?;
    if let Some(expected_block) = expected_block {
        if summary.tip_block_number != expected_block {
            return Err(anyhow!(
                "live-cell summary history key/value block mismatch: key_block={} value_block={}",
                expected_block,
                summary.tip_block_number,
            ));
        }
    }
    Ok(summary)
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
    use crate::batch::StoreBatch;
    use crate::keys::sync_meta_keys;

    fn live_cell_summary(block: i64) -> LiveCellSummary {
        LiveCellSummary {
            tip_block_number: block,
            tip_block_hash: [u8::try_from(block).unwrap_or(0xFF); 32],
            dao: u64::try_from(block + 1).unwrap(),
            typed_non_dao: 2,
            plain: 3,
            data_bearing: 1,
        }
    }

    #[test]
    fn live_cell_summary_roundtrip_retains_exactly_reorg_window() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let summaries = (0..=50).map(live_cell_summary).collect::<Vec<_>>();

        let mut batch = StoreBatch::new(&store);
        batch.put_live_cell_summary_snapshots(&summaries).unwrap();
        batch.commit().unwrap();

        assert_eq!(
            store.get_live_cell_summary().unwrap(),
            Some(live_cell_summary(50))
        );
        assert!(store.is_live_cell_summary_initialized().unwrap());
        assert!(store.get_live_cell_summary_at(13).unwrap().is_none());
        for block in 14..=50 {
            assert_eq!(
                store.get_live_cell_summary_at(block).unwrap(),
                Some(live_cell_summary(block))
            );
        }
    }

    #[test]
    fn live_cell_summary_write_is_domain_only_and_never_targets_cells_cf() {
        let dir = tempfile::tempdir().unwrap();
        let append_store = CkbadgerStore::open_append_only(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&append_store);
        let error = batch
            .put_live_cell_summary_snapshots(&[live_cell_summary(0)])
            .unwrap_err();
        let delete_error = batch.delete_live_cell_summary_current().unwrap_err();

        assert!(error.to_string().contains("domain store"));
        assert!(delete_error.to_string().contains("domain store"));
        assert_eq!(
            crate::cf_write_policy(crate::CF_SYNC_META),
            crate::CfWritePolicy::FinalSnapshot
        );
        assert!(!crate::is_append_only_cf_name(crate::CF_SYNC_META));
        assert!(append_store
            .iterator_cf(append_store.cf_cells(), IteratorMode::Start)
            .next()
            .is_none());
    }

    #[test]
    fn live_cell_summary_read_fails_on_corrupt_fixed_width_value() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store
            .put_cf(
                store.cf_sync_meta(),
                sync_meta_keys::LIVE_CELL_SUMMARY_CURRENT,
                b"not-72-bytes",
            )
            .unwrap();

        let error = store.get_live_cell_summary().unwrap_err();
        assert!(error.to_string().contains("value length"));
    }

    #[test]
    fn live_cell_summary_read_rejects_current_without_initialized_marker() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store
            .put_cf(
                store.cf_sync_meta(),
                sync_meta_keys::LIVE_CELL_SUMMARY_CURRENT,
                &bincode::serialize(&live_cell_summary(0)).unwrap(),
            )
            .unwrap();

        let error = store.get_live_cell_summary().unwrap_err();
        assert!(error.to_string().contains("without initialized marker"));
    }

    #[test]
    fn live_cell_summary_initialized_marker_rejects_unknown_value() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store
            .put_cf(
                store.cf_sync_meta(),
                sync_meta_keys::LIVE_CELL_SUMMARY_INITIALIZED,
                &[2],
            )
            .unwrap();

        let error = store.is_live_cell_summary_initialized().unwrap_err();
        assert!(error.to_string().contains("initialized marker"));
    }

    #[test]
    fn interrupted_rollback_restores_current_summary_from_exact_tip_history() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let summary = live_cell_summary(7);
        let header = crate::types::CachedBlockHeader {
            hash: summary.tip_block_hash.to_vec(),
            parent_hash: vec![0x06; 32],
            timestamp: 1_700_000_007_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        };
        let mut seed = StoreBatch::new(&store);
        seed.put_block_header(7, &header);
        seed.put_live_cell_summary_snapshots(&[summary]).unwrap();
        seed.commit().unwrap();
        store
            .set_sync_status(&SyncStatus {
                tip_block_number: 7,
                tip_block_hash: summary.tip_block_hash.to_vec(),
                total_cells_created: 20,
                total_cells_consumed: 7,
                ..Default::default()
            })
            .unwrap();

        let mut withdraw = StoreBatch::new(&store);
        withdraw.put_sync_meta(sync_meta_keys::ROLLBACK_CLEANUP_IN_PROGRESS, &[1]);
        withdraw.delete_live_cell_summary_current().unwrap();
        withdraw.commit().unwrap();
        assert_eq!(store.get_live_cell_summary().unwrap(), None);

        store
            .restore_live_cell_summary_visibility_after_interrupted_rollback()
            .unwrap();

        assert_eq!(store.get_live_cell_summary().unwrap(), Some(summary));
        assert!(!store.is_rollback_cleanup_in_progress().unwrap());
    }

    #[test]
    fn initialized_summary_missing_current_and_history_fails_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        store
            .set_sync_status(&SyncStatus {
                tip_block_number: 7,
                tip_block_hash: vec![0x07; 32],
                ..Default::default()
            })
            .unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_sync_meta(sync_meta_keys::LIVE_CELL_SUMMARY_INITIALIZED, &[1]);
        batch.put_sync_meta(sync_meta_keys::ROLLBACK_CLEANUP_IN_PROGRESS, &[1]);
        batch.commit().unwrap();

        let error = store
            .restore_live_cell_summary_visibility_after_interrupted_rollback()
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("lost both current and tip history"));
        assert!(store.is_rollback_cleanup_in_progress().unwrap());
    }

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

    fn seed_reorg_event(store: &CkbadgerStore, detected_at_ms: i64, fork_point: i64, seq: u8) {
        let event = ReorgEvent {
            detected_at: detected_at_ms / 1000,
            kind: crate::types::ReorgEventKind::Automatic,
            fork_point,
            fork_point_hash: vec![seq; 32],
            old_tip: fork_point + 3,
            old_tip_hash: vec![seq; 32],
            new_tip: fork_point + 4,
            new_tip_hash: vec![seq; 32],
            depth: 3,
            orphaned_blocks: 3,
            orphaned_txs: 6,
        };
        let key = keys::encode_reorg_event_key(detected_at_ms, &[seq; 16]).unwrap();
        store
            .put_cf(
                store.cf_sync_meta(),
                key.as_bytes(),
                &bincode::serialize(&event).unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn test_get_latest_reorg_event_returns_none_without_history() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        assert!(store.get_latest_reorg_event().unwrap().is_none());
        assert_eq!(store.count_reorg_events().unwrap(), 0);
    }

    #[test]
    fn test_list_reorg_events_is_newest_first_and_pages_by_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        seed_reorg_event(&store, 1_700_000_000_000, 100, 0x01);
        seed_reorg_event(&store, 1_700_000_060_000, 200, 0x02);
        seed_reorg_event(&store, 1_700_000_120_000, 300, 0x03);
        // Other sync_meta keys must not leak into the history range.
        store.set_network_identity("testnet").unwrap();

        assert_eq!(store.count_reorg_events().unwrap(), 3);

        let page1 = store.list_reorg_events(2, None).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].event.fork_point, 300);
        assert_eq!(page1[0].detected_at_ms, 1_700_000_120_000);
        assert_eq!(page1[1].event.fork_point, 200);

        let page2 = store
            .list_reorg_events(2, Some(page1[1].key.as_bytes()))
            .unwrap();
        assert_eq!(page2.len(), 1, "cursor row itself must be excluded");
        assert_eq!(page2[0].event.fork_point, 100);

        let latest = store.get_latest_reorg_event().unwrap().unwrap();
        assert_eq!(latest.event.fork_point, 300);
    }

    #[test]
    fn test_get_reorg_event_by_detection_millisecond() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        seed_reorg_event(&store, 1_700_000_000_000, 100, 0x01);
        seed_reorg_event(&store, 1_700_000_060_000, 200, 0x02);

        let found = store.get_reorg_event(1_700_000_060_000).unwrap().unwrap();
        assert_eq!(found.event.fork_point, 200);
        assert!(store.get_reorg_event(1_700_000_030_000).unwrap().is_none());
    }

    #[test]
    fn test_get_reorg_event_fails_on_same_millisecond_collision() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        seed_reorg_event(&store, 1_700_000_000_000, 100, 0x01);
        seed_reorg_event(&store, 1_700_000_000_000, 101, 0x02);

        let err = store.get_reorg_event(1_700_000_000_000).unwrap_err();
        assert!(
            err.to_string().contains("ambiguous reorg event id"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_list_reorg_events_rejects_malformed_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let err = store.list_reorg_events(10, Some(b"reorg:1:x")).unwrap_err();
        assert!(
            err.to_string().contains("malformed reorg event key"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_list_reorg_events_fails_on_malformed_history_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        seed_reorg_event(&store, 1_700_000_000_000, 100, 0x01);
        // A key inside the history range that breaks the ordering invariant.
        store
            .put_cf(store.cf_sync_meta(), b"reorg:bad-ts:1", b"whatever")
            .unwrap();

        let err = store.list_reorg_events(10, None).unwrap_err();
        assert!(
            err.to_string().contains("malformed reorg event key"),
            "unexpected error: {err}"
        );
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
    fn genesis_baseline_roundtrip_and_absent_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        assert_eq!(store.get_genesis_baseline().unwrap(), None);

        let baseline = GenesisBaseline {
            total_issuance: 3_360_000_145_238_488_200,
            burnt: 840_000_000_000_000_000,
            virtual_occupied: 504_000_000_000_000_000,
        };
        store.set_genesis_baseline(&baseline).unwrap();
        assert_eq!(store.get_genesis_baseline().unwrap(), Some(baseline));
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
    fn test_get_latest_reorg_event_fails_on_malformed_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let key = keys::encode_reorg_event_key(1_700_000_000_000, &[0x07; 16]).unwrap();
        store
            .put_cf(store.cf_sync_meta(), key.as_bytes(), b"invalid-payload")
            .unwrap();

        let err = store.get_latest_reorg_event().unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to deserialize reorg event in sync_meta"),
            "unexpected error: {err}"
        );
    }
}
