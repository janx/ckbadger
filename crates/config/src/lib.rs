//! Configuration crate for ckbadger.
//!
//! Provides TOML-based configuration parsing, work directory resolution,
//! and share directory discovery.
//!
//! Priority: CLI args > ckbadger.toml > defaults

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Config structs
// ---------------------------------------------------------------------------

/// Top-level configuration, parsed from ckbadger.toml.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CkbadgerConfig {
    pub ckb: CkbConfig,
    pub api: ApiConfig,
    pub frontend: FrontendConfig,
    pub indexer: IndexerConfig,
    pub store: StoreConfig,
    pub log: LogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CkbConfig {
    pub rpc_url: String,
    pub network: String,
    /// Path to the CKB node config directory used by `ckb -C <path> run`.
    /// Empty means "not configured yet" and must fail fast at service startup.
    pub workdir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ApiConfig {
    pub host: String,
    pub port: u16,
    pub rate_limit: u32,
    pub rate_limit_burst: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FrontendConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct IndexerConfig {
    pub batch_size: usize,
    pub parallel_fetch_size: usize,
    pub pipeline_buffer: usize,
    pub bulk_sync_threshold: u64,
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct StoreConfig {
    pub domain_data_path: String,
    pub append_only_data_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_budget_gb: Option<u64>,
    pub direct_io_reads: bool,
    #[serde(default = "default_decoder_cache_path")]
    pub decoder_cache_path: String,
}

fn default_decoder_cache_path() -> String {
    "data/decoder-cache".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
}

// ---------------------------------------------------------------------------
// Default impls — these MUST match the values in the TOML example
// ---------------------------------------------------------------------------

impl Default for CkbConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://127.0.0.1:8114".to_string(),
            network: "mainnet".to_string(),
            workdir: Some(String::new()),
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8101,
            rate_limit: 100,
            rate_limit_burst: 200,
        }
    }
}

impl Default for FrontendConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8100,
        }
    }
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            batch_size: 10000,
            parallel_fetch_size: 64,
            pipeline_buffer: 8,
            bulk_sync_threshold: 1000,
            poll_interval_ms: 1000,
        }
    }
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            domain_data_path: "data/domain".to_string(),
            append_only_data_path: "data/append-only".to_string(),
            memory_budget_gb: None,
            direct_io_reads: true,
            decoder_cache_path: default_decoder_cache_path(),
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// WorkDir
// ---------------------------------------------------------------------------

/// Resolved paths for a work directory.
///
/// All paths are constructed deterministically from `root`.
/// `metadata` is `Some` only when the corresponding path exists on disk
/// at resolve time.
#[derive(Debug, Clone)]
pub struct WorkDir {
    /// The work directory root.
    pub root: PathBuf,
    /// Path to ckbadger.toml.
    pub config_path: PathBuf,
    /// Mutable canonical state (RocksDB domain store).
    pub domain_data: PathBuf,
    /// Immutable history (RocksDB append-only store).
    pub append_only_data: PathBuf,
    /// Runtime state directory.
    pub run_dir: PathBuf,
    /// Performance artifacts directory.
    pub perf_dir: PathBuf,
    /// Bulk-sync performance artifacts directory.
    pub bulk_sync_perf_dir: PathBuf,
    /// Supervisor PID file.
    pub supervisor_pid: PathBuf,
    /// Indexer IPC socket.
    pub indexer_sock: PathBuf,
    /// Process log directory.
    pub log_dir: PathBuf,
    /// Local metadata override directory (if it exists on disk).
    pub metadata: Option<PathBuf>,
}

impl WorkDir {
    /// Construct all work directory paths from `root`.
    ///
    /// `metadata` is set to `Some` only when the corresponding path
    /// already exists on disk.
    pub fn resolve(root: &Path) -> Self {
        let root = root.to_path_buf();
        let config_path = root.join("ckbadger.toml");
        let data_dir = root.join("data");
        let domain_data = data_dir.join("domain");
        let append_only_data = data_dir.join("append-only");
        let run_dir = root.join("run");
        let perf_dir = root.join("perf");
        let bulk_sync_perf_dir = perf_dir.join("bulk-sync");
        let supervisor_pid = run_dir.join("supervisor.pid");
        let indexer_sock = run_dir.join("indexer.sock");
        let log_dir = run_dir.join("logs");

        let metadata_path = root.join("metadata");
        let metadata = if metadata_path.exists() {
            Some(metadata_path)
        } else {
            None
        };

        Self {
            root,
            config_path,
            domain_data,
            append_only_data,
            run_dir,
            perf_dir,
            bulk_sync_perf_dir,
            supervisor_pid,
            indexer_sock,
            log_dir,
            metadata,
        }
    }

    /// Returns true if the work directory has been initialized
    /// (i.e. ckbadger.toml exists).
    pub fn is_initialized(&self) -> bool {
        self.config_path.exists()
    }
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

/// Load configuration from `ckbadger.toml` inside the given work directory.
///
/// Missing keys in the TOML file fall back to their default values.
/// If the file does not exist, returns an error (the work directory
/// should be initialized first via `ckbadger init`).
pub fn load_config(work_dir: &Path) -> Result<CkbadgerConfig> {
    let config_path = work_dir.join("ckbadger.toml");
    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read config file: {}", config_path.display()))?;
    parse_config(&content)
}

/// Parse a TOML string into `CkbadgerConfig`.
///
/// Missing keys fall back to their default values via `#[serde(default)]`.
pub fn parse_config(toml_str: &str) -> Result<CkbadgerConfig> {
    let value: toml::Value = toml::from_str(toml_str).context("failed to parse ckbadger.toml")?;
    if value
        .get("ckb")
        .and_then(toml::Value::as_table)
        .and_then(|ckb| ckb.get("data_path"))
        .is_some()
    {
        bail!("[ckb].data_path has been removed; use [ckb].workdir");
    }

    toml::from_str(toml_str).context("failed to parse ckbadger.toml")
}

// ---------------------------------------------------------------------------
// Default config generation
// ---------------------------------------------------------------------------

/// Generate the default ckbadger.toml content for `ckbadger init`.
///
/// The output is a hand-crafted TOML string (not serialized from the struct)
/// so we can include comments explaining each field.
pub fn default_config_toml() -> String {
    r#"[ckb]
rpc_url = "http://127.0.0.1:8114"
network = "mainnet"               # mainnet | testnet
workdir = ""                      # REQUIRED: CKB node config directory

[api]
host = "127.0.0.1"
port = 8101
rate_limit = 100
rate_limit_burst = 200

[frontend]
host = "127.0.0.1"
port = 8100

[indexer]
batch_size = 10000
parallel_fetch_size = 64
pipeline_buffer = 8
bulk_sync_threshold = 1000
poll_interval_ms = 1000

[store]
domain_data_path = "data/domain"
append_only_data_path = "data/append-only"
# memory_budget_gb = 32           # Optional RocksDB RAM budget override
direct_io_reads = true

[log]
level = "info"
"#
    .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStorePaths {
    pub domain_data: PathBuf,
    pub append_only_data: PathBuf,
    pub decoder_cache: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCkbPaths {
    pub ckb_workdir: PathBuf,
    pub ckb_config_path: PathBuf,
    pub ckb_data_dir: PathBuf,
    pub ckb_db_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CkbNodeConfig {
    data_dir: Option<String>,
    #[serde(default)]
    db: CkbNodeDbConfig,
}

#[derive(Debug, Default, Deserialize)]
struct CkbNodeDbConfig {
    path: Option<String>,
}

pub fn resolve_workdir_path(work_dir: &Path, configured_path: &str) -> PathBuf {
    let configured = PathBuf::from(configured_path);
    if configured.is_absolute() {
        configured
    } else {
        work_dir.join(configured)
    }
}

pub fn resolve_store_paths(work_dir: &Path, store: &StoreConfig) -> ResolvedStorePaths {
    ResolvedStorePaths {
        domain_data: resolve_workdir_path(work_dir, &store.domain_data_path),
        append_only_data: resolve_workdir_path(work_dir, &store.append_only_data_path),
        decoder_cache: resolve_workdir_path(work_dir, &store.decoder_cache_path),
    }
}

pub fn resolve_ckb_paths(work_dir: &Path, ckb: &CkbConfig) -> Result<ResolvedCkbPaths> {
    let configured_workdir = ckb.workdir.as_deref().map(str::trim).unwrap_or_default();
    if configured_workdir.is_empty() {
        bail!("ckb.workdir is required");
    }

    let ckb_workdir = resolve_workdir_path(work_dir, configured_workdir);
    let ckb_config_path = ckb_workdir.join("ckb.toml");
    if !ckb_config_path.is_file() {
        bail!("ckb.toml not found under {}", ckb_workdir.display());
    }

    let ckb_config_content = std::fs::read_to_string(&ckb_config_path)
        .with_context(|| format!("failed to read CKB config at {}", ckb_config_path.display()))?;
    let ckb_node_config: CkbNodeConfig =
        toml::from_str(&ckb_config_content).with_context(|| {
            format!(
                "failed to parse CKB config at {}",
                ckb_config_path.display()
            )
        })?;

    let data_dir = ckb_node_config
        .data_dir
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if data_dir.is_empty() {
        bail!("CKB config data_dir is blank");
    }

    let ckb_data_dir = resolve_workdir_path(&ckb_workdir, data_dir);
    let db_path = ckb_node_config
        .db
        .path
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    let ckb_db_path = if db_path.is_empty() {
        ckb_data_dir.join("db")
    } else {
        resolve_workdir_path(&ckb_workdir, db_path)
    };

    if !ckb_db_path.exists() {
        bail!(
            "resolved CKB RocksDB path does not exist: {}",
            ckb_db_path.display()
        );
    }

    Ok(ResolvedCkbPaths {
        ckb_workdir,
        ckb_config_path,
        ckb_data_dir,
        ckb_db_path,
    })
}

// ---------------------------------------------------------------------------
// Share directory resolution
// ---------------------------------------------------------------------------

/// Resolve the share directory for bundled assets.
///
/// Looks for `../share/` relative to the running binary's location.
/// Returns `None` if `current_exe()` fails or the share directory
/// does not exist.
///
/// Expected layout:
/// ```text
/// install-prefix/
///   bin/ckbadger          <- binary
///   share/                <- share dir
///     frontend/
/// ```
pub fn resolve_share_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // Resolve symlinks so we get the real binary location
    let exe = exe.canonicalize().ok()?;
    let bin_dir = exe.parent()?;
    let share_dir = bin_dir.parent()?.join("share");
    if share_dir.is_dir() {
        Some(share_dir)
    } else {
        None
    }
}

/// Resolve the share directory from an explicit binary path.
///
/// Same logic as [`resolve_share_dir`] but uses the provided path instead
/// of `std::env::current_exe()`. Useful for testing.
pub fn resolve_share_dir_from(exe_path: &Path) -> Option<PathBuf> {
    let bin_dir = exe_path.parent()?;
    let share_dir = bin_dir.parent()?.join("share");
    if share_dir.is_dir() {
        Some(share_dir)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // -- Default values --

    #[test]
    fn test_default_config_values() {
        let cfg = CkbadgerConfig::default();

        assert_eq!(cfg.ckb.rpc_url, "http://127.0.0.1:8114");
        assert_eq!(cfg.ckb.network, "mainnet");
        assert_eq!(cfg.ckb.workdir, Some(String::new()));

        assert_eq!(cfg.api.host, "127.0.0.1");
        assert_eq!(cfg.api.port, 8101);
        assert_eq!(cfg.api.rate_limit, 100);
        assert_eq!(cfg.api.rate_limit_burst, 200);

        assert_eq!(cfg.frontend.host, "127.0.0.1");
        assert_eq!(cfg.frontend.port, 8100);

        assert_eq!(cfg.indexer.batch_size, 10000);
        assert_eq!(cfg.indexer.parallel_fetch_size, 64);
        assert_eq!(cfg.indexer.pipeline_buffer, 8);
        assert_eq!(cfg.indexer.bulk_sync_threshold, 1000);
        assert_eq!(cfg.indexer.poll_interval_ms, 1000);

        assert_eq!(cfg.store.domain_data_path, "data/domain");
        assert_eq!(cfg.store.append_only_data_path, "data/append-only");
        assert_eq!(cfg.store.memory_budget_gb, None);
        assert!(cfg.store.direct_io_reads);

        assert_eq!(cfg.log.level, "info");
    }

    // -- TOML parsing --

    #[test]
    fn test_parse_empty_toml_uses_defaults() {
        let cfg = parse_config("").unwrap();
        assert_eq!(cfg, CkbadgerConfig::default());
    }

    #[test]
    fn test_parse_partial_toml_fills_defaults() {
        let toml = r#"
[ckb]
network = "testnet"

[api]
port = 9999
"#;
        let cfg = parse_config(toml).unwrap();
        assert_eq!(cfg.ckb.network, "testnet");
        assert_eq!(cfg.ckb.rpc_url, "http://127.0.0.1:8114"); // default
        assert_eq!(cfg.api.port, 9999);
        assert_eq!(cfg.api.host, "127.0.0.1"); // default
        assert_eq!(cfg.frontend.host, "127.0.0.1"); // default
        assert_eq!(cfg.frontend.port, 8100); // default
    }

    #[test]
    fn test_parse_full_toml() {
        let toml = r#"
[ckb]
rpc_url = "http://10.0.0.1:8114"
network = "testnet"
workdir = "/data/ckb-node"

[api]
host = "0.0.0.0"
port = 3001
rate_limit = 50
rate_limit_burst = 100

[frontend]
host = "0.0.0.0"
port = 3000

[indexer]
batch_size = 5000
parallel_fetch_size = 32
pipeline_buffer = 4
bulk_sync_threshold = 500
poll_interval_ms = 2000

[store]
domain_data_path = "/data/domain"
append_only_data_path = "/data/append"
memory_budget_gb = 48
direct_io_reads = false

[log]
level = "debug"
"#;
        let cfg = parse_config(toml).unwrap();
        assert_eq!(cfg.ckb.rpc_url, "http://10.0.0.1:8114");
        assert_eq!(cfg.ckb.network, "testnet");
        assert_eq!(cfg.ckb.workdir, Some("/data/ckb-node".to_string()));
        assert_eq!(cfg.api.host, "0.0.0.0");
        assert_eq!(cfg.api.port, 3001);
        assert_eq!(cfg.api.rate_limit, 50);
        assert_eq!(cfg.api.rate_limit_burst, 100);
        assert_eq!(cfg.frontend.host, "0.0.0.0");
        assert_eq!(cfg.frontend.port, 3000);
        assert_eq!(cfg.indexer.batch_size, 5000);
        assert_eq!(cfg.indexer.parallel_fetch_size, 32);
        assert_eq!(cfg.indexer.pipeline_buffer, 4);
        assert_eq!(cfg.indexer.bulk_sync_threshold, 500);
        assert_eq!(cfg.indexer.poll_interval_ms, 2000);
        assert_eq!(cfg.store.domain_data_path, "/data/domain");
        assert_eq!(cfg.store.append_only_data_path, "/data/append");
        assert_eq!(cfg.store.memory_budget_gb, Some(48));
        assert!(!cfg.store.direct_io_reads);
        assert_eq!(cfg.log.level, "debug");
    }

    #[test]
    fn test_parse_invalid_toml_returns_error() {
        let result = parse_config("not valid [[[toml");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_wrong_type_returns_error() {
        let toml = r#"
[api]
port = "not_a_number"
"#;
        let result = parse_config(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_config_rejects_legacy_ckb_data_path() {
        let err = parse_config(
            r#"
[ckb]
data_path = "/var/lib/ckb/data/db"
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("[ckb].data_path has been removed"));
    }

    // -- Default config TOML generation --

    #[test]
    fn test_default_config_toml_round_trips_to_defaults() {
        let toml_str = default_config_toml();
        let cfg = parse_config(&toml_str).unwrap();
        assert_eq!(cfg, CkbadgerConfig::default());
    }

    #[test]
    fn test_default_config_toml_declares_ckb_workdir() {
        let toml_str = default_config_toml();
        assert!(toml_str.contains("workdir = "));
        assert!(!toml_str.contains("\ndata_path = "));
    }

    #[test]
    fn test_default_config_toml_contains_all_sections() {
        let toml_str = default_config_toml();
        assert!(toml_str.contains("[ckb]"));
        assert!(toml_str.contains("[api]"));
        assert!(toml_str.contains("[frontend]"));
        assert!(toml_str.contains("[indexer]"));
        assert!(toml_str.contains("[store]"));
        assert!(toml_str.contains("[log]"));
    }

    #[test]
    fn test_resolve_workdir_path_keeps_absolute_paths() {
        let root = Path::new("/tmp/ckbadger");
        assert_eq!(
            resolve_workdir_path(root, "/var/lib/ckbadger/domain"),
            PathBuf::from("/var/lib/ckbadger/domain")
        );
    }

    #[test]
    fn test_resolve_workdir_path_resolves_relative_paths() {
        let root = Path::new("/tmp/ckbadger");
        assert_eq!(
            resolve_workdir_path(root, "data/domain"),
            PathBuf::from("/tmp/ckbadger/data/domain")
        );
    }

    #[test]
    fn test_resolve_store_paths_uses_store_config() {
        let root = Path::new("/tmp/ckbadger");
        let store = StoreConfig {
            domain_data_path: "custom/domain".to_string(),
            append_only_data_path: "/ssd/append-only".to_string(),
            memory_budget_gb: Some(32),
            direct_io_reads: false,
            decoder_cache_path: default_decoder_cache_path(),
        };

        let resolved = resolve_store_paths(root, &store);
        assert_eq!(
            resolved.domain_data,
            PathBuf::from("/tmp/ckbadger/custom/domain")
        );
        assert_eq!(resolved.append_only_data, PathBuf::from("/ssd/append-only"));
    }

    #[test]
    fn test_resolve_ckb_paths_uses_relative_data_dir_default_db_path() {
        let ckbadger_root = TempDir::new().unwrap();
        let ckb_root = ckbadger_root.path().join("node");
        std::fs::create_dir_all(ckb_root.join("data/db")).unwrap();
        std::fs::write(ckb_root.join("ckb.toml"), "data_dir = \"data\"\n").unwrap();

        let config = parse_config("[ckb]\nworkdir = \"node\"\n").unwrap();
        let resolved = resolve_ckb_paths(ckbadger_root.path(), &config.ckb).unwrap();

        assert_eq!(resolved.ckb_workdir, ckb_root);
        assert_eq!(resolved.ckb_config_path, ckb_root.join("ckb.toml"));
        assert_eq!(resolved.ckb_data_dir, ckb_root.join("data"));
        assert_eq!(resolved.ckb_db_path, ckb_root.join("data/db"));
    }

    #[test]
    fn test_resolve_ckb_paths_uses_relative_db_path_override() {
        let ckbadger_root = TempDir::new().unwrap();
        let ckb_root = ckbadger_root.path().join("node");
        std::fs::create_dir_all(ckb_root.join("custom-db")).unwrap();
        std::fs::write(
            ckb_root.join("ckb.toml"),
            "data_dir = \"data\"\n[db]\npath = \"custom-db\"\n",
        )
        .unwrap();

        let config = parse_config("[ckb]\nworkdir = \"node\"\n").unwrap();
        let resolved = resolve_ckb_paths(ckbadger_root.path(), &config.ckb).unwrap();

        assert_eq!(resolved.ckb_db_path, ckb_root.join("custom-db"));
    }

    #[test]
    fn test_resolve_ckb_paths_rejects_missing_ckb_toml() {
        let ckbadger_root = TempDir::new().unwrap();
        let config = parse_config("[ckb]\nworkdir = \"node\"\n").unwrap();

        let err = resolve_ckb_paths(ckbadger_root.path(), &config.ckb).unwrap_err();

        assert!(err.to_string().contains("ckb.toml not found under"));
    }

    // -- load_config --

    #[test]
    fn test_load_config_from_file() {
        let dir = TempDir::new().unwrap();
        let config_content = r#"
[ckb]
network = "testnet"
"#;
        std::fs::write(dir.path().join("ckbadger.toml"), config_content).unwrap();

        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg.ckb.network, "testnet");
        assert_eq!(cfg.api.port, 8101); // default
    }

    #[test]
    fn test_load_config_missing_file_returns_error() {
        let dir = TempDir::new().unwrap();
        let result = load_config(dir.path());
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("failed to read config file"),
            "unexpected error: {}",
            err_msg
        );
    }

    // -- WorkDir --

    #[test]
    fn test_workdir_resolve_paths() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let wd = WorkDir::resolve(root);

        assert_eq!(wd.root, root);
        assert_eq!(wd.config_path, root.join("ckbadger.toml"));
        assert_eq!(wd.domain_data, root.join("data/domain"));
        assert_eq!(wd.append_only_data, root.join("data/append-only"));
        assert_eq!(wd.run_dir, root.join("run"));
        assert_eq!(wd.perf_dir, root.join("perf"));
        assert_eq!(wd.bulk_sync_perf_dir, root.join("perf/bulk-sync"));
        assert_eq!(wd.supervisor_pid, root.join("run/supervisor.pid"));
        assert_eq!(wd.indexer_sock, root.join("run/indexer.sock"));
        assert_eq!(wd.log_dir, root.join("run/logs"));
        assert!(wd.metadata.is_none());
    }

    #[test]
    fn test_workdir_resolve_with_metadata_dir() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("metadata")).unwrap();

        let wd = WorkDir::resolve(root);
        assert_eq!(wd.metadata, Some(root.join("metadata")));
    }

    #[test]
    fn test_workdir_resolve_without_metadata_dir() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let wd = WorkDir::resolve(root);
        assert!(wd.metadata.is_none());
    }

    #[test]
    fn test_workdir_is_initialized() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let wd = WorkDir::resolve(root);
        assert!(!wd.is_initialized());

        std::fs::write(root.join("ckbadger.toml"), "").unwrap();
        let wd = WorkDir::resolve(root);
        assert!(wd.is_initialized());
    }

    // -- Share directory resolution --

    #[test]
    fn test_resolve_share_dir_from_with_existing_share() {
        let dir = TempDir::new().unwrap();
        // Simulate: install_prefix/bin/ckbadger and install_prefix/share/
        let bin_dir = dir.path().join("bin");
        let share_dir = dir.path().join("share");
        std::fs::create_dir(&bin_dir).unwrap();
        std::fs::create_dir(&share_dir).unwrap();
        let exe_path = bin_dir.join("ckbadger");

        let result = resolve_share_dir_from(&exe_path);
        assert_eq!(result, Some(share_dir));
    }

    #[test]
    fn test_resolve_share_dir_from_without_share() {
        let dir = TempDir::new().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir(&bin_dir).unwrap();
        let exe_path = bin_dir.join("ckbadger");

        let result = resolve_share_dir_from(&exe_path);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_share_dir_from_share_is_file_not_dir() {
        let dir = TempDir::new().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir(&bin_dir).unwrap();
        // share exists but is a file, not a directory
        std::fs::write(dir.path().join("share"), "not a dir").unwrap();
        let exe_path = bin_dir.join("ckbadger");

        let result = resolve_share_dir_from(&exe_path);
        assert!(result.is_none());
    }
}
