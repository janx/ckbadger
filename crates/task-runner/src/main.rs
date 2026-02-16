use anyhow::Result;
use ckbadger_store::CkbadgerStore;
use ckbadger_task_runner::executor::TaskExecutor;
use clap::Parser;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "ckbadger-task-runner")]
#[command(about = "Background task runner for ckbadger")]
struct Args {
    #[arg(
        long,
        env = "CKBADGER_DATA_PATH",
        default_value = "./data/ckbadger-store"
    )]
    data_path: String,

    #[arg(long, env = "CKB_RPC_URL", default_value = "http://localhost:8114")]
    ckb_rpc_url: String,

    #[arg(long, env = "TOKEN_LABELS_PATH", default_value = "docs/token-labels")]
    token_labels_path: String,

    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,

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

    let store = Arc::new(CkbadgerStore::open(&args.data_path)?);

    info!("Opened ckbadger-store at {}", args.data_path);

    let executor = TaskExecutor::new(
        store,
        runner_id,
        args.ckb_rpc_url,
        args.token_labels_path,
        args.redis_url,
        None,
    )
    .await;

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
