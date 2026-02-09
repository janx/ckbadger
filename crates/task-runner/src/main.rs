use anyhow::Result;
use ckbadger_task_runner::executor::TaskExecutor;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "ckbadger-task-runner")]
#[command(about = "Background task runner for ckbadger")]
struct Args {
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    #[arg(long, env = "CKB_RPC_URL", default_value = "http://localhost:8114")]
    ckb_rpc_url: String,

    #[arg(long, env = "TOKEN_LABELS_PATH", default_value = "docs/token-labels")]
    token_labels_path: String,

    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,

    #[arg(long, default_value = "10")]
    index_rebuild_parallel: usize,

    #[arg(long, default_value = "50")]
    cycles_batch_size: i64,

    #[arg(long, default_value = "4")]
    cycles_concurrent: usize,

    #[arg(long, default_value = "5")]
    poll_interval_secs: u64,

    #[arg(long)]
    runner_id: Option<String>,

    #[arg(long)]
    once: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let args = Args::parse();

    let runner_id = args
        .runner_id
        .unwrap_or_else(|| format!("runner-{}", &uuid::Uuid::new_v4().to_string()[..8]));

    info!(
        "Starting task runner '{}' with poll interval {}s",
        runner_id, args.poll_interval_secs
    );

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&args.database_url)
        .await?;

    info!("Connected to database");

    let executor = TaskExecutor::new(
        pool,
        runner_id,
        args.ckb_rpc_url,
        args.token_labels_path,
        args.redis_url,
        args.index_rebuild_parallel,
        args.cycles_batch_size,
        args.cycles_concurrent,
    );

    if args.once {
        info!("Running in single-task mode");
        executor.run_once().await?;
    } else {
        info!("Running in continuous mode");
        executor
            .run_continuous(Duration::from_secs(args.poll_interval_secs))
            .await?;
    }

    Ok(())
}
