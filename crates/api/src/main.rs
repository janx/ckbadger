use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_api::{create_router, db::create_pool, AppConfig};

#[derive(Parser, Debug)]
#[command(name = "ckbadger-api")]
#[command(about = "API server for ckbadger CKB explorer")]
struct Args {
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,

    #[arg(long, env = "CLICKHOUSE_URL")]
    clickhouse_url: Option<String>,

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

    let database_url = args
        .database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .expect("DATABASE_URL is required");

    info!("Connecting to database");
    let pool = create_pool(&database_url).await?;

    let redis_url = args.redis_url.or_else(|| std::env::var("REDIS_URL").ok());
    let clickhouse_url = args
        .clickhouse_url
        .or_else(|| std::env::var("CLICKHOUSE_URL").ok());

    let config = AppConfig {
        pool,
        redis_url,
        clickhouse_url,
        ckb_rpc_url: args.ckb_rpc_url,
        ckb_network: args.ckb_network,
        rate_limit_per_second: Some(args.rate_limit),
        rate_limit_burst: Some(args.rate_limit_burst),
        start_background_tasks: true,
    };
    let app = create_router(config).await;

    let addr = format!("{}:{}", args.host, args.port);
    info!("Starting API server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
