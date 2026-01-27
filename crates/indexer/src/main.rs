use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_indexer::Config;

#[derive(Parser, Debug)]
#[command(name = "ckbadger-indexer")]
#[command(about = "CKB blockchain indexer for ckbadger explorer")]
struct Args {
    #[arg(long, env = "CKB_RPC_URL")]
    ckb_rpc_url: Option<String>,

    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,

    #[arg(long, env = "TOKEN_LABELS_PATH")]
    token_labels_path: Option<String>,

    #[arg(long, env = "CLICKHOUSE_URL")]
    clickhouse_url: Option<String>,

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
        clickhouse_url: args
            .clickhouse_url
            .or_else(|| std::env::var("CLICKHOUSE_URL").ok())
            .expect("CLICKHOUSE_URL is required"),
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

    info!("ClickHouse backend selected");
    info!("Connecting to ClickHouse: {}", config.clickhouse_url);
    info!("Connecting to CKB node: {}", config.ckb_rpc_url);

    // TODO: Implement ClickHouse indexer pipeline
    // This requires:
    // 1. Initialize ClickHouseClient
    // 2. Create ClickHouseWriter
    // 3. Implement conversion from ParsedBlock/ParsedTransaction to BlockRow/TransactionRow
    // 4. Adapt sync pipeline to use ClickHouse writer instead of PostgreSQL writer
    //
    // For now, this is a stub to enable compilation and configuration testing.

    eprintln!("ClickHouse backend is not yet fully implemented.");
    eprintln!("TODO: Implement conversion logic from ParsedBlock to BlockRow");
    eprintln!("TODO: Adapt sync pipeline to use ClickHouseWriter");
    std::process::exit(1);
}
