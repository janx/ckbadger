use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckb_store_reader::CkbChainReader;
use ckbadger_common::LabelImportConfig;
use ckbadger_indexer::{
    cycles_worker::spawn_cycles_task_worker, db::Repository, label_import::run_label_import,
    runtime_diag::generate_run_id, runtime_diag::read_cgroup_memory_snapshot, sync::Indexer,
    verify, Config,
};
use ckbadger_store::{CkbadgerStore, RuntimeStatus};

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

fn should_force_startup_cleanup(
    previous_runtime: &ckbadger_store::RuntimeStatus,
    rollback_cleanup_in_progress: bool,
) -> bool {
    if rollback_cleanup_in_progress {
        return true;
    }

    let Some(active_run_id) = previous_runtime.active_run_id.as_deref() else {
        return previous_runtime.last_incident_summary.as_deref()
            == Some("pipeline_batch_write_failed")
            && previous_runtime.last_incident_at >= previous_runtime.run_started_at;
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
    if oom_kill_delta.is_some_and(|delta| delta > 0) {
        "cgroup_oom_kill"
    } else if previous_runtime.last_incident_summary.is_some() {
        "application_incident_before_unclean_exit"
    } else if previous_runtime.last_shutdown_reason.is_none()
        && previous_runtime.last_exit_code.is_none()
    {
        "external_sigkill_or_host_oom_or_abort"
    } else {
        "unknown_unclean_exit"
    }
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
        live_capacity = invalid.live_capacity,
        live_occupied_capacity = invalid.live_occupied_capacity,
        capacity_delta = invalid.capacity_delta,
        occupied_delta = invalid.occupied_delta,
        "Detected invalid token daily deltas; rebuilding from cells"
    );

    let rebuilt = store.rebuild_token_daily_deltas_from_cells()?;
    info!(
        token_daily_cleared = rebuilt.token_daily_cleared,
        token_daily_written = rebuilt.token_daily_written,
        live_cells_scanned = rebuilt.live_cells_scanned,
        consumed_cells_scanned = rebuilt.consumed_cells_scanned,
        "Startup token daily delta rebuild complete"
    );

    if let Some(still_invalid) = store.find_first_invalid_token_daily_delta()? {
        anyhow::bail!(
            "token daily delta rebuild failed validation: type_hash=0x{}, date={}, live_capacity={}, live_occupied_capacity={}, capacity_delta={}, occupied_delta={}",
            bytes_to_hex(&still_invalid.type_hash),
            still_invalid.date_yyyymmdd,
            still_invalid.live_capacity,
            still_invalid.live_occupied_capacity,
            still_invalid.capacity_delta,
            still_invalid.occupied_delta
        );
    }

    info!("Startup token daily deltas validation passed after rebuild");
    Ok(())
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
    let rollback_cleanup_in_progress = store.is_rollback_cleanup_in_progress()?;
    let previous_run_unclean =
        should_force_startup_cleanup(&previous_runtime, rollback_cleanup_in_progress);
    config.force_startup_cleanup = previous_run_unclean;
    if previous_run_unclean {
        info!(
            rollback_cleanup_in_progress,
            "Forcing startup rollback cleanup to reconcile derived state"
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

    reconcile_token_daily_deltas_on_startup(&store)?;

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

    let indexer = Indexer::new(run_id, config.clone(), store.clone()).await?;
    let indexer = Arc::new(indexer);

    spawn_cycles_task_worker(
        store.clone(),
        config.ckb_rpc_url.clone(),
        config.redis_url.clone(),
    );

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
            let ema_tx_rate = progress.ema_txs_per_second();
            let eta = progress.eta_formatted();
            let bps = progress.blocks_per_second();
            let txps = progress.txs_per_second();
            let last_batch_blocks = progress.last_batch_blocks();

            let (perf_rpc_ms, perf_db_ms) = indexer_for_progress.perf_snapshot_ms();
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
        classify_unclean_shutdown_hint, monotonic_counter_delta, queue_fill_pct,
        reconcile_token_daily_deltas_on_startup, should_emit_rate_limited,
        should_force_startup_cleanup, should_warn_progress_stall,
    };
    use ckbadger_store::{
        types::{CachedBlockHeader, LiveCellInfo, TokenDailyDelta},
        CkbadgerStore, RuntimeStatus, StoreBatch,
    };
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
    fn test_monotonic_counter_delta() {
        assert_eq!(monotonic_counter_delta(Some(10), Some(7)), Some(3));
        assert_eq!(monotonic_counter_delta(Some(7), Some(10)), None);
        assert_eq!(monotonic_counter_delta(Some(7), None), None);
        assert_eq!(monotonic_counter_delta(None, Some(7)), None);
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
    fn test_reconcile_token_daily_deltas_on_startup_rebuilds_invalid_rows() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
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
            created_at_block: 1,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x11; 32]),
            type_args: Some(vec![0x22; 32]),
            data_size: 16,
            occupied_capacity: 600,
            udt_amount: Some(1),
        };
        let consumed_cell = LiveCellInfo {
            capacity: 400,
            created_at_block: 1,
            lock_script_hash: vec![0xBB; 32],
            lock_code_hash: vec![0x33; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(vec![0x11; 32]),
            type_args: Some(vec![0x22; 32]),
            data_size: 16,
            occupied_capacity: 300,
            udt_amount: Some(1),
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(1, &header1);
        batch.put_block_header(2, &header2);
        batch.put_cell(&[0x10; 32], 0, &live_cell);
        batch.put_consumed_cell(&[0x20; 32], 0, &consumed_cell, 2);
        batch.commit().unwrap();

        store
            .put_token_daily_delta(
                &type_hash,
                day1,
                &TokenDailyDelta {
                    live_capacity_delta: 100,
                    live_occupied_capacity_delta: 200,
                },
            )
            .unwrap();
        store
            .put_token_daily_delta(
                &type_hash,
                day2,
                &TokenDailyDelta {
                    live_capacity_delta: 50,
                    live_occupied_capacity_delta: 50,
                },
            )
            .unwrap();
        assert!(store
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .is_some());

        reconcile_token_daily_deltas_on_startup(&store).unwrap();

        let day1_delta = store
            .get_token_daily_delta(&type_hash, day1)
            .unwrap()
            .expect("missing day1 delta");
        let day2_delta = store
            .get_token_daily_delta(&type_hash, day2)
            .unwrap()
            .expect("missing day2 delta");
        assert_eq!(day1_delta.live_capacity_delta, 1_400);
        assert_eq!(day1_delta.live_occupied_capacity_delta, 900);
        assert_eq!(day2_delta.live_capacity_delta, -400);
        assert_eq!(day2_delta.live_occupied_capacity_delta, -300);
        assert!(store
            .find_first_invalid_token_daily_delta()
            .unwrap()
            .is_none());
    }
}
