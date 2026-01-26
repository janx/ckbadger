use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_indexer::{sync::Indexer, Config, ControlPlaneClient, JobExecutor};

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

    #[arg(long, env = "CONTROL_DATABASE_URL")]
    control_database_url: Option<String>,

    #[arg(long, env = "CKB_NETWORK", default_value = "mainnet")]
    network: String,

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
        default_value = "false",
        help = "Enable bulk sync mode for faster initial sync"
    )]
    bulk_sync_mode: bool,

    #[arg(
        long,
        default_value = "1000",
        help = "Blocks behind tip to exit bulk sync mode"
    )]
    bulk_sync_threshold: u64,
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
        bulk_sync_mode: args.bulk_sync_mode,
        bulk_sync_threshold: args.bulk_sync_threshold,
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

    let control_plane = if let Some(control_url) = args
        .control_database_url
        .or_else(|| std::env::var("CONTROL_DATABASE_URL").ok())
    {
        info!("Connecting to control plane: {}", control_url);
        match ControlPlaneClient::connect(
            &control_url,
            &config.database_url,
            &config.ckb_rpc_url,
            &args.network,
        )
        .await
        {
            Ok(client) => Some(Arc::new(client)),
            Err(e) => {
                tracing::warn!("Failed to connect to control plane: {}. Continuing without it.", e);
                None
            }
        }
    } else {
        None
    };

    let token_labels_path = args
        .token_labels_path
        .or_else(|| std::env::var("TOKEN_LABELS_PATH").ok());

    if let Some(ref cp) = control_plane {
        info!("Starting JobExecutor for Control Plane jobs");
        let executor = JobExecutor::new(
            pool.clone(),
            Arc::clone(cp),
            config.ckb_rpc_url.clone(),
            token_labels_path.clone(),
        );
        tokio::spawn(async move {
            executor.run().await;
        });
    } else {
        info!("No Control Plane configured - integrity jobs must be triggered via TUI");
    }

    let indexer = Indexer::new(config, pool, control_plane.clone()).await?;

    let progress = indexer.progress();
    let control_plane_reporter = control_plane.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            let current = progress.current();
            let target = progress.target();
            let bps = progress.blocks_per_second();

            info!(
                "Progress: {:.2}% ({}/{}) - {:.2} blocks/sec",
                progress.progress_percentage(),
                current,
                target,
                bps
            );

            if let Some(ref cp) = control_plane_reporter {
                cp.update_progress(current, target, bps).await;
            }
        }
    });

    indexer.run().await
}
