use anyhow::Result;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_indexer::jobs::{ScriptLabelsTask, UdtLabelsTask};

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

    if args.scripts_only {
        info!("Importing script labels only");
        ScriptLabelsTask::new(pool, Some(args.token_labels_path))
            .run_standalone()
            .await?;
    } else if args.udt_only {
        info!("Importing UDT labels only");
        UdtLabelsTask::new(pool, Some(args.token_labels_path))
            .run_standalone()
            .await?;
    } else {
        info!("Importing all labels (script + UDT)");
        let script_task = ScriptLabelsTask::new(pool.clone(), Some(args.token_labels_path.clone()));
        let udt_task = UdtLabelsTask::new(pool, Some(args.token_labels_path));

        script_task.run_standalone().await?;
        udt_task.run_standalone().await?;
    }

    info!("Import completed");
    Ok(())
}
