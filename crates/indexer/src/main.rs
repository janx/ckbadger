use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_indexer::{sync::Indexer, Config};
use ckbadger_store::CkbadgerStore;

#[derive(Parser, Debug)]
#[command(name = "ckbadger-indexer")]
#[command(about = "CKB blockchain indexer for ckbadger explorer")]
struct Args {
    #[arg(
        long,
        env = "CKBADGER_DATA_PATH",
        default_value = "./data/ckbadger-store"
    )]
    data_path: String,

    #[arg(long, env = "CKB_RPC_URL")]
    ckb_rpc_url: Option<String>,

    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,

    #[arg(long, default_value = "10000")]
    batch_size: usize,

    #[arg(long, default_value = "1000")]
    poll_interval_ms: u64,

    #[arg(long, default_value = "64")]
    parallel_fetch_size: usize,

    #[arg(long, default_value = "true")]
    pipeline_enabled: bool,

    #[arg(long, default_value = "4")]
    pipeline_buffer: usize,

    #[arg(
        long,
        default_value = "1000",
        help = "Blocks behind tip to exit bulk sync mode"
    )]
    bulk_sync_threshold: u64,

    #[arg(
        long,
        env = "CKB_DATA_PATH",
        help = "Path to CKB node's RocksDB data directory for direct reads (e.g., /var/lib/ckb/data/db)"
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

    let config = Config {
        data_path: args.data_path.clone(),
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
        ckb_data_path: args.ckb_data_path,
    };

    info!("Opening ckbadger-store at: {}", config.data_path);
    let store = Arc::new(CkbadgerStore::open(&config.data_path)?);

    let sync_status = store.get_sync_status()?;
    let db_tip = sync_status.tip_block_number;
    let is_fresh_sync = db_tip == 0;

    if is_fresh_sync {
        info!("Fresh database detected (tip=0), starting initial sync");
    } else {
        info!("Resuming sync from block {}", db_tip);
    }

    // Check deferred state
    if sync_status.activities_deferred || sync_status.address_balances_deferred {
        info!(
            "Deferred states: activities={}, address_balances={}",
            sync_status.activities_deferred, sync_status.address_balances_deferred
        );
    }

    info!("Connecting to CKB node: {}", config.ckb_rpc_url);

    let indexer = Indexer::new(config.clone(), store.clone()).await?;
    let indexer = Arc::new(indexer);

    let data_source = if indexer.is_direct_db_read() {
        "DB"
    } else {
        "RPC"
    };

    let indexer_for_progress = Arc::clone(&indexer);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

            let progress = indexer_for_progress.progress();
            let ema_rate = progress.ema_blocks_per_second();
            let eta = progress.eta_formatted();
            let bps = progress.blocks_per_second();

            let sync_data = ckbadger_common::SyncProgressData {
                current_block: progress.current(),
                target_block: progress.target(),
                blocks_per_second: bps,
                ema_blocks_per_second: ema_rate,
                eta_seconds: progress.eta_seconds(),
                eta_formatted: eta.clone(),
                progress_percentage: progress.progress_percentage(),
                updated_at: chrono::Utc::now().timestamp(),
                is_direct_db_read: data_source == "DB",
            };
            indexer_for_progress
                .cache_invalidator()
                .publish_sync_progress(&sync_data)
                .await;

            let memory_stats = indexer_for_progress.get_memory_stats();
            indexer_for_progress
                .cache_invalidator()
                .publish_memory_stats(&memory_stats)
                .await;

            if indexer_for_progress.is_bulk_sync_active() {
                let (color_start, color_end) = if ema_rate >= 1000.0 {
                    ("\x1b[32m", "\x1b[0m")
                } else if ema_rate >= 100.0 {
                    ("\x1b[33m", "\x1b[0m")
                } else {
                    ("\x1b[31m", "\x1b[0m")
                };

                eprintln!(
                    "[{}] Progress: {:.2}% ({}/{}) - {}{:.2} blocks/sec{} (EMA: {}{:.2}{}) | ETA: {}",
                    data_source,
                    progress.progress_percentage(),
                    progress.current(),
                    progress.target(),
                    color_start,
                    bps,
                    color_end,
                    color_start,
                    ema_rate,
                    color_end,
                    eta
                );
            } else {
                info!(
                    "[{}] Synced to block {} (tip: {}, {} behind)",
                    data_source,
                    progress.current(),
                    progress.target(),
                    progress.blocks_remaining()
                );
            }
        }
    });

    let indexer_for_shutdown = Arc::clone(&indexer);
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Received shutdown signal, shutting down gracefully...");
                // RocksDB handles durability automatically via WAL
                let _ = indexer_for_shutdown; // keep alive until shutdown
                std::process::exit(0);
            }
            Err(e) => {
                tracing::error!("Failed to listen for shutdown signal: {}", e);
            }
        }
    });

    indexer.run().await
}
