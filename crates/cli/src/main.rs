mod sequencer;
mod supervisor;

#[cfg(test)]
mod build_version_format;

// Match CKB's allocator configuration. The unprefixed malloc symbols make the
// same allocator own Rust allocations and RocksDB's C++ allocations, so large
// bulk-build WriteBatch buffers can be purged instead of remaining stranded in
// the system allocator's arenas.
#[cfg(all(not(target_env = "msvc"), not(target_os = "macos")))]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use ckbadger_config::{
    co_resident_network_count, default_config_toml, default_orchestrator_toml, is_orchestrator,
    load_config, load_orchestrator_config, network_workdir, resolve_ckb_paths, resolve_share_dir,
    resolve_store_paths, resolve_workdir_path, validate_network_entry, CkbadgerConfig,
    ResolvedCkbPaths, StoreConfig, WorkDir,
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
use ckbadger_tui::TuiNetwork;

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
/// The indexer opens many RocksDB SST files during bulk sync (59 column
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
    /// Start only specific services in single-network mode (comma-separated: indexer,api,frontend-server,crawler)
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
async fn main() -> ExitCode {
    match run_cli().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::from(exit_code_for_error(&error))
        }
    }
}

fn exit_code_for_error(error: &anyhow::Error) -> u8 {
    if ckbadger_indexer::lifecycle::is_rebuild_required(error) {
        ckbadger_indexer::lifecycle::REBUILD_REQUIRED_EXIT_CODE
    } else {
        1
    }
}

async fn run_cli() -> Result<()> {
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

fn store_runtime_config(store: &StoreConfig, workdir: &Path) -> Result<StoreRuntimeConfig> {
    Ok(StoreRuntimeConfig {
        memory_budget_gb: store.memory_budget_gb,
        direct_io_reads: store.direct_io_reads,
        vector_memtable: false, // set by indexer entry at runtime
        // Derived from ckbadger.toml rather than passed down from the supervisor,
        // so openers the supervisor never spawns (tui, verify, a direct
        // `internal indexer`) size themselves correctly too.
        network_count: co_resident_network_count(workdir)?,
    })
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
        bulk_memory_budget_gb: config.indexer.bulk_memory_budget_gb,
        store_runtime_config: store_runtime_config(&config.store, workdir)?,
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
/// network subdir listed in the top-level `ckbadger.toml`, plus one shared
/// frontend proxy serving all networks.
///
/// Each subdir is a standard single-network workdir with its own `config.toml`.
/// Services are spawned as `ckbadger internal <service> -C <subdir>` children of
/// one shared supervisor rooted at the orchestrator dir. The frontend is NOT
/// spawned per-network; a single frontend child (spawned with `-C <root>`) serves
/// every network, and each API is also reachable on its own configured port.
async fn cmd_run_orchestrator(root: &Path, args: &RunArgs) -> Result<()> {
    // `--only` selects services within a single-network workdir; there is no
    // coherent meaning at an orchestrator root (which network?). Fail fast
    // rather than silently ignoring the flag.
    if args.only.is_some() {
        bail!(
            "--only is not supported in orchestrator mode; run a single network with `ckbadger -C <root>/<network> run --only ...`"
        );
    }

    let orch = load_orchestrator_config(root)?;
    let root_work = WorkDir::resolve(root);

    // At least one indexer will run; fail fast if the fd limit is too low.
    let fd_limit = raise_fd_limit()?;
    check_fd_limit_for_indexer(fd_limit)?;

    // Immediate children (frontend + every api + every enabled crawler) start up
    // front; indexers are deferred and started one at a time by the sequencer so
    // only one network bulk-syncs at a time (see `run_supervisor_sequenced`).
    let mut immediate: Vec<supervisor::ChildSpec> = Vec::new();
    let mut indexers: Vec<supervisor::SequencedIndexer> = Vec::new();
    // (network_name, api_port, resolved_workdir) for fail-fast binding checks.
    let mut nets: Vec<(String, u16, PathBuf)> = Vec::new();
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
        let network = validate_network_entry(entry, &cfg.ckb.network)?.to_string();
        nets.push((network.clone(), cfg.api.port, sub.clone()));
        let sub_str = sub.to_string_lossy().to_string();

        immediate.push(supervisor::ChildSpec {
            label: format!("{network}/api"),
            service: "api".to_string(),
            workdir: sub_str.clone(),
        });
        if cfg.crawler.enabled {
            immediate.push(supervisor::ChildSpec {
                label: format!("{network}/crawler"),
                service: "crawler".to_string(),
                workdir: sub_str.clone(),
            });
        }
        let store_paths = resolve_store_paths(&sub, &cfg.store);
        indexers.push(supervisor::SequencedIndexer {
            spec: supervisor::ChildSpec {
                label: format!("{network}/indexer"),
                service: "indexer".to_string(),
                workdir: sub_str,
            },
            domain_data_path: store_paths.domain_data,
            bulk_sync_threshold: cfg.indexer.bulk_sync_threshold,
        });
        println!(
            "network '{}' -> {} (api :{})",
            network,
            sub.display(),
            cfg.api.port
        );
    }

    // Fail fast on host-global collisions workdir isolation can't catch:
    // duplicate TCP ports (second API restart-loops) and duplicate workdirs
    // (RocksDB LOCK / DB-identity confusion). Must run BEFORE any child spawns.
    check_orchestrator_bindings(&nets)?;

    // One shared frontend proxy for all networks (the child re-reads the
    // orchestrator config; label "frontend" -> run/logs/frontend.log).
    immediate.push(frontend_child_spec(root));
    println!(
        "frontend proxy -> :{} (serves /{{network}}/...)",
        orch.frontend.port
    );
    println!("bulk sync is sequential: one network at a time, in [[network]] order");
    supervisor::run_supervisor_sequenced(&root_work, immediate, indexers).await
}

/// Fail-fast validation of orchestrator network bindings before spawning.
/// `nets` = (network_name, api_port, resolved_workdir) per [[network]].
///
/// Workdir isolation covers sockets/logs/data but NOT host-global TCP ports,
/// so two networks sharing an `[api].port` would make the second API fail to
/// bind and restart-loop into a degraded state. Two entries resolving to the
/// same workdir would collide on the RocksDB LOCK / DB identity. Reject both
/// up front instead of degrading at runtime.
fn check_orchestrator_bindings(nets: &[(String, u16, PathBuf)]) -> Result<()> {
    for i in 0..nets.len() {
        for j in (i + 1)..nets.len() {
            if nets[i].1 == nets[j].1 {
                bail!(
                    "networks '{}' and '{}' both use API port {} — each network needs a distinct [api].port",
                    nets[i].0, nets[j].0, nets[i].1
                );
            }
            if nets[i].2 == nets[j].2 {
                bail!(
                    "networks '{}' and '{}' resolve to the same workdir {} — give each a distinct [[network]].dir",
                    nets[i].0, nets[j].0, nets[i].2.display()
                );
            }
        }
    }
    Ok(())
}

/// Build the shared frontend's multi-network config from an orchestrator root.
///
/// Re-reads `<root>/ckbadger.toml` plus each network subdir's `config.toml`
/// (the same shape [`cmd_run_orchestrator`] uses) so the frontend child — spawned
/// with `-C <root>` — can route to every network's API. `default_network` is the
/// first listed network (the `/` redirect target for the proxy). `frontend_dir`
/// is resolved by the caller and passed through unchanged.
fn build_orchestrator_frontend_config(
    root: &Path,
    frontend_dir: Option<PathBuf>,
) -> Result<FrontendServiceConfig> {
    let orch = load_orchestrator_config(root)?;
    let mut networks = Vec::new();
    for entry in &orch.networks {
        let sub = network_workdir(root, entry);
        let cfg = load_config(&sub)
            .with_context(|| format!("frontend: reading config for network '{}'", entry.name))?;
        let network = validate_network_entry(entry, &cfg.ckb.network)?;
        networks.push(ckbadger_api::entry::FrontendNetwork {
            name: network.to_string(),
            api_port: cfg.api.port,
        });
    }
    let default_network = networks
        .first()
        .map(|n| n.name.clone())
        .ok_or_else(|| anyhow::anyhow!("orchestrator has no networks for the frontend"))?;
    Ok(FrontendServiceConfig {
        host: orch.frontend.host.clone(),
        port: orch.frontend.port,
        api_port: networks[0].api_port, // legacy field; proxy uses `networks`
        ckb_network: default_network.clone(),
        ckb_rpc_url: String::new(), // per-network rpc not surfaced to the frontend proxy
        build_version: BUILD_VERSION.to_string(),
        frontend_dir,
        default_network,
        networks,
    })
}

/// The shared-frontend supervised child. Spawned with `-C <root>` (the
/// orchestrator dir) so [`cmd_internal`] detects `is_orchestrator` and builds the
/// multi-network config. Label "frontend" -> `run/logs/frontend.log`.
fn frontend_child_spec(root: &Path) -> supervisor::ChildSpec {
    supervisor::ChildSpec {
        label: "frontend".to_string(),
        service: "frontend-server".to_string(),
        workdir: root.to_string_lossy().to_string(),
    }
}

/// Split networks into immediate child labels (apis + enabled crawlers + one shared
/// frontend) and ordered indexer labels. Pure; encodes the same spec-building order
/// [`cmd_run_orchestrator`] uses, so the sequenced supervisor's index matching stays
/// correct. Test-only: the real orchestrated run builds full specs inline (its
/// subprocess spawning isn't unit-testable), so this mirror is the ordering coverage.
#[cfg(test)]
fn orchestrator_child_split(
    networks: impl Iterator<Item = (String, bool)>, // (name, crawler_enabled)
) -> (Vec<String>, Vec<String>) {
    let mut immediate = Vec::new();
    let mut indexers = Vec::new();
    for (name, crawler_enabled) in networks {
        immediate.push(format!("{name}/api"));
        if crawler_enabled {
            immediate.push(format!("{name}/crawler"));
        }
        indexers.push(format!("{name}/indexer"));
    }
    immediate.push("frontend".to_string());
    (immediate, indexers)
}

// ---------------------------------------------------------------------------
// internal command (subprocess entry points)
// ---------------------------------------------------------------------------

async fn cmd_internal(workdir: &Path, args: &InternalArgs) -> Result<()> {
    // The shared frontend is spawned with `-C <orchestrator-root>`, which holds
    // `ckbadger.toml` (not the single-network `config.toml`). Build its
    // multi-network config from the orchestrator config here, before the
    // single-network `load_config` below (which would fail at that root).
    if matches!(args.service, InternalService::FrontendServer) && is_orchestrator(workdir) {
        let work = WorkDir::resolve(workdir);
        let frontend_dir = resolve_frontend_dir(&work);
        let frontend_config = build_orchestrator_frontend_config(workdir, frontend_dir)?;
        return run_frontend_server(frontend_config).await;
    }

    let config = load_config(workdir)?;
    let work = WorkDir::resolve(workdir);
    let store_runtime_config = store_runtime_config(&config.store, workdir)?;
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
            // Single-network mode: one-element `networks` matching this workdir's
            // own API. The orchestrator (multi-network) case returns early above.
            let frontend_dir = resolve_frontend_dir(&work);
            let networks = vec![ckbadger_api::entry::FrontendNetwork {
                name: config.ckb.network.clone(),
                api_port: config.api.port,
            }];
            let frontend_config = FrontendServiceConfig {
                host: config.frontend.host.clone(),
                port: config.frontend.port,
                api_port: config.api.port,
                ckb_network: config.ckb.network.clone(),
                ckb_rpc_url: config.ckb.rpc_url.clone(),
                build_version: BUILD_VERSION.to_string(),
                frontend_dir,
                default_network: config.ckb.network.clone(),
                networks,
            };
            run_frontend_server(frontend_config).await
        }
        InternalService::Crawler => ckbadger_crawler::entry::run_crawler(workdir, false).await,
    }
}

// ---------------------------------------------------------------------------
// tui command
// ---------------------------------------------------------------------------

/// Resolve one network subdir into a `TuiNetwork` (paths + api endpoint).
fn build_tui_network(name: &str, workdir: &Path) -> Result<TuiNetwork> {
    let config = load_config(workdir)?;
    let entry = ckbadger_config::NetworkEntry {
        name: name.to_string(),
        dir: None,
    };
    let network = validate_network_entry(&entry, &config.ckb.network)?;
    let work = WorkDir::resolve(workdir);
    let store_paths = resolve_store_paths(workdir, &config.store);
    // CKB node paths are display-only on the System tab (the TUI never opens the CKB
    // RocksDB). A network whose CKB node isn't on disk must still be monitorable via its
    // own store + API, so degrade to empty paths rather than aborting the whole (possibly
    // multi-network) TUI. The System tab renders empty CKB paths as "unavailable".
    let (ckb_workdir, ckb_db_path) = match resolve_ckb_paths(workdir, &config.ckb) {
        Ok(p) => (
            p.ckb_workdir.to_string_lossy().to_string(),
            p.ckb_db_path.to_string_lossy().to_string(),
        ),
        Err(e) => {
            eprintln!("ckbadger tui: network '{name}': CKB node paths unavailable ({e})");
            (String::new(), String::new())
        }
    };
    Ok(TuiNetwork {
        name: network.to_string(),
        domain_data_path: store_paths.domain_data.to_string_lossy().to_string(),
        append_only_data_path: store_paths.append_only_data.to_string_lossy().to_string(),
        ckbadger_workdir: work.root.to_string_lossy().to_string(),
        ckb_workdir,
        ckb_db_path,
        api_url: format!("http://{}:{}/api/v1", config.api.host, config.api.port),
        store_runtime_config: store_runtime_config(&config.store, workdir)?,
    })
}

/// Build the TUI config for either a single-network workdir or an orchestrator root.
/// Orchestrator: iterate `[[network]]`, resolve each subdir, and use the SHARED
/// root supervisor socket + log dir (mirrors `cmd_status_orchestrator`).
fn resolve_tui_service_config(workdir: &Path) -> Result<TuiServiceConfig> {
    if ckbadger_config::is_orchestrator(workdir) {
        use ckbadger_config::{load_orchestrator_config, network_workdir};
        let orch = load_orchestrator_config(workdir)?;
        let root_work = WorkDir::resolve(workdir);

        let mut networks = Vec::with_capacity(orch.networks.len());
        for entry in &orch.networks {
            let sub = network_workdir(workdir, entry);
            let config_path = sub.join("config.toml");
            if !config_path.is_file() {
                anyhow::bail!(
                    "network '{}' is missing its config.toml at {} (has `ckbadger init` scaffolded it?)",
                    entry.name,
                    config_path.display()
                );
            }
            networks.push(build_tui_network(&entry.name, &sub)?);
        }

        Ok(TuiServiceConfig {
            networks,
            refresh_ms: 1000,
            supervisor_socket_path: Some(root_work.indexer_sock.to_string_lossy().to_string()),
            service_log_dir: Some(root_work.log_dir.to_string_lossy().to_string()),
            build_version: BUILD_VERSION.to_string(),
        })
    } else {
        let config = load_config(workdir)?;
        let work = WorkDir::resolve(workdir);
        let network = build_tui_network(&config.ckb.network, workdir)?;
        Ok(TuiServiceConfig {
            networks: vec![network],
            refresh_ms: 1000,
            supervisor_socket_path: Some(work.indexer_sock.to_string_lossy().to_string()),
            service_log_dir: Some(work.log_dir.to_string_lossy().to_string()),
            build_version: BUILD_VERSION.to_string(),
        })
    }
}

async fn cmd_tui(workdir: &Path) -> Result<()> {
    let tui_config = resolve_tui_service_config(workdir)?;
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
        store_runtime_config(&config.store, workdir)?,
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
        let config = load_config(&sub)
            .with_context(|| format!("status: reading config for network '{}'", entry.name))?;
        let network = validate_network_entry(entry, &config.ckb.network)?;
        println!("[{network}] {}", sub.display());
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

#[derive(Debug)]
struct VerifyTarget {
    network: String,
    workdir: PathBuf,
    api_url: String,
    rpc_url: String,
    explorer_url: String,
    cache_dir: PathBuf,
}

/// Explorer API base for verify, network-aware, failing fast on unknown nets.
fn verify_explorer_url(network: &str) -> Result<String> {
    ckbadger_common::network::explorer_api_url(network)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("no explorer API URL for network '{network}'"))
}

fn build_verify_target(
    network: &str,
    workdir: PathBuf,
    config: &CkbadgerConfig,
) -> Result<VerifyTarget> {
    Ok(VerifyTarget {
        network: network.to_string(),
        api_url: format!("http://{}:{}/api/v1", config.api.host, config.api.port),
        rpc_url: config.ckb.rpc_url.clone(),
        explorer_url: verify_explorer_url(network)?,
        cache_dir: workdir.join(".verify-cache"),
        workdir,
    })
}

/// Resolve and validate every network before starting any potentially long-running
/// verification. A plain workdir produces one target; an orchestrator root produces
/// one target per `[[network]]`, in declaration order.
fn resolve_verify_targets(workdir: &Path) -> Result<Vec<VerifyTarget>> {
    if !is_orchestrator(workdir) {
        let config = load_config(workdir)?;
        return Ok(vec![build_verify_target(
            &config.ckb.network,
            workdir.to_path_buf(),
            &config,
        )?]);
    }

    let orchestrator = load_orchestrator_config(workdir)?;
    let mut targets = Vec::with_capacity(orchestrator.networks.len());
    for entry in &orchestrator.networks {
        let network_dir = network_workdir(workdir, entry);
        let config = load_config(&network_dir).with_context(|| {
            format!(
                "verify: reading config for network '{}' at {}",
                entry.name,
                network_dir.display()
            )
        })?;
        let network = validate_network_entry(entry, &config.ckb.network)?;
        targets.push(build_verify_target(network, network_dir, &config)?);
    }
    Ok(targets)
}

#[cfg(test)]
mod verify_url_tests {
    use super::*;
    use tempfile::TempDir;

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

    #[test]
    fn orchestrator_targets_each_declared_network_in_order() {
        let root = TempDir::new().unwrap();
        std::fs::write(
            root.path().join("ckbadger.toml"),
            "[[network]]\nname = \"mainnet\"\n\n[[network]]\nname = \"testnet\"\ndir = \"chains/test\"\n",
        )
        .unwrap();

        let mainnet = root.path().join("mainnet");
        let testnet = root.path().join("chains/test");
        std::fs::create_dir_all(&mainnet).unwrap();
        std::fs::create_dir_all(&testnet).unwrap();
        std::fs::write(
            mainnet.join("config.toml"),
            default_config_toml("mainnet", 8101),
        )
        .unwrap();
        std::fs::write(
            testnet.join("config.toml"),
            default_config_toml("testnet", 8102),
        )
        .unwrap();

        let targets = resolve_verify_targets(root.path()).unwrap();

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].network, "mainnet");
        assert_eq!(targets[0].workdir, mainnet);
        assert_eq!(targets[0].api_url, "http://127.0.0.1:8101/api/v1");
        assert_eq!(targets[0].cache_dir, mainnet.join(".verify-cache"));
        assert_eq!(targets[1].network, "testnet");
        assert_eq!(targets[1].workdir, testnet);
        assert_eq!(targets[1].api_url, "http://127.0.0.1:8102/api/v1");
        assert_eq!(targets[1].cache_dir, testnet.join(".verify-cache"));
    }

    #[test]
    fn orchestrator_rejects_child_network_mismatch_before_verify() {
        let root = TempDir::new().unwrap();
        std::fs::write(
            root.path().join("ckbadger.toml"),
            "[[network]]\nname = \"mainnet\"\n",
        )
        .unwrap();
        let mainnet = root.path().join("mainnet");
        std::fs::create_dir_all(&mainnet).unwrap();
        std::fs::write(
            mainnet.join("config.toml"),
            default_config_toml("testnet", 8102),
        )
        .unwrap();

        let error = resolve_verify_targets(root.path()).unwrap_err().to_string();
        assert!(
            error.contains("network mismatch"),
            "unexpected error: {error}"
        );
        assert!(error.contains("mainnet"), "unexpected error: {error}");
        assert!(error.contains("testnet"), "unexpected error: {error}");
    }
}

#[cfg(test)]
mod orchestrator_split_tests {
    use super::*;

    #[test]
    fn split_puts_apis_crawlers_frontend_immediate_and_indexers_ordered() {
        // (network_name, crawler_enabled) in [[network]] order
        let nets = [("mainnet", false), ("testnet", true)];
        let (immediate_labels, indexer_labels) =
            orchestrator_child_split(nets.iter().map(|(n, c)| (n.to_string(), *c)));
        // immediate = every api + every enabled crawler + one shared frontend
        assert!(immediate_labels.contains(&"mainnet/api".to_string()));
        assert!(immediate_labels.contains(&"testnet/api".to_string()));
        assert!(immediate_labels.contains(&"testnet/crawler".to_string()));
        assert!(!immediate_labels.contains(&"mainnet/crawler".to_string())); // crawler disabled
        assert!(immediate_labels.contains(&"frontend".to_string()));
        // indexers = one per network, in array order, NOT in immediate
        assert_eq!(indexer_labels, vec!["mainnet/indexer", "testnet/indexer"]);
        for l in &indexer_labels {
            assert!(!immediate_labels.contains(l));
        }
    }
}

#[cfg(test)]
mod tui_config_tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    /// Write a network subdir that `resolve_tui_service_config` can fully resolve:
    /// a `config.toml` pointing `ckb.workdir` at a minimal CKB node dir (a `ckb.toml`
    /// whose resolved RocksDB path exists on disk), which `resolve_ckb_paths` requires.
    fn write_single_network(dir: &Path, name: &str, api_port: u16) {
        std::fs::create_dir_all(dir).unwrap();
        let config = ckbadger_config::default_config_toml(name, api_port)
            .replace("workdir = \"\"", "workdir = \"ckb\"");
        std::fs::write(dir.join("config.toml"), config).unwrap();

        // Minimal resolvable CKB node under <dir>/ckb: ckb.toml (data_dir = "data")
        // + the default db path <dir>/ckb/data/db must exist.
        let ckb_dir = dir.join("ckb");
        std::fs::create_dir_all(ckb_dir.join("data").join("db")).unwrap();
        std::fs::write(ckb_dir.join("ckb.toml"), "data_dir = \"data\"\n").unwrap();
    }

    #[test]
    fn resolve_orchestrator_builds_one_network_per_entry() {
        let root = TempDir::new().unwrap();
        std::fs::write(
            root.path().join("ckbadger.toml"),
            "[[network]]\nname=\"mainnet\"\n[[network]]\nname=\"testnet\"\n",
        )
        .unwrap();
        write_single_network(&root.path().join("mainnet"), "mainnet", 8101);
        write_single_network(&root.path().join("testnet"), "testnet", 8102);

        let cfg = resolve_tui_service_config(root.path()).unwrap();
        assert_eq!(cfg.networks.len(), 2);
        assert_eq!(cfg.networks[0].name, "mainnet");
        assert_eq!(cfg.networks[1].name, "testnet");
        assert!(cfg.networks[0].api_url.contains(":8101"));
        assert!(cfg.networks[1].api_url.contains(":8102"));

        // Shared socket + log dir come from the orchestrator ROOT, not a subdir.
        let root_sock = WorkDir::resolve(root.path())
            .indexer_sock
            .to_string_lossy()
            .to_string();
        assert_eq!(
            cfg.supervisor_socket_path.as_deref(),
            Some(root_sock.as_str())
        );
    }

    #[test]
    fn resolve_orchestrator_missing_config_errors_with_network_name() {
        let root = TempDir::new().unwrap();
        std::fs::write(
            root.path().join("ckbadger.toml"),
            "[[network]]\nname=\"mainnet\"\n[[network]]\nname=\"testnet\"\n",
        )
        .unwrap();
        write_single_network(&root.path().join("mainnet"), "mainnet", 8101);
        // testnet subdir intentionally has no config.toml.

        let err = resolve_tui_service_config(root.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("testnet"), "error names the network: {msg}");
        assert!(msg.contains("config.toml"), "error names the file: {msg}");
    }

    #[test]
    fn resolve_single_network_builds_one_network() {
        let dir = TempDir::new().unwrap();
        write_single_network(dir.path(), "mainnet", 8101);

        let cfg = resolve_tui_service_config(dir.path()).unwrap();
        assert_eq!(cfg.networks.len(), 1);
        assert_eq!(cfg.networks[0].name, "mainnet");
    }

    /// A network whose config.toml exists but whose CKB node isn't on disk: the
    /// default template's `workdir = ""` is unresolvable, so `resolve_ckb_paths`
    /// fails and `build_tui_network` degrades the CKB paths (rather than aborting).
    fn write_network_no_ckb_node(dir: &Path, name: &str, api_port: u16) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            ckbadger_config::default_config_toml(name, api_port),
        )
        .unwrap();
    }

    #[test]
    fn resolve_tolerates_a_network_with_no_ckb_node() {
        let root = TempDir::new().unwrap();
        std::fs::write(
            root.path().join("ckbadger.toml"),
            "[[network]]\nname=\"mainnet\"\n[[network]]\nname=\"testnet\"\n",
        )
        .unwrap();
        write_single_network(&root.path().join("mainnet"), "mainnet", 8101);
        write_network_no_ckb_node(&root.path().join("testnet"), "testnet", 8102);

        // A missing CKB node no longer aborts the whole TUI: testnet degrades, both build.
        let cfg = resolve_tui_service_config(root.path()).unwrap();
        assert_eq!(cfg.networks.len(), 2);
        assert!(
            !cfg.networks[0].ckb_workdir.is_empty(),
            "mainnet CKB node resolved"
        );
        assert!(
            cfg.networks[1].ckb_workdir.is_empty(),
            "testnet CKB paths degraded to empty"
        );
        assert!(cfg.networks[1].ckb_db_path.is_empty());
    }
}

async fn cmd_verify(workdir: &Path, args: &VerifyArgs) -> Result<()> {
    let depth = match args.depth.to_lowercase().as_str() {
        "fast" => indexer_verify::checks::CheckTier::Fast,
        "sampling" => indexer_verify::checks::CheckTier::Sampling,
        _ => bail!("Invalid depth: {}. Use fast or sampling", args.depth),
    };

    let targets = resolve_verify_targets(workdir)?;
    let show_network = targets.len() > 1;

    for (index, target) in targets.into_iter().enumerate() {
        // The check registry is identical for every network, so list it once after
        // all target configs have been validated.
        if args.list_checks && index > 0 {
            break;
        }
        if show_network && !args.list_checks {
            println!("[{}] {}", target.network, target.workdir.display());
        }

        let network = target.network.clone();
        let verify_args = indexer_verify::VerifyArgs {
            api_url: target.api_url,
            rpc_url: Some(target.rpc_url),
            explorer_url: target.explorer_url,
            no_explorer: false,
            depth,
            sample_count: 1000,
            seed: 42,
            tolerance: 0.001,
            format: indexer_verify::OutputFormat::Text,
            checks: None,
            list_checks: args.list_checks,
            cache_dir: Some(target.cache_dir.to_string_lossy().into_owned()),
        };

        tokio::task::spawn_blocking(move || indexer_verify::run(verify_args))
            .await
            .with_context(|| format!("verify task failed for network '{network}'"))?
            .with_context(|| format!("verification failed for network '{network}'"))?;
    }

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
        store_runtime_config: store_runtime_config(&config.store, workdir)?,
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
    // Orchestrator root: no config.toml at the root — purge each network subdir
    // plus the shared root run dir (mirrors cmd_run/cmd_status orchestrator handling).
    if is_orchestrator(workdir) {
        return cmd_purge_orchestrator(workdir, args);
    }

    let work = WorkDir::resolve(workdir);

    if !work.is_initialized() {
        bail!(
            "work directory not initialized (no config.toml at {})",
            work.config_path.display()
        );
    }

    if !args.confirm {
        bail!("purge requires --confirm flag to proceed");
    }

    let mut deleted = Vec::new();
    purge_workdir(workdir, &mut deleted)?;

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

/// Purge every network's derived data in an orchestrator deployment, plus the shared
/// root run dir (supervisor socket/logs/pid). Preserves `ckbadger.toml` and each
/// network's `config.toml`.
fn cmd_purge_orchestrator(root: &Path, args: &PurgeArgs) -> Result<()> {
    use ckbadger_config::{load_orchestrator_config, network_workdir};

    if !args.confirm {
        bail!("purge requires --confirm flag to proceed");
    }

    let orch = load_orchestrator_config(root)?;
    let root_work = WorkDir::resolve(root);
    let mut deleted = Vec::new();

    // Validate every child before deleting anything, so a misplaced config is
    // reported atomically rather than leaving a partially purged deployment.
    for entry in &orch.networks {
        let sub = network_workdir(root, entry);
        if !sub.join("config.toml").is_file() {
            bail!(
                "network '{}' has no config.toml at {}",
                entry.name,
                sub.display()
            );
        }
        let config = load_config(&sub)?;
        validate_network_entry(entry, &config.ckb.network)?;
    }

    for entry in &orch.networks {
        let sub = network_workdir(root, entry);
        purge_workdir(&sub, &mut deleted)?;
    }

    // The shared root run dir holds the supervisor socket/logs/pid (each network's own
    // run dir is purged by purge_workdir above).
    if root_work.run_dir.exists() {
        remove_dir_contents(&root_work.run_dir)
            .with_context(|| format!("failed to purge {}", root_work.run_dir.display()))?;
        deleted.push(format!("  {}/", root_work.run_dir.display()));
    }

    if deleted.is_empty() {
        println!("Nothing to purge.");
    } else {
        info!(path = %root.display(), "purged derived data (orchestrator)");
        println!("Purged derived data:");
        for d in &deleted {
            println!("{d}");
        }
        println!();
        println!("Preserved:");
        println!("  {}", root.join("ckbadger.toml").display());
        for entry in &orch.networks {
            println!("  {}/config.toml", network_workdir(root, entry).display());
        }
    }

    Ok(())
}

/// Purge one workdir's derived data: domain/append-only/decoder-cache stores, DOB
/// media, run + bench dirs, and all known secondary-store dirs. Appends each deleted
/// path to `deleted`. Does not check init/confirm or print; reused by both the
/// single-network and orchestrator purge paths.
fn purge_workdir(workdir: &Path, deleted: &mut Vec<String>) -> Result<()> {
    let work = WorkDir::resolve(workdir);
    let config = load_config(workdir)?;
    let store_paths = resolve_store_paths(workdir, &config.store);

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
    fn test_run_help_names_the_single_network_only_services() {
        let mut cmd = Cli::command();
        let run = cmd
            .find_subcommand_mut("run")
            .expect("run subcommand must exist");
        let help = run.render_help().to_string();

        assert!(help.contains("single-network mode"));
        assert!(help.contains("indexer,api,frontend-server,crawler"));
    }

    #[test]
    fn rebuild_required_error_maps_to_dedicated_process_exit_code() {
        let error = anyhow::Error::new(ckbadger_indexer::lifecycle::RebuildRequiredError::new(
            "test rebuild",
        ))
        .context("indexer startup failed");

        assert_eq!(
            exit_code_for_error(&error),
            ckbadger_indexer::lifecycle::REBUILD_REQUIRED_EXIT_CODE
        );
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

    // -- verify command --

    #[tokio::test]
    async fn test_verify_accepts_orchestrator_root() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        cmd_init(&root, &InitArgs { with_testnet: true }).unwrap();

        cmd_verify(
            &root,
            &VerifyArgs {
                depth: "sampling".to_string(),
                list_checks: true,
            },
        )
        .await
        .unwrap();
    }

    // -- orchestrator binding fail-fast --

    #[test]
    fn test_check_orchestrator_bindings_distinct_ports_and_dirs_ok() {
        let nets = vec![
            ("mainnet".to_string(), 8101, PathBuf::from("/root/mainnet")),
            ("testnet".to_string(), 8102, PathBuf::from("/root/testnet")),
        ];
        assert!(check_orchestrator_bindings(&nets).is_ok());
    }

    #[test]
    fn test_check_orchestrator_bindings_duplicate_port_errors() {
        let nets = vec![
            ("mainnet".to_string(), 8101, PathBuf::from("/root/mainnet")),
            ("testnet".to_string(), 8101, PathBuf::from("/root/testnet")),
        ];
        let err = check_orchestrator_bindings(&nets).unwrap_err().to_string();
        assert!(err.contains("mainnet"), "error names first network: {err}");
        assert!(err.contains("testnet"), "error names second network: {err}");
        assert!(err.contains("8101"), "error names the port: {err}");
    }

    #[test]
    fn test_check_orchestrator_bindings_duplicate_workdir_errors() {
        let nets = vec![
            ("mainnet".to_string(), 8101, PathBuf::from("/root/shared")),
            ("testnet".to_string(), 8102, PathBuf::from("/root/shared")),
        ];
        let err = check_orchestrator_bindings(&nets).unwrap_err().to_string();
        assert!(err.contains("mainnet"), "error names first network: {err}");
        assert!(err.contains("testnet"), "error names second network: {err}");
        assert!(
            err.contains("/root/shared"),
            "error names the shared workdir: {err}"
        );
    }

    // -- shared frontend (orchestrator) --

    #[test]
    fn test_build_orchestrator_frontend_config_reads_every_network_port() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Orchestrator root lists two networks; each subdir is a single-network
        // workdir with a distinct [api].port.
        std::fs::write(
            root.join("ckbadger.toml"),
            default_orchestrator_toml(&["mainnet", "testnet"]),
        )
        .unwrap();
        std::fs::create_dir_all(root.join("mainnet")).unwrap();
        std::fs::create_dir_all(root.join("testnet")).unwrap();
        std::fs::write(
            root.join("mainnet").join("config.toml"),
            default_config_toml("mainnet", 8101),
        )
        .unwrap();
        std::fs::write(
            root.join("testnet").join("config.toml"),
            default_config_toml("testnet", 8102),
        )
        .unwrap();

        let cfg = build_orchestrator_frontend_config(root, None).unwrap();

        assert_eq!(cfg.networks.len(), 2);
        assert_eq!(cfg.networks[0].name, "mainnet");
        assert_eq!(cfg.networks[0].api_port, 8101);
        assert_eq!(cfg.networks[1].name, "testnet");
        assert_eq!(cfg.networks[1].api_port, 8102);
        // default_network is the first listed network.
        assert_eq!(cfg.default_network, "mainnet");
        // Legacy single-port field mirrors the first (default) network.
        assert_eq!(cfg.api_port, 8101);
        // Shared frontend host/port come from the orchestrator [frontend] section.
        assert_eq!(cfg.port, 8100);
        // No frontend assets dir was passed through.
        assert!(cfg.frontend_dir.is_none());
    }

    #[test]
    fn test_build_orchestrator_frontend_config_rejects_entry_child_network_mismatch() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        std::fs::write(
            root.join("ckbadger.toml"),
            default_orchestrator_toml(&["mainnet"]),
        )
        .unwrap();
        std::fs::create_dir_all(root.join("mainnet")).unwrap();
        std::fs::write(
            root.join("mainnet").join("config.toml"),
            default_config_toml("testnet", 8101),
        )
        .unwrap();

        let err = match build_orchestrator_frontend_config(root, None) {
            Ok(_) => panic!("mismatched child network must be rejected"),
            Err(err) => err.to_string(),
        };
        assert!(err.contains("mainnet"));
        assert!(err.contains("testnet"));
        assert!(err.contains("mismatch"));
    }

    #[test]
    fn test_frontend_child_spec_targets_orchestrator_root() {
        let spec = frontend_child_spec(Path::new("/srv/ckb"));
        assert_eq!(spec.label, "frontend");
        // Maps to `ckbadger internal frontend-server`.
        assert_eq!(spec.service, "frontend-server");
        // Spawned with `-C <root>` so cmd_internal detects the orchestrator config.
        assert_eq!(spec.workdir, "/srv/ckb");
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
    fn test_purge_orchestrator_purges_all_networks() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        // Real orchestrator scaffold: ckbadger.toml + mainnet/ + testnet/.
        cmd_init(&root, &InitArgs { with_testnet: true }).unwrap();
        assert!(
            is_orchestrator(&root),
            "init --with-testnet makes an orchestrator root"
        );

        // Seed derived data in each network + a stale file in the shared root run dir.
        let mainnet_domain = root.join("mainnet/data/domain/x.db");
        let testnet_domain = root.join("testnet/data/domain/x.db");
        std::fs::create_dir_all(mainnet_domain.parent().unwrap()).unwrap();
        std::fs::create_dir_all(testnet_domain.parent().unwrap()).unwrap();
        std::fs::write(&mainnet_domain, "m").unwrap();
        std::fs::write(&testnet_domain, "t").unwrap();
        std::fs::create_dir_all(root.join("run")).unwrap();
        let root_run_file = root.join("run/supervisor.pid");
        std::fs::write(&root_run_file, "999").unwrap();

        // The bug: this used to bail "not initialized (no config.toml at <root>/config.toml)"
        // because cmd_purge never checked is_orchestrator.
        let args = PurgeArgs { confirm: true };
        cmd_purge(&root, &args).unwrap();

        assert!(!mainnet_domain.exists(), "mainnet data purged");
        assert!(!testnet_domain.exists(), "testnet data purged");
        assert!(!root_run_file.exists(), "shared root run dir purged");
        // Configs preserved.
        assert!(
            root.join("ckbadger.toml").is_file(),
            "orchestrator config preserved"
        );
        assert!(
            root.join("mainnet/config.toml").is_file(),
            "mainnet config preserved"
        );
        assert!(
            root.join("testnet/config.toml").is_file(),
            "testnet config preserved"
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

    #[test]
    #[cfg(target_os = "linux")]
    fn test_cli_process_exposes_jemalloc_stats() {
        let snapshot = ckbadger_indexer::runtime_diag::read_process_memory_snapshot().unwrap();
        assert!(snapshot.jemalloc_stats_available);
        assert!(snapshot.jemalloc_allocated_bytes > 0);
        assert!(snapshot.jemalloc_resident_bytes > 0);
    }
}

#[cfg(test)]
mod store_runtime_config_tests {
    use super::*;
    use std::num::NonZeroUsize;
    use tempfile::TempDir;

    #[test]
    fn network_count_comes_from_the_governing_orchestrator_config() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("ckbadger.toml"),
            "[[network]]\nname = \"mainnet\"\n\n[[network]]\nname = \"testnet\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("mainnet")).unwrap();

        let cfg = store_runtime_config(&StoreConfig::default(), &root.join("mainnet")).unwrap();

        // Two co-resident networks -> each store sizes to half the host RAM.
        assert_eq!(cfg.network_count, NonZeroUsize::new(2).unwrap());
    }

    #[test]
    fn network_count_is_one_for_a_single_network_workdir() {
        let tmp = TempDir::new().unwrap();
        let workdir = tmp.path().join("work");
        std::fs::create_dir_all(&workdir).unwrap();

        let cfg = store_runtime_config(&StoreConfig::default(), &workdir).unwrap();

        // No governing orchestrator: the degenerate N=1 case, full detected RAM.
        assert_eq!(cfg.network_count, NonZeroUsize::MIN);
    }

    #[test]
    fn an_unreadable_governing_orchestrator_config_propagates_instead_of_sizing_to_one() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // An orchestrator IS present but will not parse. Silently sizing to 1 here
        // would hand every network the whole host — the over-commit this fix exists
        // to prevent — so the builder must propagate, not guess.
        std::fs::write(root.join("ckbadger.toml"), "not valid toml {{{").unwrap();
        let workdir = root.join("mainnet");
        std::fs::create_dir_all(&workdir).unwrap();

        assert!(store_runtime_config(&StoreConfig::default(), &workdir).is_err());
    }

    #[test]
    fn explicit_memory_budget_is_preserved_alongside_the_network_count() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("ckbadger.toml"),
            "[[network]]\nname = \"mainnet\"\n\n[[network]]\nname = \"testnet\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("mainnet")).unwrap();

        let store = StoreConfig {
            memory_budget_gb: Some(40),
            ..StoreConfig::default()
        };
        let cfg = store_runtime_config(&store, &root.join("mainnet")).unwrap();

        // The store layer decides precedence (override wins, undivided); the
        // builder just carries both values through.
        assert_eq!(cfg.memory_budget_gb, Some(40));
        assert_eq!(cfg.network_count, NonZeroUsize::new(2).unwrap());
    }
}
