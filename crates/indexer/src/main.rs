use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_indexer::{
    db::{apply_pg_tuning, IndexManager},
    sync::Indexer,
    Config,
};

#[derive(Parser, Debug)]
#[command(name = "ckbadger-indexer")]
#[command(about = "CKB blockchain indexer for ckbadger explorer")]
struct Args {
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,

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

    #[arg(long, default_value = "16")]
    pipeline_buffer: usize,

    #[arg(
        long,
        default_value = "1000",
        help = "Blocks behind tip to exit bulk sync mode"
    )]
    bulk_sync_threshold: u64,

    #[arg(
        long,
        default_value = "true",
        help = "Use PostgreSQL COPY for bulk sync (5-10x faster)"
    )]
    use_copy_bulk_sync: bool,

    #[arg(
        long,
        default_value = "24",
        help = "Number of connections in the COPY connection pool"
    )]
    copy_pool_size: usize,

    #[arg(
        long,
        default_value = "false",
        help = "Drop non-essential indexes during bulk sync for faster writes (auto-rebuilds when caught up)"
    )]
    defer_indexes: bool,

    #[arg(
        long,
        default_value = "false",
        help = "Disable auto defer-indexes optimization for fresh database sync"
    )]
    no_auto_defer_indexes: bool,

    #[arg(
        long,
        default_value = "10",
        help = "Max parallel connections for index rebuild task (per partitioned table)"
    )]
    index_rebuild_parallel: usize,

    #[arg(
        long,
        default_value = "false",
        help = "Apply PostgreSQL session-level tuning for bulk sync optimization"
    )]
    apply_pg_tuning: bool,

    #[arg(
        long,
        default_value = "100",
        help = "Flush LiveCellStore to database every N batches (default 100)"
    )]
    live_cell_flush_interval: u64,

    #[arg(
        long,
        default_value = "./data/live_cells",
        help = "Path to RocksDB live cell store directory"
    )]
    live_cell_db_path: String,
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
        database_url: args
            .database_url
            .or_else(|| std::env::var("DATABASE_URL").ok())
            .expect("DATABASE_URL is required"),
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
        use_copy_bulk_sync: args.use_copy_bulk_sync,
        copy_pool_size: args.copy_pool_size,
        defer_indexes: args.defer_indexes,
        index_rebuild_parallel: args.index_rebuild_parallel,
        apply_pg_tuning: args.apply_pg_tuning,
        live_cell_flush_interval: args.live_cell_flush_interval,
        live_cell_db_path: args.live_cell_db_path,
    };

    info!("Connecting to database: {}", config.database_url);
    let pool = PgPoolOptions::new()
        .max_connections(32)
        .connect(&config.database_url)
        .await?;

    if config.apply_pg_tuning {
        apply_pg_tuning(&pool).await?;
    }

    info!("Running migrations");
    sqlx::migrate!("../../migrations/postgres")
        .run(&pool)
        .await?;

    let cache_invalidator =
        ckbadger_indexer::cache::CacheInvalidator::new(config.redis_url.as_deref()).await;
    let index_manager = IndexManager::with_cache(pool.clone(), cache_invalidator.clone());

    let indexes_currently_deferred = index_manager.is_indexes_deferred().await?;

    let db_tip: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
        .fetch_one(&pool)
        .await?;

    let is_fresh_sync = db_tip == 0;
    let should_auto_defer =
        is_fresh_sync && !indexes_currently_deferred && !args.no_auto_defer_indexes;

    if should_auto_defer {
        info!(
            "Fresh database detected (tip=0), auto-enabling deferred indexes/constraints for faster initial sync"
        );
        let dropped_indexes = index_manager.drop_deferrable_indexes().await?;
        let dropped_constraints = index_manager.drop_deferrable_constraints().await?;
        info!(
            "Dropped {} indexes and {} constraints (will auto-rebuild when caught up)",
            dropped_indexes, dropped_constraints
        );
    } else if is_fresh_sync && args.no_auto_defer_indexes {
        info!("Fresh database detected, but auto-defer disabled via --no-auto-defer-indexes");
    } else if config.defer_indexes && !indexes_currently_deferred {
        info!("Defer indexes mode enabled, dropping non-essential indexes/constraints for faster bulk sync");
        let dropped_indexes = index_manager.drop_deferrable_indexes().await?;
        let dropped_constraints = index_manager.drop_deferrable_constraints().await?;
        info!(
            "Dropped {} indexes and {} constraints",
            dropped_indexes, dropped_constraints
        );
    } else if indexes_currently_deferred {
        info!("Indexes/constraints are deferred (from previous run), will auto-rebuild when caught up");
    }

    info!("Connecting to CKB node: {}", config.ckb_rpc_url);

    let indexer = Indexer::new(config.clone(), pool.clone()).await?;
    let indexer = Arc::new(indexer);

    let indexer_for_progress = Arc::clone(&indexer);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

            let progress = indexer_for_progress.progress();
            let ema_rate = progress.ema_blocks_per_second();
            let eta = progress.eta_formatted();
            let bps = progress.blocks_per_second();

            let sync_data = ckbadger_common::SyncProgressData {
                current_block: progress.current(),
                target_block: progress.target(),
                blocks_per_second: bps,
                ema_blocks_per_second: ema_rate,
                eta_seconds: progress.eta_seconds(),
                eta_formatted: eta.clone(),
                progress_percentage: progress.progress_percentage(),
                updated_at: chrono::Utc::now().timestamp(),
            };
            indexer_for_progress
                .cache_invalidator()
                .publish_sync_progress(&sync_data)
                .await;

            if indexer_for_progress.is_bulk_sync_active() {
                // ANSI color codes for speed: green (>=1000), yellow (>=100), red (<100)
                let (color_start, color_end) = if ema_rate >= 1000.0 {
                    ("\x1b[32m", "\x1b[0m") // green
                } else if ema_rate >= 100.0 {
                    ("\x1b[33m", "\x1b[0m") // yellow
                } else {
                    ("\x1b[31m", "\x1b[0m") // red
                };

                eprintln!(
                    "Progress: {:.2}% ({}/{}) - {}{:.2} blocks/sec{} (EMA: {}{:.2}{}) | ETA: {}",
                    progress.progress_percentage(),
                    progress.current(),
                    progress.target(),
                    color_start,
                    bps,
                    color_end,
                    color_start,
                    ema_rate,
                    color_end,
                    eta
                );
            } else {
                info!(
                    "Synced to block {} (tip: {}, {} behind)",
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
                info!("Received shutdown signal, flushing LiveCellStore...");
                if let Some(store) = indexer_for_shutdown.writer().live_cell_store() {
                    match store
                        .flush_to_db(indexer_for_shutdown.writer().pool())
                        .await
                    {
                        Ok((inserts, removals)) => {
                            info!(
                                "Shutdown flush completed: {} inserts, {} removals",
                                inserts, removals
                            );
                        }
                        Err(e) => {
                            tracing::error!("Failed to flush on shutdown: {}", e);
                        }
                    }
                }
                std::process::exit(0);
            }
            Err(e) => {
                tracing::error!("Failed to listen for shutdown signal: {}", e);
            }
        }
    });

    indexer.run().await
}
