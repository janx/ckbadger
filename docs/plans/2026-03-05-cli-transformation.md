# CLI Transformation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Transform ckbadger from Docker-based multi-service deployment to a standalone CLI binary with zero external dependencies (no Docker, no Redis, no Node.js).

**Architecture:** Single `ckbadger` binary with subcommands. `ckbadger run` acts as a supervisor forking child processes for indexer, api, and frontend-server. Config via `ckbadger.toml` (no .env). RocksDB + Unix socket IPC replaces Redis. Frontend served as static files via Axum.

**Tech Stack:** clap 4.5 (CLI), toml/serde (config), tokio (async + process), Unix sockets (IPC), tower-http (static files)

---

## Phase 1: Config System & CLI Crate Foundation

### Task 1: Create ckbadger-config crate

**Files:**

- Create: `crates/config/Cargo.toml`
- Create: `crates/config/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: Create the config crate**

`crates/config/Cargo.toml`:

```toml
[package]
name = "ckbadger-config"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true, features = ["derive"] }
toml = "0.8"
anyhow = { workspace = true }
directories = "5"
```

`crates/config/src/lib.rs` — Define the TOML config struct and loading logic:

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CkbadgerConfig {
    pub ckb: CkbConfig,
    pub api: ApiConfig,
    pub frontend: FrontendConfig,
    pub indexer: IndexerConfig,
    pub log: LogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CkbConfig {
    pub rpc_url: String,
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiConfig {
    pub host: String,
    pub port: u16,
    pub rate_limit: u32,
    pub rate_limit_burst: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FrontendConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexerConfig {
    pub batch_size: usize,
    pub parallel_fetch_size: usize,
    pub pipeline_buffer: usize,
    pub bulk_sync_threshold: u64,
    pub poll_interval_ms: u64,
    pub pipeline_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
}

// Implement Default for all config structs with values from design doc
// Defaults: api.port=8101, frontend.port=8100, ckb.rpc_url="http://127.0.0.1:8114", etc.

/// Resolved paths for a work directory
#[derive(Debug, Clone)]
pub struct WorkDir {
    pub root: PathBuf,
    pub config_path: PathBuf,        // ckbadger.toml
    pub domain_data: PathBuf,        // data/domain/
    pub append_only_data: PathBuf,   // data/append-only/
    pub run_dir: PathBuf,            // run/
    pub supervisor_pid: PathBuf,     // run/supervisor.pid
    pub indexer_sock: PathBuf,       // run/indexer.sock
    pub log_dir: PathBuf,            // run/logs/
    pub token_labels: Option<PathBuf>, // token-labels/ (if exists)
    pub labels_toml: Option<PathBuf>,  // labels.toml (if exists)
}

impl WorkDir {
    pub fn resolve(root: &Path) -> Self { /* ... */ }
    pub fn is_initialized(&self) -> bool { self.config_path.exists() }
}

/// Load config: CLI args > ckbadger.toml > defaults
pub fn load_config(work_dir: &Path) -> Result<CkbadgerConfig> { /* ... */ }

/// Generate default ckbadger.toml content
pub fn default_config_toml() -> String { /* ... */ }

/// Resolve share directory (for frontend assets, default token-labels)
/// Looks for ../share/ relative to binary location
pub fn resolve_share_dir() -> Option<PathBuf> { /* ... */ }

/// Resolve token labels path: work_dir/token-labels > share/token-labels
pub fn resolve_token_labels_path(work_dir: &WorkDir, share_dir: Option<&Path>) -> Option<PathBuf> { /* ... */ }
```

**Step 2: Add workspace member**

In root `Cargo.toml`, add `"crates/config"` to workspace members.

**Step 3: Write tests for config loading**

Test: load from TOML string, verify defaults, verify CLI override merging. Test WorkDir::resolve path construction. Test token-labels resolution order.

**Step 4: Run tests**

Run: `cargo test -p ckbadger-config`

**Step 5: Commit**

```bash
git add crates/config/ Cargo.toml Cargo.lock
git commit -m "feat: add ckbadger-config crate with TOML config system"
```

---

### Task 2: Create CLI crate with subcommand skeleton

**Files:**

- Create: `crates/cli/Cargo.toml`
- Create: `crates/cli/src/main.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: Create the CLI crate**

`crates/cli/Cargo.toml`:

```toml
[package]
name = "ckbadger"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "ckbadger"
path = "src/main.rs"

[dependencies]
ckbadger-config = { path = "../config" }
clap = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

`crates/cli/src/main.rs`:

```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    Purge,
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

// ... VerifyArgs, LabelImportArgs, InternalArgs structs

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let workdir = cli.workdir.unwrap_or_else(|| std::env::current_dir().unwrap());

    match cli.command {
        Command::Init => cmd_init(&workdir),
        Command::Purge => cmd_purge(&workdir),
        // ... other commands as stubs returning Ok(())
        _ => {
            eprintln!("Command not yet implemented");
            Ok(())
        }
    }
}
```

**Step 2: Implement `init` command**

Creates work directory structure, writes default `ckbadger.toml`, creates `data/` and `run/` directories.

**Step 3: Implement `purge` command**

Deletes `data/domain/`, `data/append-only/`, `run/` contents. Preserves `ckbadger.toml`, `token-labels/`, `labels.toml`.

**Step 4: Write tests**

Test init creates expected directory structure and config file. Test purge removes data but keeps config.

**Step 5: Run and verify**

```bash
cargo build -p ckbadger
./target/debug/ckbadger init -C /tmp/test-ckbadger
ls -la /tmp/test-ckbadger/
cat /tmp/test-ckbadger/ckbadger.toml
./target/debug/ckbadger purge -C /tmp/test-ckbadger
```

**Step 6: Commit**

```bash
git commit -m "feat: add ckbadger CLI crate with init and purge commands"
```

---

### Task 3: Labels.toml support

**Files:**

- Create: `crates/config/src/labels.rs`
- Modify: `crates/config/src/lib.rs`

**Step 1: Define labels.toml types**

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LabelsConfig {
    #[serde(default)]
    pub script_name_overrides: HashMap<String, String>,
    #[serde(default)]
    pub nft_storage_tier_overrides: HashMap<String, String>,
    #[serde(default)]
    pub deprecated: Vec<String>,
}
```

**Step 2: Load and merge logic**

Load from `work_dir/labels.toml` if exists. This replaces `docs/script-name-overrides.json`.

**Step 3: Tests**

Parse sample labels.toml, verify all fields.

**Step 4: Commit**

```bash
git commit -m "feat: add labels.toml config support"
```

---

## Phase 2: Convert Existing Crates to Libraries

### Task 4: Convert indexer to library

**Files:**

- Modify: `crates/indexer/Cargo.toml` — remove `[[bin]]` section, or keep for backward compat
- Modify: `crates/indexer/src/main.rs` → extract logic into `src/lib.rs` public functions
- Create: `crates/indexer/src/entry.rs` — public entry points

**Step 1: Extract run_sync into a public async function**

Move the startup sequence from `main.rs::run_sync()` into a public function in `src/entry.rs`:

```rust
/// Configuration for starting the indexer service
pub struct IndexerServiceConfig {
    pub domain_data_path: String,
    pub append_only_data_path: String,
    pub ckb_rpc_url: String,
    pub ckb_data_path: Option<String>,
    pub token_labels_path: Option<String>,
    pub batch_size: usize,
    pub poll_interval_ms: u64,
    pub parallel_fetch_size: usize,
    pub pipeline_enabled: bool,
    pub pipeline_buffer: usize,
    pub bulk_sync_threshold: u64,
}

/// Run the indexer sync daemon. Blocks until shutdown signal or error.
pub async fn run_indexer(config: IndexerServiceConfig) -> anyhow::Result<()> {
    // Move logic from main.rs::run_sync()
}

/// Run label import
pub async fn run_label_import(config: LabelImportConfig) -> anyhow::Result<()> {
    // Move logic from main.rs::run_label_import_command()
}
```

**Step 2: Keep main.rs as thin wrapper**

`main.rs` becomes a thin wrapper that parses CLI args and calls the lib functions. This maintains backward compatibility during transition.

**Step 3: Remove redis_url from IndexerServiceConfig**

Redis will be removed in Phase 3. For now, keep it but mark as `Option<String>` with deprecation comment.

**Step 4: Verify existing tests still pass**

Run: `cargo test -p ckbadger-indexer`

**Step 5: Commit**

```bash
git commit -m "refactor: extract indexer entry points to lib for CLI integration"
```

---

### Task 5: Convert API to library

**Files:**

- Modify: `crates/api/src/lib.rs` — already has `create_router()`, ensure full public API
- Modify: `crates/api/src/main.rs` — thin wrapper
- Create: `crates/api/src/entry.rs` — public entry point

**Step 1: Create public entry point**

```rust
pub struct ApiServiceConfig {
    pub domain_data_path: String,
    pub append_only_data_path: String,
    pub ckb_rpc_url: String,
    pub ckb_network: String,
    pub host: String,
    pub port: u16,
    pub rate_limit: u32,
    pub rate_limit_burst: u32,
    pub ckb_data_path: Option<String>,
    pub frontend_dir: Option<PathBuf>,  // NEW: static frontend assets
}

/// Run the API server. Blocks until shutdown.
pub async fn run_api(config: ApiServiceConfig) -> anyhow::Result<()> {
    // Move logic from main.rs
    // Add frontend static serving if frontend_dir is Some
}
```

**Step 2: Add static file serving support**

Add `tower-http = { workspace = true, features = ["fs"] }` to api Cargo.toml.

When `frontend_dir` is provided, merge a `ServeDir` fallback into the Axum router:

```rust
use tower_http::services::{ServeDir, ServeFile};

if let Some(frontend_dir) = &config.frontend_dir {
    let spa_fallback = ServeFile::new(frontend_dir.join("index.html"));
    let serve = ServeDir::new(frontend_dir).fallback(spa_fallback);
    router = router.fallback_service(serve);
}
```

**Step 3: Verify**

Run: `cargo test -p ckbadger-api`

**Step 4: Commit**

```bash
git commit -m "refactor: extract API entry points to lib, add static frontend serving"
```

---

### Task 6: Convert TUI to library

**Files:**

- Modify: `crates/tui/src/main.rs` — extract to lib
- Create: `crates/tui/src/entry.rs` — public entry point

**Step 1: Create public entry point**

```rust
pub struct TuiServiceConfig {
    pub domain_data_path: String,
    pub append_only_data_path: String,
    pub api_url: String,
    pub refresh_ms: u64,
}

/// Run the TUI. Blocks until user exits.
pub async fn run_tui(config: TuiServiceConfig) -> anyhow::Result<()> {
    // Move logic from main.rs
}
```

Note: Redis references will be removed in Phase 3. For now, set redis_url to None internally.

**Step 2: Verify**

Run: `cargo test -p ckbadger-tui`

**Step 3: Commit**

```bash
git commit -m "refactor: extract TUI entry points to lib"
```

---

### Task 7: Wire CLI to library entry points

**Files:**

- Modify: `crates/cli/Cargo.toml` — add deps on indexer, api, tui
- Modify: `crates/cli/src/main.rs` — implement run, tui, verify, label-import commands

**Step 1: Add dependencies**

```toml
ckbadger-indexer = { path = "../indexer" }
ckbadger-api = { path = "../api" }
ckbadger-tui = { path = "../tui" }
```

**Step 2: Implement `ckbadger verify`**

Load config from ckbadger.toml, construct VerifyArgs, call `ckbadger_indexer::verify::run()`.

**Step 3: Implement `ckbadger label-import`**

Load config, resolve token-labels path (work_dir > share), load labels.toml overrides, call indexer label import.

**Step 4: Implement `ckbadger tui`**

Load config, construct TuiServiceConfig, call `ckbadger_tui::run_tui()`.

**Step 5: Implement `ckbadger run` (simple mode first)**

For initial implementation, run indexer + api in-process as tokio tasks (single process, no supervisor yet). This gets things working before Phase 4 adds proper process isolation.

```rust
Command::Run(args) => {
    let config = load_config(&workdir)?;
    let work = WorkDir::resolve(&workdir);

    // Start API as background task
    let api_handle = tokio::spawn(run_api(ApiServiceConfig { ... }));

    // Start indexer as background task
    let indexer_handle = tokio::spawn(run_indexer(IndexerServiceConfig { ... }));

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    // Graceful shutdown...
}
```

**Step 6: Verify end-to-end**

```bash
cargo build -p ckbadger
./target/debug/ckbadger init -C /tmp/test
# Edit /tmp/test/ckbadger.toml with valid CKB RPC URL
./target/debug/ckbadger run -C /tmp/test
```

**Step 7: Commit**

```bash
git commit -m "feat: wire CLI commands to indexer/api/tui libraries"
```

---

## Phase 3: Remove Redis

### Task 8: Replace Redis sync data with RocksDB

**Files:**

- Modify: `crates/indexer/src/cache.rs` — remove Redis, write to RocksDB
- Modify: `crates/ckbadger-store/src/batch.rs` — add sync progress/memory stats put methods if needed
- Modify: `crates/ckbadger-store/src/*_ops.rs` — add read methods for sync progress/memory stats

**Step 1: Audit what sync data Redis stores**

Already stored in RocksDB (from exploration):

- `get_sync_tip()` / `get_sync_status()` — already in store
- `get_runtime_status()` — already in store

Need to add to RocksDB:

- `SyncProgressData` — pipeline metrics, ETA, blocks/sec (currently only in Redis)
- `MemoryStatsData` — RocksDB stats, memory diagnostics (currently only in Redis)

**Step 2: Add RocksDB column families or key patterns**

Add store methods to write/read SyncProgressData and MemoryStatsData to the domain store. These are ephemeral monitoring data that can be overwritten on restart.

**Step 3: Update CacheInvalidator**

Remove all `#[cfg(feature = "redis-cache")]` blocks. Replace Redis publish with direct RocksDB writes:

```rust
pub struct CacheInvalidator {
    store: Arc<CkbadgerStore>,
}

impl CacheInvalidator {
    pub fn publish_sync_progress(&self, data: &SyncProgressData) -> Result<()> {
        self.store.put_sync_progress(data)
    }

    pub fn publish_memory_stats(&self, data: &MemoryStatsData) -> Result<()> {
        self.store.put_memory_stats(data)
    }

    // Chart cache invalidation: no longer needed (was Redis-specific)
    // API cache: move to in-memory LRU (Task 9)
}
```

**Step 4: Update indexer main to not pass redis_url**

Remove redis_url from Config struct.

**Step 5: Verify**

Run: `cargo test -p ckbadger-indexer`
Run: `cargo check -p ckbadger-indexer` (ensure no redis references)

**Step 6: Commit**

```bash
git commit -m "refactor: replace Redis sync data with RocksDB in indexer"
```

---

### Task 9: Replace Redis API cache with in-memory LRU

**Files:**

- Modify: `crates/api/src/cache/mod.rs` — replace CacheBackend with in-memory only
- Remove: `crates/api/src/cache/redis_cache.rs`
- Modify: `crates/api/src/lib.rs` — remove Redis initialization

**Step 1: Simplify CacheBackend**

The API already has `CacheBackend::None` fallback. Replace `CacheBackend::Redis` with `CacheBackend::InMemory` using the existing `lru` dependency:

```rust
use lru::LruCache;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct CacheEntry {
    value: String, // JSON
    expires_at: Instant,
}

pub struct InMemoryCache {
    cache: Mutex<LruCache<String, CacheEntry>>,
}
```

**Step 2: Remove redis_cache.rs**

Delete the file. Remove `#[cfg(feature = "redis-cache")]` references in mod.rs.

**Step 3: Remove Redis from API AppConfig and AppState**

Remove `redis_url` from AppConfig. Remove CacheBackend::Redis variant.

**Step 4: Verify**

Run: `cargo test -p ckbadger-api`

**Step 5: Commit**

```bash
git commit -m "refactor: replace Redis API cache with in-memory LRU"
```

---

### Task 10: Remove Redis from cycles worker

**Files:**

- Modify: `crates/indexer/src/cycles_worker.rs` — replace Redis queue with in-process channel
- Modify: `crates/api/src/cycles.rs` — replace Redis with in-process channel

**Step 1: Replace Redis task queue with tokio mpsc channel**

Since indexer and api will run in the same process (or communicate via IPC), use `tokio::sync::mpsc` for cycles task dispatch:

```rust
// In cycles_worker.rs
pub fn spawn_cycles_task_worker(
    store: Arc<CkbadgerStore>,
    ckb_rpc_url: String,
    task_rx: mpsc::Receiver<String>, // tx_hash
    result_tx: broadcast::Sender<CyclesTaskResult>,
) { /* ... */ }
```

When running in multi-process mode (supervisor), cycles tasks can go through the Unix socket IPC instead.

**Step 2: Update CyclesClient in API**

Replace Redis connection with channel sender.

**Step 3: Verify**

Run: `cargo test -p ckbadger-indexer`
Run: `cargo test -p ckbadger-api`

**Step 4: Commit**

```bash
git commit -m "refactor: replace Redis cycles queue with in-process channels"
```

---

### Task 11: Remove Redis from TUI

**Files:**

- Modify: `crates/tui/src/db.rs` — remove all Redis, read from RocksDB + API
- Modify: `crates/tui/Cargo.toml` — remove redis dependency

**Step 1: Replace Redis reads with RocksDB reads**

TUI reads three Redis keys:

- `sync:status` → Read from RocksDB `get_sync_status()`
- `sync:progress` → Read from RocksDB `get_sync_progress()` (new method from Task 8)
- `memory:stats` → Read from RocksDB `get_memory_stats()` (new method from Task 8)

Open the store in secondary mode (same as API):

```rust
pub struct TuiDb {
    store: Arc<CkbadgerStore>,  // secondary mode
    api_url: String,
    http: reqwest::Client,
    // ... remove redis field
}
```

**Step 2: Remove Redis service info panel from TUI**

The Redis diagnostics panel (PING, DBSIZE, INFO) is no longer relevant. Replace with RocksDB diagnostics or remove.

**Step 3: Remove redis dependency from Cargo.toml**

**Step 4: Verify**

Run: `cargo test -p ckbadger-tui`

**Step 5: Commit**

```bash
git commit -m "refactor: remove Redis from TUI, read sync data from RocksDB"
```

---

### Task 12: Remove Redis from workspace

**Files:**

- Modify: `Cargo.toml` — remove redis from workspace dependencies
- Modify: `crates/common/src/sync.rs` — remove Redis key constants
- Modify: `crates/common/src/cycles_task.rs` — remove Redis key constants
- Modify: `crates/common/src/proposal.rs` — remove Redis key constants
- Modify: `crates/indexer/Cargo.toml` — remove redis dependency and feature
- Modify: `crates/api/Cargo.toml` — remove redis dependency and feature

**Step 1: Remove all Redis key constants from common crate**

**Step 2: Remove redis from all Cargo.toml files**

**Step 3: Remove `redis-cache` feature flags from indexer and API**

**Step 4: Full workspace build**

Run: `cargo check` (entire workspace)
Run: `cargo test` (entire workspace)

**Step 5: Commit**

```bash
git commit -m "chore: remove Redis dependency from entire workspace"
```

---

### Task 13: Handle proposals cache without Redis

**Files:**

- Modify: `crates/indexer/src/cache.rs` — proposals handling
- Modify: `crates/ckbadger-store/src/` — add proposals ops if needed

**Step 1: Audit proposals cache usage**

Currently indexer caches pending proposals in Redis Hash (`proposals:pending`). Determine if this should move to RocksDB or in-memory.

Since proposals are ephemeral mempool data, in-memory `HashMap` in the indexer is appropriate:

```rust
pub struct ProposalsCache {
    pending: Mutex<HashMap<String, ProposalData>>,
}
```

**Step 2: Update CacheInvalidator to use in-memory proposals**

**Step 3: Tests**

**Step 4: Commit**

```bash
git commit -m "refactor: move proposals cache from Redis to in-memory"
```

---

## Phase 4: Supervisor & IPC

### Task 14: Create IPC protocol

**Files:**

- Create: `crates/ipc/Cargo.toml`
- Create: `crates/ipc/src/lib.rs`
- Create: `crates/ipc/src/protocol.rs`
- Create: `crates/ipc/src/server.rs`
- Create: `crates/ipc/src/client.rs`

**Step 1: Define IPC message protocol**

JSON-over-Unix-socket with newline delimiter:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcRequest {
    GetSyncStatus,
    GetSyncProgress,
    GetMemoryStats,
    GetServiceStatus,
    Shutdown { reason: String },
    RefreshSecondary, // Tell API to refresh its secondary RocksDB reader
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcResponse {
    SyncStatus { data: SyncStatusData },
    SyncProgress { data: SyncProgressData },
    MemoryStats { data: MemoryStatsData },
    ServiceStatus { services: Vec<ServiceInfo> },
    Ok,
    Error { message: String },
}

pub struct ServiceInfo {
    pub name: String,          // "indexer", "api", "frontend"
    pub pid: u32,
    pub status: ServiceStatus, // Running, Stopped, Restarting
    pub uptime_secs: u64,
}
```

**Step 2: Implement Unix socket server**

```rust
pub struct IpcServer {
    socket_path: PathBuf,
    handler: Arc<dyn IpcHandler>,
}

#[async_trait]
pub trait IpcHandler: Send + Sync {
    async fn handle(&self, request: IpcRequest) -> IpcResponse;
}

impl IpcServer {
    pub async fn listen(self) -> Result<()> {
        let listener = UnixListener::bind(&self.socket_path)?;
        loop {
            let (stream, _) = listener.accept().await?;
            // Spawn handler task per connection
        }
    }
}
```

**Step 3: Implement Unix socket client**

```rust
pub struct IpcClient {
    socket_path: PathBuf,
}

impl IpcClient {
    pub async fn send(&self, request: IpcRequest) -> Result<IpcResponse> {
        let stream = UnixStream::connect(&self.socket_path).await?;
        // Send request, read response
    }
}
```

**Step 4: Tests**

Test round-trip: start server in background, client sends request, gets response.

**Step 5: Commit**

```bash
git commit -m "feat: add IPC crate with Unix socket protocol"
```

---

### Task 15: Create supervisor crate

**Files:**

- Create: `crates/supervisor/Cargo.toml`
- Create: `crates/supervisor/src/lib.rs`

**Step 1: Implement supervisor process manager**

```rust
pub struct Supervisor {
    work_dir: WorkDir,
    config: CkbadgerConfig,
    children: Vec<ChildProcess>,
}

struct ChildProcess {
    name: String,              // "indexer", "api", "frontend"
    process: tokio::process::Child,
    restart_count: u32,
    started_at: Instant,
}

impl Supervisor {
    pub async fn start(work_dir: WorkDir, config: CkbadgerConfig, only: Option<Vec<String>>) -> Result<()> {
        // 1. Write PID file
        // 2. Start IPC server on indexer.sock
        // 3. Fork child processes: ckbadger _internal indexer/api/frontend-server
        // 4. Monitor children, restart on crash
        // 5. Handle SIGTERM/SIGINT → graceful shutdown all children
    }

    fn spawn_child(&mut self, service: &str) -> Result<ChildProcess> {
        let exe = std::env::current_exe()?;
        let child = tokio::process::Command::new(&exe)
            .arg("_internal")
            .arg(service)
            .arg("-C")
            .arg(&self.work_dir.root)
            .spawn()?;
        // ...
    }
}
```

**Step 2: Implement health monitoring**

Check child process status periodically. Auto-restart crashed processes with backoff.

**Step 3: Implement signal handling**

On SIGTERM/SIGINT: send SIGTERM to all children, wait for graceful shutdown (timeout 30s), then SIGKILL.

**Step 4: Tests**

Test PID file creation/cleanup. Test child spawn/restart logic with mock processes.

**Step 5: Commit**

```bash
git commit -m "feat: add supervisor crate for process management"
```

---

### Task 16: Implement `_internal` subcommands

**Files:**

- Modify: `crates/cli/src/main.rs` — implement Internal subcommand handling

**Step 1: Define InternalArgs**

```rust
#[derive(clap::Args)]
struct InternalArgs {
    #[command(subcommand)]
    service: InternalService,
}

#[derive(Subcommand)]
enum InternalService {
    Indexer,
    Api,
    FrontendServer,
}
```

**Step 2: Implement internal service runners**

Each internal command: load config from ckbadger.toml, start the single service, run until signal.

```rust
Command::Internal(args) => match args.service {
    InternalService::Indexer => {
        let config = load_config(&workdir)?;
        run_indexer(IndexerServiceConfig::from_config(&config, &work_dir)).await
    }
    InternalService::Api => {
        let config = load_config(&workdir)?;
        run_api(ApiServiceConfig::from_config(&config, &work_dir)).await
    }
    InternalService::FrontendServer => {
        let config = load_config(&workdir)?;
        run_frontend_server(&config, &work_dir).await
    }
}
```

**Step 3: Wire `ckbadger run` to supervisor**

```rust
Command::Run(args) => {
    let config = load_config(&workdir)?;
    let work = WorkDir::resolve(&workdir);
    let only = args.only.map(|s| s.split(',').map(String::from).collect());
    Supervisor::start(work, config, only).await
}
```

**Step 4: Test end-to-end**

```bash
cargo build -p ckbadger
./target/debug/ckbadger init -C /tmp/test
./target/debug/ckbadger run -C /tmp/test
# Verify indexer, api, frontend processes are running
# Ctrl+C → verify graceful shutdown
```

**Step 5: Commit**

```bash
git commit -m "feat: implement supervisor run and internal subcommands"
```

---

### Task 17: Implement `status` command

**Files:**

- Modify: `crates/cli/src/main.rs` — implement status command

**Step 1: Implement status via IPC**

```rust
Command::Status => {
    let work = WorkDir::resolve(&workdir);
    let client = IpcClient::new(&work.indexer_sock);

    match client.send(IpcRequest::GetServiceStatus).await {
        Ok(IpcResponse::ServiceStatus { services }) => {
            for svc in services {
                println!("{}: {} (pid {}, uptime {}s)",
                    svc.name, svc.status, svc.pid, svc.uptime_secs);
            }
        }
        Err(_) => {
            // Fallback: check PID file, check if process alive
            // Read sync status from RocksDB directly
        }
    }

    // Always show sync status from RocksDB
    let store = CkbadgerStore::open_domain_secondary(...)?;
    let sync = store.get_sync_status()?;
    println!("Sync: block {} / {} ({:.1}%)", sync.tip, sync.target, sync.progress);
}
```

**Step 2: Test**

**Step 3: Commit**

```bash
git commit -m "feat: implement ckbadger status command"
```

---

## Phase 5: Frontend Static Export

### Task 18: Convert Next.js to static export

**Files:**

- Modify: `frontend/next.config.ts` — change output to 'export'
- Modify: `frontend/app/page.tsx` — convert from server component to client
- Remove: `frontend/middleware.ts`
- Remove: `frontend/app/ai-md/` directory
- Remove: `frontend/app/ai-raw/` directory
- Remove: `frontend/app/capabilities/` directory

**Step 1: Change next.config.ts**

```typescript
const nextConfig: NextConfig = {
  output: 'export',
  trailingSlash: true, // Ensures /blocks/ generates blocks/index.html
};
```

**Step 2: Convert home page to client component**

```typescript
'use client';

import { useQuery } from '@tanstack/react-query';
// ... move fetchServerData to client-side using TanStack Query
```

**Step 3: Remove server-only files**

Delete `middleware.ts`, `app/ai-md/`, `app/ai-raw/`, `app/capabilities/`.

Note: AI format routes will be re-implemented in the Rust API (Phase 6).

**Step 4: Handle dynamic routes**

Dynamic routes like `/blocks/[id]` are already client-side (use `useParams`). With static export + SPA fallback in Axum, they work as-is. No `generateStaticParams` needed since Axum serves index.html for all unmatched routes.

Add a catch-all `app/[...slug]/page.tsx` that renders the same as the dynamic route pages, or configure the static export to generate stub pages.

Actually simpler: since all dynamic routes already use `'use client'` + `useParams()`, we just need the Axum SPA fallback to serve the root `index.html` for any unmatched path. The client-side router (Next.js) handles the rest.

**Step 5: Build and verify**

```bash
cd frontend && pnpm build
ls -la out/  # Static export output
```

**Step 6: Commit**

```bash
git commit -m "feat: convert frontend to static export (no Node.js required)"
```

---

### Task 19: Add frontend serving to API

**Files:**

- Modify: `crates/api/src/entry.rs` — add static frontend serving
- Modify: `crates/cli/src/main.rs` — pass frontend_dir to API config

**Step 1: Frontend server as separate Axum instance**

The frontend server runs on port 8100 (separate from API on 8101). It serves static files and proxies `/api/v1/*` to the API:

```rust
pub async fn run_frontend_server(config: &CkbadgerConfig, work_dir: &WorkDir) -> Result<()> {
    let frontend_dir = resolve_frontend_dir(work_dir)?;
    let api_upstream = format!("http://{}:{}", config.api.host, config.api.port);

    let spa_fallback = ServeFile::new(frontend_dir.join("index.html"));
    let serve = ServeDir::new(&frontend_dir).fallback(spa_fallback);

    let app = Router::new()
        // Proxy /api/v1/* to API server
        .route("/api/v1/{*path}", any(proxy_to_api))
        // Proxy /ws to API WebSocket
        .route("/ws", any(proxy_to_api))
        .fallback_service(serve);

    let addr = format!("0.0.0.0:{}", config.frontend.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

**Step 2: Implement reverse proxy**

Use `reqwest` or `hyper` client to proxy API requests:

```rust
async fn proxy_to_api(
    State(upstream): State<String>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    // Forward request to API upstream, return response
}
```

**Step 3: Resolve frontend directory**

Look for static files in:

1. `<work_dir>/frontend/` (user override)
2. `<share_dir>/frontend/` (installed assets)

**Step 4: Test**

Build frontend, copy `out/` to share/frontend/, run ckbadger, access http://localhost:8100.

**Step 5: Commit**

```bash
git commit -m "feat: add frontend static serving with API proxy"
```

---

## Phase 6: AI Format Migration to Rust API (can be deferred)

### Task 20: Port content negotiation to Axum middleware

**Files:**

- Create: `crates/api/src/middleware/content_negotiation.rs`

Port the logic from `frontend/lib/ai/markdown-request.ts` (`resolveMarkdownRewrite`) to Axum middleware. This handles `?format=md`, `.md` suffix, and `Accept: text/markdown` header to route to markdown rendering.

This is a significant porting effort (~100KB of TypeScript rendering code). Can be done incrementally:

1. First port the content negotiation middleware
2. Then port the capabilities endpoint (small)
3. Then port markdown-renderer page by page
4. Then port raw-renderer page by page

**Note:** This phase can be deferred. The core CLI functionality works without AI format routes. Mark as follow-up.

---

## Phase 7: Cleanup & Documentation

### Task 21: Remove Docker and legacy files

**Files:**

- Remove: `docker/` directory (all Dockerfiles)
- Remove: `docker-compose.yml`
- Modify: `.env` → convert to `.env.example` or remove
- Modify: `Makefile` → simplify to development shortcuts only

**Step 1: Remove Docker files**

```bash
rm -rf docker/ docker-compose.yml
```

**Step 2: Update or remove Makefile**

Keep a minimal Makefile for development:

```makefile
.PHONY: build check test lint

build:
	cargo build -p ckbadger

check:
	cargo check && cargo clippy

test:
	cargo test && cd frontend && pnpm test

lint:
	cd frontend && pnpm lint && pnpm type-check
```

**Step 3: Remove old binary targets**

Remove `[[bin]]` from indexer, api, tui Cargo.toml files (keeping only library targets). Update workspace to only have `ckbadger` binary.

**Step 4: Update .gitignore**

Add: `run/`, `data/`, `*.sock`, `*.pid`

**Step 5: Commit**

```bash
git commit -m "chore: remove Docker files and simplify build infrastructure"
```

---

### Task 22: Update CLAUDE.md and documentation

**Files:**

- Modify: `CLAUDE.md` — update commands, structure, workflows
- Modify: `README.md` — update installation and usage
- Modify: `docs/ARCHITECTURE_MAP.md` — update crate structure

**Step 1: Update CLAUDE.md**

Replace Docker-based commands with:

```bash
# Build
cargo build -p ckbadger

# Usage
ckbadger init
ckbadger run
ckbadger tui
ckbadger status
ckbadger verify --depth fast
ckbadger label-import
ckbadger purge
```

Update project structure to reflect new crates (cli, config, ipc, supervisor).

**Step 2: Update README.md**

**Step 3: Commit**

```bash
git commit -m "docs: update documentation for CLI-based workflow"
```

---

### Task 23: Remove old binary main.rs files

**Files:**

- Remove: `crates/indexer/src/main.rs` (if fully migrated)
- Remove: `crates/api/src/main.rs` (if fully migrated)
- Remove: `crates/tui/src/main.rs` (if fully migrated)

Only do this after confirming all functionality is accessible through `ckbadger` CLI.

**Step 1: Remove main.rs files**

**Step 2: Update Cargo.toml files to lib-only**

**Step 3: Full build and test**

```bash
cargo check && cargo test
```

**Step 4: Commit**

```bash
git commit -m "chore: remove standalone binary entry points, ckbadger CLI is the sole binary"
```

---

## Execution Order & Dependencies

```
Phase 1 (Foundation)     Phase 2 (Lib conversion)     Phase 3 (Redis removal)
  Task 1 ──────────────→ Task 4 ──────────────────→ Task 8
  Task 2 ──────────────→ Task 5 ──────────────────→ Task 9
  Task 3                  Task 6 ──────────────────→ Task 10, 11
                          Task 7 ←─── depends on 4,5,6  Task 12 ←── depends on 8-11
                                                        Task 13

Phase 4 (Supervisor)          Phase 5 (Frontend)      Phase 7 (Cleanup)
  Task 14 ────→ Task 15       Task 18 ────→ Task 19   Task 21
  Task 16 ←── depends on 15                           Task 22
  Task 17                                             Task 23

Phase 6 (AI formats) — can be deferred
  Task 20
```

**Critical path:** Tasks 1-2 → 4-7 → 8-12 → 14-16 → 18-19 → 21-23

**Parallelizable:** Tasks 4/5/6 can be done in parallel. Tasks 8/9/10/11 can be done in parallel.
