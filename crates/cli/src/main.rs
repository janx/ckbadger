mod supervisor;

#[cfg(test)]
mod build_version_format;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use ckbadger_config::{
    default_config_toml, default_orchestrator_toml, is_orchestrator, load_config,
    load_orchestrator_config, network_workdir, resolve_ckb_paths, resolve_share_dir,
    resolve_store_paths, resolve_workdir_path, CkbadgerConfig, ResolvedCkbPaths, StoreConfig,
    WorkDir,
};

use ckbadger_api::entry::{run_api, run_frontend_server, ApiServiceConfig, FrontendServiceConfig};
use ckbadger_indexer::entry::{
    run_indexer, run_label_import, IndexerServiceConfig, LabelImportServiceConfig,
};
use ckbadger_indexer::verify as indexer_verify;
use ckbadger_store::{
    known_append_only_secondary_store_paths, known_domain_secondary_store_paths,
    secondary_store_path, CkbadgerStore, SecondaryStoreOwner, StoreRuntimeConfig,
};
use ckbadger_tui::entry::{run_tui, TuiServiceConfig};

const BUILD_VERSION: &str = env!("CKBADGER_BUILD_VERSION");

// ---------------------------------------------------------------------------
// File descriptor limit management
// ---------------------------------------------------------------------------

/// Minimum fd limit required to run the indexer (including bulk sync).
const FD_LIMIT_MIN: u64 = 4096;

/// Target fd limit we attempt to raise to.
const FD_LIMIT_TARGET: u64 = 65535;

/// Attempt to raise the process fd limit to [`FD_LIMIT_TARGET`].
///
/// Strategy:
/// 1. If the hard limit is `RLIM_INFINITY` or ≥ target → raise soft limit only.
/// 2. If the hard limit is below target → try raising *both* hard and soft
///    (works as root on Linux; works for regular users on macOS up to the
///    kernel maximum `kern.maxfilesperproc`).
/// 3. If the hard-limit raise is denied → raise soft limit to current hard
///    limit as a best effort.
///
/// Returns the effective soft limit after the raise attempt.
#[cfg(unix)]
#[allow(clippy::unnecessary_cast)] // rlim_t is u32 on some platforms, u64 on others
fn raise_fd_limit() -> Result<u64> {
    unsafe {
        let mut rlim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) != 0 {
            bail!(
                "getrlimit(RLIMIT_NOFILE) failed: {}",
                std::io::Error::last_os_error()
            );
        }

        let target = FD_LIMIT_TARGET as libc::rlim_t;

        // Determine the hard-limit ceiling without requiring privilege.
        let hard_ceiling = if rlim.rlim_max == libc::RLIM_INFINITY {
            target // unlimited — soft can reach target freely
        } else {
            rlim.rlim_max
        };

        if hard_ceiling < target {
            // Hard limit is lower than what we need.  Try raising both
            // (requires CAP_SYS_RESOURCE / root on Linux; often works for
            // regular users on macOS up to kern.maxfilesperproc).
            let raised = libc::rlimit {
                rlim_cur: target,
                rlim_max: target,
            };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &raised) == 0 {
                return Ok(FD_LIMIT_TARGET);
            }
            // Hard-limit raise denied.  Raise soft to current hard ceiling
            // as a best effort.
            if hard_ceiling > rlim.rlim_cur {
                let capped = libc::rlimit {
                    rlim_cur: hard_ceiling,
                    rlim_max: rlim.rlim_max,
                };
                if libc::setrlimit(libc::RLIMIT_NOFILE, &capped) == 0 {
                    return Ok(hard_ceiling as u64);
                }
            }
            // Could not change anything.
            return Ok(rlim.rlim_cur as u64);
        }

        // Hard limit is already sufficient; raise soft limit only.
        if (rlim.rlim_cur as u64) < FD_LIMIT_TARGET {
            let raised = libc::rlimit {
                rlim_cur: target,
                rlim_max: rlim.rlim_max,
            };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &raised) == 0 {
                return Ok(FD_LIMIT_TARGET);
            }
        }
        Ok(rlim.rlim_cur as u64)
    }
}

#[cfg(not(unix))]
fn raise_fd_limit() -> Result<u64> {
    Ok(FD_LIMIT_TARGET) // Windows has no meaningful fd limit
}

/// Fail fast if `fd_limit` is too low to run the indexer.
///
/// The indexer opens many RocksDB SST files during bulk sync (60 column
/// families × multiple SST levels).  Below [`FD_LIMIT_MIN`] the open will
/// fail mid-sync, leaving RocksDB in an incomplete state.
fn check_fd_limit_for_indexer(fd_limit: u64) -> Result<()> {
    if fd_limit < FD_LIMIT_MIN {
        bail!(
            "fd limit {} is too low to run the indexer (need >={}).\n\
             The indexer opens many RocksDB SST files during bulk sync;\n\
             a hard limit that cannot be raised will cause a mid-sync crash.\n\n\
             Fix on macOS:\n\
               sudo launchctl limit maxfiles {} {}\n\
               (then reopen the terminal -- launchctl applies to new sessions)\n\n\
             Fix on Linux (requires root):\n\
               echo DefaultLimitNOFILE={} >> /etc/systemd/system.conf\n\
               systemctl daemon-reexec && reboot\n\
               (or for the current shell only: ulimit -Sn {})",
            fd_limit,
            FD_LIMIT_MIN,
            FD_LIMIT_TARGET,
            FD_LIMIT_TARGET,
            FD_LIMIT_TARGET,
            FD_LIMIT_TARGET
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "ckbadger",
    about = "A local-first and agent-friendly CKB explorer",
    version = BUILD_VERSION
)]
struct Cli {
    /// Work directory (default: current directory)
    #[arg(short = 'C', long, global = true)]
    workdir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize work directory
    Init(InitArgs),
    /// Start all services (supervisor mode)
    Run(RunArgs),
    /// Terminal monitoring UI
    Tui,
    /// Show sync and service status
    Status,
    /// Verify data integrity
    Verify(VerifyArgs),
    /// Run one or continuous whole-network peer crawl rounds
    Crawl(CrawlArgs),
    /// Import token and script labels
    LabelImport(LabelImportArgs),
    /// Purge derived data, keep config
    Purge(PurgeArgs),
    /// Internal subprocess commands (not user-facing)
    #[command(hide = true)]
    Internal(InternalArgs),
}

#[derive(clap::Args)]
struct InitArgs {
    /// Also scaffold a testnet network subdir (api port 8102).
    #[arg(long)]
    with_testnet: bool,
}

#[derive(clap::Args)]
struct RunArgs {
    /// Start only specific services (comma-separated: indexer,api,frontend)
    #[arg(long)]
    only: Option<String>,
}

#[derive(clap::Args)]
struct VerifyArgs {
    /// Verification depth
    #[arg(long, default_value = "fast")]
    depth: String,

    /// List available checks and exit
    #[arg(long)]
    list_checks: bool,
}

#[derive(clap::Args)]
struct CrawlArgs {
    /// Run a single round and exit (manual verification)
    #[arg(long)]
    once: bool,
}

#[derive(clap::Args)]
struct LabelImportArgs {
    // Will be expanded later
}

#[derive(clap::Args)]
struct PurgeArgs {
    /// Confirm destructive operation
    #[arg(long)]
    confirm: bool,
}

#[derive(clap::Args)]
struct InternalArgs {
    #[command(subcommand)]
    service: InternalService,
}

#[derive(Subcommand)]
enum InternalService {
    /// Run the indexer subprocess
    Indexer,
    /// Run the API subprocess
    Api,
    /// Run the frontend server subprocess
    FrontendServer,
    /// Run the crawler subprocess
    Crawler,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let workdir = cli
        .workdir
        .unwrap_or_else(|| std::env::current_dir().expect("cannot determine current directory"));

    // Print ASCII banner for all commands except TUI
    // (TUI manages its own terminal and shows the version in the header).
    if !matches!(cli.command, Command::Tui) {
        print_banner();
    }

    match cli.command {
        Command::Init(args) => {
            // For init, set up tracing with default "info" level since
            // no config file exists yet.
            init_tracing("info");
            cmd_init(&workdir, &args)
        }
        Command::Purge(args) => {
            // For purge, load config to get log level if available,
            // otherwise fall back to "info".
            let log_level = load_config(&workdir)
                .map(|c| c.log.level)
                .unwrap_or_else(|_| "info".to_string());
            init_tracing(&log_level);
            cmd_purge(&workdir, &args)
        }
        Command::Run(args) => {
            init_tracing_from_config(&workdir);
            cmd_run(&workdir, &args).await
        }
        Command::Tui => {
            // TUI manages its own terminal; do not init tracing (it would
            // write to stdout and corrupt the TUI display).
            cmd_tui(&workdir).await
        }
        Command::Status => {
            init_tracing_from_config(&workdir);
            cmd_status(&workdir).await
        }
        Command::Verify(args) => {
            init_tracing_from_config(&workdir);
            cmd_verify(&workdir, &args).await
        }
        Command::Crawl(args) => {
            init_tracing_from_config(&workdir);
            ckbadger_crawler::entry::run_crawler(&workdir, args.once).await
        }
        Command::LabelImport(_) => {
            init_tracing_from_config(&workdir);
            cmd_label_import(&workdir).await
        }
        Command::Internal(args) => {
            init_tracing_from_config(&workdir);
            cmd_internal(&workdir, &args).await
        }
    }
}

// ---------------------------------------------------------------------------
// Tracing setup
// ---------------------------------------------------------------------------

fn init_tracing(level: &str) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn init_tracing_from_config(workdir: &Path) {
    let log_level = load_config(workdir)
        .map(|c| c.log.level)
        .unwrap_or_else(|_| "info".to_string());
    init_tracing(&log_level);
}

fn store_runtime_config(store: &StoreConfig) -> StoreRuntimeConfig {
    StoreRuntimeConfig {
        memory_budget_gb: store.memory_budget_gb,
        direct_io_reads: store.direct_io_reads,
        vector_memtable: false, // set by indexer entry at runtime
    }
}

fn build_indexer_service_config(
    workdir: &Path,
    work: &WorkDir,
    config: &CkbadgerConfig,
    ckb_paths: &ResolvedCkbPaths,
    build_version: &str,
) -> Result<IndexerServiceConfig> {
    let store_paths = resolve_store_paths(workdir, &config.store);

    Ok(IndexerServiceConfig {
        domain_data_path: store_paths.domain_data.to_string_lossy().to_string(),
        append_only_data_path: store_paths.append_only_data.to_string_lossy().to_string(),
        bulk_sync_perf_output_root: work.bulk_sync_perf_dir.to_string_lossy().to_string(),
        build_version: build_version.to_string(),
        ckb_rpc_url: config.ckb.rpc_url.clone(),
        ckb_db_path: ckb_paths.ckb_db_path.to_string_lossy().to_string(),
        metadata_path: work
            .metadata
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        network: config.ckb.network.clone(),
        poll_interval_ms: config.indexer.poll_interval_ms,
        bulk_sync_threshold: config.indexer.bulk_sync_threshold,
        store_runtime_config: store_runtime_config(&config.store),
        decoder_cache_path: store_paths.decoder_cache.to_string_lossy().to_string(),
        dob_decode_dir: work.dob_decode_dir.to_string_lossy().to_string(),
        cycles_request_dir: Some(work.cycles_request_dir.to_string_lossy().to_string()),
    })
}

// ---------------------------------------------------------------------------
// run command
// ---------------------------------------------------------------------------

async fn cmd_run(workdir: &Path, args: &RunArgs) -> Result<()> {
    // Orchestrator dir (`ckbadger.toml`) launches one stack per network subdir.
    if is_orchestrator(workdir) {
        return cmd_run_orchestrator(workdir, args).await;
    }

    // --- single-network path ---
    let config = load_config(workdir)?;
    let work = WorkDir::resolve(workdir);

    // An explicit `--only` selects services verbatim; otherwise the default
    // spawn set comes from `enabled_services`, which appends the crawler only
    // when `[crawler].enabled` is set.
    let services = match &args.only {
        Some(_) => parse_only_flag(&args.only),
        None => supervisor::enabled_services(&config)
            .into_iter()
            .map(String::from)
            .collect(),
    };

    if services.is_empty() {
        bail!("no services selected to run");
    }

    let fd_limit = raise_fd_limit()?;

    if services.iter().any(|s| s == "indexer") {
        check_fd_limit_for_indexer(fd_limit)?;
    }

    print_startup_info(workdir, &config, &work, &services, fd_limit);

    supervisor::run_supervisor(&work, &config, services).await
}

/// Orchestrator-mode `run`: launch one `(indexer, api[, crawler])` stack per
/// network subdir listed in the top-level `ckbadger.toml`.
///
/// Each subdir is a standard single-network workdir with its own `config.toml`.
/// Services are spawned as `ckbadger internal <service> -C <subdir>` children of
/// one shared supervisor rooted at the orchestrator dir. The frontend is NOT
/// spawned per-network (a unified frontend proxy is a later plan); each API is
/// reachable on its own configured port.
async fn cmd_run_orchestrator(root: &Path, _args: &RunArgs) -> Result<()> {
    let orch = load_orchestrator_config(root)?;
    let root_work = WorkDir::resolve(root);

    // At least one indexer will run; fail fast if the fd limit is too low.
    let fd_limit = raise_fd_limit()?;
    check_fd_limit_for_indexer(fd_limit)?;

    let mut specs = Vec::new();
    for entry in &orch.networks {
        let sub = network_workdir(root, entry);
        if !sub.join("config.toml").is_file() {
            bail!(
                "network '{}' has no config.toml at {}",
                entry.name,
                sub.display()
            );
        }
        let cfg = load_config(&sub)?;
        // Per-network services EXCEPT the frontend (one shared frontend proxy
        // is added in a later plan).
        let mut services = vec!["indexer", "api"];
        if cfg.crawler.enabled {
            services.push("crawler");
        }
        for svc in services {
            specs.push(supervisor::ChildSpec {
                label: format!("{}/{}", entry.name, svc),
                service: svc.to_string(),
                workdir: sub.to_string_lossy().to_string(),
            });
        }
        println!(
            "network '{}' -> {} (api :{})",
            entry.name,
            sub.display(),
            cfg.api.port
        );
    }
    println!("Frontend proxy: deferred to a later plan (reach each API on its own port for now).");
    supervisor::run_supervisor_multi(&root_work, specs).await
}

// ---------------------------------------------------------------------------
// internal command (subprocess entry points)
// ---------------------------------------------------------------------------

async fn cmd_internal(workdir: &Path, args: &InternalArgs) -> Result<()> {
    let config = load_config(workdir)?;
    let work = WorkDir::resolve(workdir);
    let store_runtime_config = store_runtime_config(&config.store);
    let store_paths = resolve_store_paths(workdir, &config.store);

    match args.service {
        InternalService::Indexer => {
            // Safety net for direct `ckbadger internal indexer` invocation.
            // When launched via supervisor the parent already raised the limit
            // and the child inherits it, so this is usually a no-op check.
            check_fd_limit_for_indexer(raise_fd_limit()?)?;
            let ckb_paths = resolve_ckb_paths(workdir, &config.ckb)?;
            let indexer_config =
                build_indexer_service_config(workdir, &work, &config, &ckb_paths, BUILD_VERSION)?;
            run_indexer(indexer_config).await
        }
        InternalService::Api => {
            let ckb_paths = resolve_ckb_paths(workdir, &config.ckb)?;
            let api_config = ApiServiceConfig {
                domain_data_path: store_paths.domain_data.to_string_lossy().to_string(),
                append_only_data_path: store_paths.append_only_data.to_string_lossy().to_string(),
                ckb_rpc_url: config.ckb.rpc_url.clone(),
                ckb_network: config.ckb.network.clone(),
                host: config.api.host.clone(),
                port: config.api.port,
                rate_limit: config.api.rate_limit,
                rate_limit_burst: config.api.rate_limit_burst,
                slow_request_threshold_ms: config.api.slow_request_threshold_ms,
                ckb_db_path: ckb_paths.ckb_db_path.to_string_lossy().to_string(),
                store_runtime_config,
                dob_decode_dir: work.dob_decode_dir.clone(),
                cycles_request_dir: Some(work.cycles_request_dir.clone()),
                // Resolve the network-store path the same way the crawler does
                // (workdir + relative), so the API secondary targets the crawler's
                // primary. Opening is opt-in and handled in run_api.
                network_data_path: resolve_workdir_path(workdir, &config.store.network_data_path)
                    .to_string_lossy()
                    .to_string(),
                crawler_enabled: config.crawler.enabled,
            };
            run_api(api_config).await
        }
        InternalService::FrontendServer => {
            let frontend_dir = resolve_frontend_dir(&work);
            let frontend_config = FrontendServiceConfig {
                host: config.frontend.host.clone(),
                port: config.frontend.port,
                api_port: config.api.port,
                ckb_network: config.ckb.network.clone(),
                ckb_rpc_url: config.ckb.rpc_url.clone(),
                build_version: BUILD_VERSION.to_string(),
                frontend_dir,
            };
            run_frontend_server(frontend_config).await
        }
        InternalService::Crawler => ckbadger_crawler::entry::run_crawler(workdir, false).await,
    }
}

// ---------------------------------------------------------------------------
// tui command
// ---------------------------------------------------------------------------

async fn cmd_tui(workdir: &Path) -> Result<()> {
    let config = load_config(workdir)?;
    let work = WorkDir::resolve(workdir);
    let store_paths = resolve_store_paths(workdir, &config.store);
    let ckb_paths = resolve_ckb_paths(workdir, &config.ckb)?;

    let tui_config = TuiServiceConfig {
        domain_data_path: store_paths.domain_data.to_string_lossy().to_string(),
        append_only_data_path: store_paths.append_only_data.to_string_lossy().to_string(),
        ckbadger_workdir: work.root.to_string_lossy().to_string(),
        ckb_workdir: ckb_paths.ckb_workdir.to_string_lossy().to_string(),
        ckb_db_path: ckb_paths.ckb_db_path.to_string_lossy().to_string(),
        api_url: format!("http://{}:{}/api/v1", config.api.host, config.api.port),
        refresh_ms: 1000,
        supervisor_socket_path: Some(work.indexer_sock.to_string_lossy().to_string()),
        service_log_dir: Some(work.log_dir.to_string_lossy().to_string()),
        store_runtime_config: store_runtime_config(&config.store),
        build_version: BUILD_VERSION.to_string(),
    };

    run_tui(tui_config).await
}

// ---------------------------------------------------------------------------
// status command
// ---------------------------------------------------------------------------

async fn cmd_status(workdir: &Path) -> Result<()> {
    if ckbadger_config::is_orchestrator(workdir) {
        return cmd_status_orchestrator(workdir).await;
    }

    let work = WorkDir::resolve(workdir);

    // 1. Try IPC to get service status from the supervisor
    if work.indexer_sock.exists() {
        match ckbadger_ipc::ipc_request(
            &work.indexer_sock,
            &ckbadger_ipc::IpcRequest::GetServiceStatus,
        )
        .await
        {
            Ok(ckbadger_ipc::IpcResponse::ServiceStatus { services }) => {
                println!("Services:");
                for svc in &services {
                    println!(
                        "  {}: {} (pid {}, uptime {}s)",
                        svc.name, svc.status, svc.pid, svc.uptime_secs
                    );
                }
                println!();
            }
            Ok(other) => {
                println!("Unexpected IPC response: {:?}", other);
            }
            Err(e) => {
                println!("Supervisor not reachable: {}", e);
            }
        }
    } else {
        // Check PID file as fallback
        if work.supervisor_pid.exists() {
            if let Ok(pid_str) = std::fs::read_to_string(&work.supervisor_pid) {
                println!(
                    "Supervisor PID file exists: {} (pid {})",
                    work.supervisor_pid.display(),
                    pid_str.trim()
                );
                println!("  (IPC socket not found; supervisor may have crashed)");
                println!();
            }
        } else {
            println!("Supervisor: not running");
            println!();
        }
    }

    // 2. Always show sync status from RocksDB
    print_single_network_sync_status(workdir).await
}

/// Print the RocksDB sync status for a single-network workdir.
///
/// Extracted verbatim from `cmd_status`'s "sync status from RocksDB" block so
/// both the single-network `cmd_status` and the orchestrator aggregate can reuse
/// it against each network's subdir store.
async fn print_single_network_sync_status(workdir: &Path) -> Result<()> {
    let config = load_config(workdir)?;
    let store_paths = resolve_store_paths(workdir, &config.store);

    let secondary_path = secondary_store_path(&store_paths.domain_data, SecondaryStoreOwner::Cli);
    match CkbadgerStore::open_domain_secondary_with_runtime(
        &store_paths.domain_data,
        &secondary_path,
        store_runtime_config(&config.store),
    ) {
        Ok(store) => match store.get_sync_status() {
            Ok(status) => {
                println!("Sync status:");
                println!("  Tip block:           {}", status.tip_block_number);
                println!("  Total transactions:  {}", status.total_transactions);
                println!("  Total cells created: {}", status.total_cells_created);
                println!("  Total cells consumed:{}", status.total_cells_consumed);
                if status.deep_fork_detected {
                    println!("  WARNING: deep fork detected");
                }
            }
            Err(e) => {
                println!("Could not read sync status: {}", e);
            }
        },
        Err(e) => {
            println!("Could not open store: {} (has the indexer run yet?)", e);
        }
    }

    Ok(())
}

/// Aggregate `status` across all networks in an orchestrator root: query the
/// orchestrator supervisor for service status, then print each network's sync
/// status from its subdir store.
async fn cmd_status_orchestrator(root: &Path) -> Result<()> {
    use ckbadger_config::{load_orchestrator_config, network_workdir};
    let orch = load_orchestrator_config(root)?;
    let root_work = WorkDir::resolve(root);

    if root_work.indexer_sock.exists() {
        match ckbadger_ipc::ipc_request(
            &root_work.indexer_sock,
            &ckbadger_ipc::IpcRequest::GetServiceStatus,
        )
        .await
        {
            Ok(ckbadger_ipc::IpcResponse::ServiceStatus { services }) => {
                println!("Services:");
                for svc in &services {
                    println!(
                        "  {}: {} (pid {}, uptime {}s)",
                        svc.name, svc.status, svc.pid, svc.uptime_secs
                    );
                }
                println!();
            }
            Ok(other) => println!("Unexpected IPC response: {:?}", other),
            Err(e) => println!("Supervisor not reachable: {}", e),
        }
    } else {
        println!("Supervisor: not running\n");
    }

    for entry in &orch.networks {
        let sub = network_workdir(root, entry);
        println!("[{}] {}", entry.name, sub.display());
        // Delegate to the existing per-workdir sync-status reader.
        if let Err(e) = print_single_network_sync_status(&sub).await {
            println!("  could not read sync status: {e}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// verify command
// ---------------------------------------------------------------------------

/// Explorer API base for verify, network-aware, failing fast on unknown nets.
fn verify_explorer_url(network: &str) -> Result<String> {
    ckbadger_common::network::explorer_api_url(network)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("no explorer API URL for network '{network}'"))
}

#[cfg(test)]
mod verify_url_tests {
    use super::*;
    #[test]
    fn explorer_url_is_network_aware() {
        assert_eq!(
            verify_explorer_url("mainnet").unwrap(),
            "https://mainnet-api.explorer.nervos.org"
        );
        assert_eq!(
            verify_explorer_url("testnet").unwrap(),
            "https://testnet-api.explorer.nervos.org"
        );
        assert!(verify_explorer_url("devnet").is_err());
    }
}

async fn cmd_verify(workdir: &Path, args: &VerifyArgs) -> Result<()> {
    let config = load_config(workdir)?;

    let depth = match args.depth.to_lowercase().as_str() {
        "fast" => indexer_verify::checks::CheckTier::Fast,
        "sampling" => indexer_verify::checks::CheckTier::Sampling,
        _ => bail!("Invalid depth: {}. Use fast or sampling", args.depth),
    };

    let verify_args = indexer_verify::VerifyArgs {
        api_url: format!("http://{}:{}/api/v1", config.api.host, config.api.port),
        rpc_url: Some(config.ckb.rpc_url.clone()),
        explorer_url: verify_explorer_url(&config.ckb.network)?,
        no_explorer: false,
        depth,
        sample_count: 1000,
        seed: 42,
        tolerance: 0.001,
        format: indexer_verify::OutputFormat::Text,
        checks: None,
        list_checks: args.list_checks,
        cache_dir: None,
    };

    tokio::task::spawn_blocking(move || indexer_verify::run(verify_args))
        .await
        .expect("verify task panicked")?;

    Ok(())
}

// ---------------------------------------------------------------------------
// label-import command
// ---------------------------------------------------------------------------

async fn cmd_label_import(workdir: &Path) -> Result<()> {
    let config = load_config(workdir)?;
    let work = WorkDir::resolve(workdir);
    let store_paths = resolve_store_paths(workdir, &config.store);

    let import_config = LabelImportServiceConfig {
        domain_data_path: store_paths.domain_data.to_string_lossy().to_string(),
        append_only_data_path: store_paths.append_only_data.to_string_lossy().to_string(),
        metadata_path: work.metadata.map(|p| p.to_string_lossy().to_string()),
        network: config.ckb.network.clone(),
        store_runtime_config: store_runtime_config(&config.store),
    };

    run_label_import(import_config).await
}

// ---------------------------------------------------------------------------
// init command
// ---------------------------------------------------------------------------

/// Scaffold an orchestrator work directory: a top-level `ckbadger.toml`
/// (`[[network]]` array) plus one standard single-network workdir subdir per
/// network (each with its own `config.toml` and data/log dirs).
fn cmd_init(root: &Path, args: &InitArgs) -> Result<()> {
    let orch_path = root.join("ckbadger.toml");
    if orch_path.exists() {
        println!("Already initialized: {}", orch_path.display());
        return Ok(());
    }

    // Networks + their api ports.
    let mut nets: Vec<(&str, u16)> = vec![("mainnet", 8101)];
    if args.with_testnet {
        nets.push(("testnet", 8102));
    }

    std::fs::create_dir_all(root)
        .with_context(|| format!("failed to create {}", root.display()))?;
    let orch_toml = default_orchestrator_toml(&nets.iter().map(|(n, _)| *n).collect::<Vec<_>>());
    std::fs::write(&orch_path, orch_toml)
        .with_context(|| format!("failed to write {}", orch_path.display()))?;

    for (name, api_port) in &nets {
        let sub = root.join(name);
        let sub_work = WorkDir::resolve(&sub);
        let sub_store = resolve_store_paths(&sub, &CkbadgerConfig::default().store);
        std::fs::create_dir_all(&sub_store.domain_data)
            .with_context(|| format!("failed to create {}", sub_store.domain_data.display()))?;
        std::fs::create_dir_all(&sub_store.append_only_data).with_context(|| {
            format!("failed to create {}", sub_store.append_only_data.display())
        })?;
        std::fs::create_dir_all(&sub_work.log_dir)
            .with_context(|| format!("failed to create {}", sub_work.log_dir.display()))?;
        std::fs::write(&sub_work.config_path, default_config_toml(name, *api_port))
            .with_context(|| format!("failed to write {}", sub_work.config_path.display()))?;
        info!(network = %name, path = %sub.display(), "network workdir initialized");
        println!("initialized network '{}' at {}", name, sub.display());
    }

    println!("Orchestrator written: {}", orch_path.display());
    println!("Edit each <network>/config.toml to set [ckb].workdir before `ckbadger run`.");
    Ok(())
}

// ---------------------------------------------------------------------------
// purge command
// ---------------------------------------------------------------------------

fn cmd_purge(workdir: &Path, args: &PurgeArgs) -> Result<()> {
    let work = WorkDir::resolve(workdir);

    if !work.is_initialized() {
        bail!(
            "work directory not initialized (no config.toml at {})",
            work.config_path.display()
        );
    }

    let config = load_config(workdir)?;
    let store_paths = resolve_store_paths(workdir, &config.store);

    if !args.confirm {
        bail!("purge requires --confirm flag to proceed");
    }

    let mut deleted = Vec::new();

    // Delete domain data contents
    if store_paths.domain_data.exists() {
        remove_dir_contents(&store_paths.domain_data)
            .with_context(|| format!("failed to purge {}", store_paths.domain_data.display()))?;
        deleted.push(format!("  {}/", store_paths.domain_data.display()));
    }

    // Delete append-only data contents
    if store_paths.append_only_data.exists() {
        remove_dir_contents(&store_paths.append_only_data).with_context(|| {
            format!("failed to purge {}", store_paths.append_only_data.display())
        })?;
        deleted.push(format!("  {}/", store_paths.append_only_data.display()));
    }

    // Delete decoder cache contents (DOB decoder RISC-V binaries)
    if store_paths.decoder_cache.exists() {
        remove_dir_contents(&store_paths.decoder_cache)
            .with_context(|| format!("failed to purge {}", store_paths.decoder_cache.display()))?;
        deleted.push(format!("  {}/", store_paths.decoder_cache.display()));
    }

    // Delete media directory contents (decoded DOB media blobs)
    if work.dob_decode_dir.exists() {
        remove_dir_contents(&work.dob_decode_dir)
            .with_context(|| format!("failed to purge {}", work.dob_decode_dir.display()))?;
        deleted.push(format!("  {}/", work.dob_decode_dir.display()));
    }

    // Delete run directory contents
    if work.run_dir.exists() {
        remove_dir_contents(&work.run_dir)
            .with_context(|| format!("failed to purge {}", work.run_dir.display()))?;
        deleted.push(format!("  {}/", work.run_dir.display()));
    }

    // Delete bench report directory contents
    if work.bench_dir.exists() {
        remove_dir_contents(&work.bench_dir)
            .with_context(|| format!("failed to purge {}", work.bench_dir.display()))?;
        deleted.push(format!("  {}/", work.bench_dir.display()));
    }

    for secondary_path in known_domain_secondary_store_paths(&store_paths.domain_data) {
        if remove_dir_if_exists(&secondary_path)
            .with_context(|| format!("failed to purge {}", secondary_path.display()))?
        {
            deleted.push(format!("  {}/", secondary_path.display()));
        }
    }

    for secondary_path in known_append_only_secondary_store_paths(&store_paths.append_only_data) {
        if remove_dir_if_exists(&secondary_path)
            .with_context(|| format!("failed to purge {}", secondary_path.display()))?
        {
            deleted.push(format!("  {}/", secondary_path.display()));
        }
    }

    if deleted.is_empty() {
        println!("Nothing to purge.");
    } else {
        info!(path = %work.root.display(), "purged derived data");
        println!("Purged derived data:");
        for d in &deleted {
            println!("{d}");
        }
        println!();
        println!("Preserved:");
        println!("  {}", work.config_path.display());
        if let Some(ref md) = work.metadata {
            println!("  {}/", md.display());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

const BANNER: &str = r#"  _______ __    ___  ___   ___  ____________
 / ___/ //_/___/ _ )/ _ | / _ \/ ___/ __/ _ \
/ /__/ ,< /___/ _  / __ |/ // / (_ / _// , _/
\___/_/|_|   /____/_/ |_/____/\___/___/_/|_|"#;

fn print_banner() {
    let use_color = std::io::stdout().is_terminal();
    if use_color {
        // Green banner + dim version
        println!("\x1b[32m{BANNER}\x1b[0m");
        println!("\x1b[90m{BUILD_VERSION}\x1b[0m");
    } else {
        println!("{BANNER}");
        println!("{BUILD_VERSION}");
    }
}

fn print_startup_info(
    workdir: &Path,
    config: &CkbadgerConfig,
    work: &WorkDir,
    services: &[String],
    fd_limit: u64,
) {
    let use_color = std::io::stdout().is_terminal();
    let dim = if use_color { "\x1b[90m" } else { "" };
    let cyan = if use_color { "\x1b[36m" } else { "" };
    let reset = if use_color { "\x1b[0m" } else { "" };
    let bold = if use_color { "\x1b[1m" } else { "" };
    let yellow = if use_color { "\x1b[33m" } else { "" };

    println!();

    // System environment
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get().to_string())
        .unwrap_or_else(|_| "?".to_string());
    let ram_info = get_total_ram_gb()
        .map(|gb| format!(" · {gb} GB RAM"))
        .unwrap_or_default();
    let fd_info = if fd_limit < FD_LIMIT_TARGET {
        format!(" · {yellow}fd limit {fd_limit} (WARNING: below target {FD_LIMIT_TARGET}){reset}")
    } else {
        format!(" · fd limit {fd_limit}")
    };
    println!(
        "  {dim}System{reset}      {} {} · {cpus} cores{ram_info}{fd_info}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!(
        "  {dim}Network{reset}     {bold}{}{reset}",
        config.ckb.network
    );
    println!("  {dim}CKB RPC{reset}     {}", config.ckb.rpc_url);
    if let Some(ref ckb_wd) = config.ckb.workdir {
        if !ckb_wd.is_empty() {
            let resolved = resolve_workdir_path(workdir, ckb_wd);
            println!("  {dim}CKB node{reset}    {}", resolved.display());
        }
    }
    println!("  {dim}Workdir{reset}     {}", work.root.display());
    println!("  {dim}Log level{reset}   {}", config.log.level);

    // Services
    println!();
    println!("  {dim}Services{reset}");
    for svc in services {
        match svc.as_str() {
            "frontend-server" => {
                println!(
                    "    {dim}Frontend{reset}    {cyan}http://{}:{}{reset}",
                    config.frontend.host, config.frontend.port
                );
            }
            "api" => {
                println!(
                    "    {dim}API{reset}         {cyan}http://{}:{}{reset}",
                    config.api.host, config.api.port
                );
            }
            "indexer" => {
                println!(
                    "    {dim}Indexer{reset}     threshold={} poll={}ms",
                    config.indexer.bulk_sync_threshold, config.indexer.poll_interval_ms
                );
            }
            other => {
                println!("    {other}");
            }
        }
    }

    // Storage
    let store_paths = resolve_store_paths(workdir, &config.store);
    println!();
    println!("  {dim}Storage{reset}");
    println!(
        "    {dim}Domain{reset}      {}",
        store_paths.domain_data.display()
    );
    println!(
        "    {dim}Append{reset}      {}",
        store_paths.append_only_data.display()
    );
    if let Some(gb) = config.store.memory_budget_gb {
        println!("    {dim}RocksDB{reset}     {gb} GB memory budget");
    }
    println!("    {dim}Logs{reset}        {}", work.log_dir.display());

    // Tips
    println!();
    println!(
        "  {dim}Tip: use {reset}{bold}ckbadger tui{reset}{dim} for live monitoring, \
         {reset}{bold}ckbadger status{reset}{dim} for sync progress{reset}"
    );
    println!();
}

fn get_total_ram_gb() -> Option<u64> {
    unsafe {
        let pages = libc::sysconf(libc::_SC_PHYS_PAGES);
        let page_size = libc::sysconf(libc::_SC_PAGESIZE);
        if pages > 0 && page_size > 0 {
            Some((pages as u64 * page_size as u64) / (1024 * 1024 * 1024))
        } else {
            None
        }
    }
}

#[cfg(test)]
fn parse_meminfo_total_gb(content: &str) -> Option<u64> {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb / 1_048_576);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Remove all contents of a directory without removing the directory itself.
fn remove_dir_contents(dir: &std::path::Path) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        } else {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }
    Ok(())
}

fn remove_dir_if_exists(path: &std::path::Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    std::fs::remove_dir_all(path)
        .with_context(|| format!("failed to remove {}", path.display()))?;
    Ok(true)
}

fn parse_only_flag(only: &Option<String>) -> Vec<String> {
    match only {
        Some(s) => s.split(',').map(|s| s.trim().to_string()).collect(),
        None => vec![
            "indexer".to_string(),
            "api".to_string(),
            "frontend-server".to_string(),
        ],
    }
}

/// Resolve frontend assets directory.
fn resolve_frontend_dir(work: &WorkDir) -> Option<PathBuf> {
    // Check work_dir/frontend/dist first
    let work_frontend_dist = work.root.join("frontend").join("dist");
    if work_frontend_dist.is_dir() {
        return Some(work_frontend_dist);
    }

    // Check work_dir/frontend/
    let work_frontend = work.root.join("frontend");
    if work_frontend.is_dir() {
        return Some(work_frontend);
    }

    // Check share/frontend/dist and share/frontend/
    if let Some(share) = resolve_share_dir() {
        let share_frontend_dist = share.join("frontend").join("dist");
        if share_frontend_dist.is_dir() {
            return Some(share_frontend_dist);
        }
        let share_frontend = share.join("frontend");
        if share_frontend.is_dir() {
            return Some(share_frontend);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::path::Path;
    use tempfile::TempDir;

    // -- clap metadata --

    #[test]
    fn test_cli_help_uses_project_positioning_title() {
        let mut cmd = Cli::command();
        let help = cmd.render_help().to_string();

        assert!(help.contains("A local-first and agent-friendly CKB explorer"));
    }

    #[test]
    fn test_format_build_version_omits_main_branch_label() {
        assert_eq!(
            build_version_format::format_build_version("0.1.0", Some("main"), "abcdef123456"),
            "0.1.0@abcdef123456"
        );
    }

    #[test]
    fn test_format_build_version_includes_non_main_branch_label_verbatim() {
        assert_eq!(
            build_version_format::format_build_version(
                "0.1.0",
                Some("feature/foo"),
                "abcdef123456"
            ),
            "0.1.0+feature/foo@abcdef123456"
        );
    }

    #[test]
    fn test_cli_version_uses_semver_optional_branch_and_commit_hash() {
        let cmd = Cli::command();
        let version = cmd.get_version().expect("cli version should be present");
        let (version_prefix, hash) = version
            .rsplit_once('@')
            .expect("version should contain a single '@'");

        assert!(
            !version.contains("+main@"),
            "main branch should not be rendered explicitly: {version}"
        );
        assert!(
            !version_prefix.is_empty(),
            "version prefix should not be empty"
        );
        assert!(hash.len() >= 7, "hash should use at least 7 hex chars");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex: {hash}"
        );
    }

    // -- init command --

    /// Scaffold a single-network workdir at `root` (the pre-orchestrator layout).
    /// Used by the purge tests, which operate on a plain single-network workdir.
    fn init_single_network(root: &Path) {
        let work = WorkDir::resolve(root);
        let store_paths = resolve_store_paths(root, &CkbadgerConfig::default().store);
        std::fs::create_dir_all(&store_paths.domain_data).unwrap();
        std::fs::create_dir_all(&store_paths.append_only_data).unwrap();
        std::fs::create_dir_all(&work.log_dir).unwrap();
        std::fs::create_dir_all(&work.perf_dir).unwrap();
        std::fs::write(&work.config_path, default_config_toml("mainnet", 8101)).unwrap();
    }

    #[test]
    fn test_init_creates_orchestrator_structure() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(
            &root,
            &InitArgs {
                with_testnet: false,
            },
        )
        .unwrap();

        // Orchestrator config at the root.
        assert!(
            root.join("ckbadger.toml").exists(),
            "orchestrator ckbadger.toml should exist"
        );
        assert!(
            is_orchestrator(&root),
            "root should be detected as an orchestrator"
        );

        // Default is mainnet-only: mainnet subdir is a full single-network workdir.
        let mainnet = WorkDir::resolve(&root.join("mainnet"));
        assert!(mainnet.config_path.exists(), "mainnet/config.toml exists");
        assert!(mainnet.domain_data.exists(), "mainnet data/domain/ exists");
        assert!(
            mainnet.append_only_data.exists(),
            "mainnet data/append-only/ exists"
        );
        assert!(mainnet.log_dir.exists(), "mainnet run/logs/ exists");

        // No testnet subdir without --with-testnet.
        assert!(
            !root.join("testnet").exists(),
            "testnet subdir should not exist by default"
        );
    }

    #[test]
    fn test_init_with_testnet_scaffolds_second_network() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(&root, &InitArgs { with_testnet: true }).unwrap();

        // Both networks listed in the orchestrator config.
        let orch = load_orchestrator_config(&root).unwrap();
        let names: Vec<&str> = orch.networks.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["mainnet", "testnet"]);

        // Testnet subdir is a full workdir with the testnet config on port 8102.
        let testnet = WorkDir::resolve(&root.join("testnet"));
        assert!(testnet.config_path.exists(), "testnet/config.toml exists");
        let cfg = load_config(&root.join("testnet")).unwrap();
        assert_eq!(cfg.ckb.network, "testnet");
        assert_eq!(cfg.api.port, 8102);
    }

    #[test]
    fn test_init_mainnet_config_matches_defaults() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(
            &root,
            &InitArgs {
                with_testnet: false,
            },
        )
        .unwrap();

        // The written mainnet config should parse and match defaults.
        let cfg = load_config(&root.join("mainnet")).unwrap();
        assert_eq!(cfg, ckbadger_config::CkbadgerConfig::default());
    }

    #[test]
    fn test_init_idempotent_when_already_initialized() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(
            &root,
            &InitArgs {
                with_testnet: false,
            },
        )
        .unwrap();

        // Modify the orchestrator config so we can verify it isn't overwritten.
        let orch_path = root.join("ckbadger.toml");
        let original = std::fs::read_to_string(&orch_path).unwrap();
        let modified = format!("# sentinel comment\n{original}");
        std::fs::write(&orch_path, &modified).unwrap();

        // Second init should NOT overwrite (orchestrator already exists).
        cmd_init(
            &root,
            &InitArgs {
                with_testnet: false,
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(&orch_path).unwrap();
        assert!(
            content.contains("sentinel comment"),
            "orchestrator config should not be overwritten on re-init"
        );
    }

    #[test]
    fn test_init_creates_parent_directories() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("deeply").join("nested").join("workdir");

        cmd_init(
            &root,
            &InitArgs {
                with_testnet: false,
            },
        )
        .unwrap();

        assert!(root.join("ckbadger.toml").exists());
        assert!(root.join("mainnet/config.toml").exists());
        assert!(root.join("mainnet/data/domain").exists());
        assert!(root.join("mainnet/data/append-only").exists());
        assert!(root.join("mainnet/run/logs").exists());
    }

    // -- purge command --

    #[test]
    fn test_purge_requires_confirm_flag() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        init_single_network(&root);

        let args = PurgeArgs { confirm: false };
        let result = cmd_purge(&root, &args);
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("--confirm"),
            "error should mention --confirm: {err}"
        );
    }

    #[test]
    fn test_purge_fails_if_not_initialized() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        let args = PurgeArgs { confirm: true };
        let result = cmd_purge(&root, &args);
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("not initialized"),
            "error should mention not initialized: {err}"
        );
    }

    #[test]
    fn test_purge_deletes_data_and_run_contents() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        init_single_network(&root);

        // Create some files in the data directories
        let domain_file = root.join("data/domain/test.db");
        std::fs::write(&domain_file, "domain data").unwrap();

        let append_file = root.join("data/append-only/test.db");
        std::fs::write(&append_file, "append data").unwrap();

        let decoder_cache_dir = root.join("data/decoder-cache");
        std::fs::create_dir_all(&decoder_cache_dir).unwrap();
        let decoder_file = decoder_cache_dir.join("0xabc.bin");
        std::fs::write(&decoder_file, "decoder binary").unwrap();

        let log_file = root.join("run/logs/test.log");
        std::fs::write(&log_file, "log data").unwrap();

        let run_file = root.join("run/supervisor.pid");
        std::fs::write(&run_file, "12345").unwrap();

        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        // Data should be gone
        assert!(!domain_file.exists(), "domain data should be deleted");
        assert!(!append_file.exists(), "append data should be deleted");
        assert!(!decoder_file.exists(), "decoder cache should be deleted");
        assert!(!log_file.exists(), "log files should be deleted");
        assert!(!run_file.exists(), "run files should be deleted");

        // Directories themselves should still exist
        assert!(
            root.join("data/domain").exists(),
            "domain dir should remain"
        );
        assert!(
            root.join("data/append-only").exists(),
            "append-only dir should remain"
        );
        assert!(
            decoder_cache_dir.exists(),
            "decoder-cache dir should remain"
        );
        assert!(root.join("run").exists(), "run dir should remain");
    }

    #[test]
    fn test_purge_preserves_config_and_metadata() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        init_single_network(&root);

        // Create metadata directory
        std::fs::create_dir_all(root.join("metadata/tokens")).unwrap();
        std::fs::write(root.join("metadata/tokens/test.toml"), "name = \"Test\"").unwrap();

        // Create some data to be purged
        std::fs::write(root.join("data/domain/test.db"), "data").unwrap();

        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        // Config and metadata should be preserved
        assert!(
            root.join("config.toml").exists(),
            "config should be preserved"
        );
        assert!(
            root.join("metadata").exists(),
            "metadata dir should be preserved"
        );
        assert!(
            root.join("metadata/tokens/test.toml").exists(),
            "metadata contents should be preserved"
        );

        // Data should be gone
        assert!(
            !root.join("data/domain/test.db").exists(),
            "data should be purged"
        );
    }

    #[test]
    fn test_purge_preserves_perf_contents() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        init_single_network(&root);

        let perf_run_dir = root.join("perf/bulk-sync/run-1");
        std::fs::create_dir_all(&perf_run_dir).unwrap();
        let perf_metrics = perf_run_dir.join("metrics.env");
        std::fs::write(&perf_metrics, "status=completed\n").unwrap();

        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        assert!(perf_metrics.exists(), "perf artifacts should be preserved");
    }

    #[test]
    fn test_purge_deletes_bench_reports() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        init_single_network(&root);

        let bench_dir = root.join("bench");
        std::fs::create_dir_all(&bench_dir).unwrap();
        let report_file = bench_dir.join("2026-04-01T12-00-00.json");
        std::fs::write(&report_file, "{}").unwrap();

        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        assert!(!report_file.exists(), "bench reports should be purged");
        assert!(
            bench_dir.exists(),
            "bench directory itself should be preserved"
        );
    }

    #[test]
    fn test_purge_handles_empty_directories() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        init_single_network(&root);

        // Purge with empty data directories should succeed
        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();
    }

    #[test]
    fn test_purge_handles_nested_directories() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        init_single_network(&root);

        // Create nested directory structure in domain data
        let nested = root.join("data/domain/subdir/deep/deeper");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("file.db"), "deep data").unwrap();

        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        assert!(
            !root.join("data/domain/subdir").exists(),
            "nested dirs should be deleted"
        );
        assert!(
            root.join("data/domain").exists(),
            "top-level dir should remain"
        );
    }

    #[test]
    fn test_purge_deletes_secondary_store_directories() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        init_single_network(&root);

        let secondary_dirs = [
            root.join("data/domain-api-secondary"),
            root.join("data/domain-tui-secondary"),
            root.join("data/domain-cli-secondary"),
            root.join("data/append-only-api-secondary"),
        ];

        for path in &secondary_dirs {
            std::fs::create_dir_all(path.join("nested")).unwrap();
            std::fs::write(path.join("nested/LOCK"), "secondary state").unwrap();
        }

        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        for path in &secondary_dirs {
            assert!(
                !path.exists(),
                "secondary store dir should be deleted: {}",
                path.display()
            );
        }
    }

    #[test]
    fn test_purge_respects_store_paths_from_config() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        init_single_network(&root);

        let config_path = root.join("config.toml");
        let custom_config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace(
                r#"domain_data_path = "data/domain""#,
                r#"domain_data_path = "custom/domain""#,
            )
            .replace(
                r#"append_only_data_path = "data/append-only""#,
                r#"append_only_data_path = "custom/append-only""#,
            );
        std::fs::write(&config_path, custom_config).unwrap();

        let config = load_config(&root).unwrap();
        let store_paths = resolve_store_paths(&root, &config.store);
        std::fs::create_dir_all(&store_paths.domain_data).unwrap();
        std::fs::create_dir_all(&store_paths.append_only_data).unwrap();
        std::fs::write(store_paths.domain_data.join("custom.db"), "domain").unwrap();
        std::fs::write(store_paths.append_only_data.join("custom.db"), "append").unwrap();

        cmd_purge(&root, &PurgeArgs { confirm: true }).unwrap();

        assert!(
            store_paths.domain_data.exists(),
            "custom domain dir should remain"
        );
        assert!(
            store_paths.append_only_data.exists(),
            "custom append dir should remain"
        );
        assert!(
            !store_paths.domain_data.join("custom.db").exists(),
            "custom domain contents should be purged"
        );
        assert!(
            !store_paths.append_only_data.join("custom.db").exists(),
            "custom append-only contents should be purged"
        );
    }

    // -- remove_dir_contents --

    #[test]
    fn test_remove_dir_contents_removes_files_and_dirs() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        std::fs::write(root.join("file1.txt"), "data").unwrap();
        std::fs::write(root.join("file2.txt"), "data").unwrap();
        let sub = root.join("subdir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("nested.txt"), "data").unwrap();

        remove_dir_contents(root).unwrap();

        assert!(root.exists(), "directory itself should remain");
        assert!(
            std::fs::read_dir(root).unwrap().count() == 0,
            "directory should be empty"
        );
    }

    #[test]
    fn test_remove_dir_contents_empty_dir() {
        let dir = TempDir::new().unwrap();
        remove_dir_contents(dir.path()).unwrap();
        assert!(dir.path().exists());
    }

    // -- parse_only_flag --

    #[test]
    fn test_parse_only_flag_none_returns_default() {
        let services = parse_only_flag(&None);
        assert!(services.contains(&"indexer".to_string()));
        assert!(services.contains(&"api".to_string()));
        assert!(services.contains(&"frontend-server".to_string()));
    }

    #[test]
    fn test_parse_only_flag_single() {
        let services = parse_only_flag(&Some("indexer".to_string()));
        assert_eq!(services, vec!["indexer".to_string()]);
    }

    #[test]
    fn test_parse_only_flag_multiple() {
        let services = parse_only_flag(&Some("indexer,api".to_string()));
        assert_eq!(services, vec!["indexer".to_string(), "api".to_string()]);
    }

    #[test]
    fn test_parse_only_flag_with_spaces() {
        let services = parse_only_flag(&Some(" indexer , api ".to_string()));
        assert_eq!(services, vec!["indexer".to_string(), "api".to_string()]);
    }

    // -- startup info --

    #[test]
    fn test_parse_meminfo_total_gb_valid() {
        let content = "MemTotal:       65758916 kB\nMemFree:        12345678 kB\n";
        // 65758916 / 1048576 = 62
        assert_eq!(parse_meminfo_total_gb(content), Some(62));
    }

    #[test]
    fn test_parse_meminfo_total_gb_missing_field() {
        let content = "MemFree:        12345678 kB\nMemAvailable:   9876543 kB\n";
        assert_eq!(parse_meminfo_total_gb(content), None);
    }

    #[test]
    fn test_parse_meminfo_total_gb_empty() {
        assert_eq!(parse_meminfo_total_gb(""), None);
    }

    #[test]
    fn test_parse_meminfo_total_gb_small_value() {
        // 8 GB = 8388608 kB
        let content = "MemTotal:        8388608 kB\n";
        assert_eq!(parse_meminfo_total_gb(content), Some(8));
    }

    #[test]
    fn test_print_startup_info_does_not_panic() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let config = CkbadgerConfig::default();
        let work = WorkDir::resolve(root);
        let services = vec![
            "indexer".to_string(),
            "api".to_string(),
            "frontend-server".to_string(),
        ];
        // Should not panic with any config
        print_startup_info(root, &config, &work, &services, FD_LIMIT_TARGET);
    }

    #[test]
    fn test_print_startup_info_partial_services() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let config = CkbadgerConfig::default();
        let work = WorkDir::resolve(root);
        let services = vec!["indexer".to_string()];
        print_startup_info(root, &config, &work, &services, FD_LIMIT_TARGET);
    }

    #[test]
    fn test_print_startup_info_custom_config() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let mut config = CkbadgerConfig::default();
        config.ckb.network = "testnet".to_string();
        config.ckb.workdir = Some("ckb-node".to_string());
        config.store.memory_budget_gb = Some(48);
        let work = WorkDir::resolve(root);
        let services = vec!["api".to_string(), "frontend-server".to_string()];
        print_startup_info(root, &config, &work, &services, FD_LIMIT_TARGET);
    }

    // -- resolve_frontend_dir --

    #[test]
    fn test_resolve_frontend_dir_none_when_not_present() {
        let dir = TempDir::new().unwrap();
        let work = WorkDir::resolve(dir.path());
        assert!(resolve_frontend_dir(&work).is_none());
    }

    #[test]
    fn test_resolve_frontend_dir_finds_workdir_frontend() {
        let dir = TempDir::new().unwrap();
        let frontend = dir.path().join("frontend");
        std::fs::create_dir(&frontend).unwrap();

        let work = WorkDir::resolve(dir.path());
        assert_eq!(resolve_frontend_dir(&work), Some(frontend));
    }

    #[test]
    fn test_resolve_frontend_dir_prefers_workdir_frontend_dist() {
        let dir = TempDir::new().unwrap();
        let frontend = dir.path().join("frontend");
        let dist = frontend.join("dist");
        std::fs::create_dir_all(&dist).unwrap();

        let work = WorkDir::resolve(dir.path());
        assert_eq!(resolve_frontend_dir(&work), Some(dist));
    }

    fn write_test_ckb_config(root: &Path) {
        let ckb_workdir = root.join("ckb-node");
        let ckb_db_path = ckb_workdir.join("data").join("db");
        std::fs::create_dir_all(&ckb_workdir).unwrap();
        std::fs::create_dir_all(&ckb_db_path).unwrap();
        std::fs::write(
            ckb_workdir.join("ckb.toml"),
            r#"
data_dir = "data"
"#,
        )
        .unwrap();
    }

    #[test]
    fn test_build_indexer_service_config_uses_cli_build_version() {
        let dir = TempDir::new().unwrap();
        write_test_ckb_config(dir.path());

        let work = WorkDir::resolve(dir.path());
        let mut config = CkbadgerConfig::default();
        config.ckb.workdir = Some("ckb-node".to_string());
        let ckb_paths = resolve_ckb_paths(dir.path(), &config.ckb).unwrap();

        let service =
            build_indexer_service_config(dir.path(), &work, &config, &ckb_paths, BUILD_VERSION)
                .unwrap();

        assert_eq!(service.build_version, BUILD_VERSION);
    }

    // -- fd limit --

    #[test]
    #[cfg(unix)]
    fn test_raise_fd_limit_returns_nonzero() {
        let limit = raise_fd_limit().expect("raise_fd_limit should succeed");
        assert!(limit > 0, "raise_fd_limit() must return a positive value");
    }

    #[test]
    #[cfg(unix)]
    fn test_raise_fd_limit_is_idempotent() {
        let first = raise_fd_limit().expect("raise_fd_limit should succeed");
        let second = raise_fd_limit().expect("raise_fd_limit should succeed");
        assert_eq!(first, second, "raise_fd_limit() must be idempotent");
    }

    #[test]
    fn test_check_fd_limit_for_indexer_passes_at_and_above_min() {
        assert!(check_fd_limit_for_indexer(FD_LIMIT_MIN).is_ok());
        assert!(check_fd_limit_for_indexer(FD_LIMIT_TARGET).is_ok());
        assert!(check_fd_limit_for_indexer(u64::MAX).is_ok());
    }

    #[test]
    fn test_check_fd_limit_for_indexer_fails_below_min() {
        let err = check_fd_limit_for_indexer(FD_LIMIT_MIN - 1).unwrap_err();
        assert!(
            format!("{err}").contains("too low"),
            "error should explain the limit is too low: {err}"
        );
        assert!(
            check_fd_limit_for_indexer(256).is_err(),
            "macOS default limit 256 must be rejected"
        );
    }

    #[test]
    fn test_check_fd_limit_for_indexer_error_includes_fix_instructions() {
        let err = check_fd_limit_for_indexer(100).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("launchctl") || msg.contains("systemd"),
            "error should include fix instructions: {msg}"
        );
    }
}
