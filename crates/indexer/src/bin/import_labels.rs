use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_indexer::integrity::{DataIntegrityService, IntegrityCheck};

#[derive(Parser, Debug)]
#[command(name = "import-labels")]
#[command(about = "Import token and script labels from token-labels repository")]
struct Args {
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    #[arg(long, env = "TOKEN_LABELS_PATH", default_value = "docs/token-labels")]
    token_labels_path: String,

    #[arg(long, help = "Import only script labels")]
    scripts_only: bool,

    #[arg(long, help = "Import only UDT labels")]
    udt_only: bool,
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

    info!("Connecting to database: {}", database_url);
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    info!("Running migrations");
    sqlx::migrate!("../../migrations/postgres")
        .run(&pool)
        .await?;

    info!("Importing labels from: {}", args.token_labels_path);

    let (service, handle) =
        DataIntegrityService::new(pool, String::new(), Some(args.token_labels_path));

    let service_handle = tokio::spawn(async move {
        service.run().await;
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    if args.scripts_only {
        handle.trigger(IntegrityCheck::ScriptInfoUpdate).await;
    } else if args.udt_only {
        handle.trigger(IntegrityCheck::UdtInfoUpdate).await;
    } else {
        handle.trigger(IntegrityCheck::AllLabelsUpdate).await;
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    info!("Import completed");
    service_handle.abort();

    Ok(())
}
