use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_indexer::{integrity::DataIntegrityService, sync::Indexer, Config};

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

    #[arg(long, default_value = "1000")]
    batch_size: usize,

    #[arg(long, default_value = "1000")]
    poll_interval_ms: u64,

    #[arg(long, default_value = "32")]
    parallel_fetch_size: usize,

    #[arg(long, default_value = "true")]
    pipeline_enabled: bool,

    #[arg(long, default_value = "6")]
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
        default_value = "8",
        help = "Number of connections in the COPY connection pool"
    )]
    copy_pool_size: usize,
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
    };

    info!("Connecting to database: {}", config.database_url);
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;

    info!("Running migrations");
    sqlx::migrate!("../../migrations/postgres")
        .run(&pool)
        .await?;

    info!("Connecting to CKB node: {}", config.ckb_rpc_url);

    let token_labels_path = args
        .token_labels_path
        .or_else(|| std::env::var("TOKEN_LABELS_PATH").ok());

    let (integrity_service, integrity_handle) =
        DataIntegrityService::new(pool.clone(), config.ckb_rpc_url.clone(), token_labels_path);

    tokio::spawn(async move {
        integrity_service.run().await;
    });

    let indexer = Indexer::new(config, pool, Some(integrity_handle)).await?;

    let progress = indexer.progress();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            info!(
                "Progress: {:.2}% ({}/{}) - {:.2} blocks/sec",
                progress.progress_percentage(),
                progress.current(),
                progress.target(),
                progress.blocks_per_second()
            );
        }
    });

    indexer.run().await
}
