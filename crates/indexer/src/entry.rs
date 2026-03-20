use anyhow::Result;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use ckbadger_common::LabelImportConfig;
use ckbadger_store::{types::SyncStatus, CkbadgerStore, RuntimeStatus, StoreRuntimeConfig};

use crate::cycles_worker::spawn_cycles_task_worker;
use crate::db::Repository;
use crate::label_import::{run_label_import_bundled, run_label_import_staged};
use crate::runtime_diag::{generate_run_id, read_cgroup_memory_snapshot};
use crate::sync::Indexer;
use crate::Config;

/// Configuration for starting the indexer sync daemon.
/// This is the interface the CLI binary uses to start the indexer.
pub struct IndexerServiceConfig {
    pub domain_data_path: String,
    pub append_only_data_path: String,
    pub bulk_sync_perf_output_root: String,
    pub build_version: String,
    pub ckb_rpc_url: String,
    pub ckb_db_path: String,
    pub token_labels_path: String,
    pub network: String,
    pub batch_size: usize,
    pub poll_interval_ms: u64,
    pub parallel_fetch_size: usize,
    pub pipeline_buffer: usize,
    pub bulk_sync_threshold: u64,
    pub store_runtime_config: StoreRuntimeConfig,
}

impl From<IndexerServiceConfig> for Config {
    fn from(svc: IndexerServiceConfig) -> Self {
        Config {
            domain_data_path: svc.domain_data_path,
            append_only_data_path: svc.append_only_data_path,
            bulk_sync_perf_output_root: svc.bulk_sync_perf_output_root,
            build_version: svc.build_version,
            ckb_rpc_url: svc.ckb_rpc_url,
            batch_size: svc.batch_size,
            poll_interval_ms: svc.poll_interval_ms,
            start_block: None,
            parallel_fetch_size: svc.parallel_fetch_size,
            pipeline_buffer: svc.pipeline_buffer,
            bulk_sync_threshold: svc.bulk_sync_threshold,
            fast_sync_mode: true,
            ckb_db_path: svc.ckb_db_path,
            token_labels_path: svc.token_labels_path,
            network: svc.network,
            force_startup_cleanup: false,
            store_runtime_config: svc.store_runtime_config,
        }
    }
}

/// Configuration for the label import command.
pub struct LabelImportServiceConfig {
    pub domain_data_path: String,
    pub append_only_data_path: String,
    pub token_labels_path: String,
    pub network: String,
    pub import_udt: bool,
    pub import_scripts: bool,
    pub use_bundled: bool,
    pub store_runtime_config: StoreRuntimeConfig,
}

/// Run the indexer sync daemon from a service config. Blocks until shutdown signal or error.
pub async fn run_indexer(config: IndexerServiceConfig) -> Result<()> {
    let config: Config = config.into();
    config.validate()?;
    run_indexer_sync(config).await
}

async fn run_startup_label_import(store: Arc<CkbadgerStore>, config: &Config) -> Result<()> {
    let token_labels_path = config.token_labels_path.clone();
    let has_fs_labels = !token_labels_path.is_empty()
        && std::path::Path::new(&token_labels_path)
            .join("information")
            .exists();
    let network = config.network.clone();

    info!(
        has_filesystem_labels = has_fs_labels,
        network = %network,
        "Running label import before sync startup"
    );

    let summary = tokio::task::spawn_blocking(move || {
        if has_fs_labels {
            let label_config = LabelImportConfig {
                token_labels_path,
                network,
                import_udt: true,
                import_scripts: true,
            };
            run_label_import_staged(store.as_ref(), &label_config)
        } else {
            run_label_import_bundled(store.as_ref(), &network)
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!("startup label import task panicked: {}", e))??;

    if summary.errors.is_empty() {
        info!(
            "Startup label import completed: {} UDT, {} scripts, 0 errors",
            summary.udt_labels_imported, summary.script_labels_imported
        );
    } else {
        for err in &summary.errors {
            warn!("Label import error: {}", err);
        }
        warn!(
            "Startup label import completed with {} errors: {} UDT, {} scripts",
            summary.errors.len(),
            summary.udt_labels_imported,
            summary.script_labels_imported
        );
    }

    Ok(())
}

/// Run the indexer sync daemon from an internal Config. Blocks until shutdown signal or error.
///
/// This is the shared core that both `main.rs` and the CLI entry point call.
pub async fn run_indexer_sync(mut config: Config) -> Result<()> {
    config.validate()?;
    info!(
        "Opening ckbadger domain store at: {}",
        config.domain_data_path
    );
    let store = Arc::new(CkbadgerStore::open_domain_with_runtime(
        &config.domain_data_path,
        config.store_runtime_config,
    )?);
    info!(
        "Opening ckbadger append-only store at: {}",
        config.append_only_data_path
    );
    let append_only_store = Arc::new(CkbadgerStore::open_append_only_with_runtime(
        &config.append_only_data_path,
        config.store_runtime_config,
    )?);
    store.log_config();

    let mut sync_status = store.get_sync_status()?;
    let previous_runtime = store.get_runtime_status()?;
    let rollback_cleanup_in_progress = store.is_rollback_cleanup_in_progress()?;
    let startup_cleanup = startup_cleanup_decision(&previous_runtime, rollback_cleanup_in_progress);
    let previous_run_unclean =
        should_force_startup_cleanup(&previous_runtime, rollback_cleanup_in_progress);
    debug_assert_eq!(previous_run_unclean, startup_cleanup.force_cleanup);
    config.force_startup_cleanup = previous_run_unclean;
    info!(
        force_startup_cleanup = startup_cleanup.force_cleanup,
        startup_cleanup_reason = startup_cleanup.reason,
        rollback_cleanup_in_progress,
        previous_active_run_id = ?previous_runtime.active_run_id,
        previous_last_run_id = ?previous_runtime.last_run_id,
        previous_last_shutdown_reason = ?previous_runtime.last_shutdown_reason,
        previous_last_exit_code = ?previous_runtime.last_exit_code,
        previous_run_started_at = previous_runtime.run_started_at,
        previous_last_shutdown_at = previous_runtime.last_shutdown_at,
        previous_last_incident_summary = ?previous_runtime.last_incident_summary,
        previous_last_incident_at = previous_runtime.last_incident_at,
        "Startup cleanup decision evaluated"
    );
    if previous_run_unclean {
        info!(
            rollback_cleanup_in_progress,
            startup_cleanup_reason = startup_cleanup.reason,
            "Forcing startup rollback cleanup to reconcile append-only state"
        );
    }
    reconcile_token_daily_deltas_on_startup(&store)?;

    let repo = Repository::new(store.clone());
    let (db_tip, db_tip_hash) = repo.get_sync_tip().await?;
    if let Some(reason) = sync_status_repair_reason(&sync_status, db_tip, db_tip_hash.as_deref()) {
        info!(
            repair_reason = ?reason,
            sync_status_tip_block_number = sync_status.tip_block_number,
            sync_status_tip_block_hash = if sync_status.tip_block_hash.is_empty() {
                "none".to_string()
            } else {
                format!("0x{}", hex::encode(&sync_status.tip_block_hash))
            },
            db_tip_block_number = db_tip,
            db_tip_block_hash = db_tip_hash
                .as_ref()
                .map(|hash| format!("0x{}", hex::encode(hash)))
                .unwrap_or_else(|| "none".to_string()),
            "sync_status tip metadata differs from authoritative block_headers tip, repairing sync_status"
        );
        let repaired_tip_hash = db_tip_hash.clone();
        store.update_sync_status(|status| {
            status.tip_block_number = db_tip;
            match &repaired_tip_hash {
                Some(hash) => status.tip_block_hash = hash.clone(),
                None if db_tip == 0 => status.tip_block_hash.clear(),
                None => {}
            }
        })?;
        sync_status = store.get_sync_status()?;
        info!(
            "Repaired sync_status tip to {} from block_headers tip",
            sync_status.tip_block_number
        );
    }
    let run_id = generate_run_id();
    let startup_cgroup = read_cgroup_memory_snapshot();
    let startup_oom_events_since_last_heartbeat = monotonic_counter_delta(
        startup_cgroup.oom_events,
        previous_runtime.last_heartbeat_oom_events,
    );
    let startup_oom_kill_events_since_last_heartbeat = monotonic_counter_delta(
        startup_cgroup.oom_kill_events,
        previous_runtime.last_heartbeat_oom_kill_events,
    );
    let unclean_shutdown_hint = classify_unclean_shutdown_hint(
        &previous_runtime,
        startup_oom_kill_events_since_last_heartbeat,
    );
    if previous_run_unclean {
        info!(
            run_id = %run_id,
            previous_active_run_id = ?previous_runtime.active_run_id,
            previous_last_run_id = ?previous_runtime.last_run_id,
            previous_last_shutdown_reason = ?previous_runtime.last_shutdown_reason,
            previous_last_exit_code = ?previous_runtime.last_exit_code,
            previous_last_shutdown_at = previous_runtime.last_shutdown_at,
            previous_last_heartbeat_at = previous_runtime.last_heartbeat_at,
            previous_last_heartbeat_block = previous_runtime.last_heartbeat_block,
            previous_last_heartbeat_target_block = previous_runtime.last_heartbeat_target_block,
            previous_last_heartbeat_stage = ?previous_runtime.last_heartbeat_stage,
            previous_last_heartbeat_oom_events = ?previous_runtime.last_heartbeat_oom_events,
            previous_last_heartbeat_oom_kill_events = ?previous_runtime.last_heartbeat_oom_kill_events,
            previous_last_incident_id = ?previous_runtime.last_incident_id,
            cgroup_memory_current_bytes = startup_cgroup.memory_current_bytes,
            cgroup_memory_max_bytes = startup_cgroup.memory_max_bytes,
            cgroup_memory_max_raw = ?startup_cgroup.memory_max_raw,
            cgroup_oom_events = startup_cgroup.oom_events,
            cgroup_oom_kill_events = startup_cgroup.oom_kill_events,
            startup_oom_events_since_last_heartbeat = ?startup_oom_events_since_last_heartbeat,
            startup_oom_kill_events_since_last_heartbeat = ?startup_oom_kill_events_since_last_heartbeat,
            unclean_shutdown_hint,
            "Detected previous run without graceful shutdown marker"
        );
        warn!(
            run_id = %run_id,
            unclean_shutdown_hint,
            startup_oom_events_since_last_heartbeat = ?startup_oom_events_since_last_heartbeat,
            startup_oom_kill_events_since_last_heartbeat = ?startup_oom_kill_events_since_last_heartbeat,
            previous_last_heartbeat_stage = ?previous_runtime.last_heartbeat_stage,
            previous_last_heartbeat_block = previous_runtime.last_heartbeat_block,
            previous_last_heartbeat_target_block = previous_runtime.last_heartbeat_target_block,
            "Unclean shutdown diagnostics summary"
        );
    } else {
        info!(
            run_id = %run_id,
            previous_last_run_id = ?previous_runtime.last_run_id,
            previous_last_shutdown_reason = ?previous_runtime.last_shutdown_reason,
            previous_last_exit_code = ?previous_runtime.last_exit_code,
            previous_last_shutdown_at = previous_runtime.last_shutdown_at,
            previous_last_heartbeat_at = previous_runtime.last_heartbeat_at,
            previous_last_heartbeat_block = previous_runtime.last_heartbeat_block,
            previous_last_heartbeat_target_block = previous_runtime.last_heartbeat_target_block,
            previous_last_heartbeat_stage = ?previous_runtime.last_heartbeat_stage,
            previous_last_heartbeat_oom_events = ?previous_runtime.last_heartbeat_oom_events,
            previous_last_heartbeat_oom_kill_events = ?previous_runtime.last_heartbeat_oom_kill_events,
            previous_last_incident_id = ?previous_runtime.last_incident_id,
            cgroup_memory_current_bytes = startup_cgroup.memory_current_bytes,
            cgroup_memory_max_bytes = startup_cgroup.memory_max_bytes,
            cgroup_memory_max_raw = ?startup_cgroup.memory_max_raw,
            cgroup_oom_events = startup_cgroup.oom_events,
            cgroup_oom_kill_events = startup_cgroup.oom_kill_events,
            startup_oom_events_since_last_heartbeat = ?startup_oom_events_since_last_heartbeat,
            startup_oom_kill_events_since_last_heartbeat = ?startup_oom_kill_events_since_last_heartbeat,
            "Runtime diagnostics at startup"
        );
    }
    store.mark_runtime_run_start(&run_id, db_tip)?;
    info!(run_id = %run_id, startup_tip = db_tip, "Runtime run marker persisted");

    let is_fresh_sync = db_tip == 0 && db_tip_hash.is_none();

    if is_fresh_sync {
        info!("Fresh database detected (tip=0), starting initial sync");
    } else {
        info!("Resuming sync from block {}", db_tip);
    }

    info!("Connecting to CKB node: {}", config.ckb_rpc_url);
    run_startup_label_import(store.clone(), &config).await?;

    let indexer = Indexer::new(
        run_id,
        config.clone(),
        store.clone(),
        append_only_store.clone(),
    )
    .await?;
    let indexer = Arc::new(indexer);
    indexer.mark_label_import_started();

    let (_cycles_tx, _cycles_result_store) =
        spawn_cycles_task_worker(store.clone(), config.ckb_rpc_url.clone());

    let data_source = if indexer.is_direct_db_read() {
        "DB"
    } else {
        "RPC"
    };

    let indexer_for_progress = Arc::clone(&indexer);
    tokio::spawn(async move {
        let mut last_queue_pressure_warn_at: Option<Instant> = None;
        let mut suppressed_queue_pressure_warns: u64 = 0;
        let mut last_progress_block: Option<u64> = None;
        let mut last_progress_advanced_at = Instant::now();
        let mut last_stall_warn_at: Option<Instant> = None;
        let mut suppressed_stall_warns: u64 = 0;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

            let progress = indexer_for_progress.progress();
            let ema_rate = progress.ema_blocks_per_second();
            let ema_tx_rate = progress.ema_txs_per_second();
            let eta = progress.eta_formatted();
            let bps = progress.blocks_per_second();
            let txps = progress.txs_per_second();
            let last_batch_blocks = progress.last_batch_blocks();

            let (perf_rpc_ms, perf_db_stage_ms, perf_db_commit_ms) =
                indexer_for_progress.perf_snapshot_ms();
            let pipeline = indexer_for_progress.pipeline_progress_snapshot();
            let pipeline_log = pipeline.clone();
            let adaptive = indexer_for_progress.adaptive_batch_snapshot();
            let pipeline_reset = indexer_for_progress.pipeline_reset_snapshot();
            let heartbeat_stage = indexer_for_progress.startup_phase().unwrap_or_else(|| {
                if indexer_for_progress.is_bulk_sync_active() {
                    "bulk_sync".to_string()
                } else {
                    "tip_sync".to_string()
                }
            });
            indexer_for_progress.record_runtime_heartbeat(
                progress.current(),
                progress.target(),
                Some(heartbeat_stage.as_str()),
            );
            let sync_data = ckbadger_common::SyncProgressData {
                current_block: progress.current(),
                target_block: progress.target(),
                last_batch_blocks: (last_batch_blocks > 0).then_some(last_batch_blocks),
                blocks_per_second: bps,
                ema_blocks_per_second: ema_rate,
                txs_per_second: (txps > 0.0).then_some(txps),
                ema_txs_per_second: (ema_tx_rate > 0.0).then_some(ema_tx_rate),
                eta_seconds: progress.eta_seconds(),
                eta_formatted: eta.clone(),
                progress_percentage: progress.progress_percentage(),
                updated_at: chrono::Utc::now().timestamp(),
                startup_phase: indexer_for_progress.startup_phase(),
                is_direct_db_read: data_source == "DB",
                db_write_ms: if perf_db_stage_ms > 0.0 {
                    Some(perf_db_stage_ms)
                } else {
                    None
                },
                db_commit_ms: if perf_db_commit_ms > 0.0 {
                    Some(perf_db_commit_ms)
                } else {
                    None
                },
                rpc_fetch_ms: if perf_rpc_ms > 0.0 {
                    Some(perf_rpc_ms)
                } else {
                    None
                },
                pipeline,
                pipeline_reset_epoch: pipeline_reset.as_ref().map(|(epoch, _)| *epoch),
                pipeline_reset_reason: pipeline_reset.as_ref().map(|(_, reason)| reason.clone()),
                adaptive_target_batch_txs: adaptive.as_ref().map(|s| s.target_batch_txs),
                adaptive_inflight_limit: adaptive.as_ref().map(|s| s.inflight_limit),
                adaptive_min_target_batch_txs: adaptive.as_ref().map(|s| s.min_target_batch_txs),
                adaptive_cooldown_steps: adaptive.as_ref().map(|s| s.cooldown_steps),
                adaptive_last_reason: adaptive.as_ref().and_then(|s| s.last_reason.clone()),
                adaptive_adjustment_seq: adaptive.as_ref().map(|s| s.adjustment_seq),
                adaptive_backoff_streak: adaptive.as_ref().map(|s| s.backoff_streak),
                adaptive_last_adjusted_at: adaptive.as_ref().and_then(|s| s.last_adjusted_at),
                bulk_build: indexer_for_progress.bulk_build_progress_snapshot(),
            };
            indexer_for_progress
                .cache_invalidator()
                .publish_sync_progress(&sync_data)
                .await;

            let memory_stats = indexer_for_progress.get_memory_stats();
            indexer_for_progress
                .cache_invalidator()
                .publish_memory_stats(&memory_stats)
                .await;
            indexer_for_progress.record_bulk_sync_perf_heartbeat_sample(
                progress.current(),
                progress.target(),
                memory_stats.compaction_pending_bytes / (1024 * 1024),
                memory_stats.l0_files_count,
                memory_stats.immutable_memtables,
            );

            info!(
                run_id = %indexer_for_progress.run_id(),
                memtable_mb = memory_stats.rocksdb_memtable_bytes / (1024 * 1024),
                block_cache_mb = memory_stats.rocksdb_block_cache_bytes / (1024 * 1024),
                compaction_pending_mb = memory_stats.compaction_pending_bytes / (1024 * 1024),
                running_compactions = memory_stats.num_running_compactions,
                l0_files = memory_stats.l0_files_count,
                l0_max = memory_stats.l0_files_max,
                l0_worst_cf = memory_stats.l0_worst_cf,
                imm_memtables = memory_stats.immutable_memtables,
                sst_size_gb = format!(
                    "{:.1}",
                    memory_stats.sst_files_size as f64 / (1024.0 * 1024.0 * 1024.0)
                ),
                "RocksDB stats"
            );

            let fetch_fill_pct = queue_fill_pct(
                pipeline_log.as_ref().and_then(|p| p.fetch_queue_depth),
                pipeline_log.as_ref().and_then(|p| p.fetch_queue_capacity),
            );
            let parse_fill_pct = queue_fill_pct(
                pipeline_log.as_ref().and_then(|p| p.parse_queue_depth),
                pipeline_log.as_ref().and_then(|p| p.parse_queue_capacity),
            );
            let writer_fill_pct = queue_fill_pct(
                pipeline_log.as_ref().and_then(|p| p.writer_queue_depth),
                pipeline_log.as_ref().and_then(|p| p.writer_queue_capacity),
            );

            let current_block = progress.current();
            match last_progress_block {
                None => {
                    last_progress_block = Some(current_block);
                    last_progress_advanced_at = Instant::now();
                }
                Some(last_block) if current_block > last_block => {
                    if let Some(last_warn_at) = last_stall_warn_at.take() {
                        info!(
                            run_id = %indexer_for_progress.run_id(),
                            resumed_at = current_block,
                            stalled_seconds = last_progress_advanced_at.elapsed().as_secs(),
                            seconds_since_last_stall_warn = last_warn_at.elapsed().as_secs(),
                            suppressed_since_last = suppressed_stall_warns,
                            "Sync progress resumed after stall"
                        );
                    }
                    last_progress_block = Some(current_block);
                    last_progress_advanced_at = Instant::now();
                    suppressed_stall_warns = 0;
                }
                Some(_) if current_block < progress.target() => {
                    let stalled_for = last_progress_advanced_at.elapsed();
                    let now = Instant::now();
                    if should_warn_progress_stall(
                        last_progress_advanced_at,
                        now,
                        current_block,
                        progress.target(),
                        Duration::from_secs(60),
                    ) {
                        if should_emit_rate_limited(
                            last_stall_warn_at,
                            now,
                            Duration::from_secs(60),
                        ) {
                            warn!(
                                run_id = %indexer_for_progress.run_id(),
                                current = current_block,
                                target = progress.target(),
                                blocks_remaining = progress.blocks_remaining(),
                                stalled_seconds = stalled_for.as_secs(),
                                bps = format!("{:.1}", bps),
                                ema_bps = format!("{:.1}", ema_rate),
                                db_stage_write_ms = ?(perf_db_stage_ms > 0.0).then_some(format!("{:.1}", perf_db_stage_ms)),
                                db_commit_ms = ?(perf_db_commit_ms > 0.0).then_some(format!("{:.1}", perf_db_commit_ms)),
                                pipeline_fetch_ms = ?pipeline_log.as_ref().and_then(|p| p.fetch_ms).map(|v| format!("{:.1}", v)),
                                pipeline_parse_ms = ?pipeline_log.as_ref().and_then(|p| p.parse_ms).map(|v| format!("{:.1}", v)),
                                pipeline_write_stage_ms = ?pipeline_log.as_ref().and_then(|p| p.write_ms).map(|v| format!("{:.1}", v)),
                                pipeline_write_commit_ms = ?pipeline_log.as_ref().and_then(|p| p.commit_ms).map(|v| format!("{:.1}", v)),
                                pipeline_writer_wait_ms = ?pipeline_log.as_ref().and_then(|p| p.writer_wait_ms).map(|v| format!("{:.1}", v)),
                                fetch_queue_fill_pct = ?fetch_fill_pct.map(|v| format!("{:.1}", v)),
                                parse_queue_fill_pct = ?parse_fill_pct.map(|v| format!("{:.1}", v)),
                                writer_queue_fill_pct = ?writer_fill_pct.map(|v| format!("{:.1}", v)),
                                suppressed_since_last = suppressed_stall_warns,
                                "Sync progress stalled"
                            );
                            last_stall_warn_at = Some(now);
                            suppressed_stall_warns = 0;
                        } else {
                            suppressed_stall_warns = suppressed_stall_warns.saturating_add(1);
                        }
                    }
                }
                Some(_) => {}
            }

            if indexer_for_progress.is_bulk_sync_active() {
                info!(
                    run_id = %indexer_for_progress.run_id(),
                    source = data_source,
                    progress_pct = format!("{:.2}", progress.progress_percentage()),
                    current = progress.current(),
                    target = progress.target(),
                    bps = format!("{:.1}", bps),
                    ema_bps = format!("{:.1}", ema_rate),
                    eta = %eta,
                    db_stage_write_ms = ?(perf_db_stage_ms > 0.0).then_some(format!("{:.1}", perf_db_stage_ms)),
                    db_commit_ms = ?(perf_db_commit_ms > 0.0).then_some(format!("{:.1}", perf_db_commit_ms)),
                    pipeline_fetch_ms = ?pipeline_log.as_ref().and_then(|p| p.fetch_ms).map(|v| format!("{:.1}", v)),
                    pipeline_parse_ms = ?pipeline_log.as_ref().and_then(|p| p.parse_ms).map(|v| format!("{:.1}", v)),
                    pipeline_write_stage_ms = ?pipeline_log.as_ref().and_then(|p| p.write_ms).map(|v| format!("{:.1}", v)),
                    pipeline_write_commit_ms = ?pipeline_log.as_ref().and_then(|p| p.commit_ms).map(|v| format!("{:.1}", v)),
                    pipeline_writer_wait_ms = ?pipeline_log.as_ref().and_then(|p| p.writer_wait_ms).map(|v| format!("{:.1}", v)),
                    fetch_queue_fill_pct = ?fetch_fill_pct.map(|v| format!("{:.1}", v)),
                    parse_queue_fill_pct = ?parse_fill_pct.map(|v| format!("{:.1}", v)),
                    writer_queue_fill_pct = ?writer_fill_pct.map(|v| format!("{:.1}", v)),
                    "Bulk sync progress"
                );
                if parse_fill_pct.is_some_and(|p| p >= 80.0)
                    || writer_fill_pct.is_some_and(|p| p >= 80.0)
                {
                    let now = Instant::now();
                    if should_emit_rate_limited(
                        last_queue_pressure_warn_at,
                        now,
                        Duration::from_secs(60),
                    ) {
                        warn!(
                            run_id = %indexer_for_progress.run_id(),
                            parse_queue_fill_pct = ?parse_fill_pct.map(|v| format!("{:.1}", v)),
                            writer_queue_fill_pct = ?writer_fill_pct.map(|v| format!("{:.1}", v)),
                            suppressed_since_last = suppressed_queue_pressure_warns,
                            "Pipeline queue pressure high"
                        );
                        last_queue_pressure_warn_at = Some(now);
                        suppressed_queue_pressure_warns = 0;
                    } else {
                        suppressed_queue_pressure_warns =
                            suppressed_queue_pressure_warns.saturating_add(1);
                    }
                } else if let Some(last_warn_at) = last_queue_pressure_warn_at.take() {
                    info!(
                        run_id = %indexer_for_progress.run_id(),
                        seconds_since_last_warn = last_warn_at.elapsed().as_secs(),
                        suppressed_since_last = suppressed_queue_pressure_warns,
                        "Pipeline queue pressure normalized"
                    );
                    suppressed_queue_pressure_warns = 0;
                }
            } else {
                info!(
                    run_id = %indexer_for_progress.run_id(),
                    source = data_source,
                    bps = format!("{:.1}", bps),
                    ema_bps = format!("{:.1}", ema_rate),
                    eta = %eta,
                    db_stage_write_ms = ?(perf_db_stage_ms > 0.0).then_some(format!("{:.1}", perf_db_stage_ms)),
                    db_commit_ms = ?(perf_db_commit_ms > 0.0).then_some(format!("{:.1}", perf_db_commit_ms)),
                    pipeline_writer_wait_ms = ?pipeline_log.as_ref().and_then(|p| p.writer_wait_ms).map(|v| format!("{:.1}", v)),
                    parse_queue_fill_pct = ?parse_fill_pct.map(|v| format!("{:.1}", v)),
                    "[{}] Synced to block {} (tip: {}, {} behind)",
                    data_source,
                    progress.current(),
                    progress.target(),
                    progress.blocks_remaining()
                );
                if let Some(last_warn_at) = last_queue_pressure_warn_at.take() {
                    info!(
                        run_id = %indexer_for_progress.run_id(),
                        seconds_since_last_warn = last_warn_at.elapsed().as_secs(),
                        suppressed_since_last = suppressed_queue_pressure_warns,
                        "Pipeline queue pressure normalized"
                    );
                    suppressed_queue_pressure_warns = 0;
                }
            }
        }
    });

    let indexer_for_shutdown = Arc::clone(&indexer);
    tokio::spawn(async move {
        match wait_for_shutdown_signal().await {
            Ok(reason) => {
                info!(
                    reason,
                    "Received shutdown signal, requesting graceful shutdown..."
                );
                indexer_for_shutdown
                    .shutdown_flag()
                    .store(true, Ordering::SeqCst);
            }
            Err(e) => {
                tracing::error!("Failed to listen for shutdown signal: {}", e);
            }
        }
    });

    let run_result = indexer.run().await;
    match &run_result {
        Ok(_) => {
            indexer.finalize_bulk_sync_perf_failed();
            indexer.mark_runtime_shutdown("run_completed", 0);
        }
        Err(e) => {
            tracing::error!("Indexer terminated with error: {}", e);
            indexer.finalize_bulk_sync_perf_failed();
            indexer.mark_runtime_shutdown("run_error", 1);
        }
    }
    run_result
}

/// Run label import from a service config.
pub async fn run_label_import(config: LabelImportServiceConfig) -> Result<()> {
    info!(
        "Opening ckbadger domain store at: {}",
        config.domain_data_path
    );
    let core_store = Arc::new(CkbadgerStore::open_domain_with_runtime(
        &config.domain_data_path,
        config.store_runtime_config,
    )?);

    let network = config.network.clone();
    let use_bundled = config.use_bundled;

    let result = if use_bundled {
        info!("Using bundled label data (no filesystem override found)");
        tokio::task::spawn_blocking(move || {
            crate::label_import::run_label_import_bundled(core_store.as_ref(), &network)
        })
        .await
        .expect("label import task panicked")?
    } else {
        let base_config = LabelImportConfig {
            token_labels_path: config.token_labels_path,
            network,
            import_udt: config.import_udt,
            import_scripts: config.import_scripts,
        };
        tokio::task::spawn_blocking(move || {
            run_label_import_staged(core_store.as_ref(), &base_config)
        })
        .await
        .expect("label import task panicked")?
    };

    info!(
        "Label import completed: {} UDT, {} scripts, {} errors",
        result.udt_labels_imported,
        result.script_labels_imported,
        result.errors.len()
    );

    Ok(())
}

// --- Helper functions (moved from main.rs) ---

fn queue_fill_pct(depth: Option<u64>, capacity: Option<u64>) -> Option<f64> {
    match (depth, capacity) {
        (Some(d), Some(c)) if c > 0 => Some((d as f64 / c as f64) * 100.0),
        _ => None,
    }
}

fn should_emit_rate_limited(
    last_emit_at: Option<Instant>,
    now: Instant,
    min_emit_interval: Duration,
) -> bool {
    match last_emit_at {
        None => true,
        Some(last) => now.duration_since(last) >= min_emit_interval,
    }
}

fn should_warn_progress_stall(
    last_progress_advanced_at: Instant,
    now: Instant,
    current_block: u64,
    target_block: u64,
    min_stall_duration: Duration,
) -> bool {
    current_block < target_block
        && now.duration_since(last_progress_advanced_at) >= min_stall_duration
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncStatusRepairReason {
    TipNumberDrift,
    TipHashDrift,
    GenesisHashResidue,
}

fn sync_status_repair_reason(
    sync_status: &SyncStatus,
    db_tip: i64,
    db_tip_hash: Option<&[u8]>,
) -> Option<SyncStatusRepairReason> {
    if sync_status.tip_block_number != db_tip {
        return Some(SyncStatusRepairReason::TipNumberDrift);
    }

    if db_tip == 0 && db_tip_hash.is_none() && !sync_status.tip_block_hash.is_empty() {
        return Some(SyncStatusRepairReason::GenesisHashResidue);
    }

    if db_tip > 0 {
        if let Some(hash) = db_tip_hash {
            if sync_status.tip_block_hash != hash {
                return Some(SyncStatusRepairReason::TipHashDrift);
            }
        }
    }

    None
}

fn is_clean_shutdown_reason(reason: &str) -> bool {
    matches!(
        reason,
        "graceful_shutdown" | "sigterm_shutdown" | "run_completed"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartupCleanupDecision {
    force_cleanup: bool,
    reason: &'static str,
}

fn startup_cleanup_decision(
    previous_runtime: &RuntimeStatus,
    rollback_cleanup_in_progress: bool,
) -> StartupCleanupDecision {
    if rollback_cleanup_in_progress {
        return StartupCleanupDecision {
            force_cleanup: true,
            reason: "rollback_cleanup_in_progress",
        };
    }

    let Some(active_run_id) = previous_runtime.active_run_id.as_deref() else {
        let incident_requires_cleanup = previous_runtime.last_incident_summary.as_deref()
            == Some("pipeline_batch_write_failed")
            && previous_runtime.last_incident_at >= previous_runtime.run_started_at;
        return if incident_requires_cleanup {
            StartupCleanupDecision {
                force_cleanup: true,
                reason: "last_incident_pipeline_batch_write_failed",
            }
        } else {
            StartupCleanupDecision {
                force_cleanup: false,
                reason: "no_force_cleanup_signal",
            }
        };
    };
    let clean_shutdown_marker_for_same_run = previous_runtime.last_run_id.as_deref()
        == Some(active_run_id)
        && previous_runtime.last_exit_code == Some(0)
        && previous_runtime
            .last_shutdown_reason
            .as_deref()
            .is_some_and(is_clean_shutdown_reason)
        && previous_runtime.last_shutdown_at >= previous_runtime.run_started_at;
    if clean_shutdown_marker_for_same_run {
        StartupCleanupDecision {
            force_cleanup: false,
            reason: "active_run_has_clean_shutdown_marker",
        }
    } else {
        StartupCleanupDecision {
            force_cleanup: true,
            reason: "active_run_missing_clean_shutdown_marker",
        }
    }
}

fn should_force_startup_cleanup(
    previous_runtime: &RuntimeStatus,
    rollback_cleanup_in_progress: bool,
) -> bool {
    startup_cleanup_decision(previous_runtime, rollback_cleanup_in_progress).force_cleanup
}

fn monotonic_counter_delta(current: Option<u64>, previous: Option<u64>) -> Option<u64> {
    match (current, previous) {
        (Some(curr), Some(prev)) if curr >= prev => Some(curr - prev),
        _ => None,
    }
}

fn classify_unclean_shutdown_hint(
    previous_runtime: &RuntimeStatus,
    oom_kill_delta: Option<u64>,
) -> &'static str {
    if oom_kill_delta.is_some_and(|d| d > 0) {
        return "cgroup_oom_kill";
    }

    let has_known_incident = previous_runtime.last_incident_summary.is_some();
    if has_known_incident {
        return "application_incident_before_unclean_exit";
    }

    "external_sigkill_or_host_oom_or_abort"
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

fn reconcile_token_daily_deltas_on_startup(store: &CkbadgerStore) -> Result<()> {
    let Some(invalid) = store.find_first_invalid_token_daily_delta()? else {
        return Ok(());
    };

    let type_hash_hex = bytes_to_hex(&invalid.type_hash);
    warn!(
        type_hash = %type_hash_hex,
        date = invalid.date_yyyymmdd,
        owned_capacity = invalid.owned_capacity,
        owned_knowledge = invalid.owned_knowledge,
        capacity_delta = invalid.capacity_delta,
        used_delta = invalid.used_delta,
        "Detected invalid token daily deltas at startup; fail-fast without automatic rebuild"
    );

    anyhow::bail!(
        "invalid token daily deltas detected at startup: type_hash=0x{}, date={}, owned_capacity={}, owned_knowledge={}, capacity_delta={}, used_delta={}; automatic rebuild is disabled, delete RocksDB and re-sync from genesis",
        type_hash_hex,
        invalid.date_yyyymmdd,
        invalid.owned_capacity,
        invalid.owned_knowledge,
        invalid.capacity_delta,
        invalid.used_delta
    );
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() -> std::io::Result<&'static str> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate())?;
    tokio::select! {
        ctrl = tokio::signal::ctrl_c() => {
            ctrl?;
            Ok("graceful_shutdown")
        }
        _ = sigterm.recv() => Ok("sigterm_shutdown"),
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() -> std::io::Result<&'static str> {
    tokio::signal::ctrl_c().await?;
    Ok("graceful_shutdown")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_fill_pct() {
        assert_eq!(queue_fill_pct(Some(5), Some(10)), Some(50.0));
        assert_eq!(queue_fill_pct(Some(1), Some(0)), None);
        assert_eq!(queue_fill_pct(None, Some(10)), None);
        assert_eq!(queue_fill_pct(Some(1), None), None);
    }

    #[test]
    fn test_should_emit_rate_limited() {
        let now = Instant::now();
        assert!(should_emit_rate_limited(None, now, Duration::from_secs(60)));
        assert!(!should_emit_rate_limited(
            Some(now),
            now + Duration::from_secs(10),
            Duration::from_secs(60)
        ));
        assert!(should_emit_rate_limited(
            Some(now),
            now + Duration::from_secs(60),
            Duration::from_secs(60)
        ));
    }

    #[test]
    fn test_should_warn_progress_stall() {
        let now = Instant::now();
        let last_advanced = now - Duration::from_secs(75);
        assert!(should_warn_progress_stall(
            last_advanced,
            now,
            100,
            200,
            Duration::from_secs(60)
        ));
        assert!(!should_warn_progress_stall(
            now - Duration::from_secs(30),
            now,
            100,
            200,
            Duration::from_secs(60)
        ));
        assert!(!should_warn_progress_stall(
            last_advanced,
            now,
            200,
            200,
            Duration::from_secs(60)
        ));
    }

    #[test]
    fn test_monotonic_counter_delta() {
        assert_eq!(monotonic_counter_delta(Some(10), Some(7)), Some(3));
        assert_eq!(monotonic_counter_delta(Some(7), Some(10)), None);
        assert_eq!(monotonic_counter_delta(Some(7), None), None);
        assert_eq!(monotonic_counter_delta(None, Some(7)), None);
    }

    #[test]
    fn test_sync_status_repair_reason_for_tip_hash_mismatch_same_height() {
        let sync_status = SyncStatus {
            tip_block_number: 128,
            tip_block_hash: vec![0x11; 32],
            ..Default::default()
        };
        let db_hash = vec![0x22; 32];
        assert_eq!(
            sync_status_repair_reason(&sync_status, 128, Some(&db_hash)),
            Some(SyncStatusRepairReason::TipHashDrift)
        );
    }

    #[test]
    fn test_sync_status_repair_reason_for_genesis_hash_mismatch() {
        let sync_status = SyncStatus {
            tip_block_number: 0,
            tip_block_hash: vec![0x33; 32],
            ..Default::default()
        };
        assert_eq!(
            sync_status_repair_reason(&sync_status, 0, None),
            Some(SyncStatusRepairReason::GenesisHashResidue)
        );
    }

    #[test]
    fn test_sync_status_repair_reason_none_when_tip_and_hash_match() {
        let db_hash = vec![0x44; 32];
        let sync_status = SyncStatus {
            tip_block_number: 42,
            tip_block_hash: db_hash.clone(),
            ..Default::default()
        };
        assert_eq!(
            sync_status_repair_reason(&sync_status, 42, Some(&db_hash)),
            None
        );
    }

    #[test]
    fn test_classify_unclean_shutdown_hint_prefers_oom_kill_delta() {
        let runtime = RuntimeStatus::default();
        assert_eq!(
            classify_unclean_shutdown_hint(&runtime, Some(1)),
            "cgroup_oom_kill"
        );
    }

    #[test]
    fn test_classify_unclean_shutdown_hint_for_external_kill_like_case() {
        let runtime = RuntimeStatus::default();
        assert_eq!(
            classify_unclean_shutdown_hint(&runtime, Some(0)),
            "external_sigkill_or_host_oom_or_abort"
        );
    }

    #[test]
    fn test_classify_unclean_shutdown_hint_for_known_incident() {
        let runtime = RuntimeStatus {
            last_incident_summary: Some("pipeline_batch_write_failed".to_string()),
            ..Default::default()
        };
        assert_eq!(
            classify_unclean_shutdown_hint(&runtime, Some(0)),
            "application_incident_before_unclean_exit"
        );
    }

    #[test]
    fn test_should_force_startup_cleanup_when_active_run_exists() {
        let runtime = RuntimeStatus {
            active_run_id: Some("run-xyz".to_string()),
            ..Default::default()
        };
        assert!(should_force_startup_cleanup(&runtime, false));
    }

    #[test]
    fn test_should_not_force_startup_cleanup_when_no_active_run() {
        let runtime = RuntimeStatus::default();
        assert!(!should_force_startup_cleanup(&runtime, false));
    }

    #[test]
    fn test_should_not_force_startup_cleanup_with_clean_shutdown_marker_for_same_run() {
        let runtime = RuntimeStatus {
            active_run_id: Some("run-xyz".to_string()),
            last_run_id: Some("run-xyz".to_string()),
            run_started_at: 100,
            last_shutdown_at: 101,
            last_shutdown_reason: Some("sigterm_shutdown".to_string()),
            last_exit_code: Some(0),
            ..Default::default()
        };
        assert!(!should_force_startup_cleanup(&runtime, false));
    }

    #[test]
    fn test_should_force_startup_cleanup_when_shutdown_marker_is_stale() {
        let runtime = RuntimeStatus {
            active_run_id: Some("run-xyz".to_string()),
            last_run_id: Some("run-xyz".to_string()),
            run_started_at: 200,
            last_shutdown_at: 100,
            last_shutdown_reason: Some("sigterm_shutdown".to_string()),
            last_exit_code: Some(0),
            ..Default::default()
        };
        assert!(should_force_startup_cleanup(&runtime, false));
    }

    #[test]
    fn test_should_force_startup_cleanup_when_rollback_marker_set() {
        let runtime = RuntimeStatus::default();
        assert!(should_force_startup_cleanup(&runtime, true));
    }

    #[test]
    fn test_startup_cleanup_decision_reason_for_rollback_marker() {
        let runtime = RuntimeStatus::default();
        let decision = startup_cleanup_decision(&runtime, true);
        assert!(decision.force_cleanup);
        assert_eq!(decision.reason, "rollback_cleanup_in_progress");
    }

    #[test]
    fn test_startup_cleanup_decision_reason_for_clean_active_run_marker() {
        let runtime = RuntimeStatus {
            active_run_id: Some("run-xyz".to_string()),
            last_run_id: Some("run-xyz".to_string()),
            run_started_at: 100,
            last_shutdown_at: 101,
            last_shutdown_reason: Some("sigterm_shutdown".to_string()),
            last_exit_code: Some(0),
            ..Default::default()
        };
        let decision = startup_cleanup_decision(&runtime, false);
        assert!(!decision.force_cleanup);
        assert_eq!(decision.reason, "active_run_has_clean_shutdown_marker");
    }

    #[test]
    fn test_startup_cleanup_decision_reason_for_batch_write_incident() {
        let runtime = RuntimeStatus {
            run_started_at: 100,
            last_incident_at: 101,
            last_incident_summary: Some("pipeline_batch_write_failed".to_string()),
            ..Default::default()
        };
        let decision = startup_cleanup_decision(&runtime, false);
        assert!(decision.force_cleanup);
        assert_eq!(decision.reason, "last_incident_pipeline_batch_write_failed");
    }

    #[test]
    fn test_should_force_startup_cleanup_when_previous_run_had_batch_write_incident() {
        let runtime = RuntimeStatus {
            run_started_at: 100,
            last_incident_at: 101,
            last_incident_summary: Some("pipeline_batch_write_failed".to_string()),
            ..Default::default()
        };
        assert!(should_force_startup_cleanup(&runtime, false));
    }

    #[test]
    fn test_should_not_force_startup_cleanup_for_old_incident_before_run_start() {
        let runtime = RuntimeStatus {
            run_started_at: 200,
            last_incident_at: 100,
            last_incident_summary: Some("pipeline_batch_write_failed".to_string()),
            ..Default::default()
        };
        assert!(!should_force_startup_cleanup(&runtime, false));
    }

    #[test]
    fn test_reconcile_token_daily_deltas_on_startup_fails_on_invalid_rows() {
        use ckbadger_store::{
            types::{CachedBlockHeader, LiveCellInfo, TokenDailyDelta},
            StoreBatch,
        };

        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let type_hash = vec![0xAB; 32];
        let day1_ts = 1_704_067_200_000i64; // 2024-01-01T00:00:00Z
        let day2_ts = 1_704_153_600_000i64; // 2024-01-02T00:00:00Z
        let day1 = ckbadger_store::keys::timestamp_ms_to_date(day1_ts);
        let day2 = ckbadger_store::keys::timestamp_ms_to_date(day2_ts);

        let header1 = CachedBlockHeader {
            hash: vec![0x01; 32],
            timestamp: day1_ts,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let header2 = CachedBlockHeader {
            hash: vec![0x02; 32],
            timestamp: day2_ts,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
        };
        let live_cell = LiveCellInfo {
            capacity: 1_000,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x11; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x22; 32]),
            data_size: 16,
            occupied_capacity: 600,
            udt_amount: Some(1),
            data_hash: None,
        };
        let consumed_cell = LiveCellInfo {
            capacity: 400,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x11; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x22; 32]),
            data_size: 16,
            occupied_capacity: 300,
            udt_amount: Some(1),
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&[0x10; 32], 0, &live_cell, 1);
        batch.put_consumed_cell(&[0x20; 32], 0, &consumed_cell, 1, 2);
        batch.commit().unwrap();

        store
            .put_token_daily_delta(
                &type_hash,
                day1,
                &TokenDailyDelta {
                    owned_capacity_delta: 100,
                    owned_knowledge_delta: 200,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_hash,
                day2,
                &TokenDailyDelta {
                    owned_capacity_delta: 50,
                    owned_knowledge_delta: 50,
                },
            )
            .unwrap();
        assert!(store
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .is_some());

        let err = reconcile_token_daily_deltas_on_startup(&store).unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("invalid token daily deltas detected at startup"));
        assert!(err_msg.contains("delete RocksDB and re-sync from genesis"));

        let day1_delta = store
            .get_token_daily_delta(&type_hash, day1)
            .unwrap()
            .expect("missing day1 delta");
        let day2_delta = store
            .get_token_daily_delta(&type_hash, day2)
            .unwrap()
            .expect("missing day2 delta");
        assert_eq!(day1_delta.owned_capacity_delta, 100);
        assert_eq!(day1_delta.owned_knowledge_delta, 200);
        assert_eq!(day2_delta.owned_capacity_delta, 50);
        assert_eq!(day2_delta.owned_knowledge_delta, 50);
        assert!(store
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_indexer_service_config_converts_to_config() {
        let svc = IndexerServiceConfig {
            domain_data_path: "/data/domain".to_string(),
            append_only_data_path: "/data/append".to_string(),
            bulk_sync_perf_output_root: "/workdir/perf/bulk-sync".to_string(),
            build_version: "0.1.0+feature/foo@abcdef123456".to_string(),
            ckb_rpc_url: "http://localhost:8114".to_string(),
            ckb_db_path: "/ckb/data/db".to_string(),
            token_labels_path: "docs/labels".to_string(),
            network: "mainnet".to_string(),
            batch_size: 5000,
            poll_interval_ms: 500,
            parallel_fetch_size: 32,
            pipeline_buffer: 4,
            bulk_sync_threshold: 100,
            store_runtime_config: StoreRuntimeConfig {
                memory_budget_gb: Some(24),
                direct_io_reads: false,
            },
        };

        let config: Config = svc.into();
        assert_eq!(config.domain_data_path, "/data/domain");
        assert_eq!(config.append_only_data_path, "/data/append");
        assert_eq!(config.bulk_sync_perf_output_root, "/workdir/perf/bulk-sync");
        assert_eq!(config.build_version, "0.1.0+feature/foo@abcdef123456");
        assert_eq!(config.ckb_rpc_url, "http://localhost:8114");
        assert_eq!(config.ckb_db_path, "/ckb/data/db");
        assert_eq!(config.token_labels_path, "docs/labels");
        assert_eq!(config.network, "mainnet");
        assert_eq!(config.batch_size, 5000);
        assert_eq!(config.poll_interval_ms, 500);
        assert_eq!(config.parallel_fetch_size, 32);
        assert_eq!(config.pipeline_buffer, 4);
        assert_eq!(config.bulk_sync_threshold, 100);
        assert!(config.fast_sync_mode);
        assert!(!config.force_startup_cleanup);
        assert!(config.start_block.is_none());
        assert_eq!(config.store_runtime_config.memory_budget_gb, Some(24));
        assert!(!config.store_runtime_config.direct_io_reads);
    }
}
