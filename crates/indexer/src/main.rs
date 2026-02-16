use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_indexer::{sync::Indexer, Config};
use ckbadger_store::CkbadgerStore;
use ckbadger_task_runner::executor::TaskExecutor;

#[derive(Parser, Debug)]
#[command(name = "ckbadger-indexer")]
#[command(about = "CKB blockchain indexer for ckbadger explorer")]
struct Args {
    #[arg(
        long,
        env = "CKBADGER_DATA_PATH",
        default_value = "./data/ckbadger-store"
    )]
    data_path: String,

    #[arg(long, env = "CKB_RPC_URL")]
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

    // Task runner settings (embedded)
    #[arg(long, env = "TOKEN_LABELS_PATH", default_value = "docs/token-labels")]
    token_labels_path: String,

    #[arg(long, default_value = "5")]
    task_poll_interval_secs: u64,

    #[arg(long, default_value = "false", help = "Disable embedded task runner")]
    no_task_runner: bool,

    #[arg(
        long,
        default_value = "false",
        help = "Force rebuild of activity entries from scratch on startup"
    )]
    rebuild_activities: bool,
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

    let args = Args::parse();

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

    // One-time migration: rebuild DAO daily snapshots from deposit history
    // v3: fixes AR-vs-deposit bug, adds cumulative_deposit_amount, and
    //     stores actual C/S from DAO header for circulation ratio chart
    // v4: adds secondary issuance breakdown (miner/dao/treasury) from block headers
    // v5: fixes total supply chart burnt using cum_treasury instead of secondary_pool,
    //     and ensures incremental indexer updates cum_treasury/cum_dao correctly
    let sync_status = store.get_sync_status()?;
    if sync_status.dao_snapshots_version < 5 && sync_status.tip_block_number > 0 {
        info!("DAO snapshots migration v5: rebuilding from deposit history...");
        let written = store.rebuild_dao_daily_snapshots()?;
        info!(
            "DAO snapshots migration v5 complete: {} snapshots rebuilt",
            written
        );
        store.update_sync_status(|s| {
            s.dao_snapshots_version = 5;
        })?;
    }

    // One-time migration: rebuild avg_block_time_ms from block headers
    let sync_status = store.get_sync_status()?;
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
    }

    let sync_status = store.get_sync_status()?;
    let db_tip = sync_status.tip_block_number;
    let is_fresh_sync = db_tip == 0;

    if is_fresh_sync {
        info!("Fresh database detected (tip=0), starting initial sync");
    } else {
        info!("Resuming sync from block {}", db_tip);
    }

    // Force activities rebuild if requested via CLI flag
    if args.rebuild_activities && !sync_status.activities_deferred {
        info!("--rebuild-activities flag set: marking activities as deferred for rebuild");
        store.update_sync_status(|s| {
            s.activities_deferred = true;
        })?;
    }

    // Check deferred state
    if sync_status.address_balances_deferred
        || sync_status.activities_deferred
        || args.rebuild_activities
    {
        info!(
            "Deferred states: address_balances={}, activities={}",
            sync_status.address_balances_deferred,
            sync_status.activities_deferred || args.rebuild_activities
        );
    }

    info!("Connecting to CKB node: {}", config.ckb_rpc_url);

    let indexer = Indexer::new(config.clone(), store.clone()).await?;
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
            let sync_data = ckbadger_common::SyncProgressData {
                current_block: progress.current(),
                target_block: progress.target(),
                blocks_per_second: bps,
                ema_blocks_per_second: ema_rate,
                eta_seconds: progress.eta_seconds(),
                eta_formatted: eta.clone(),
                progress_percentage: progress.progress_percentage(),
                updated_at: chrono::Utc::now().timestamp(),
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
                    "[{}] Synced to block {} (tip: {}, {} behind)",
                    data_source,
                    progress.current(),
                    progress.target(),
                    progress.blocks_remaining()
                );
            }
        }
    });

    // Spawn embedded task runner
    if !args.no_task_runner {
        let runner_id = format!("indexer-runner-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let task_executor = TaskExecutor::new(
            store.clone(),
            runner_id.clone(),
            config.ckb_rpc_url.clone(),
            args.token_labels_path.clone(),
            config.redis_url.clone(),
            indexer.ckb_store(),
        );
        let poll_interval = Duration::from_secs(args.task_poll_interval_secs);
        info!(
            "Starting embedded task runner '{}' with poll interval {}s",
            runner_id, args.task_poll_interval_secs
        );
        tokio::spawn(async move {
            task_executor.run_continuous(poll_interval).await.ok();
        });
    }

    let indexer_for_shutdown = Arc::clone(&indexer);
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Received shutdown signal, shutting down gracefully...");
                // RocksDB handles durability automatically via WAL
                let _ = indexer_for_shutdown; // keep alive until shutdown
                std::process::exit(0);
            }
            Err(e) => {
                tracing::error!("Failed to listen for shutdown signal: {}", e);
            }
        }
    });

    indexer.run().await
}
