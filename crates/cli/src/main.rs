mod supervisor;

#[cfg(test)]
mod build_version_format;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use ckbadger_config::{
    default_config_toml, load_config, resolve_ckb_paths, resolve_share_dir, resolve_store_paths,
    resolve_workdir_path, CkbadgerConfig, ResolvedCkbPaths, StoreConfig, WorkDir,
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
    Init,
    /// Start all services (supervisor mode)
    Run(RunArgs),
    /// Terminal monitoring UI
    Tui,
    /// Show sync and service status
    Status,
    /// Verify data integrity
    Verify(VerifyArgs),
    /// Import token and script labels
    LabelImport(LabelImportArgs),
    /// Purge derived data, keep config
    Purge(PurgeArgs),
    /// Internal subprocess commands (not user-facing)
    #[command(hide = true)]
    Internal(InternalArgs),
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
        Command::Init => {
            // For init, set up tracing with default "info" level since
            // no config file exists yet.
            init_tracing("info");
            cmd_init(&workdir)
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
    })
}

// ---------------------------------------------------------------------------
// run command
// ---------------------------------------------------------------------------

async fn cmd_run(workdir: &Path, args: &RunArgs) -> Result<()> {
    let config = load_config(workdir)?;
    let work = WorkDir::resolve(workdir);

    let services = parse_only_flag(&args.only);

    if services.is_empty() {
        bail!("no services selected to run");
    }

    print_startup_info(workdir, &config, &work, &services);

    supervisor::run_supervisor(&work, &config, services).await
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
                ckb_db_path: ckb_paths.ckb_db_path.to_string_lossy().to_string(),
                store_runtime_config,
                dob_decode_dir: work.dob_decode_dir.clone(),
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
    let config = load_config(workdir)?;
    let work = WorkDir::resolve(workdir);
    let store_paths = resolve_store_paths(workdir, &config.store);

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

// ---------------------------------------------------------------------------
// verify command
// ---------------------------------------------------------------------------

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
        explorer_url: "https://mainnet-api.explorer.nervos.org".to_string(),
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

fn cmd_init(workdir: &Path) -> Result<()> {
    let work = WorkDir::resolve(workdir);
    let default_config = CkbadgerConfig::default();
    let store_paths = resolve_store_paths(workdir, &default_config.store);

    if work.is_initialized() {
        println!("Already initialized: {}", work.config_path.display());
        return Ok(());
    }

    // Create directory structure
    std::fs::create_dir_all(&store_paths.domain_data)
        .with_context(|| format!("failed to create {}", store_paths.domain_data.display()))?;
    std::fs::create_dir_all(&store_paths.append_only_data).with_context(|| {
        format!(
            "failed to create {}",
            store_paths.append_only_data.display()
        )
    })?;
    std::fs::create_dir_all(&work.log_dir)
        .with_context(|| format!("failed to create {}", work.log_dir.display()))?;
    std::fs::create_dir_all(&work.perf_dir)
        .with_context(|| format!("failed to create {}", work.perf_dir.display()))?;

    // Write default config
    let config_content = default_config_toml();
    std::fs::write(&work.config_path, &config_content)
        .with_context(|| format!("failed to write {}", work.config_path.display()))?;

    info!(path = %work.root.display(), "work directory initialized");
    println!("Initialized work directory: {}", work.root.display());
    println!("  config:      {}", work.config_path.display());
    println!("  domain data: {}", store_paths.domain_data.display());
    println!("  append-only: {}", store_paths.append_only_data.display());
    println!("  logs:        {}", work.log_dir.display());
    println!("  perf:        {}", work.perf_dir.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// purge command
// ---------------------------------------------------------------------------

fn cmd_purge(workdir: &Path, args: &PurgeArgs) -> Result<()> {
    let work = WorkDir::resolve(workdir);

    if !work.is_initialized() {
        bail!(
            "work directory not initialized (no ckbadger.toml at {})",
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
) {
    let use_color = std::io::stdout().is_terminal();
    let dim = if use_color { "\x1b[90m" } else { "" };
    let cyan = if use_color { "\x1b[36m" } else { "" };
    let reset = if use_color { "\x1b[0m" } else { "" };
    let bold = if use_color { "\x1b[1m" } else { "" };

    println!();

    // System environment
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get().to_string())
        .unwrap_or_else(|_| "?".to_string());
    let ram_info = get_total_ram_gb()
        .map(|gb| format!(" · {gb} GB RAM"))
        .unwrap_or_default();
    println!(
        "  {dim}System{reset}      {} {} · {cpus} cores{ram_info}",
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
            build_version_format::format_build_version("0.1.0", "main", "abcdef123456"),
            "0.1.0@abcdef123456"
        );
    }

    #[test]
    fn test_format_build_version_includes_non_main_branch_label_verbatim() {
        assert_eq!(
            build_version_format::format_build_version("0.1.0", "feature/foo", "abcdef123456"),
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

    #[test]
    fn test_init_creates_directory_structure() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(&root).unwrap();

        let work = WorkDir::resolve(&root);
        assert!(work.config_path.exists(), "ckbadger.toml should exist");
        assert!(work.domain_data.exists(), "data/domain/ should exist");
        assert!(
            work.append_only_data.exists(),
            "data/append-only/ should exist"
        );
        assert!(work.log_dir.exists(), "run/logs/ should exist");
        assert!(work.perf_dir.exists(), "perf/ should exist");
    }

    #[test]
    fn test_init_writes_valid_config() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(&root).unwrap();

        // The written config should be parseable and match defaults
        let cfg = load_config(&root).unwrap();
        assert_eq!(cfg, ckbadger_config::CkbadgerConfig::default());
    }

    #[test]
    fn test_init_idempotent_when_already_initialized() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(&root).unwrap();

        // Modify the config so we can verify it isn't overwritten
        let config_path = root.join("ckbadger.toml");
        let original = std::fs::read_to_string(&config_path).unwrap();
        let modified = original.replace("mainnet", "testnet");
        std::fs::write(&config_path, &modified).unwrap();

        // Second init should NOT overwrite
        cmd_init(&root).unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            content.contains("testnet"),
            "config should not be overwritten on re-init"
        );
    }

    #[test]
    fn test_init_creates_parent_directories() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("deeply").join("nested").join("workdir");

        cmd_init(&root).unwrap();

        assert!(root.join("ckbadger.toml").exists());
        assert!(root.join("data/domain").exists());
        assert!(root.join("data/append-only").exists());
        assert!(root.join("run/logs").exists());
    }

    // -- purge command --

    #[test]
    fn test_purge_requires_confirm_flag() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(&root).unwrap();

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

        cmd_init(&root).unwrap();

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

        cmd_init(&root).unwrap();

        // Create metadata directory
        std::fs::create_dir_all(root.join("metadata/tokens")).unwrap();
        std::fs::write(root.join("metadata/tokens/test.toml"), "name = \"Test\"").unwrap();

        // Create some data to be purged
        std::fs::write(root.join("data/domain/test.db"), "data").unwrap();

        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        // Config and metadata should be preserved
        assert!(
            root.join("ckbadger.toml").exists(),
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

        cmd_init(&root).unwrap();

        let perf_run_dir = root.join("perf/bulk-sync/run-1");
        std::fs::create_dir_all(&perf_run_dir).unwrap();
        let perf_metrics = perf_run_dir.join("metrics.env");
        std::fs::write(&perf_metrics, "status=completed\n").unwrap();

        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        assert!(perf_metrics.exists(), "perf artifacts should be preserved");
    }

    #[test]
    fn test_purge_handles_empty_directories() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(&root).unwrap();

        // Purge with empty data directories should succeed
        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();
    }

    #[test]
    fn test_purge_handles_nested_directories() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(&root).unwrap();

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

        cmd_init(&root).unwrap();

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

        cmd_init(&root).unwrap();

        let config_path = root.join("ckbadger.toml");
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
        print_startup_info(root, &config, &work, &services);
    }

    #[test]
    fn test_print_startup_info_partial_services() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let config = CkbadgerConfig::default();
        let work = WorkDir::resolve(root);
        let services = vec!["indexer".to_string()];
        print_startup_info(root, &config, &work, &services);
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
        print_startup_info(root, &config, &work, &services);
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
}
