use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_api::{create_router, AppConfig};
use ckbadger_store::CkbadgerStore;

#[derive(Parser, Debug)]
#[command(name = "ckbadger-api")]
#[command(about = "API server for ckbadger CKB explorer")]
struct Args {
    #[arg(
        long,
        env = "CKBADGER_DATA_PATH",
        default_value = "./data/ckbadger-store"
    )]
    data_path: String,

    #[arg(long, env = "CKBADGER_DERIVED_DATA_PATH")]
    derived_data_path: Option<String>,

    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,

    #[arg(long, env = "CKB_RPC_URL", default_value = "http://127.0.0.1:8114")]
    ckb_rpc_url: String,

    #[arg(long, env = "CKB_NETWORK", default_value = "mainnet")]
    ckb_network: String,

    #[arg(long, env = "API_HOST", default_value = "0.0.0.0")]
    host: String,

    #[arg(long, env = "API_PORT", default_value = "3001")]
    port: u16,

    #[arg(long, env = "API_RATE_LIMIT", default_value = "100")]
    rate_limit: u32,

    #[arg(long, env = "API_RATE_LIMIT_BURST", default_value = "200")]
    rate_limit_burst: u32,

    #[arg(
        long,
        env = "CKB_DATA_PATH",
        help = "Path to CKB node's RocksDB data directory for direct reads"
    )]
    ckb_data_path: Option<String>,
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

    let redis_url = args.redis_url.or_else(|| std::env::var("REDIS_URL").ok());

    let secondary_path = format!("{}-api-secondary", args.data_path);
    info!(
        "Opening ckbadger-store (secondary) at: {} -> {}",
        args.data_path, secondary_path
    );
    let store = Arc::new(CkbadgerStore::open_secondary(
        &args.data_path,
        &secondary_path,
    )?);
    let derived_data_path = args
        .derived_data_path
        .unwrap_or_else(|| format!("{}-derived", args.data_path));
    let derived_secondary_path = format!("{}-api-secondary", derived_data_path);
    info!(
        "Opening ckbadger-derived-store (secondary) at: {} -> {}",
        derived_data_path, derived_secondary_path
    );
    let derived_store = Arc::new(CkbadgerStore::open_secondary(
        &derived_data_path,
        &derived_secondary_path,
    )?);

    let config = AppConfig {
        store,
        derived_store,
        redis_url,
        ckb_rpc_url: args.ckb_rpc_url,
        ckb_network: args.ckb_network,
        rate_limit_per_second: Some(args.rate_limit),
        rate_limit_burst: Some(args.rate_limit_burst),
        start_background_tasks: true,
        ckb_data_path: args.ckb_data_path,
    };
    let app = create_router(config).await;

    let addr = format!("{}:{}", args.host, args.port);
    info!("Starting API server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
