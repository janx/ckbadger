use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::sync::Arc;
use tracing::info;
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

async fn run_sync(args: Cli) -> Result<()> {
    let config = Config {
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
    if previous_runtime.active_run_id.is_some() {
        info!(
            run_id = %run_id,
            previous_active_run_id = ?previous_runtime.active_run_id,
            previous_last_run_id = ?previous_runtime.last_run_id,
            previous_last_shutdown_reason = ?previous_runtime.last_shutdown_reason,
            previous_last_exit_code = ?previous_runtime.last_exit_code,
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
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

            let progress = indexer_for_progress.progress();
            let ema_rate = progress.ema_blocks_per_second();
            let eta = progress.eta_formatted();
            let bps = progress.blocks_per_second();

            let (perf_rpc_ms, perf_db_ms) = indexer_for_progress.perf_snapshot_ms();
            let pipeline = indexer_for_progress.pipeline_progress_snapshot();
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
                    "Bulk sync progress"
                );
            } else {
                info!(
                    run_id = %indexer_for_progress.run_id(),
                    "[{}] Synced to block {} (tip: {}, {} behind)",
                    data_source,
                    progress.current(),
                    progress.target(),
                    progress.blocks_remaining()
                );
            }
        }
    });

    let indexer_for_shutdown = Arc::clone(&indexer);
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Received shutdown signal, shutting down gracefully...");
                indexer_for_shutdown.mark_runtime_shutdown("graceful_shutdown", 0);
                // RocksDB handles durability automatically via WAL
                let _ = indexer_for_shutdown; // keep alive until shutdown
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
