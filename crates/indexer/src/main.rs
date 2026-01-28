use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_indexer::{
    db::{apply_pg_tuning, IndexManager},
    integrity::DataIntegrityService,
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

    #[arg(long, env = "TOKEN_LABELS_PATH")]
    token_labels_path: Option<String>,

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
        default_value = "false",
        help = "Only rebuild indexes without syncing blocks"
    )]
    rebuild_indexes_only: bool,

    #[arg(
        long,
        default_value = "10",
        help = "Max parallel connections for index rebuild (per partitioned table)"
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
        default_value = "8589934592",
        help = "Maximum memory limit for LiveCellStore in bytes (default 8GB = 8589934592)"
    )]
    live_cell_memory_limit: usize,
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
        rebuild_indexes_only: args.rebuild_indexes_only,
        index_rebuild_parallel: args.index_rebuild_parallel,
        apply_pg_tuning: args.apply_pg_tuning,
        live_cell_memory_limit: args.live_cell_memory_limit,
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

    let index_manager = IndexManager::new(pool.clone());

    if config.rebuild_indexes_only {
        info!("Running in index rebuild only mode");
        let progress = index_manager
            .rebuild_indexes_parallel(config.index_rebuild_parallel)
            .await?;
        info!(
            "Index rebuild completed: {}/{} succeeded, {} failed",
            progress.completed,
            progress.total,
            progress.failed.len()
        );
        if !progress.failed.is_empty() {
            info!("Failed indexes: {:?}", progress.failed);
        }
        return Ok(());
    }

    let indexes_currently_deferred = index_manager.is_indexes_deferred().await?;

    let (db_tip, _): (i64, Option<Vec<u8>>) =
        sqlx::query_as("SELECT tip_block_number, tip_block_hash FROM sync_status WHERE id = 1")
            .fetch_one(&pool)
            .await?;

    let is_fresh_sync = db_tip == 0;
    let should_auto_defer =
        is_fresh_sync && !indexes_currently_deferred && !args.no_auto_defer_indexes;

    if should_auto_defer {
        info!(
            "Fresh database detected (tip=0), auto-enabling deferred indexes for faster initial sync"
        );
        let dropped = index_manager.drop_deferrable_indexes().await?;
        info!(
            "Dropped {} indexes (will auto-rebuild when caught up)",
            dropped
        );
    } else if is_fresh_sync && args.no_auto_defer_indexes {
        info!("Fresh database detected, but auto-defer disabled via --no-auto-defer-indexes");
    } else if config.defer_indexes && !indexes_currently_deferred {
        info!("Defer indexes mode enabled, dropping non-essential indexes for faster bulk sync");
        let dropped = index_manager.drop_deferrable_indexes().await?;
        info!("Dropped {} indexes", dropped);
    } else if indexes_currently_deferred {
        info!("Indexes are deferred (from previous run), will auto-rebuild when caught up");
    }

    info!("Connecting to CKB node: {}", config.ckb_rpc_url);

    let token_labels_path = args
        .token_labels_path
        .or_else(|| std::env::var("TOKEN_LABELS_PATH").ok());

    let (integrity_service, integrity_handle) =
        DataIntegrityService::new(pool.clone(), config.ckb_rpc_url.clone(), token_labels_path);

    tokio::spawn(async move {
        integrity_service.run().await;
    });

    let indexer = Indexer::new(config.clone(), pool.clone(), Some(integrity_handle)).await?;

    let progress = indexer.progress();
    let progress_clone = progress.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            info!(
                "Progress: {:.2}% ({}/{}) - {:.2} blocks/sec",
                progress_clone.progress_percentage(),
                progress_clone.current(),
                progress_clone.target(),
                progress_clone.blocks_per_second()
            );
        }
    });

    let should_monitor_rebuild =
        indexes_currently_deferred || config.defer_indexes || should_auto_defer;
    if should_monitor_rebuild {
        let bulk_threshold = config.bulk_sync_threshold;
        let rebuild_parallel = config.index_rebuild_parallel;
        let pool_for_rebuild = pool.clone();
        let progress_for_rebuild = progress.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

                let blocks_remaining = progress_for_rebuild.blocks_remaining();
                if blocks_remaining > bulk_threshold {
                    continue;
                }

                let mgr = IndexManager::new(pool_for_rebuild.clone());
                if let Ok(true) = mgr.is_indexes_deferred().await {
                    info!(
                        "Caught up to tip (remaining={} <= threshold={}), starting index rebuild",
                        blocks_remaining, bulk_threshold
                    );
                    match mgr.rebuild_indexes_parallel(rebuild_parallel).await {
                        Ok(result) => {
                            info!(
                                "Index rebuild completed: {}/{} succeeded",
                                result.completed, result.total
                            );
                        }
                        Err(e) => {
                            tracing::error!("Index rebuild failed: {}", e);
                        }
                    }
                    break;
                }
            }
        });
    }

    indexer.run().await
}
