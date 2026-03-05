//! Configuration crate for ckbadger.
//!
//! Provides TOML-based configuration parsing, work directory resolution,
//! share directory discovery, and token labels path resolution.
//!
//! Priority: CLI args > ckbadger.toml > defaults

use anyhow::{Context, Result};
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
    pub log: LogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CkbConfig {
    pub rpc_url: String,
    pub network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ApiConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FrontendConfig {
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
    pub pipeline_enabled: bool,
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
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8101,
        }
    }
}

impl Default for FrontendConfig {
    fn default() -> Self {
        Self { port: 8100 }
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
            pipeline_enabled: true,
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
/// `token_labels` and `labels_toml` are `Some` only when the
/// corresponding path exists on disk at resolve time.
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
    /// Supervisor PID file.
    pub supervisor_pid: PathBuf,
    /// Indexer IPC socket.
    pub indexer_sock: PathBuf,
    /// Process log directory.
    pub log_dir: PathBuf,
    /// Local token-labels override directory (if it exists on disk).
    pub token_labels: Option<PathBuf>,
    /// Local labels.toml override file (if it exists on disk).
    pub labels_toml: Option<PathBuf>,
}

impl WorkDir {
    /// Construct all work directory paths from `root`.
    ///
    /// `token_labels` and `labels_toml` are set to `Some` only when the
    /// corresponding path already exists on disk.
    pub fn resolve(root: &Path) -> Self {
        let root = root.to_path_buf();
        let config_path = root.join("ckbadger.toml");
        let data_dir = root.join("data");
        let domain_data = data_dir.join("domain");
        let append_only_data = data_dir.join("append-only");
        let run_dir = root.join("run");
        let supervisor_pid = run_dir.join("supervisor.pid");
        let indexer_sock = run_dir.join("indexer.sock");
        let log_dir = run_dir.join("logs");

        let token_labels_path = root.join("token-labels");
        let token_labels = if token_labels_path.exists() {
            Some(token_labels_path)
        } else {
            None
        };

        let labels_toml_path = root.join("labels.toml");
        let labels_toml = if labels_toml_path.exists() {
            Some(labels_toml_path)
        } else {
            None
        };

        Self {
            root,
            config_path,
            domain_data,
            append_only_data,
            run_dir,
            supervisor_pid,
            indexer_sock,
            log_dir,
            token_labels,
            labels_toml,
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

[api]
host = "127.0.0.1"
port = 8101

[frontend]
port = 8100

[indexer]
batch_size = 10000
parallel_fetch_size = 64
pipeline_buffer = 8
bulk_sync_threshold = 1000
poll_interval_ms = 1000
pipeline_enabled = true

[log]
level = "info"
"#
    .to_string()
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
///     token-labels/
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
// Token labels resolution
// ---------------------------------------------------------------------------

/// Resolve the token labels path.
///
/// Priority:
/// 1. `work_dir/token-labels/` (local override) — if it exists
/// 2. `share_dir/token-labels/` (bundled default) — if share_dir is Some and the path exists
/// 3. `None` — no token labels available
pub fn resolve_token_labels_path(work_dir: &WorkDir, share_dir: Option<&Path>) -> Option<PathBuf> {
    // 1. Check work_dir local override (already resolved in WorkDir)
    if work_dir.token_labels.is_some() {
        return work_dir.token_labels.clone();
    }

    // 2. Check share directory
    if let Some(share) = share_dir {
        let share_labels = share.join("token-labels");
        if share_labels.is_dir() {
            return Some(share_labels);
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

    // -- Default values --

    #[test]
    fn test_default_config_values() {
        let cfg = CkbadgerConfig::default();

        assert_eq!(cfg.ckb.rpc_url, "http://127.0.0.1:8114");
        assert_eq!(cfg.ckb.network, "mainnet");

        assert_eq!(cfg.api.host, "127.0.0.1");
        assert_eq!(cfg.api.port, 8101);

        assert_eq!(cfg.frontend.port, 8100);

        assert_eq!(cfg.indexer.batch_size, 10000);
        assert_eq!(cfg.indexer.parallel_fetch_size, 64);
        assert_eq!(cfg.indexer.pipeline_buffer, 8);
        assert_eq!(cfg.indexer.bulk_sync_threshold, 1000);
        assert_eq!(cfg.indexer.poll_interval_ms, 1000);
        assert!(cfg.indexer.pipeline_enabled);

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
        assert_eq!(cfg.frontend.port, 8100); // default
    }

    #[test]
    fn test_parse_full_toml() {
        let toml = r#"
[ckb]
rpc_url = "http://10.0.0.1:8114"
network = "testnet"

[api]
host = "0.0.0.0"
port = 3001

[frontend]
port = 3000

[indexer]
batch_size = 5000
parallel_fetch_size = 32
pipeline_buffer = 4
bulk_sync_threshold = 500
poll_interval_ms = 2000
pipeline_enabled = false

[log]
level = "debug"
"#;
        let cfg = parse_config(toml).unwrap();
        assert_eq!(cfg.ckb.rpc_url, "http://10.0.0.1:8114");
        assert_eq!(cfg.ckb.network, "testnet");
        assert_eq!(cfg.api.host, "0.0.0.0");
        assert_eq!(cfg.api.port, 3001);
        assert_eq!(cfg.frontend.port, 3000);
        assert_eq!(cfg.indexer.batch_size, 5000);
        assert_eq!(cfg.indexer.parallel_fetch_size, 32);
        assert_eq!(cfg.indexer.pipeline_buffer, 4);
        assert_eq!(cfg.indexer.bulk_sync_threshold, 500);
        assert_eq!(cfg.indexer.poll_interval_ms, 2000);
        assert!(!cfg.indexer.pipeline_enabled);
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

    // -- Default config TOML generation --

    #[test]
    fn test_default_config_toml_round_trips_to_defaults() {
        let toml_str = default_config_toml();
        let cfg = parse_config(&toml_str).unwrap();
        assert_eq!(cfg, CkbadgerConfig::default());
    }

    #[test]
    fn test_default_config_toml_contains_all_sections() {
        let toml_str = default_config_toml();
        assert!(toml_str.contains("[ckb]"));
        assert!(toml_str.contains("[api]"));
        assert!(toml_str.contains("[frontend]"));
        assert!(toml_str.contains("[indexer]"));
        assert!(toml_str.contains("[log]"));
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
        assert_eq!(wd.supervisor_pid, root.join("run/supervisor.pid"));
        assert_eq!(wd.indexer_sock, root.join("run/indexer.sock"));
        assert_eq!(wd.log_dir, root.join("run/logs"));
        assert!(wd.token_labels.is_none());
        assert!(wd.labels_toml.is_none());
    }

    #[test]
    fn test_workdir_resolve_with_existing_token_labels() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("token-labels")).unwrap();

        let wd = WorkDir::resolve(root);
        assert_eq!(wd.token_labels, Some(root.join("token-labels")));
        assert!(wd.labels_toml.is_none());
    }

    #[test]
    fn test_workdir_resolve_with_existing_labels_toml() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::write(root.join("labels.toml"), "# labels").unwrap();

        let wd = WorkDir::resolve(root);
        assert!(wd.token_labels.is_none());
        assert_eq!(wd.labels_toml, Some(root.join("labels.toml")));
    }

    #[test]
    fn test_workdir_resolve_with_both_overrides() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("token-labels")).unwrap();
        std::fs::write(root.join("labels.toml"), "# labels").unwrap();

        let wd = WorkDir::resolve(root);
        assert_eq!(wd.token_labels, Some(root.join("token-labels")));
        assert_eq!(wd.labels_toml, Some(root.join("labels.toml")));
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

    // -- Token labels resolution --

    #[test]
    fn test_resolve_token_labels_workdir_takes_priority() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Set up both work_dir/token-labels and share/token-labels
        std::fs::create_dir(root.join("token-labels")).unwrap();
        let share_dir = dir.path().join("share-test");
        std::fs::create_dir(&share_dir).unwrap();
        std::fs::create_dir(share_dir.join("token-labels")).unwrap();

        let wd = WorkDir::resolve(root);
        let result = resolve_token_labels_path(&wd, Some(&share_dir));
        assert_eq!(result, Some(root.join("token-labels")));
    }

    #[test]
    fn test_resolve_token_labels_falls_back_to_share() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        // No work_dir/token-labels, but share/token-labels exists
        let share_dir = dir.path().join("share-test");
        std::fs::create_dir(&share_dir).unwrap();
        std::fs::create_dir(share_dir.join("token-labels")).unwrap();

        let wd = WorkDir::resolve(root);
        let result = resolve_token_labels_path(&wd, Some(&share_dir));
        assert_eq!(result, Some(share_dir.join("token-labels")));
    }

    #[test]
    fn test_resolve_token_labels_none_when_neither_exists() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let wd = WorkDir::resolve(root);
        let result = resolve_token_labels_path(&wd, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_token_labels_none_when_share_has_no_labels() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let share_dir = dir.path().join("share-test");
        std::fs::create_dir(&share_dir).unwrap();
        // share exists but has no token-labels subdirectory

        let wd = WorkDir::resolve(root);
        let result = resolve_token_labels_path(&wd, Some(&share_dir));
        assert!(result.is_none());
    }
}
