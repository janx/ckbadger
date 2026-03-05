use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ckbadger_indexer::{
    entry::{self, LabelImportServiceConfig},
    verify, Config,
};

#[derive(Parser, Debug)]
#[command(name = "ckbadger-indexer")]
#[command(about = "CKB blockchain indexer for ckbadger explorer")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    // ---- Sync args (used when no subcommand or `sync` subcommand) ----
    #[arg(
        long = "domain-data-path",
        env = "CKBADGER_DOMAIN_DATA_PATH",
        global = true
    )]
    domain_data_path: Option<String>,

    #[arg(
        long = "append-only-data-path",
        env = "CKBADGER_APPEND_ONLY_DATA_PATH",
        global = true
    )]
    append_only_data_path: Option<String>,

    #[arg(long, env = "CKB_RPC_URL", global = true)]
    ckb_rpc_url: Option<String>,

    #[arg(long, default_value = "10000")]
    batch_size: usize,

    #[arg(long, default_value = "1000")]
    poll_interval_ms: u64,

    #[arg(long, default_value = "64")]
    parallel_fetch_size: usize,

    #[arg(long, default_value = "true")]
    pipeline_enabled: bool,

    #[arg(long, default_value = "8")]
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

    // Label import settings
    #[arg(long, env = "TOKEN_LABELS_PATH", default_value = "docs/token-labels")]
    token_labels_path: String,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the sync daemon (default behavior).
    Sync,
    /// Verify data integrity of the store.
    Verify(verify::VerifyArgs),
    /// Import UDT and script labels directly (without task system).
    LabelImport(LabelImportArgs),
}

#[derive(Args, Debug)]
struct LabelImportArgs {
    #[arg(long, env = "TOKEN_LABELS_PATH", default_value = "docs/token-labels")]
    token_labels_path: String,

    #[arg(long, default_value = "mainnet")]
    network: String,

    #[arg(long, default_value_t = true)]
    import_udt: bool,

    #[arg(long, default_value_t = true)]
    import_scripts: bool,
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

    let cli = Cli::parse();
    let domain_data_path = resolve_domain_data_path(cli.domain_data_path.clone());
    let append_only_data_path =
        resolve_append_only_data_path(cli.append_only_data_path.clone(), &domain_data_path);
    let ckb_data_path = cli.ckb_data_path.clone();

    match cli.command {
        Some(Command::Verify(args)) => {
            // Run on a blocking thread so reqwest::blocking's internal
            // tokio runtime isn't nested inside #[tokio::main].
            tokio::task::spawn_blocking(move || verify::run(args))
                .await
                .expect("verify task panicked")?;
            Ok(())
        }
        Some(Command::LabelImport(args)) => {
            entry::run_label_import(LabelImportServiceConfig {
                domain_data_path,
                append_only_data_path,
                ckb_data_path,
                token_labels_path: args.token_labels_path,
                network: args.network,
                import_udt: args.import_udt,
                import_scripts: args.import_scripts,
            })
            .await
        }
        // Default (no subcommand) or explicit `sync` -> run sync daemon
        None | Some(Command::Sync) => {
            let config = Config {
                domain_data_path,
                append_only_data_path,
                ckb_rpc_url: cli
                    .ckb_rpc_url
                    .or_else(|| std::env::var("CKB_RPC_URL").ok())
                    .expect("CKB_RPC_URL is required"),
                batch_size: cli.batch_size,
                poll_interval_ms: cli.poll_interval_ms,
                start_block: None,
                parallel_fetch_size: cli.parallel_fetch_size,
                pipeline_enabled: cli.pipeline_enabled,
                pipeline_buffer: cli.pipeline_buffer,
                bulk_sync_threshold: cli.bulk_sync_threshold,
                fast_sync_mode: true,
                ckb_data_path,
                token_labels_path: cli.token_labels_path,
                force_startup_cleanup: false,
            };
            config.validate()?;
            entry::run_indexer_sync(config).await
        }
    }
}

fn resolve_domain_data_path(explicit: Option<String>) -> String {
    resolve_domain_data_path_from_sources(explicit, std::env::var("CKBADGER_DOMAIN_DATA_PATH").ok())
}

fn resolve_append_only_data_path(explicit: Option<String>, domain_data_path: &str) -> String {
    resolve_append_only_data_path_from_sources(
        explicit,
        std::env::var("CKBADGER_APPEND_ONLY_DATA_PATH").ok(),
        domain_data_path,
    )
}

fn resolve_domain_data_path_from_sources(
    explicit: Option<String>,
    domain_env: Option<String>,
) -> String {
    explicit
        .or(domain_env)
        .unwrap_or_else(|| "./data/ckbadger-store".to_string())
}

fn resolve_append_only_data_path_from_sources(
    explicit: Option<String>,
    append_only_env: Option<String>,
    domain_data_path: &str,
) -> String {
    explicit
        .or(append_only_env)
        .unwrap_or_else(|| format!("{}-append-only", domain_data_path))
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_append_only_data_path_from_sources, resolve_domain_data_path_from_sources,
    };

    #[test]
    fn test_resolve_domain_data_path_from_sources() {
        assert_eq!(
            resolve_domain_data_path_from_sources(
                Some("/explicit/domain".to_string()),
                Some("/env/domain".to_string()),
            ),
            "/explicit/domain"
        );
        assert_eq!(
            resolve_domain_data_path_from_sources(None, Some("/env/domain".to_string())),
            "/env/domain"
        );
        assert_eq!(
            resolve_domain_data_path_from_sources(None, None),
            "./data/ckbadger-store"
        );
    }

    #[test]
    fn test_resolve_append_only_data_path_from_sources() {
        assert_eq!(
            resolve_append_only_data_path_from_sources(
                Some("/explicit/append".to_string()),
                Some("/env/append".to_string()),
                "/domain",
            ),
            "/explicit/append"
        );
        assert_eq!(
            resolve_append_only_data_path_from_sources(
                None,
                Some("/env/append".to_string()),
                "/domain"
            ),
            "/env/append"
        );
        assert_eq!(
            resolve_append_only_data_path_from_sources(None, None, "/domain"),
            "/domain-append-only"
        );
    }
}
