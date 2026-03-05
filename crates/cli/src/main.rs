use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use ckbadger_config::{
    default_config_toml, load_config, resolve_share_dir, resolve_token_labels_path, WorkDir,
};

use ckbadger_api::entry::{run_api, ApiServiceConfig};
use ckbadger_indexer::entry::{
    run_indexer, run_label_import, IndexerServiceConfig, LabelImportServiceConfig,
};
use ckbadger_indexer::verify as indexer_verify;
use ckbadger_store::CkbadgerStore;
use ckbadger_tui::entry::{run_tui, TuiServiceConfig};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "ckbadger", about = "CKB blockchain explorer")]
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
            cmd_status(&workdir)
        }
        Command::Verify(args) => {
            init_tracing_from_config(&workdir);
            cmd_verify(&workdir, &args).await
        }
        Command::LabelImport(_) => {
            init_tracing_from_config(&workdir);
            cmd_label_import(&workdir).await
        }
        Command::Internal(_) => {
            init_tracing_from_config(&workdir);
            println!("internal: not yet implemented");
            Ok(())
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

// ---------------------------------------------------------------------------
// run command
// ---------------------------------------------------------------------------

async fn cmd_run(workdir: &Path, args: &RunArgs) -> Result<()> {
    let config = load_config(workdir)?;
    let work = WorkDir::resolve(workdir);

    let services = parse_only_flag(&args.only);

    let mut handles = vec![];

    if services.contains(&"indexer".to_string()) {
        let token_labels_path = resolve_token_labels_path(&work, resolve_share_dir().as_deref())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let indexer_config = IndexerServiceConfig {
            domain_data_path: work.domain_data.to_string_lossy().to_string(),
            append_only_data_path: work.append_only_data.to_string_lossy().to_string(),
            ckb_rpc_url: config.ckb.rpc_url.clone(),
            ckb_data_path: config.ckb.data_path.clone(),
            token_labels_path,
            batch_size: config.indexer.batch_size,
            poll_interval_ms: config.indexer.poll_interval_ms,
            parallel_fetch_size: config.indexer.parallel_fetch_size,
            pipeline_enabled: config.indexer.pipeline_enabled,
            pipeline_buffer: config.indexer.pipeline_buffer,
            bulk_sync_threshold: config.indexer.bulk_sync_threshold,
            redis_url: None,
        };
        handles.push(tokio::spawn(async move {
            if let Err(e) = run_indexer(indexer_config).await {
                tracing::error!("Indexer failed: {}", e);
            }
        }));
    }

    if services.contains(&"api".to_string()) {
        let frontend_dir = resolve_frontend_dir(&work);
        let api_config = ApiServiceConfig {
            domain_data_path: work.domain_data.to_string_lossy().to_string(),
            append_only_data_path: work.append_only_data.to_string_lossy().to_string(),
            ckb_rpc_url: config.ckb.rpc_url.clone(),
            ckb_network: config.ckb.network.clone(),
            host: config.api.host.clone(),
            port: config.api.port,
            rate_limit: config.api.rate_limit,
            rate_limit_burst: config.api.rate_limit_burst,
            ckb_data_path: config.ckb.data_path.clone(),
            redis_url: None,
            frontend_dir,
        };
        handles.push(tokio::spawn(async move {
            if let Err(e) = run_api(api_config).await {
                tracing::error!("API server failed: {}", e);
            }
        }));
    }

    if handles.is_empty() {
        bail!("no services selected to run");
    }

    info!("Started services: {}", services.join(", "));

    // Wait for ctrl+c
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    // Drop handles to cancel tasks
    drop(handles);

    Ok(())
}

// ---------------------------------------------------------------------------
// tui command
// ---------------------------------------------------------------------------

async fn cmd_tui(workdir: &Path) -> Result<()> {
    let config = load_config(workdir)?;
    let work = WorkDir::resolve(workdir);

    let tui_config = TuiServiceConfig {
        domain_data_path: work.domain_data.to_string_lossy().to_string(),
        append_only_data_path: work.append_only_data.to_string_lossy().to_string(),
        api_url: format!("http://{}:{}/api/v1", config.api.host, config.api.port),
        refresh_ms: 1000,
        redis_url: None,
    };

    run_tui(tui_config).await
}

// ---------------------------------------------------------------------------
// status command
// ---------------------------------------------------------------------------

fn cmd_status(workdir: &Path) -> Result<()> {
    let work = WorkDir::resolve(workdir);

    let primary_path = work.domain_data.to_string_lossy().to_string();
    let secondary_path = format!("{}-cli-secondary", primary_path);
    match CkbadgerStore::open_domain_secondary(primary_path.as_str(), secondary_path.as_str()) {
        Ok(store) => match store.get_sync_status() {
            Ok(status) => {
                println!("Sync tip block:        {}", status.tip_block_number);
                println!("Derived tip block:     {}", status.derived_tip_block_number);
                println!("Total transactions:    {}", status.total_transactions);
                println!("Total cells created:   {}", status.total_cells_created);
                println!("Total cells consumed:  {}", status.total_cells_consumed);
                if status.deep_fork_detected {
                    println!("WARNING: deep fork detected");
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

    let token_labels_path = resolve_token_labels_path(&work, resolve_share_dir().as_deref())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    if token_labels_path.is_empty() {
        bail!(
            "No token labels directory found. Place labels in {}/token-labels/ \
             or install to share/token-labels/.",
            work.root.display()
        );
    }

    let import_config = LabelImportServiceConfig {
        domain_data_path: work.domain_data.to_string_lossy().to_string(),
        append_only_data_path: work.append_only_data.to_string_lossy().to_string(),
        ckb_data_path: config.ckb.data_path.clone(),
        token_labels_path,
        network: config.ckb.network.clone(),
        import_udt: true,
        import_scripts: true,
    };

    run_label_import(import_config).await
}

// ---------------------------------------------------------------------------
// init command
// ---------------------------------------------------------------------------

fn cmd_init(workdir: &Path) -> Result<()> {
    let work = WorkDir::resolve(workdir);

    if work.is_initialized() {
        println!("Already initialized: {}", work.config_path.display());
        return Ok(());
    }

    // Create directory structure
    std::fs::create_dir_all(&work.domain_data)
        .with_context(|| format!("failed to create {}", work.domain_data.display()))?;
    std::fs::create_dir_all(&work.append_only_data)
        .with_context(|| format!("failed to create {}", work.append_only_data.display()))?;
    std::fs::create_dir_all(&work.log_dir)
        .with_context(|| format!("failed to create {}", work.log_dir.display()))?;

    // Write default config
    let config_content = default_config_toml();
    std::fs::write(&work.config_path, &config_content)
        .with_context(|| format!("failed to write {}", work.config_path.display()))?;

    info!(path = %work.root.display(), "work directory initialized");
    println!("Initialized work directory: {}", work.root.display());
    println!("  config:      {}", work.config_path.display());
    println!("  domain data: {}", work.domain_data.display());
    println!("  append-only: {}", work.append_only_data.display());
    println!("  logs:        {}", work.log_dir.display());

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

    if !args.confirm {
        bail!("purge requires --confirm flag to proceed");
    }

    let mut deleted = Vec::new();

    // Delete domain data contents
    if work.domain_data.exists() {
        remove_dir_contents(&work.domain_data)
            .with_context(|| format!("failed to purge {}", work.domain_data.display()))?;
        deleted.push(format!("  {}/", work.domain_data.display()));
    }

    // Delete append-only data contents
    if work.append_only_data.exists() {
        remove_dir_contents(&work.append_only_data)
            .with_context(|| format!("failed to purge {}", work.append_only_data.display()))?;
        deleted.push(format!("  {}/", work.append_only_data.display()));
    }

    // Delete run directory contents
    if work.run_dir.exists() {
        remove_dir_contents(&work.run_dir)
            .with_context(|| format!("failed to purge {}", work.run_dir.display()))?;
        deleted.push(format!("  {}/", work.run_dir.display()));
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
        if let Some(ref tl) = work.token_labels {
            println!("  {}/", tl.display());
        }
        if let Some(ref lt) = work.labels_toml {
            println!("  {}", lt.display());
        }
    }

    Ok(())
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

fn parse_only_flag(only: &Option<String>) -> Vec<String> {
    match only {
        Some(s) => s.split(',').map(|s| s.trim().to_string()).collect(),
        None => vec!["indexer".to_string(), "api".to_string()],
    }
}

/// Resolve frontend assets directory.
fn resolve_frontend_dir(work: &WorkDir) -> Option<PathBuf> {
    // Check work_dir/frontend/ first
    let work_frontend = work.root.join("frontend");
    if work_frontend.is_dir() {
        return Some(work_frontend);
    }
    // Check share/frontend/
    if let Some(share) = resolve_share_dir() {
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
    use tempfile::TempDir;

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

        let log_file = root.join("run/logs/test.log");
        std::fs::write(&log_file, "log data").unwrap();

        let run_file = root.join("run/supervisor.pid");
        std::fs::write(&run_file, "12345").unwrap();

        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        // Data should be gone
        assert!(!domain_file.exists(), "domain data should be deleted");
        assert!(!append_file.exists(), "append data should be deleted");
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
        assert!(root.join("run").exists(), "run dir should remain");
    }

    #[test]
    fn test_purge_preserves_config_and_labels() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(&root).unwrap();

        // Create token-labels directory and labels.toml
        std::fs::create_dir(root.join("token-labels")).unwrap();
        std::fs::write(root.join("token-labels/info.json"), "{}").unwrap();
        std::fs::write(root.join("labels.toml"), "[labels]").unwrap();

        // Create some data to be purged
        std::fs::write(root.join("data/domain/test.db"), "data").unwrap();

        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        // Config and labels should be preserved
        assert!(
            root.join("ckbadger.toml").exists(),
            "config should be preserved"
        );
        assert!(
            root.join("token-labels").exists(),
            "token-labels dir should be preserved"
        );
        assert!(
            root.join("token-labels/info.json").exists(),
            "token-labels contents should be preserved"
        );
        assert!(
            root.join("labels.toml").exists(),
            "labels.toml should be preserved"
        );

        // Data should be gone
        assert!(
            !root.join("data/domain/test.db").exists(),
            "data should be purged"
        );
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
}
