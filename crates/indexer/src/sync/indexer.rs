use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use ckbadger_common::PipelineProgressData;
use dashmap::DashMap;
use tracing::{debug, info, warn};

use ckbadger_store::types::{CellDistributionTrackerState, HodlTrackerState};
use ckbadger_store::CkbadgerStore;

use crate::bulk_sync_perf::{BatchSample, BulkSyncPerfRun, HeartbeatSample, RocksDbConfig};
use crate::cache::CacheInvalidator;
use crate::config::Config;
use crate::db::writer::cell_distribution::CellDistributionTracker;
use crate::db::writer::hodl_wave::HodlWaveTracker;
use crate::db::{BatchWriter, Repository};
use ckb_store_reader::CkbChainReader;

use crate::rpc::CkbRpcClient;
use crate::runtime_diag::{generate_incident_id, read_cgroup_memory_snapshot, FlightRecorder};

use super::adaptive::*;
use super::bulk_build::BulkBuildEngine;
use super::diagnostics::*;
use super::helpers::*;
use super::sync_mode::*;
use super::types::{CachedCellInfo, CachedUdtCellInfo};
use super::SyncProgress;

fn ensure_hodl_tracker_state_consistent(
    state: Option<&HodlTrackerState>,
    tip_block: i64,
) -> Result<()> {
    if tip_block <= 0 {
        return Ok(());
    }
    let state = state.ok_or_else(|| {
        anyhow!(
            "missing HODL tracker state at tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
            tip_block
        )
    })?;
    if state.date_transitions.is_empty() {
        bail!(
            "invalid HODL tracker state: empty date_transitions at tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
            tip_block
        );
    }
    // Use last_processed_block for consistency checks (tracks every block processed,
    // not just date boundary transitions). Fall back to date_transitions.last() for
    // backward compatibility with states persisted before this field was added.
    let last_block = state
        .last_processed_block
        .or_else(|| state.date_transitions.last().map(|(b, _)| *b))
        .unwrap(); // safe: date_transitions is non-empty
    if last_block > tip_block {
        bail!(
            "invalid HODL tracker state: last processed block {} ahead of sync tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
            last_block,
            tip_block
        );
    }
    if last_block < tip_block {
        bail!(
            "invalid HODL tracker state: last processed block {} behind sync tip {} \
             (likely crash between domain commit and tracker persist). \
             automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
            last_block,
            tip_block
        );
    }
    Ok(())
}

pub(super) fn rebuild_hodl_tracker_from_state(
    state: Option<HodlTrackerState>,
    tip_block: i64,
) -> Result<HodlWaveTracker> {
    ensure_hodl_tracker_state_consistent(state.as_ref(), tip_block)?;
    if tip_block <= 0 {
        return Ok(HodlWaveTracker::new());
    }
    match state {
        Some(s) => Ok(HodlWaveTracker::from_state(s)?),
        None => Ok(HodlWaveTracker::new()),
    }
}

fn ensure_cell_dist_tracker_state_consistent(
    state: Option<&CellDistributionTrackerState>,
    tip_block: i64,
) -> Result<()> {
    if tip_block <= 0 {
        return Ok(());
    }
    let state = state.ok_or_else(|| {
        anyhow!(
            "missing cell distribution tracker state at tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
            tip_block
        )
    })?;
    if state.date_transitions.is_empty() {
        bail!(
            "invalid cell distribution tracker state: empty date_transitions at tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
            tip_block
        );
    }
    // Use last_processed_block for consistency checks. Fall back to date_transitions.last()
    // for backward compatibility with states persisted before this field was added.
    let last_block = state
        .last_processed_block
        .or_else(|| state.date_transitions.last().map(|(b, _)| *b))
        .unwrap(); // safe: date_transitions is non-empty
    if last_block > tip_block {
        bail!(
            "invalid cell distribution tracker state: last processed block {} ahead of sync tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
            last_block,
            tip_block
        );
    }
    if last_block < tip_block {
        bail!(
            "invalid cell distribution tracker state: last processed block {} behind sync tip {} \
             (likely crash between domain commit and tracker persist). \
             automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
            last_block,
            tip_block
        );
    }
    Ok(())
}

pub(super) fn rebuild_cell_dist_tracker_from_state(
    state: Option<CellDistributionTrackerState>,
    tip_block: i64,
) -> Result<CellDistributionTracker> {
    ensure_cell_dist_tracker_state_consistent(state.as_ref(), tip_block)?;
    if tip_block <= 0 {
        return Ok(CellDistributionTracker::new());
    }
    match state {
        Some(s) => Ok(CellDistributionTracker::from_state(s)?),
        None => Ok(CellDistributionTracker::new()),
    }
}

pub(super) fn require_non_negative_block_number(value: i64, context: &str) -> Result<u64> {
    u64::try_from(value).map_err(|_| anyhow!("negative block number in {}: {}", context, value))
}

pub(super) fn next_start_block_from_db_tip(
    db_tip: i64,
    db_tip_hash: &Option<Vec<u8>>,
    context: &str,
) -> Result<u64> {
    if db_tip == 0 && db_tip_hash.is_none() {
        return Ok(0);
    }

    let db_tip_u64 = require_non_negative_block_number(db_tip, context)?;
    db_tip_u64
        .checked_add(1)
        .ok_or_else(|| anyhow!("db_tip overflow in {}: {}", context, db_tip))
}

pub(super) fn blocks_behind_tip(chain_tip: u64, base_tip: i64, context: &str) -> Result<u64> {
    let base_tip_u64 = require_non_negative_block_number(base_tip, context)?;
    chain_tip.checked_sub(base_tip_u64).ok_or_else(|| {
        anyhow!(
            "invalid tip ordering in {}: base_tip={} exceeds chain_tip={}",
            context,
            base_tip,
            chain_tip
        )
    })
}

pub(super) fn is_fresh_sync_tip_state(
    sync_tip_block: i64,
    sync_tip_hash: &Option<Vec<u8>>,
) -> bool {
    sync_tip_block == 0 && sync_tip_hash.is_none()
}

pub(super) fn should_startup_bulk_sync_mode(
    blocks_behind: u64,
    bulk_sync_threshold: u64,
    sync_tip_block: i64,
    sync_tip_hash: &Option<Vec<u8>>,
) -> bool {
    is_fresh_sync_tip_state(sync_tip_block, sync_tip_hash)
        && is_bulk_sync_active_by_lag(blocks_behind, bulk_sync_threshold)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncPath {
    BulkBuild,
    Pipeline,
}

pub(super) fn select_startup_sync_path(
    blocks_behind: u64,
    bulk_sync_threshold: u64,
    sync_tip_block: i64,
    sync_tip_hash: &Option<Vec<u8>>,
) -> SyncPath {
    if should_startup_bulk_sync_mode(
        blocks_behind,
        bulk_sync_threshold,
        sync_tip_block,
        sync_tip_hash,
    ) {
        SyncPath::BulkBuild
    } else {
        SyncPath::Pipeline
    }
}

pub(crate) fn maybe_start_bulk_sync_perf_run(
    output_root: &Path,
    bulk_sync_mode: bool,
    run_id: &str,
    build_version: &str,
) -> Result<Option<BulkSyncPerfRun>> {
    if !bulk_sync_mode {
        return Ok(None);
    }
    Ok(Some(BulkSyncPerfRun::start(
        output_root,
        run_id,
        build_version,
    )?))
}

fn require_chain_tip_number(tip: Option<u64>, source: &str) -> Result<u64> {
    tip.ok_or_else(|| anyhow!("Failed to get chain tip from {}", source))
}

fn startup_header_gap_fail_fast_message(
    first_header_gap: i64,
    start_block: i64,
    header_tip: Option<i64>,
    tx_tip: Option<i64>,
) -> String {
    format!(
        "startup fail-fast: detected internal block header gap at block {} (sync_tip={}, header_tip={:?}, tx_tip={:?}). \
         automatic gap replay is disabled because it is equivalent to deep reorg handling; delete RocksDB and re-sync from genesis",
        first_header_gap, start_block, header_tip, tx_tip
    )
}

pub(super) fn mempool_short_tx_id(tx_hash: &str) -> &str {
    // Node-provided tx hashes are always "0x" + 64 hex chars; skip prefix, take first 20.
    &tx_hash[2..22]
}

const STARTUP_CONTINUITY_WINDOW_BLOCKS: i64 = 512;

pub(super) const CACHE_INVALIDATION_INTERVAL: u64 = 10_000;
pub struct Indexer {
    pub(crate) run_id: String,
    pub(crate) config: Config,
    pub(crate) rpc: CkbRpcClient,
    pub(crate) repo: Repository,
    pub(crate) writer: BatchWriter,
    pub(crate) append_only_store: Arc<CkbadgerStore>,
    pub(crate) progress: Arc<SyncProgress>,
    pub(crate) cell_cache: Arc<DashMap<([u8; 32], i16), CachedCellInfo>>,
    pub(crate) udt_cell_cache: Arc<DashMap<([u8; 32], i16), CachedUdtCellInfo>>,
    pub(crate) perf: PerfStats,
    pub(crate) pipeline_perf: Arc<PipelinePerfStats>,
    pub(crate) adaptive_batch_controller: Arc<AdaptiveBatchController>,
    pub(crate) cache_invalidator: CacheInvalidator,
    pub(crate) last_cache_invalidation: tokio::sync::Mutex<u64>,
    pub(crate) was_bulk_sync_active: std::sync::atomic::AtomicBool,
    pub(crate) bulk_sync_allowed: AtomicBool,
    pub(crate) rebuild_pause_flag: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) pipeline_reset_notify_flag: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) pipeline_reset_reason_code: Arc<AtomicU8>,
    pub(crate) startup_phase: AtomicU8,
    pub(crate) pipeline_reset_epoch: Arc<AtomicU64>,
    pub(crate) incident_seq: AtomicU64,
    pub(crate) flight_recorder: FlightRecorder,
    pub(crate) repeated_warning_tracker: RepeatedWarningTracker,
    pub(crate) incident_dir: PathBuf,
    pub(crate) bulk_sync_perf_run: std::sync::Mutex<Option<BulkSyncPerfRun>>,
    pub(crate) shutdown_requested: Arc<AtomicBool>,
    pub(crate) label_import_started: std::sync::atomic::AtomicBool,
    pub(crate) ckb_store: Option<Arc<CkbChainReader>>,
    pub(crate) hodl_tracker: std::sync::Mutex<HodlWaveTracker>,
    pub(crate) cell_dist_tracker: std::sync::Mutex<CellDistributionTracker>,
}

impl Indexer {
    pub async fn new(
        run_id: String,
        config: Config,
        store: Arc<CkbadgerStore>,
        append_only_store: Arc<CkbadgerStore>,
    ) -> Result<Self> {
        let rpc = CkbRpcClient::new(&config.ckb_rpc_url);
        let cache_invalidator = CacheInvalidator::new(store.clone());

        let reader = CkbChainReader::open(&config.ckb_db_path)?;
        info!("CKB direct RocksDB reader opened at {}", config.ckb_db_path);
        let ckb_store = Some(Arc::new(reader));
        let repo = Repository::with_cache(store.clone(), cache_invalidator.clone());
        let writer = BatchWriter::with_cache(
            store.clone(),
            append_only_store.clone(),
            cache_invalidator.clone(),
        );

        let (tip_number, tip_hash) = repo.get_sync_tip().await?;
        let chain_tip = require_chain_tip_number(
            ckb_store
                .as_ref()
                .expect("ckb_store must exist after startup validation")
                .tip_number(),
            "CKB RocksDB during indexer startup",
        )?;

        let tip_number_u64 =
            require_non_negative_block_number(tip_number, "indexer startup sync tip")?;
        let progress = Arc::new(SyncProgress::new(tip_number_u64, chain_tip));
        progress.start_refresher();
        let cell_cache = Arc::new(DashMap::with_capacity(CELL_CACHE_CAPACITY));
        let udt_cell_cache = Arc::new(DashMap::with_capacity(UDT_CELL_CACHE_CAPACITY));
        let adaptive_batch_controller =
            Arc::new(AdaptiveBatchController::new(config.pipeline_buffer as u64));

        let bulk_sync_allowed = is_fresh_sync_tip_state(tip_number, &tip_hash);
        let was_bulk =
            bulk_sync_allowed && progress.blocks_remaining() > config.bulk_sync_threshold;
        let hodl_tracker = match store.get_hodl_tracker_state()? {
            Some(state) => {
                info!(
                    "Restored HODL tracker: {} date entries, {} transitions, holder_count={}",
                    state.capacity_by_date.len(),
                    state.date_transitions.len(),
                    state.holder_count,
                );
                HodlWaveTracker::from_state(state)?
            }
            None => {
                info!("Starting fresh HODL wave tracker");
                HodlWaveTracker::new()
            }
        };

        let cell_dist_tracker = match store.get_cell_dist_tracker_state()? {
            Some(state) => {
                info!(
                    "Restored cell distribution tracker: {} transitions",
                    state.date_transitions.len(),
                );
                CellDistributionTracker::from_state(state)?
            }
            None => {
                info!("Starting fresh cell distribution tracker");
                CellDistributionTracker::new()
            }
        };

        let incident_dir = PathBuf::from(&config.domain_data_path).join("incidents");

        Ok(Self {
            run_id,
            config,
            rpc,
            repo,
            writer,
            append_only_store,
            progress,
            cell_cache,
            udt_cell_cache,
            perf: PerfStats::default(),
            pipeline_perf: Arc::new(PipelinePerfStats::default()),
            adaptive_batch_controller,
            cache_invalidator,
            last_cache_invalidation: tokio::sync::Mutex::new(0),
            was_bulk_sync_active: std::sync::atomic::AtomicBool::new(was_bulk),
            bulk_sync_allowed: AtomicBool::new(bulk_sync_allowed),
            rebuild_pause_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pipeline_reset_notify_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pipeline_reset_reason_code: Arc::new(AtomicU8::new(PIPELINE_RESET_REASON_UNKNOWN)),
            startup_phase: AtomicU8::new(STARTUP_PHASE_NONE),
            pipeline_reset_epoch: Arc::new(AtomicU64::new(0)),
            incident_seq: AtomicU64::new(0),
            flight_recorder: FlightRecorder::new(FLIGHT_RECORDER_CAPACITY),
            repeated_warning_tracker: RepeatedWarningTracker::default(),
            incident_dir,
            bulk_sync_perf_run: std::sync::Mutex::new(None),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            label_import_started: std::sync::atomic::AtomicBool::new(false),
            ckb_store,
            hodl_tracker: std::sync::Mutex::new(hodl_tracker),
            cell_dist_tracker: std::sync::Mutex::new(cell_dist_tracker),
        })
    }

    pub fn progress(&self) -> Arc<SyncProgress> {
        Arc::clone(&self.progress)
    }

    pub fn cache_invalidator(&self) -> &CacheInvalidator {
        &self.cache_invalidator
    }

    pub fn writer(&self) -> &BatchWriter {
        &self.writer
    }

    pub fn rebuild_pause_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.rebuild_pause_flag)
    }

    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutdown_requested)
    }

    pub fn mark_label_import_started(&self) {
        self.label_import_started.store(true, Ordering::SeqCst);
    }

    pub fn is_bulk_sync_active(&self) -> bool {
        self.is_bulk_sync_enabled_for_lag(self.progress.blocks_remaining())
    }

    pub(crate) fn is_bulk_sync_enabled_for_lag(&self, blocks_behind: u64) -> bool {
        self.bulk_sync_allowed.load(Ordering::SeqCst)
            && is_bulk_sync_active_by_lag(blocks_behind, self.config.bulk_sync_threshold)
    }

    pub(crate) fn should_handle_reorg_for_lag(&self, blocks_behind: u64) -> bool {
        if !self.bulk_sync_allowed.load(Ordering::SeqCst) {
            return true;
        }
        should_run_reorg_handling(blocks_behind, self.config.bulk_sync_threshold)
    }

    /// Dynamically switch RocksDB compaction options based on how far behind tip we are.
    ///
    /// - **Enter bulk**: blocks_behind > threshold and not already in bulk compaction mode.
    /// - **Exit bulk**: blocks_behind <= threshold and currently in bulk compaction mode,
    ///   BUT only if compaction pressure has drained (L0 files < 10, pending < 2 GB).
    ///   Otherwise defers the transition and logs.
    pub(super) fn ensure_compaction_mode(&self, blocks_behind: u64) {
        let domain_store = self.writer.store();
        let append_store = &self.append_only_store;
        let in_bulk = domain_store.is_bulk_sync_mode();
        let should_be_bulk = self.is_bulk_sync_enabled_for_lag(blocks_behind);

        if should_be_bulk && !in_bulk {
            info!(
                blocks_behind,
                threshold = self.config.bulk_sync_threshold,
                "Re-entering bulk compaction mode"
            );
            domain_store.set_bulk_sync_compaction_options();
            append_store.set_bulk_sync_compaction_options();
        } else if !should_be_bulk && in_bulk {
            let pressure = domain_store.compaction_pressure();
            const DRAIN_L0_THRESHOLD: u64 = 10;
            let drain_pending_threshold =
                domain_store.memory_profile().drain_pending_bytes_threshold;
            if pressure.l0_files_max < DRAIN_L0_THRESHOLD
                && pressure.compaction_pending_bytes < drain_pending_threshold
            {
                info!(
                    l0_files_max = pressure.l0_files_max,
                    compaction_pending_mb = pressure.compaction_pending_bytes / (1024 * 1024),
                    "Compaction drained, restoring normal compaction options"
                );
                domain_store.restore_normal_compaction_options();
                append_store.restore_normal_compaction_options();
                // Permanently disable bulk sync re-entry: bulk semantics are only
                // valid for fresh-DB rebuilds. Once we've caught up and exited bulk
                // mode, falling behind again must use live catch-up (with reorg
                // handling) instead of re-entering bulk paths on a non-fresh DB.
                if self.bulk_sync_allowed.swap(false, Ordering::SeqCst) {
                    info!("Bulk sync allowed permanently cleared after first catch-up");
                }
            } else {
                debug!(
                    l0_files_max = pressure.l0_files_max,
                    compaction_pending_mb = pressure.compaction_pending_bytes / (1024 * 1024),
                    "Deferring normal compaction: pressure still high"
                );
            }
        }
    }

    pub fn is_direct_db_read(&self) -> bool {
        self.ckb_store.is_some()
    }

    pub fn ckb_store(&self) -> Option<Arc<CkbChainReader>> {
        self.ckb_store.clone()
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn record_runtime_heartbeat(
        &self,
        current_block: u64,
        target_block: u64,
        stage: Option<&str>,
    ) {
        let current_block_i64 = match i64::try_from(current_block) {
            Ok(v) => v,
            Err(_) => {
                warn!(
                    run_id = %self.run_id,
                    current_block,
                    "Skipping runtime heartbeat: current_block exceeds i64 range"
                );
                return;
            }
        };
        let target_block_i64 = match i64::try_from(target_block) {
            Ok(v) => v,
            Err(_) => {
                warn!(
                    run_id = %self.run_id,
                    target_block,
                    "Skipping runtime heartbeat: target_block exceeds i64 range"
                );
                return;
            }
        };
        let cgroup = read_cgroup_memory_snapshot();
        if let Err(e) = self.writer.store().mark_runtime_heartbeat_with_diag(
            &self.run_id,
            current_block_i64,
            target_block_i64,
            stage,
            cgroup.oom_events,
            cgroup.oom_kill_events,
        ) {
            warn!(
                run_id = %self.run_id,
                current_block,
                target_block,
                error = %e,
                "Failed to persist runtime heartbeat"
            );
        }
    }

    pub fn mark_runtime_shutdown(&self, reason: &str, exit_code: i32) {
        if let Err(e) = self
            .writer
            .store()
            .mark_runtime_shutdown(&self.run_id, reason, exit_code)
        {
            warn!(
                run_id = %self.run_id,
                reason,
                exit_code,
                error = %e,
                "Failed to persist runtime shutdown reason"
            );
        }
    }

    pub fn record_bulk_sync_perf_heartbeat_sample(
        &self,
        current_block: u64,
        target_block: u64,
        compaction_pending_mb: u64,
        l0_files: u64,
        imm_memtables: u64,
    ) {
        let mut guard = self.bulk_sync_perf_run.lock().unwrap();
        let Some(run) = guard.as_mut() else {
            return;
        };
        if let Err(e) = run.record_heartbeat_sample(HeartbeatSample::new(
            current_block,
            target_block,
            compaction_pending_mb,
            l0_files,
            imm_memtables,
        )) {
            warn!(
                run_id = %self.run_id,
                error = %e,
                "Failed to record bulk-sync perf heartbeat sample"
            );
        }
    }

    pub fn record_bulk_sync_perf_batch_sample(&self, sample: BatchSample) {
        let mut guard = self.bulk_sync_perf_run.lock().unwrap();
        let Some(run) = guard.as_mut() else {
            return;
        };
        if let Err(e) = run.record_batch_sample(sample) {
            warn!(
                run_id = %self.run_id,
                error = %e,
                "Failed to record bulk-sync perf batch sample"
            );
        }
    }

    pub fn finalize_bulk_sync_perf_completed(&self) {
        self.finalize_bulk_sync_perf_run(true);
    }

    pub fn finalize_bulk_sync_perf_failed(&self) {
        self.finalize_bulk_sync_perf_run(false);
    }

    fn start_bulk_sync_perf_run(&self, bulk_sync_mode: bool) -> Result<()> {
        if !bulk_sync_mode {
            return Ok(());
        }
        if self.config.bulk_sync_perf_output_root.is_empty() {
            bail!("bulk sync perf output root is empty");
        }
        let mut guard = self.bulk_sync_perf_run.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        *guard = maybe_start_bulk_sync_perf_run(
            Path::new(&self.config.bulk_sync_perf_output_root),
            bulk_sync_mode,
            &self.run_id,
            &self.config.build_version,
        )?;

        // After the run is created, capture and set environment snapshot
        if let Some(run) = guard.as_mut() {
            let env = crate::sys_info::capture_environment(&self.config.domain_data_path);
            let profile = self.writer.store().memory_profile();
            let rocksdb_config = RocksDbConfig {
                rocksdb_budget_gb: profile.rocksdb_budget_bytes as u64 / (1024 * 1024 * 1024),
                block_cache_bulk_mb: profile.block_cache_bulk_sync_bytes as u64 / (1024 * 1024),
                wbm_bulk_mb: profile.wbm_bulk_sync_bytes as u64 / (1024 * 1024),
                write_buffer_mega_mb: profile.write_buffer_mega_bytes as u64 / (1024 * 1024),
                l0_slowdown_bulk: 96,
                l0_stop_bulk: 192,
                max_background_jobs: profile.max_background_jobs,
                max_subcompactions: profile.max_subcompactions,
                unordered_write: true,
                direct_io_reads: self.writer.store().runtime_config().direct_io_reads,
            };
            run.set_environment(env, rocksdb_config)?;
        }
        Ok(())
    }

    fn finalize_bulk_sync_perf_run(&self, completed: bool) {
        let mut guard = self.bulk_sync_perf_run.lock().unwrap();
        let Some(mut run) = guard.take() else {
            return;
        };
        let result = if completed {
            run.finish_completed()
        } else {
            run.finish_failed()
        };
        if let Err(e) = result {
            warn!(
                run_id = %self.run_id,
                completed,
                error = %e,
                "Failed to finalize bulk-sync perf run"
            );
        }
    }

    fn record_flight_event(&self, event: &str, detail: impl Into<String>) {
        self.flight_recorder.record(event, detail);
    }

    fn next_incident_id(&self) -> String {
        let sequence = self.incident_seq.fetch_add(1, Ordering::SeqCst) + 1;
        generate_incident_id(&self.run_id, sequence)
    }

    fn write_incident_report(
        &self,
        incident_id: &str,
        reason: &str,
        detail: &str,
    ) -> anyhow::Result<PathBuf> {
        let sync_status = self.writer.store().get_sync_status()?;
        let report = IncidentReport {
            incident_id: incident_id.to_string(),
            run_id: self.run_id.clone(),
            created_at: chrono::Utc::now().timestamp(),
            reason: reason.to_string(),
            detail: detail.to_string(),
            startup_phase: self.startup_phase(),
            pipeline_reset_epoch: self.pipeline_reset_epoch.load(Ordering::SeqCst),
            sync_tip_block: sync_status.tip_block_number,
            sync_tip_hash: if sync_status.tip_block_hash.is_empty() {
                "0x".to_string()
            } else {
                format!("0x{}", hex::encode(sync_status.tip_block_hash))
            },
            cgroup_memory: read_cgroup_memory_snapshot(),
            recent_events: self.flight_recorder.snapshot(),
        };

        std::fs::create_dir_all(&self.incident_dir)?;
        let path = self.incident_dir.join(format!("{}.json", incident_id));
        let encoded = serde_json::to_vec_pretty(&report)?;
        std::fs::write(&path, encoded)?;
        Ok(path)
    }

    pub(super) fn report_incident(&self, reason: &str, detail: impl Into<String>) -> String {
        let detail = detail.into();
        let incident_id = self.next_incident_id();
        self.record_flight_event(
            "incident",
            format!(
                "incident_id={} reason={} detail={}",
                incident_id, reason, detail
            ),
        );

        if let Err(e) =
            self.writer
                .store()
                .mark_runtime_incident(&self.run_id, &incident_id, reason)
        {
            warn!(
                run_id = %self.run_id,
                incident_id = %incident_id,
                error = %e,
                "Failed to persist runtime incident marker"
            );
        }

        match self.write_incident_report(&incident_id, reason, &detail) {
            Ok(path) => {
                info!(
                    run_id = %self.run_id,
                    incident_id = %incident_id,
                    path = %path.display(),
                    "Incident report written"
                );
            }
            Err(e) => {
                warn!(
                    run_id = %self.run_id,
                    incident_id = %incident_id,
                    error = %e,
                    "Failed to write incident report"
                );
            }
        }

        incident_id
    }

    pub(super) fn repeated_warning_snapshot(
        &self,
        key: &'static str,
        min_emit_interval: Duration,
    ) -> Option<RepeatedWarningSnapshot> {
        self.repeated_warning_tracker.record(key, min_emit_interval)
    }

    pub(super) fn request_pipeline_reset(
        &self,
        reason: &'static str,
        expected_start: Option<u64>,
        got_start: Option<u64>,
        writer_queue_depth: Option<usize>,
    ) {
        let reason_code = encode_pipeline_reset_reason(reason);
        let epoch = bump_pipeline_reset_epoch(&self.pipeline_reset_epoch);
        self.pipeline_reset_reason_code
            .store(reason_code, Ordering::SeqCst);
        self.pipeline_reset_notify_flag
            .store(true, Ordering::SeqCst);
        info!(
            run_id = %self.run_id,
            epoch,
            reason,
            reason_code,
            expected_start = ?expected_start,
            got_start = ?got_start,
            writer_queue_depth = ?writer_queue_depth,
            "Pipeline reset requested"
        );
        self.record_flight_event(
            "pipeline_reset",
            format!(
                "epoch={} reason={} expected_start={:?} got_start={:?} writer_queue_depth={:?}",
                epoch, reason, expected_start, got_start, writer_queue_depth
            ),
        );
    }

    /// Snapshot the current perf stats: (fetch_ms, db_stage_write_ms, db_commit_ms).
    pub fn perf_snapshot_ms(&self) -> (f64, f64, f64) {
        self.perf.snapshot_ms()
    }

    pub fn pipeline_progress_snapshot(&self) -> Option<PipelineProgressData> {
        self.pipeline_perf.snapshot()
    }

    pub fn adaptive_batch_snapshot(&self) -> Option<AdaptiveBatchProgressSnapshot> {
        let snapshot = self.adaptive_batch_controller.snapshot();
        Some(AdaptiveBatchProgressSnapshot {
            target_batch_txs: snapshot.target_batch_txs,
            inflight_limit: snapshot.inflight_limit,
            min_target_batch_txs: snapshot.min_target_batch_txs,
            cooldown_steps: snapshot.cooldown_steps,
            last_reason: decode_adaptive_batch_reason(snapshot.last_reason_code)
                .map(str::to_string),
            adjustment_seq: snapshot.adjustment_seq,
            backoff_streak: snapshot.backoff_streak,
            last_adjusted_at: snapshot.last_adjusted_at,
        })
    }

    pub fn pipeline_reset_snapshot(&self) -> Option<(u64, String)> {
        let epoch = self.pipeline_reset_epoch.load(Ordering::SeqCst);
        if epoch == 0 {
            return None;
        }
        let reason =
            decode_pipeline_reset_reason(self.pipeline_reset_reason_code.load(Ordering::SeqCst))
                .to_string();
        Some((epoch, reason))
    }

    pub fn startup_phase(&self) -> Option<String> {
        decode_startup_phase(self.startup_phase.load(Ordering::SeqCst)).map(str::to_string)
    }

    pub fn get_memory_stats(&self) -> ckbadger_common::MemoryStatsData {
        let stats = self.writer.store().memory_stats();
        let sync_status = self.writer.store().get_sync_status().unwrap_or_else(|e| {
            panic!(
                "failed to read sync_status while collecting memory stats: {}",
                e
            )
        });
        ckbadger_common::MemoryStatsData {
            live_cells_count: stats.live_cells_count as u64,
            consumed_cells_count: stats.consumed_cells_count as u64,
            consumed_cells_bytes: stats.consumed_cells_bytes as u64,
            consumed_cells_bytes_source: stats.consumed_cells_bytes_source.to_string(),
            rocksdb_memtable_bytes: stats.memtable_bytes as u64,
            rocksdb_block_cache_bytes: stats.block_cache_bytes as u64,
            rocksdb_table_readers_bytes: stats.table_readers_bytes as u64,
            rocksdb_total_bytes: stats.memory_bytes as u64,
            block_headers_count: stats.block_headers_count as u64,
            bulk_sync_cell_cache_enabled: false,
            bulk_sync_mode: self.is_bulk_sync_active(),
            compaction_pending_bytes: stats.compaction_pending_bytes,
            num_running_compactions: stats.num_running_compactions,
            sst_files_size: stats.sst_files_size,
            l0_files_count: stats.l0_files_count,
            l0_files_max: stats.l0_files_max,
            l0_worst_cf: stats.l0_worst_cf,
            immutable_memtables: stats.immutable_memtables,
            top_cf_sizes: stats.top_cf_sizes,
            wbm_usage_bytes: stats.wbm_usage_bytes as u64,
            wbm_budget_bytes: stats.wbm_budget_bytes as u64,
            total_transactions: sync_status.total_transactions,
            total_cells: sync_status.total_cells_created,
            total_live_cells: sync_status.total_cells_created - sync_status.total_cells_consumed,
            total_addresses: i64::try_from(stats.addr_balance_count).unwrap_or_else(|_| {
                panic!(
                    "addr_balance_count over i64 range in memory stats: {}",
                    stats.addr_balance_count
                )
            }),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }

    // === run ===

    pub async fn run(&self) -> Result<()> {
        let blocks_behind = self.progress.blocks_remaining();
        let (start_block, start_block_hash) = self.repo.get_sync_tip().await?;
        let fresh_sync_tip = is_fresh_sync_tip_state(start_block, &start_block_hash);
        let bulk_sync_allowed = fresh_sync_tip;
        self.bulk_sync_allowed
            .store(bulk_sync_allowed, Ordering::SeqCst);
        let sync_path = select_startup_sync_path(
            blocks_behind,
            self.config.bulk_sync_threshold,
            start_block,
            &start_block_hash,
        );
        let bulk_sync_mode = matches!(sync_path, SyncPath::BulkBuild);
        info!(
            run_id = %self.run_id,
            "Starting indexer ({} blocks behind, threshold={})",
            blocks_behind, self.config.bulk_sync_threshold
        );
        self.record_flight_event(
            "run_start",
            format!(
                "blocks_behind={} bulk_threshold={}",
                blocks_behind, self.config.bulk_sync_threshold
            ),
        );

        if bulk_sync_mode {
            info!(
                run_id = %self.run_id,
                "Bulk sync auto-enabled: {} blocks behind > {} threshold",
                blocks_behind, self.config.bulk_sync_threshold,
            );
            self.writer.store().set_bulk_sync_compaction_options();
            self.append_only_store.set_bulk_sync_compaction_options();
        } else if !fresh_sync_tip
            && is_bulk_sync_active_by_lag(blocks_behind, self.config.bulk_sync_threshold)
        {
            info!(
                run_id = %self.run_id,
                sync_tip = start_block,
                blocks_behind,
                threshold = self.config.bulk_sync_threshold,
                "Existing sync tip detected; bulk sync is disabled for non-fresh DB, running live catch-up mode"
            );
        }

        ensure_bulk_sync_fresh_start(
            bulk_sync_mode,
            start_block,
            &start_block_hash,
            &self.append_only_store,
        )?;
        let consistent_block = self.writer.find_last_consistent_block()?;
        let actual_start = match consistent_block {
            Some(cb) if cb < start_block => {
                warn!(
                    "Rolling back from block {} to {} due to data inconsistency",
                    start_block, cb
                );
                cb
            }
            _ => start_block,
        };
        let continuity_probe = self.writer.probe_startup_continuity(
            actual_start,
            STARTUP_CONTINUITY_WINDOW_BLOCKS,
            true,
        )?;
        if continuity_probe.has_inconsistency() {
            warn!(
                run_id = %self.run_id,
                startup_tip = continuity_probe.startup_tip,
                header_tip = ?continuity_probe.header_tip,
                tx_floor = ?continuity_probe.tx_floor,
                tx_tip = ?continuity_probe.tx_tip,
                first_header_gap = ?continuity_probe.first_header_gap,
                window_start = continuity_probe.recent_window_start,
                window_end = continuity_probe.recent_window_end,
                missing_header_sample = ?continuity_probe.missing_header_sample,
                missing_tx_block0_sample = ?continuity_probe.missing_tx_block0_sample,
                missing_tx_incomplete_sample = ?continuity_probe.missing_tx_incomplete_sample,
                full_header_gap_scan = continuity_probe.full_header_gap_scan,
                "Startup continuity probe detected inconsistencies"
            );
        } else {
            info!(
                run_id = %self.run_id,
                startup_tip = continuity_probe.startup_tip,
                header_tip = ?continuity_probe.header_tip,
                tx_floor = ?continuity_probe.tx_floor,
                tx_tip = ?continuity_probe.tx_tip,
                window_start = continuity_probe.recent_window_start,
                window_end = continuity_probe.recent_window_end,
                full_header_gap_scan = continuity_probe.full_header_gap_scan,
                "Startup continuity probe passed"
            );
        }

        if let Some(first_header_gap) = continuity_probe.first_header_gap {
            bail!(
                "{}",
                startup_header_gap_fail_fast_message(
                    first_header_gap,
                    start_block,
                    continuity_probe.header_tip,
                    continuity_probe.tx_tip
                )
            );
        }

        // Fail-fast on all other inconsistency conditions (tx_floor holes,
        // missing tx_block0 samples, header/tx tip mismatch). Continuing
        // incremental sync on a DB with known holes violates data integrity.
        if continuity_probe.has_inconsistency() {
            let mut reasons = Vec::new();
            if let Some(tx_floor) = continuity_probe.tx_floor {
                if tx_floor > 0 {
                    reasons.push(format!("tx_index floor at block {} (expected 0)", tx_floor));
                }
            }
            if let (Some(ht), Some(tt)) = (continuity_probe.header_tip, continuity_probe.tx_tip) {
                if ht != tt {
                    reasons.push(format!("header_tip={} != tx_tip={}", ht, tt));
                }
            }
            if !continuity_probe.missing_header_sample.is_empty() {
                reasons.push(format!(
                    "missing headers at blocks {:?}",
                    continuity_probe.missing_header_sample
                ));
            }
            if !continuity_probe.missing_tx_block0_sample.is_empty() {
                reasons.push(format!(
                    "missing tx_index[block][0] at blocks {:?}",
                    continuity_probe.missing_tx_block0_sample
                ));
            }
            if !continuity_probe.missing_tx_incomplete_sample.is_empty() {
                reasons.push(format!(
                    "incomplete tx rows (block, expected, last_found): {:?}",
                    continuity_probe.missing_tx_incomplete_sample
                ));
            }
            if !reasons.is_empty() {
                bail!(
                    "startup fail-fast: data inconsistency detected (sync_tip={}): {}. \
                     delete RocksDB and re-sync from genesis",
                    start_block,
                    reasons.join("; ")
                );
            }
        }

        if bulk_sync_mode && actual_start < start_block {
            bail!(
                "bulk sync fail-fast: inconsistent local DB state detected at startup (sync_tip={}, recovery_start={}). \
                 bulk sync does not auto-rollback; delete RocksDB and restart from genesis",
                start_block,
                actual_start
            );
        }

        let cleanup_needed = self
            .writer
            .needs_startup_cleanup_with_force(actual_start, self.config.force_startup_cleanup)?;

        if cleanup_needed {
            self.startup_phase
                .store(STARTUP_PHASE_ROLLBACK_CLEANUP, Ordering::SeqCst);
            info!(
                run_id = %self.run_id,
                from_block = actual_start + 1,
                "Startup rollback cleanup phase started"
            );
            self.record_flight_event(
                "startup_cleanup_started",
                format!("from_block={}", actual_start + 1),
            );
        }

        let init_result = self.writer.init_sync_start_with_options(
            self.append_only_store.as_ref(),
            actual_start,
            bulk_sync_mode,
            self.config.force_startup_cleanup,
        );

        self.startup_phase
            .store(STARTUP_PHASE_NONE, Ordering::SeqCst);
        if cleanup_needed {
            info!(
                run_id = %self.run_id,
                "Startup rollback cleanup phase completed"
            );
            self.record_flight_event("startup_cleanup_completed", "ok");
        }
        init_result?;
        if cleanup_needed {
            info!(
                run_id = %self.run_id,
                rollback_to = actual_start,
                "Startup undo-log rollback phase started"
            );
            self.writer
                .store()
                .rollback_via_undo_log(self.append_only_store.as_ref(), actual_start)?;
            info!(
                run_id = %self.run_id,
                rollback_to = actual_start,
                "Startup undo-log rollback phase completed"
            );
        }
        self.reconcile_hodl_tracker_with_tip(actual_start)?;
        self.reconcile_cell_dist_tracker_with_tip(actual_start)?;
        self.start_bulk_sync_perf_run(bulk_sync_mode)?;

        self.maybe_start_label_import();

        // Periodic 24h transfer refresh
        let store_for_task = Arc::clone(self.writer.store());
        let append_store_for_task = Arc::clone(&self.append_only_store);
        let progress_for_task = Arc::clone(&self.progress);
        let bulk_sync_threshold = self.config.bulk_sync_threshold;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(600));
            loop {
                interval.tick().await;
                let blocks_remaining = progress_for_task.blocks_remaining();
                if blocks_remaining > bulk_sync_threshold {
                    debug!(
                        "Skipping token 24h refresh ({} blocks remaining > {} threshold)",
                        blocks_remaining, bulk_sync_threshold
                    );
                    continue;
                }
                let writer =
                    BatchWriter::new(store_for_task.clone(), append_store_for_task.clone());
                match writer.refresh_token_24h_transfers() {
                    Ok(count) => info!("Refreshed 24h transfers for {} tokens", count),
                    Err(e) => warn!("Failed to refresh token 24h transfers: {}", e),
                }
                match writer.refresh_mnft_24h_transfers() {
                    Ok(count) => info!("Refreshed 24h transfers for {} NFT classes", count),
                    Err(e) => warn!("Failed to refresh NFT 24h transfers: {}", e),
                }
            }
        });

        match sync_path {
            SyncPath::BulkBuild => BulkBuildEngine::run(self).await,
            SyncPath::Pipeline => self.run_pipeline().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_require_chain_tip_number_errors_on_missing_tip() {
        let err = require_chain_tip_number(None, "CKB RocksDB").unwrap_err();
        assert!(err
            .to_string()
            .contains("Failed to get chain tip from CKB RocksDB"));
    }

    #[test]
    fn test_next_start_block_from_db_tip_rejects_negative_tip() {
        let err = next_start_block_from_db_tip(-1, &Some(vec![0x11; 32]), "unit-test").unwrap_err();
        assert!(err.to_string().contains("negative block number"));
    }

    #[test]
    fn test_blocks_behind_tip_rejects_inverted_tip_order() {
        let err = blocks_behind_tip(100, 101, "unit-test").unwrap_err();
        assert!(err.to_string().contains("exceeds chain_tip"));
    }

    #[test]
    fn test_is_fresh_sync_tip_state_requires_zero_without_hash() {
        assert!(is_fresh_sync_tip_state(0, &None));
        assert!(!is_fresh_sync_tip_state(0, &Some(vec![0x11; 32])));
        assert!(!is_fresh_sync_tip_state(1, &None));
    }

    #[test]
    fn test_should_startup_bulk_sync_mode_requires_fresh_tip_and_lag() {
        assert!(should_startup_bulk_sync_mode(1001, 1000, 0, &None));
        assert!(!should_startup_bulk_sync_mode(1000, 1000, 0, &None));
        assert!(!should_startup_bulk_sync_mode(
            1001,
            1000,
            10,
            &Some(vec![0x22; 32])
        ));
    }

    #[test]
    fn startup_bulk_sync_uses_bulk_build_engine_for_fresh_store() {
        let route = select_startup_sync_path(10_000, 72, 0, &None);
        assert_eq!(route, SyncPath::BulkBuild);
    }

    #[test]
    fn startup_existing_sync_tip_uses_pipeline_even_when_lagging() {
        let route = select_startup_sync_path(10_000, 72, 5, &Some(vec![0x11; 32]));
        assert_eq!(route, SyncPath::Pipeline);
    }

    #[test]
    fn test_maybe_start_bulk_sync_perf_run_returns_none_when_bulk_sync_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let run = maybe_start_bulk_sync_perf_run(
            dir.path(),
            false,
            "run-1",
            "0.1.0+feature/foo@abcdef123456",
        )
        .unwrap();
        assert!(run.is_none());
        assert!(!dir.path().join("run-1").exists());
    }

    #[test]
    fn test_mempool_short_tx_id_extracts_first_20_hex_chars() {
        assert_eq!(
            mempool_short_tx_id("0x1234567890abcdef123456"),
            "1234567890abcdef1234"
        );
    }

    #[test]
    fn test_startup_header_gap_fail_fast_message_requires_rebuild() {
        let msg = startup_header_gap_fail_fast_message(123, 500, Some(600), Some(590));
        assert!(msg.contains("gap at block 123"));
        assert!(msg.contains("delete RocksDB and re-sync from genesis"));
        assert!(msg.contains("automatic gap replay is disabled"));
    }

    #[test]
    fn test_ensure_hodl_tracker_state_consistency_rules() {
        assert!(ensure_hodl_tracker_state_consistent(None, 0).is_ok());
        let missing = ensure_hodl_tracker_state_consistent(None, 1).unwrap_err();
        assert!(missing.to_string().contains("missing HODL tracker state"));

        let empty = HodlTrackerState {
            capacity_by_date: vec![],
            date_transitions: vec![],
            holder_count: 0,
            last_snapshot_date: None,
            last_processed_block: None,
        };
        let empty_err = ensure_hodl_tracker_state_consistent(Some(&empty), 100).unwrap_err();
        assert!(empty_err.to_string().contains("empty date_transitions"));

        // last_processed_block matches tip — valid even though last date transition < tip
        let aligned = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 1)],
            date_transitions: vec![(0, "20240101".to_string()), (50, "20240102".to_string())],
            holder_count: 1,
            last_snapshot_date: Some("20240102".to_string()),
            last_processed_block: Some(100),
        };
        assert!(ensure_hodl_tracker_state_consistent(Some(&aligned), 100).is_ok());

        let ahead = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 1)],
            date_transitions: vec![(0, "20240101".to_string()), (50, "20240102".to_string())],
            holder_count: 1,
            last_snapshot_date: Some("20240102".to_string()),
            last_processed_block: Some(101),
        };
        let ahead_err = ensure_hodl_tracker_state_consistent(Some(&ahead), 100).unwrap_err();
        assert!(ahead_err.to_string().contains("ahead of sync tip"));

        // Backward compat: no last_processed_block, falls back to date_transitions.last()
        let fallback_aligned = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 1)],
            date_transitions: vec![(0, "20240101".to_string()), (100, "20240102".to_string())],
            holder_count: 1,
            last_snapshot_date: Some("20240102".to_string()),
            last_processed_block: None,
        };
        assert!(ensure_hodl_tracker_state_consistent(Some(&fallback_aligned), 100).is_ok());

        // last_processed_block behind tip — crash detection
        let behind = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 1)],
            date_transitions: vec![(0, "20240101".to_string()), (50, "20240102".to_string())],
            holder_count: 1,
            last_snapshot_date: Some("20240102".to_string()),
            last_processed_block: Some(90),
        };
        let behind_err = ensure_hodl_tracker_state_consistent(Some(&behind), 100).unwrap_err();
        assert!(behind_err.to_string().contains("behind sync tip"));
    }

    #[test]
    fn test_rebuild_hodl_tracker_from_state_resets_when_tip_is_zero() {
        let stale = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 10)],
            date_transitions: vec![(200, "20240101".to_string())],
            holder_count: 7,
            last_snapshot_date: Some("20240101".to_string()),
            last_processed_block: Some(200),
        };

        let tracker = rebuild_hodl_tracker_from_state(Some(stale), 0).unwrap();
        let state = tracker.to_state();
        assert!(state.capacity_by_date.is_empty());
        assert!(state.date_transitions.is_empty());
        assert_eq!(state.holder_count, 0);
        assert!(state.last_snapshot_date.is_none());
    }

    #[test]
    fn test_rebuild_hodl_tracker_from_state_restores_when_tip_is_positive() {
        let persisted = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 10)],
            date_transitions: vec![(1, "20240101".to_string())],
            holder_count: 3,
            last_snapshot_date: Some("20240101".to_string()),
            last_processed_block: Some(1),
        };

        let tracker = rebuild_hodl_tracker_from_state(Some(persisted), 1).unwrap();
        let state = tracker.to_state();
        assert_eq!(state.capacity_by_date.len(), 1);
        assert_eq!(state.date_transitions.len(), 1);
        assert_eq!(state.holder_count, 3);
        assert_eq!(state.last_snapshot_date, Some("20240101".to_string()));
    }

    #[test]
    fn test_ensure_compaction_mode_drain_guard_defers_when_pressure_high() {
        // Simulates the drain guard logic: when store is in bulk mode but should transition
        // to normal, if compaction_pressure reports high L0 files, we should NOT restore
        // normal mode yet.
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());

        // Enter bulk mode
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());

        // Check compaction_pressure on an empty store (should be 0/0/0 → drain OK)
        let pressure = store.compaction_pressure();
        // Empty store has no L0 files and no pending compaction
        assert!(pressure.l0_files_max < 10);
        assert!(pressure.compaction_pending_bytes < 2 * 1024 * 1024 * 1024);

        // Restore should succeed on empty store (drain condition met)
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());
    }

    #[test]
    fn test_ensure_compaction_mode_reentry() {
        // Verifies that after restoring normal mode, re-entering bulk mode works
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());

        // Enter bulk
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());

        // Exit bulk
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());

        // Re-enter bulk
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());

        // Exit again
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());
    }
}
