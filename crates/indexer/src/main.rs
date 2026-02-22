use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckb_store_reader::CkbChainReader;
use ckbadger_common::LabelImportConfig;
use ckbadger_indexer::{
    db::Repository, label_import::run_label_import, runtime_diag::generate_run_id,
    runtime_diag::read_cgroup_memory_snapshot, sync::Indexer, verify, Config,
};
use ckbadger_store::CkbadgerStore;

#[derive(Parser, Debug)]
#[command(name = "ckbadger-indexer")]
#[command(about = "CKB blockchain indexer for ckbadger explorer")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    // ---- Sync args (used when no subcommand or `sync` subcommand) ----
    #[arg(
        long,
        env = "CKBADGER_DATA_PATH",
        default_value = "./data/ckbadger-store",
        global = true
    )]
    data_path: String,

    #[arg(long, env = "CKB_RPC_URL", global = true)]
    ckb_rpc_url: Option<String>,

    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,

    #[arg(long, default_value = "10000")]
    batch_size: usize,

    #[arg(long, default_value = "1000")]
    poll_interval_ms: u64,

    #[arg(long, default_value = "64")]
    parallel_fetch_size: usize,

    #[arg(long, default_value = "true")]
    pipeline_enabled: bool,

    #[arg(long, default_value = "8")]
    pipeline_buffer: usize,

    #[arg(
        long,
        default_value = "1000",
        help = "Blocks behind tip to exit bulk sync mode"
    )]
    bulk_sync_threshold: u64,

    #[arg(
        long,
        env = "CKB_DATA_PATH",
        help = "Path to CKB node's RocksDB data directory for direct reads (e.g., /var/lib/ckb/data/db)"
    )]
    ckb_data_path: Option<String>,

    #[arg(
        long,
        default_value = "300000",
        help = "Maximum transactions per fetcher sub-batch (splits mega-blocks)"
    )]
    max_batch_txs: usize,

    // Label import settings
    #[arg(long, env = "TOKEN_LABELS_PATH", default_value = "docs/token-labels")]
    token_labels_path: String,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the sync daemon (default behavior).
    Sync,
    /// Verify data integrity of the store.
    Verify(verify::VerifyArgs),
    /// Import UDT and script labels directly (without task system).
    LabelImport(LabelImportArgs),
}

#[derive(Args, Debug)]
struct LabelImportArgs {
    #[arg(long, env = "TOKEN_LABELS_PATH", default_value = "docs/token-labels")]
    token_labels_path: String,

    #[arg(long, default_value = "mainnet")]
    network: String,

    #[arg(long, default_value_t = true)]
    import_udt: bool,

    #[arg(long, default_value_t = true)]
    import_scripts: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    let data_path = cli.data_path.clone();
    let ckb_data_path = cli.ckb_data_path.clone();

    match cli.command {
        Some(Command::Verify(args)) => {
            // Run on a blocking thread so reqwest::blocking's internal
            // tokio runtime isn't nested inside #[tokio::main].
            tokio::task::spawn_blocking(move || verify::run(args))
                .await
                .expect("verify task panicked")?;
            Ok(())
        }
        Some(Command::LabelImport(args)) => {
            run_label_import_command(data_path, ckb_data_path, args).await
        }
        // Default (no subcommand) or explicit `sync` → run sync daemon
        None | Some(Command::Sync) => run_sync(cli).await,
    }
}

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

fn is_clean_shutdown_reason(reason: &str) -> bool {
    matches!(
        reason,
        "graceful_shutdown" | "sigterm_shutdown" | "run_completed"
    )
}

fn should_force_startup_cleanup(previous_runtime: &ckbadger_store::RuntimeStatus) -> bool {
    let Some(active_run_id) = previous_runtime.active_run_id.as_deref() else {
        return false;
    };
    let clean_shutdown_marker_for_same_run = previous_runtime.last_run_id.as_deref()
        == Some(active_run_id)
        && previous_runtime.last_exit_code == Some(0)
        && previous_runtime
            .last_shutdown_reason
            .as_deref()
            .is_some_and(is_clean_shutdown_reason)
        && previous_runtime.last_shutdown_at >= previous_runtime.run_started_at;
    !clean_shutdown_marker_for_same_run
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

async fn run_sync(args: Cli) -> Result<()> {
    let mut config = Config {
        data_path: args.data_path.clone(),
        ckb_rpc_url: args
            .ckb_rpc_url
            .or_else(|| std::env::var("CKB_RPC_URL").ok())
            .expect("CKB_RPC_URL is required"),
        batch_size: args.batch_size,
        poll_interval_ms: args.poll_interval_ms,
        start_block: None,
        confirmations: 24,
        parallel_fetch_size: args.parallel_fetch_size,
        pipeline_enabled: args.pipeline_enabled,
        pipeline_buffer: args.pipeline_buffer,
        redis_url: args.redis_url.or_else(|| std::env::var("REDIS_URL").ok()),
        bulk_sync_threshold: args.bulk_sync_threshold,
        fast_sync_mode: true,
        ckb_data_path: args.ckb_data_path,
        max_batch_txs: args.max_batch_txs,
        token_labels_path: args.token_labels_path,
        force_startup_cleanup: false,
    };

    info!("Opening ckbadger-store at: {}", config.data_path);
    let store = Arc::new(CkbadgerStore::open(&config.data_path)?);
    CkbadgerStore::log_config();

    // One-time backfill: populate code_hash indexes if they are empty
    if !store.code_hash_indexes_populated() {
        info!("Code hash indexes empty — running one-time backfill from live_cells...");
        let count = store.backfill_code_hash_indexes()?;
        info!("Code hash index backfill complete: {} cells indexed", count);
    }

    // One-time backfill: populate addr_txs index if empty
    if !store.addr_txs_populated() {
        info!("addr_txs index empty — running one-time backfill from cells...");
        let count = store.backfill_addr_txs()?;
        info!("addr_txs backfill complete: {} entries indexed", count);
    }

    // One-time backfill: rebuild avg_block_time_ms from block headers
    let mut sync_status = store.get_sync_status()?;
    let previous_runtime = store.get_runtime_status()?;
    let previous_run_unclean = should_force_startup_cleanup(&previous_runtime);
    config.force_startup_cleanup = previous_run_unclean;
    if previous_run_unclean {
        info!(
            "Previous run did not shut down cleanly; forcing startup rollback cleanup to reconcile derived state"
        );
    }
    if !sync_status.avg_block_time_rebuilt && sync_status.tip_block_number > 0 {
        info!("avg_block_time migration: rebuilding from block headers...");
        let updated = store.rebuild_avg_block_times()?;
        info!(
            "avg_block_time migration complete: {} daily stats updated",
            updated
        );
        store.update_sync_status(|s| {
            s.avg_block_time_rebuilt = true;
        })?;
        sync_status = store.get_sync_status()?;
    }

    let repo = Repository::new(store.clone());
    let (db_tip, db_tip_hash) = repo.get_sync_tip().await?;
    if sync_status.tip_block_number != db_tip {
        info!(
            "sync_status tip ({}) differs from block_headers tip ({}), using block_headers tip",
            sync_status.tip_block_number, db_tip
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
            previous_last_incident_id = ?previous_runtime.last_incident_id,
            cgroup_memory_current_bytes = startup_cgroup.memory_current_bytes,
            cgroup_memory_max_bytes = startup_cgroup.memory_max_bytes,
            cgroup_memory_max_raw = ?startup_cgroup.memory_max_raw,
            cgroup_oom_events = startup_cgroup.oom_events,
            cgroup_oom_kill_events = startup_cgroup.oom_kill_events,
            "Detected previous run without graceful shutdown marker"
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
            previous_last_incident_id = ?previous_runtime.last_incident_id,
            cgroup_memory_current_bytes = startup_cgroup.memory_current_bytes,
            cgroup_memory_max_bytes = startup_cgroup.memory_max_bytes,
            cgroup_memory_max_raw = ?startup_cgroup.memory_max_raw,
            cgroup_oom_events = startup_cgroup.oom_events,
            cgroup_oom_kill_events = startup_cgroup.oom_kill_events,
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

    let indexer = Indexer::new(run_id, config.clone(), store.clone()).await?;
    let indexer = Arc::new(indexer);

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
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

            let progress = indexer_for_progress.progress();
            let ema_rate = progress.ema_blocks_per_second();
            let eta = progress.eta_formatted();
            let bps = progress.blocks_per_second();

            let (perf_rpc_ms, perf_db_ms) = indexer_for_progress.perf_snapshot_ms();
            let pipeline = indexer_for_progress.pipeline_progress_snapshot();
            let pipeline_log = pipeline.clone();
            indexer_for_progress.record_runtime_heartbeat(progress.current());
            let sync_data = ckbadger_common::SyncProgressData {
                current_block: progress.current(),
                target_block: progress.target(),
                blocks_per_second: bps,
                ema_blocks_per_second: ema_rate,
                eta_seconds: progress.eta_seconds(),
                eta_formatted: eta.clone(),
                progress_percentage: progress.progress_percentage(),
                updated_at: chrono::Utc::now().timestamp(),
                startup_phase: indexer_for_progress.startup_phase(),
                is_direct_db_read: data_source == "DB",
                db_write_ms: if perf_db_ms > 0.0 {
                    Some(perf_db_ms)
                } else {
                    None
                },
                rpc_fetch_ms: if perf_rpc_ms > 0.0 {
                    Some(perf_rpc_ms)
                } else {
                    None
                },
                pipeline,
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
                                pipeline_fetch_ms = ?pipeline_log.as_ref().and_then(|p| p.fetch_ms).map(|v| format!("{:.1}", v)),
                                pipeline_parse_ms = ?pipeline_log.as_ref().and_then(|p| p.parse_ms).map(|v| format!("{:.1}", v)),
                                pipeline_write_ms = ?pipeline_log.as_ref().and_then(|p| p.write_ms).map(|v| format!("{:.1}", v)),
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
                    pipeline_fetch_ms = ?pipeline_log.as_ref().and_then(|p| p.fetch_ms).map(|v| format!("{:.1}", v)),
                    pipeline_parse_ms = ?pipeline_log.as_ref().and_then(|p| p.parse_ms).map(|v| format!("{:.1}", v)),
                    pipeline_write_ms = ?pipeline_log.as_ref().and_then(|p| p.write_ms).map(|v| format!("{:.1}", v)),
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
                    "Received shutdown signal, shutting down gracefully..."
                );
                indexer_for_shutdown.mark_runtime_shutdown(reason, 0);
                std::process::exit(0);
            }
            Err(e) => {
                tracing::error!("Failed to listen for shutdown signal: {}", e);
            }
        }
    });

    let run_result = indexer.run().await;
    match &run_result {
        Ok(_) => indexer.mark_runtime_shutdown("run_completed", 0),
        Err(e) => {
            tracing::error!("Indexer terminated with error: {}", e);
            indexer.mark_runtime_shutdown("run_error", 1);
        }
    }
    run_result
}

async fn run_label_import_command(
    data_path: String,
    ckb_data_path: Option<String>,
    args: LabelImportArgs,
) -> Result<()> {
    info!("Opening ckbadger-store at: {}", data_path);
    let store = Arc::new(CkbadgerStore::open(&data_path)?);

    let ckb_store = match ckb_data_path.as_deref() {
        Some(path) => {
            let reader = CkbChainReader::open(path)?;
            info!("CKB direct RocksDB reader opened at {}", path);
            Some(Arc::new(reader))
        }
        None => None,
    };

    let config = LabelImportConfig {
        token_labels_path: args.token_labels_path,
        network: args.network,
        import_udt: args.import_udt,
        import_scripts: args.import_scripts,
    };

    let result = tokio::task::spawn_blocking(move || {
        run_label_import(store.as_ref(), ckb_store.as_deref(), &config)
    })
    .await
    .expect("label import task panicked")?;

    info!(
        "Label import completed: {} UDT, {} scripts, {} errors",
        result.udt_labels_imported,
        result.script_labels_imported,
        result.errors.len()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        queue_fill_pct, should_emit_rate_limited, should_force_startup_cleanup,
        should_warn_progress_stall,
    };
    use ckbadger_store::RuntimeStatus;
    use std::time::{Duration, Instant};

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
    fn test_should_force_startup_cleanup_when_active_run_exists() {
        let runtime = RuntimeStatus {
            active_run_id: Some("run-xyz".to_string()),
            ..Default::default()
        };
        assert!(should_force_startup_cleanup(&runtime));
    }

    #[test]
    fn test_should_not_force_startup_cleanup_when_no_active_run() {
        let runtime = RuntimeStatus::default();
        assert!(!should_force_startup_cleanup(&runtime));
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
        assert!(!should_force_startup_cleanup(&runtime));
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
        assert!(should_force_startup_cleanup(&runtime));
    }
}
