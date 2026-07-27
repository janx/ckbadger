//! Process supervisor for `ckbadger run`.
//!
//! Spawns child processes for each requested service via
//! `ckbadger internal {indexer,api}`, monitors them, and restarts
//! on crash with exponential backoff. Starts an IPC server on
//! `run/indexer.sock` for status queries and shutdown requests.

use crate::sequencer::{GateStatus, SpawnOutcome};
use anyhow::{Context, Result};
use ckbadger_config::{CkbadgerConfig, WorkDir};
use ckbadger_ipc::{IpcHandler, IpcRequest, IpcResponse, IpcServer, ServiceInfo, ServiceStatus};
use ckbadger_store::{
    secondary_store_path, CkbadgerStore, SecondaryStoreOwner, StoreRuntimeConfig,
};
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::{watch, Mutex};
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum restart attempts before giving up on a service.
const MAX_RESTART_ATTEMPTS: u32 = 10;

/// Base backoff duration for restarts (doubles each attempt, capped).
const BASE_BACKOFF: Duration = Duration::from_secs(1);

/// Maximum backoff duration.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// If a service runs longer than this, its restart counter resets to zero.
/// This prevents a long-running service that crashes once from inheriting
/// accumulated restart counts from earlier transient failures.
const STABLE_RUNNING_THRESHOLD: Duration = Duration::from_secs(120);

/// How often to check child process health.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Time to wait for a child to exit after SIGTERM before sending SIGKILL.
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// Indexer shutdown may need to finish one in-flight RocksDB batch and join
/// blocking prefetch/flush workers. This is only a safety ceiling; cooperative
/// cancellation should normally finish much sooner.
const INDEXER_GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(120);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A managed child process.
struct ManagedChild {
    name: String,
    child: Child,
    restart_count: u32,
    started_at: Instant,
    /// Timestamps of recent clean bulk-to-live handoffs, pruned to
    /// [`HANDOFF_RATE_WINDOW`]. Carried across respawns so an exit-0 loop is
    /// detectable by RATE even though each handoff resets `restart_count` and
    /// `started_at`. Same shape as main's `restart_history` (3344ad5d) so the two
    /// windows merge cleanly on rebase.
    handoff_history: Vec<Instant>,
    /// `Some(reason)` once the supervisor has stopped restarting this child.
    /// A single source of truth: the flag and its explanation cannot drift, and
    /// the sequencer can name WHY a gating indexer is holding the queue.
    blocked: Option<String>,
}

impl ManagedChild {
    fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    fn is_blocked(&self) -> bool {
        self.blocked.is_some()
    }

    fn pid(&self) -> u32 {
        self.child.id().unwrap_or(0)
    }
}

/// Shared state between the supervisor loop and the IPC handler.
struct SupervisorState {
    children: Vec<ManagedChild>,
    shutdown_requested: bool,
}

/// A single supervised child: which service, in which workdir, under what label.
#[derive(Debug, Clone)]
pub struct ChildSpec {
    /// Display/log label, e.g. "testnet/indexer" (single-network: just "indexer").
    pub label: String,
    /// The `internal <service>` name, e.g. "indexer" / "api" / "crawler".
    pub service: String,
    /// Absolute workdir passed as `-C <workdir>` (the network subdir).
    pub workdir: String,
}

impl ChildSpec {
    fn log_file_name(&self) -> String {
        format!("{}.log", self.label.replace('/', "-"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildExitAction {
    CleanHandoff,
    BlockRebuildRequired,
    Restart,
}

fn classify_child_exit(
    spec: &ChildSpec,
    exit_success: bool,
    exit_code: Option<i32>,
) -> ChildExitAction {
    if spec.service == "indexer" && exit_success {
        return ChildExitAction::CleanHandoff;
    }
    if spec.service == "indexer"
        && exit_code
            == Some(i32::from(
                ckbadger_indexer::lifecycle::REBUILD_REQUIRED_EXIT_CODE,
            ))
    {
        return ChildExitAction::BlockRebuildRequired;
    }
    ChildExitAction::Restart
}

fn graceful_shutdown_timeout(name: &str) -> Duration {
    if name == "indexer" || name.ends_with("/indexer") {
        INDEXER_GRACEFUL_SHUTDOWN_TIMEOUT
    } else {
        GRACEFUL_SHUTDOWN_TIMEOUT
    }
}

// ---------------------------------------------------------------------------
// Graceful shutdown
// ---------------------------------------------------------------------------

/// Stop a child process gracefully: SIGTERM -> wait -> SIGKILL fallback.
async fn stop_child_gracefully(name: &str, child: &mut Child) {
    let pid = child.id().unwrap_or(0);
    if pid == 0 {
        return;
    }

    let pid_i32 = match i32::try_from(pid) {
        Ok(p) => p,
        Err(_) => {
            warn!(service = %name, pid, "PID exceeds i32::MAX, falling back to SIGKILL");
            let _ = child.kill().await;
            let _ = child.wait().await;
            return;
        }
    };
    // SAFETY: pid_i32 is a valid child process ID obtained from tokio::process::Child
    let sigterm_result = unsafe { libc::kill(pid_i32, libc::SIGTERM) };
    if sigterm_result != 0 {
        let err = std::io::Error::last_os_error();
        warn!(service = %name, pid, error = %err, "failed to send SIGTERM, falling back to SIGKILL");
        let _ = child.kill().await;
        let _ = child.wait().await;
        return;
    }

    let shutdown_timeout = graceful_shutdown_timeout(name);
    match tokio::time::timeout(shutdown_timeout, child.wait()).await {
        Ok(Ok(status)) => {
            info!(service = %name, pid, ?status, "service stopped gracefully");
        }
        Ok(Err(e)) => {
            warn!(service = %name, pid, error = %e, "error waiting for service, sending SIGKILL");
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        Err(_) => {
            warn!(service = %name, pid, timeout_secs = shutdown_timeout.as_secs(),
                "service did not exit after SIGTERM, sending SIGKILL");
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// The default set of services `ckbadger run` supervises.
///
/// Returns the base set (`indexer`, `api`, `frontend-server`) always, plus
/// `crawler` when `[crawler].enabled` is set. Each name maps 1:1 to an
/// `internal <name>` subcommand that [`run_supervisor`] spawns via
/// [`spawn_child`] (e.g. `"crawler"` -> `internal crawler`). An explicit
/// `--only` flag overrides this default.
pub fn enabled_services(cfg: &CkbadgerConfig) -> Vec<&'static str> {
    let mut services = vec!["indexer", "api", "frontend-server"];
    if cfg.crawler.enabled {
        services.push("crawler");
    }
    services
}

/// Run the supervisor for a single-network workdir.
///
/// Thin wrapper over [`run_supervisor_multi`]: each requested service maps to
/// one [`ChildSpec`] rooted at `work_dir` (label == service, so its log stays
/// `<service>.log`). Blocks until Ctrl+C or IPC shutdown request.
pub async fn run_supervisor(
    work_dir: &WorkDir,
    _config: &CkbadgerConfig,
    services: Vec<String>,
) -> Result<()> {
    let workdir_str = work_dir.root.to_string_lossy().to_string();
    let specs = services
        .into_iter()
        .map(|service| ChildSpec {
            label: service.clone(),
            service,
            workdir: workdir_str.clone(),
        })
        .collect();
    run_supervisor_multi(work_dir, specs).await
}

/// Supervise an arbitrary set of children (used by orchestrator/multi-network).
///
/// Spawns every [`ChildSpec`] as a `ckbadger internal <service> -C <workdir>`
/// subprocess, starts the IPC server on `root`'s socket, and monitors/restarts
/// children with exponential backoff. Blocks until Ctrl+C or IPC shutdown.
pub async fn run_supervisor_multi(root: &WorkDir, specs: Vec<ChildSpec>) -> Result<()> {
    let initial = specs.len();
    run_supervisor_inner(root, specs, initial).await
}

/// One network's indexer, deferred until the previous network is past bulk.
pub struct SequencedIndexer {
    pub spec: ChildSpec,
    /// Resolved domain store path — opened secondary to read the network's bulk status.
    pub domain_data_path: PathBuf,
    pub bulk_sync_threshold: u64,
    /// This network's REAL store runtime config (its co-resident RAM share and
    /// explicit `[store].memory_budget_gb`). RocksDB budgets are per-process and
    /// the supervisor's process-wide `SHARED_BUDGET` is pinned by whichever open
    /// happens first, so a `StoreRuntimeConfig::default()` here would size the
    /// supervisor's cache/WriteBufferManager from UNDIVIDED host RAM and discard
    /// the operator's explicit override.
    pub store_runtime_config: StoreRuntimeConfig,
}

/// Deferred-indexer plan handed to the supervisor's sequencer task.
struct SequencerPlan {
    /// Index of the first deferred indexer in the full `specs` vec
    /// (== number of immediate children).
    first_indexer_idx: usize,
    indexers: Vec<SequencedIndexer>,
    poll: Duration,
}

/// How often the sequencer re-reads the previous network's store for bulk status.
const SEQUENCER_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Consecutive sequencer read failures on an EXISTING store before the log
/// escalates from `warn` to `error`. At [`SEQUENCER_POLL_INTERVAL`] this is
/// roughly one minute of an unreadable store — well past any single busy-primary
/// catch-up blip.
const SEQUENCER_READ_FAILURE_ESCALATE: u32 = 12;

/// Consecutive failures that make the condition PERSISTENT (~2 minutes): a
/// corrupt or undecodable record rather than transient RocksDB contention. Logged
/// loudly, naming the store path — but still never fatal to other networks.
const SEQUENCER_READ_FAILURE_PERSISTENT: u32 = 24;

/// How many times the sequencer retries a failed `spawn_child` before parking.
const SPAWN_RETRY_ATTEMPTS: u32 = 5;

/// Sliding window for the clean-handoff rate limit.
const HANDOFF_RATE_WINDOW: Duration = Duration::from_secs(600);

/// More than this many bulk-to-live handoffs within [`HANDOFF_RATE_WINDOW`] is
/// pathological. A legitimate handoff (BULK_SYNC rule 10) happens ONCE per bulk
/// completion, so a handful inside ten minutes can only mean the indexer is
/// exiting 0 immediately on startup. Kept below [`MAX_RESTART_ATTEMPTS`] because
/// a clean handoff is a much rarer event than a crash restart.
const MAX_HANDOFFS_IN_WINDOW: usize = 5;

/// Decide whether a clean handoff may proceed, given the handoff timestamps
/// already recorded inside [`HANDOFF_RATE_WINDOW`].
///
/// Pure so the limit is exhaustively testable, mirroring `classify_child_exit`.
fn handoff_allowed(handoffs_in_window: usize) -> bool {
    handoffs_in_window < MAX_HANDOFFS_IN_WINDOW
}

/// How loudly a consecutive run of sequencer read failures should be reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadFailureLevel {
    /// A blip: the primary was mid-write, or the secondary is momentarily behind.
    Transient,
    /// Sustained (~1 min): something is wrong, but still not other networks' problem.
    Sustained,
    /// Persistent (~2 min): almost certainly corrupt/undecodable state.
    Persistent,
}

/// Consecutive-failure accounting for one gating network's status reads.
///
/// Pure and clock-free so the escalation ladder is exhaustively testable. A read
/// failure on an EXISTING store is "no signal this round", never fatal: the
/// sequencer keeps polling and keeps waiting (BULK_SYNC rule 11 forbids skipping
/// ahead, and one network's transient RocksDB failure must never tear down the
/// networks that are healthy).
#[derive(Debug, Default)]
struct ReadFailureTracker {
    consecutive: u32,
}

impl ReadFailureTracker {
    /// Record a failed read; returns how loudly THIS failure should be logged.
    fn record_failure(&mut self) -> ReadFailureLevel {
        self.consecutive = self.consecutive.saturating_add(1);
        if self.consecutive >= SEQUENCER_READ_FAILURE_PERSISTENT {
            ReadFailureLevel::Persistent
        } else if self.consecutive >= SEQUENCER_READ_FAILURE_ESCALATE {
            ReadFailureLevel::Sustained
        } else {
            ReadFailureLevel::Transient
        }
    }

    /// Record a read that produced a signal (or a genuine store-missing state).
    /// Returns the failure streak it just ended, so recovery can be logged once.
    fn record_success(&mut self) -> u32 {
        std::mem::take(&mut self.consecutive)
    }

    fn consecutive(&self) -> u32 {
        self.consecutive
    }
}

/// Backoff before spawn retry `attempt` (0-based), doubling from [`BASE_BACKOFF`].
fn spawn_retry_backoff(attempt: u32) -> Duration {
    std::cmp::min(BASE_BACKOFF * 2u32.saturating_pow(attempt), MAX_BACKOFF)
}

/// Run `attempt` until it succeeds or [`SPAWN_RETRY_ATTEMPTS`] have been used,
/// sleeping [`spawn_retry_backoff`] between tries.
///
/// A spawn failure is usually transient host pressure (EMFILE while the first
/// network's bulk sync holds thousands of RocksDB fds, a momentarily unwritable
/// log dir). Returning the first error straight to the sequencer used to shut the
/// WHOLE orchestrator down; retrying absorbs the blip instead.
async fn spawn_with_retry<T, F, B>(label: &str, backoff_for: B, mut attempt: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
    B: Fn(u32) -> Duration,
{
    let mut last_error = None;
    for n in 0..SPAWN_RETRY_ATTEMPTS {
        match attempt() {
            Ok(value) => {
                if n > 0 {
                    info!(child = %label, attempts = n + 1, "spawn succeeded after retry");
                }
                return Ok(value);
            }
            Err(error) => {
                let is_last = n + 1 >= SPAWN_RETRY_ATTEMPTS;
                let backoff = backoff_for(n);
                warn!(
                    child = %label,
                    attempt = n + 1,
                    max_attempts = SPAWN_RETRY_ATTEMPTS,
                    error = %error,
                    retry_in_secs = if is_last { 0 } else { backoff.as_secs() },
                    "failed to spawn child"
                );
                last_error = Some(error);
                if !is_last {
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }
    Err(last_error.expect("every loop iteration records an error before falling through"))
}

/// One tolerant poll of a gating network's store. `past_bulk == None` is
/// "no signal this round" and never advances the gate; `lag`/`bulk_completed`
/// are carried purely so the sequencer's wait logs can say WHY it is waiting.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct BulkStatusSample {
    past_bulk: Option<bool>,
    lag: Option<i128>,
    bulk_completed: bool,
}

/// One gating network's bulk-status source: a LAZILY-opened, LONG-LIVED domain
/// store secondary, mirroring how the API and TUI hold theirs.
///
/// Opening (and dropping) a full 59-CF secondary on every 5 s poll — for the
/// hours a mainnet bulk sync takes — is pure waste and repeatedly re-enters
/// RocksDB's open path against a busy primary. Instead the secondary is opened
/// once, when the primary's `CURRENT` marker first appears, and then only
/// `refresh()`-ed per poll.
///
/// Every method here is BLOCKING (RocksDB open/catch-up/read). Callers on the
/// async supervisor runtime must go through `spawn_blocking`.
struct BulkStatusReader {
    /// The gating network's child label, used for log context.
    label: String,
    domain_data_path: PathBuf,
    secondary_path: PathBuf,
    bulk_sync_threshold: u64,
    /// The gating network's real per-network budget — see
    /// [`SequencedIndexer::store_runtime_config`].
    runtime_config: StoreRuntimeConfig,
    store: Option<CkbadgerStore>,
    failures: ReadFailureTracker,
}

impl BulkStatusReader {
    fn new(idx: &SequencedIndexer) -> Self {
        Self {
            label: idx.spec.label.clone(),
            secondary_path: secondary_store_path(
                &idx.domain_data_path,
                SecondaryStoreOwner::Supervisor,
            ),
            domain_data_path: idx.domain_data_path.clone(),
            bulk_sync_threshold: idx.bulk_sync_threshold,
            runtime_config: idx.store_runtime_config,
            store: None,
            failures: ReadFailureTracker::default(),
        }
    }

    /// One tolerant poll. `past_bulk` is `Some(true|false)` on a successful read
    /// and `None` when this round carries NO SIGNAL — either the store does not
    /// exist yet, or the read failed transiently.
    ///
    /// A failed read against a busy primary is exactly what the API
    /// (`crates/api/src/lib.rs`) and the TUI (`crates/tui/src/db.rs`) already
    /// treat as retryable. Propagating it instead used to shut down every
    /// network's indexer, API and the shared frontend — mainnet mid-bulk
    /// included. The failure is logged with an escalating consecutive-failure
    /// counter and the gate simply keeps waiting.
    fn poll(&mut self) -> BulkStatusSample {
        match self.read() {
            Ok(sample) => {
                let recovered_from = self.failures.record_success();
                if recovered_from > 0 {
                    info!(
                        child = %self.label,
                        after_failures = recovered_from,
                        "sequencer status read recovered"
                    );
                }
                sample
            }
            Err(error) => {
                let level = self.failures.record_failure();
                let consecutive = self.failures.consecutive();
                // Drop the handle so the next poll reopens the secondary from
                // scratch (the TUI does the same); a wedged handle must not
                // outlive the failure that exposed it.
                self.store = None;
                match level {
                    ReadFailureLevel::Transient => warn!(
                        child = %self.label,
                        store = %self.domain_data_path.display(),
                        consecutive,
                        error = %format!("{error:#}"),
                        "sequencer status read failed; treating as no signal this round"
                    ),
                    ReadFailureLevel::Sustained => error!(
                        child = %self.label,
                        store = %self.domain_data_path.display(),
                        consecutive,
                        error = %format!("{error:#}"),
                        "sequencer status read has been failing for ~1 minute; the next network \
                         stays deferred until this store is readable again"
                    ),
                    ReadFailureLevel::Persistent => error!(
                        child = %self.label,
                        store = %self.domain_data_path.display(),
                        consecutive,
                        error = %format!("{error:#}"),
                        "sequencer status read is PERSISTENTLY failing; this store is very likely \
                         corrupt or holds an undecodable sync-progress record. Inspect it and \
                         purge/re-sync that network; other networks keep running untouched"
                    ),
                }
                BulkStatusSample::default()
            }
        }
    }

    /// Read the gating network's bulk status. A missing RocksDB `CURRENT` is the
    /// sole not-ready state (an all-`None` sample); once the store exists, every
    /// open/refresh/read/decode failure is an error. [`Self::poll`] is the
    /// tolerant wrapper the sequencer actually uses.
    fn read(&mut self) -> Result<BulkStatusSample> {
        if self.store.is_none() {
            if !self.domain_data_path.join("CURRENT").is_file() {
                return Ok(BulkStatusSample::default());
            }
            let store = CkbadgerStore::open_domain_secondary_with_runtime(
                &self.domain_data_path,
                &self.secondary_path,
                self.runtime_config,
            )
            .with_context(|| {
                format!(
                    "failed to open existing domain store {} for sequencer status",
                    self.domain_data_path.display()
                )
            })?;
            self.store = Some(store);
        }
        let store = self
            .store
            .as_ref()
            .expect("sequencer secondary opened directly above");

        store.refresh().with_context(|| {
            format!(
                "failed to refresh sequencer secondary for {}",
                self.domain_data_path.display()
            )
        })?;
        let status = store.get_sync_status().with_context(|| {
            format!(
                "failed to read sync status from {}",
                self.domain_data_path.display()
            )
        })?;
        let bulk_completed = status.bulk_sync_completed_at.is_some();
        let lag = match store.get_sync_progress().with_context(|| {
            format!(
                "failed to read sync progress from {}",
                self.domain_data_path.display()
            )
        })? {
            None => None,
            Some(bytes) => {
                let progress: ckbadger_common::SyncProgressData = serde_json::from_slice(&bytes)
                    .with_context(|| {
                        format!(
                            "invalid sync progress in {}",
                            self.domain_data_path.display()
                        )
                    })?;
                Some(i128::from(progress.target_block) - i128::from(progress.current_block))
            }
        };
        Ok(BulkStatusSample {
            past_bulk: Some(crate::sequencer::is_past_bulk(
                bulk_completed,
                lag,
                self.bulk_sync_threshold,
            )),
            lag,
            bulk_completed,
        })
    }
}

/// Shared, mutable set of per-network bulk-status readers. `std::sync::Mutex`
/// (not tokio's) because it is only ever locked inside `spawn_blocking`.
type BulkStatusReaders = Arc<std::sync::Mutex<Vec<BulkStatusReader>>>;

/// Observe one gating network for the sequencer: its bulk status (read off the
/// async runtime) plus whether the supervisor has its indexer restart-blocked.
///
/// The tri-state is the reader's: `Some(true|false)` on a read, `None` for "no
/// signal this round". The `Result` covers only a panicked blocking task — a
/// programming bug, not a runtime condition — so store failures can no longer
/// reach [`wait_for_sequencer_failure`].
///
/// `gate_child_index` is the gating indexer's slot in `SupervisorState.children`.
/// Without this, a gating indexer that had been blocked (exit-78 rebuild-required,
/// or past `MAX_RESTART_ATTEMPTS`) left the next network deferred forever with no
/// diagnostic anywhere.
async fn observe_gate(
    readers: &BulkStatusReaders,
    state: &Arc<Mutex<SupervisorState>>,
    prev: usize,
    gate_child_index: usize,
) -> Result<GateStatus> {
    let owned = readers.clone();
    let sample = tokio::task::spawn_blocking(move || {
        owned
            .lock()
            .expect("sequencer bulk-status reader lock poisoned")[prev]
            .poll()
    })
    .await
    .context("sequencer bulk-status read task failed")?;

    let blocked = {
        let locked = state.lock().await;
        locked
            .children
            .get(gate_child_index)
            .and_then(|child| child.blocked.clone())
    };

    Ok(GateStatus {
        past_bulk: sample.past_bulk,
        lag: sample.lag,
        bulk_completed: sample.bulk_completed,
        blocked,
    })
}

/// Last-resort backstop: shut the orchestrator down if the sequencer task itself
/// fails.
///
/// It deliberately no longer fires for RUNTIME conditions. A store
/// open/refresh/read/decode failure is absorbed by [`BulkStatusReader::poll`] and
/// a spawn failure by [`SpawnOutcome::Parked`] — either used to tear down every
/// network, including a mainnet indexer hours into bulk sync, plus all APIs and
/// the shared frontend. What can still reach here is a broken invariant inside
/// the supervisor itself (child-order violation, out-of-bounds spec index) or a
/// panicked blocking task: programming bugs, where failing fast is correct.
async fn wait_for_sequencer_failure(
    handle: Option<&mut tokio::task::JoinHandle<Result<()>>>,
) -> anyhow::Error {
    let Some(handle) = handle else {
        return std::future::pending::<anyhow::Error>().await;
    };
    match handle.await {
        Ok(Err(error)) => error,
        Err(join_error) => anyhow::anyhow!("sequencer task failed: {join_error}"),
        Ok(Ok(())) => std::future::pending::<anyhow::Error>().await,
    }
}

/// Orchestrator supervisor: start `immediate` children + the FIRST indexer now, then
/// start each subsequent indexer once the previous exits bulk. Only one network
/// bulk-syncs at a time. Blocks until Ctrl+C / IPC shutdown.
pub async fn run_supervisor_sequenced(
    root: &WorkDir,
    immediate: Vec<ChildSpec>,
    indexers: Vec<SequencedIndexer>,
) -> Result<()> {
    // Full spec list = immediate ++ indexer specs (same order children are appended,
    // so the monitor's index-based restart matching stays correct).
    let mut specs: Vec<ChildSpec> = immediate.clone();
    specs.extend(indexers.iter().map(|i| i.spec.clone()));
    let first_indexer_idx = immediate.len();
    // Spawn immediate + the first indexer (if any) up front; defer the rest.
    let initial = (first_indexer_idx + 1).min(specs.len());

    // The sequencer task is spawned INSIDE run_supervisor_inner_with_sequencer once
    // state + shutdown exist. Pass the deferred plan through.
    run_supervisor_inner_with_sequencer(
        root,
        specs,
        initial,
        Some(SequencerPlan {
            first_indexer_idx,
            indexers,
            poll: SEQUENCER_POLL_INTERVAL,
        }),
    )
    .await
}

/// Supervise `specs`, spawning `specs[0..initial]` immediately and leaving
/// `specs[initial..]` to be started later (by the caller via the shared
/// `SupervisorState`). The monitor manages whatever is in `SupervisorState.children`
/// and restart-matches by index against the full `specs`.
///
/// Thin wrapper: no deferred-indexer sequencer. See
/// [`run_supervisor_inner_with_sequencer`].
async fn run_supervisor_inner(root: &WorkDir, specs: Vec<ChildSpec>, initial: usize) -> Result<()> {
    run_supervisor_inner_with_sequencer(root, specs, initial, None).await
}

/// Supervise `specs` (spawning `specs[0..initial]` immediately), and — when `plan`
/// is `Some` — additionally run a sequencer task that starts each deferred indexer
/// (`specs[initial..]`) one at a time, once the previous network is past bulk sync.
/// The monitor manages whatever is in `SupervisorState.children` and restart-matches
/// by index against the full `specs`; the sequencer appends children in `specs` order
/// to keep that matching correct.
async fn run_supervisor_inner_with_sequencer(
    root: &WorkDir,
    specs: Vec<ChildSpec>,
    initial: usize,
    plan: Option<SequencerPlan>,
) -> Result<()> {
    // Ensure run + log directories exist
    std::fs::create_dir_all(&root.run_dir)
        .with_context(|| format!("failed to create run directory: {}", root.run_dir.display()))?;
    std::fs::create_dir_all(&root.log_dir)
        .with_context(|| format!("failed to create log directory: {}", root.log_dir.display()))?;

    // Write PID file
    let pid = std::process::id();
    std::fs::write(&root.supervisor_pid, pid.to_string()).with_context(|| {
        format!(
            "failed to write PID file: {}",
            root.supervisor_pid.display()
        )
    })?;

    let exe = std::env::current_exe().context("failed to determine executable path")?;

    // Spawn initial children
    let mut children = Vec::new();
    for spec in &specs[..initial] {
        let child = spawn_child(&exe, spec, &root.log_dir)?;
        info!(child = %spec.label, pid = child.pid(), "started service");
        children.push(child);
    }

    let state = Arc::new(Mutex::new(SupervisorState {
        children,
        shutdown_requested: false,
    }));

    // Shutdown signal channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Start IPC server
    let ipc_state = state.clone();
    let ipc_shutdown_tx = shutdown_tx.clone();
    let sock_path = root.indexer_sock.clone();
    let ipc_handle = tokio::spawn(async move {
        let handler: Arc<dyn IpcHandler + Send + Sync> =
            Arc::new(SupervisorIpcHandler::new(ipc_state, ipc_shutdown_tx));
        let server = IpcServer::new(sock_path, handler);
        if let Err(e) = server.listen().await {
            warn!("IPC server error: {:#}", e);
        }
    });

    // Monitor loop
    let monitor_state = state.clone();
    let monitor_exe = exe.clone();
    let monitor_log_dir = root.log_dir.clone();
    let monitor_specs = specs.clone();
    let monitor_shutdown = shutdown_rx.clone();
    let monitor_handle = tokio::spawn(async move {
        monitor_children_multi(
            monitor_state,
            &monitor_exe,
            &monitor_log_dir,
            &monitor_specs,
            monitor_shutdown,
        )
        .await;
    });

    // When a deferred-indexer plan is present, start a sequencer task that spawns
    // each subsequent indexer once the previous network is past bulk sync. The
    // sequencer appends children in the same order as `specs` (immediate ++
    // indexers), keeping the monitor's index-based restart matching correct. Its
    // shutdown receiver is a clone of `shutdown_rx`; `shutdown_tx` stays alive until
    // cleanup below, so the sequencer's `select!` never sees a dropped sender.
    let mut seq_handle = plan.map(|plan| {
        let state = state.clone();
        let exe = exe.clone();
        let log_dir = root.log_dir.clone();
        let specs = specs.clone();
        let seq_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            let SequencerPlan {
                first_indexer_idx,
                indexers,
                poll,
            } = plan;
            // Child labels in [[network]] order, so every gate decision names the
            // network it is about.
            let names: Vec<String> = indexers.iter().map(|i| i.spec.label.clone()).collect();
            // One long-lived secondary per gating network, each sized by ITS OWN
            // per-network runtime config (see `BulkStatusReader`).
            let readers: BulkStatusReaders = Arc::new(std::sync::Mutex::new(
                indexers.iter().map(BulkStatusReader::new).collect(),
            ));
            let gate_state = state.clone();
            crate::sequencer::sequence_indexers(
                &names,
                |prev| {
                    let readers = readers.clone();
                    let gate_state = gate_state.clone();
                    async move {
                        observe_gate(&readers, &gate_state, prev, first_indexer_idx + prev).await
                    }
                },
                |i| {
                    // Spawn indexers[i] == specs[first_indexer_idx + i]; append to
                    // children so the monitor picks it up (index stays aligned).
                    let state = state.clone();
                    let exe = exe.clone();
                    let log_dir = log_dir.clone();
                    let expected_index = first_indexer_idx + i;
                    let spec = specs.get(expected_index).cloned();
                    async move {
                        let spec = spec.ok_or_else(|| {
                            anyhow::anyhow!(
                                "sequencer spec index {expected_index} is out of bounds"
                            )
                        })?;
                        {
                            let locked = state.lock().await;
                            if locked.shutdown_requested {
                                return Ok(SpawnOutcome::Started);
                            }
                            if locked.children.len() != expected_index {
                                anyhow::bail!(
                                    "sequencer child order invariant violated before spawning '{}': expected {} children, found {}",
                                    spec.label,
                                    expected_index,
                                    locked.children.len()
                                );
                            }
                        }

                        // A spawn failure is host pressure (EMFILE while the
                        // previous network's bulk sync holds thousands of fds), not
                        // a reason to shut every network down. Retry, then park.
                        let spawned = spawn_with_retry(&spec.label, spawn_retry_backoff, || {
                            spawn_child(&exe, &spec, &log_dir)
                        })
                        .await;
                        let mut child = match spawned {
                            Ok(child) => child,
                            Err(error) => {
                                error!(
                                    child = %spec.label,
                                    attempts = SPAWN_RETRY_ATTEMPTS,
                                    error = %format!("{error:#}"),
                                    "could not start sequenced indexer after retrying; the \
                                     sequencer stays parked on this network (it will not skip \
                                     ahead and will not stop the running networks) and retries \
                                     on the next poll"
                                );
                                return Ok(SpawnOutcome::Parked);
                            }
                        };
                        let mut locked = state.lock().await;
                        if locked.shutdown_requested {
                            // Shutdown fired between the past-bulk check and here, so the
                            // stop-all loop may not see this child. Stop it explicitly.
                            drop(locked);
                            stop_child_gracefully(&spec.label, &mut child.child).await;
                            return Ok(SpawnOutcome::Started);
                        }
                        if locked.children.len() != expected_index {
                            let actual = locked.children.len();
                            drop(locked);
                            stop_child_gracefully(&spec.label, &mut child.child).await;
                            anyhow::bail!(
                                "sequencer child order invariant violated after spawning '{}': expected {} children, found {}",
                                spec.label,
                                expected_index,
                                actual
                            );
                        }
                        info!(child = %spec.label, pid = child.pid(), "started service (sequenced)");
                        locked.children.push(child);
                        Ok(SpawnOutcome::Started)
                    }
                },
                poll,
                seq_shutdown,
            )
            .await
        })
    });

    // Wait for Ctrl+C or IPC shutdown
    let mut ctrl_c_rx = shutdown_rx.clone();
    let mut supervisor_error = None;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("received Ctrl+C, shutting down...");
        }
        _ = async {
            while !*ctrl_c_rx.borrow_and_update() {
                if ctrl_c_rx.changed().await.is_err() {
                    break;
                }
            }
        } => {
            info!("received IPC shutdown request, shutting down...");
        }
        error = wait_for_sequencer_failure(seq_handle.as_mut()) => {
            error!(error = %error, "sequencer failed, shutting down supervisor");
            supervisor_error = Some(error);
        }
    }

    // Signal shutdown
    let _ = shutdown_tx.send(true);

    // Stop all children gracefully (SIGTERM + timeout + SIGKILL)
    {
        let mut locked = state.lock().await;
        locked.shutdown_requested = true;
        for managed in &mut locked.children {
            info!(child = %managed.name, pid = managed.pid(), "stopping service");
            stop_child_gracefully(&managed.name, &mut managed.child).await;
        }
    }

    // Cleanup
    ipc_handle.abort();
    monitor_handle.abort();
    if let Some(h) = seq_handle {
        h.abort();
    }

    // Remove PID file and socket
    let _ = std::fs::remove_file(&root.supervisor_pid);
    let _ = std::fs::remove_file(&root.indexer_sock);

    info!("supervisor stopped");
    match supervisor_error {
        Some(error) => Err(error.context("orchestrator indexer sequencer failed")),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Service spawning
// ---------------------------------------------------------------------------

/// Spawn one child subprocess from a [`ChildSpec`].
///
/// Runs `ckbadger internal <spec.service> -C <spec.workdir>`, redirecting both
/// stdout and stderr to `<log_dir>/<spec.log_file_name()>`.
fn spawn_child(exe: &PathBuf, spec: &ChildSpec, log_dir: &Path) -> Result<ManagedChild> {
    std::fs::create_dir_all(log_dir)
        .with_context(|| format!("failed to create log directory: {}", log_dir.display()))?;

    let log_file_path = log_dir.join(spec.log_file_name());
    let stdout_log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .with_context(|| {
            format!(
                "failed to open service log file {}",
                log_file_path.display()
            )
        })?;
    let stderr_log = stdout_log
        .try_clone()
        .with_context(|| format!("failed to clone log handle for {}", spec.label))?;

    let child = Command::new(exe)
        .arg("internal")
        .arg(&spec.service)
        .arg("-C")
        .arg(&spec.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(stderr_log))
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to spawn {} subprocess", spec.label))?;

    Ok(ManagedChild {
        name: spec.label.clone(),
        child,
        restart_count: 0,
        started_at: Instant::now(),
        handoff_history: Vec::new(),
        blocked: None,
    })
}

// ---------------------------------------------------------------------------
// Health monitoring
// ---------------------------------------------------------------------------

async fn monitor_children_multi(
    state: Arc<Mutex<SupervisorState>>,
    exe: &PathBuf,
    log_dir: &Path,
    specs: &[ChildSpec],
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(HEALTH_CHECK_INTERVAL) => {}
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    return;
                }
            }
        }

        let mut locked = state.lock().await;
        if locked.shutdown_requested {
            return;
        }

        #[allow(clippy::needless_range_loop)]
        // Indexed access needed: lock is dropped/reacquired mid-loop
        for i in 0..locked.children.len() {
            if locked.children[i].is_blocked() {
                continue;
            }
            // Check if child has exited
            let exited = match locked.children[i].child.try_wait() {
                Ok(Some(status)) => Some(status),
                Ok(None) => None,
                Err(e) => {
                    warn!(
                        child = %locked.children[i].name,
                        error = %e,
                        "failed to check child status"
                    );
                    None
                }
            };

            if let Some(status) = exited {
                let label = &specs[i].label;

                match classify_child_exit(&specs[i], status.success(), status.code()) {
                    ChildExitAction::CleanHandoff => {
                        // A clean handoff bypasses restart_count/backoff entirely
                        // by design (BULK_SYNC rule 10 wants it immediate), which
                        // also made it invisible to MAX_RESTART_ATTEMPTS: any
                        // early-exit-0 bug became an unbounded respawn loop at the
                        // health-check cadence. Rate-limit it on its own window.
                        let now = Instant::now();
                        locked.children[i]
                            .handoff_history
                            .retain(|t| now.duration_since(*t) < HANDOFF_RATE_WINDOW);
                        let handoffs_in_window = locked.children[i].handoff_history.len();

                        if !handoff_allowed(handoffs_in_window) {
                            error!(
                                child = %label,
                                exit_status = %status,
                                handoffs_in_window,
                                window_secs = HANDOFF_RATE_WINDOW.as_secs(),
                                "indexer is exiting cleanly in a loop; a bulk-to-live handoff \
                                 happens once per bulk completion, so this is an early-exit bug. \
                                 Automatic handoff is blocked — investigate the indexer logs"
                            );
                            locked.children[i].blocked = Some(format!(
                                "clean-handoff loop: {handoffs_in_window} exit-0 handoffs within \
                                 {}s",
                                HANDOFF_RATE_WINDOW.as_secs()
                            ));
                            continue;
                        }

                        info!(
                            child = %label,
                            exit_status = %status,
                            handoffs_in_window,
                            "indexer exited cleanly; starting fresh process for sync-path handoff"
                        );
                        // Carry the pruned history across the respawn and record
                        // this handoff, so the window spans process lifetimes.
                        let mut carried = std::mem::take(&mut locked.children[i].handoff_history);
                        carried.push(now);

                        match spawn_child(exe, &specs[i], log_dir) {
                            Ok(mut new_child) => {
                                new_child.handoff_history = carried;
                                info!(
                                    child = %label,
                                    pid = new_child.pid(),
                                    "indexer clean-process handoff completed"
                                );
                                locked.children[i] = new_child;
                            }
                            Err(e) => {
                                // Previously this only logged and left a dead slot
                                // behind silently: the child had already exited, so
                                // nothing would ever retry it and the network was
                                // gone with no `Blocked` status anywhere.
                                error!(
                                    child = %label,
                                    error = %e,
                                    "failed to start indexer handoff process; this network has no \
                                     running indexer and needs operator action"
                                );
                                locked.children[i].handoff_history = carried;
                                locked.children[i].blocked =
                                    Some(format!("handoff respawn failed: {e}"));
                            }
                        }
                        continue;
                    }
                    ChildExitAction::BlockRebuildRequired => {
                        error!(
                            child = %label,
                            exit_status = %status,
                            workdir = %specs[i].workdir,
                            "indexer requires an explicit RocksDB purge and genesis rebuild; automatic restart is blocked"
                        );
                        locked.children[i].blocked = Some(format!(
                            "indexer requires an explicit RocksDB purge and genesis rebuild \
                             (exit {})",
                            ckbadger_indexer::lifecycle::REBUILD_REQUIRED_EXIT_CODE
                        ));
                        continue;
                    }
                    ChildExitAction::Restart => {}
                }

                let uptime = locked.children[i].started_at.elapsed();
                let mut restart_count = locked.children[i].restart_count;

                // Reset restart counter if the service ran stably before crashing.
                // This prevents a one-time crash after hours of stable operation
                // from being penalized by earlier transient startup failures.
                if uptime >= STABLE_RUNNING_THRESHOLD && restart_count > 0 {
                    info!(
                        child = %label,
                        previous_restart_count = restart_count,
                        uptime_secs = uptime.as_secs(),
                        "service ran stably, resetting restart counter"
                    );
                    restart_count = 0;
                }

                if restart_count >= MAX_RESTART_ATTEMPTS {
                    error!(
                        child = %label,
                        restarts = restart_count,
                        "service exceeded max restart attempts, giving up"
                    );
                    locked.children[i].blocked = Some(format!(
                        "exceeded max restart attempts ({MAX_RESTART_ATTEMPTS})"
                    ));
                    continue;
                }

                warn!(
                    child = %label,
                    exit_status = %status,
                    restart_count = restart_count + 1,
                    "service exited, restarting..."
                );

                // Calculate backoff
                let backoff = std::cmp::min(
                    BASE_BACKOFF * 2u32.saturating_pow(restart_count),
                    MAX_BACKOFF,
                );

                // Drop lock during sleep
                drop(locked);
                tokio::time::sleep(backoff).await;
                locked = state.lock().await;

                if locked.shutdown_requested {
                    return;
                }

                // Respawn with the correct per-child spec.
                match spawn_child(exe, &specs[i], log_dir) {
                    Ok(mut new_child) => {
                        new_child.restart_count = restart_count + 1;
                        info!(
                            child = %specs[i].label,
                            pid = new_child.pid(),
                            restart_count = restart_count + 1,
                            "service restarted"
                        );
                        locked.children[i] = new_child;
                    }
                    Err(e) => {
                        error!(
                            child = %specs[i].label,
                            error = %e,
                            "failed to restart service"
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// IPC handler
// ---------------------------------------------------------------------------

struct SupervisorIpcHandler {
    state: Arc<Mutex<SupervisorState>>,
    shutdown_tx: watch::Sender<bool>,
}

impl SupervisorIpcHandler {
    fn new(state: Arc<Mutex<SupervisorState>>, shutdown_tx: watch::Sender<bool>) -> Self {
        Self { state, shutdown_tx }
    }
}

impl IpcHandler for SupervisorIpcHandler {
    fn handle(
        &self,
        request: IpcRequest,
    ) -> Pin<Box<dyn Future<Output = IpcResponse> + Send + '_>> {
        Box::pin(async move {
            match request {
                IpcRequest::Ping => IpcResponse::Pong,

                IpcRequest::GetServiceStatus => {
                    let locked = self.state.lock().await;
                    let services = locked
                        .children
                        .iter()
                        .map(|c| {
                            let status = if c.is_blocked() {
                                ServiceStatus::Blocked
                            } else {
                                match c.child.id() {
                                    Some(_) => ServiceStatus::Running,
                                    None => ServiceStatus::Stopped,
                                }
                            };
                            ServiceInfo {
                                name: c.name.clone(),
                                pid: c.pid(),
                                status,
                                uptime_secs: c.uptime_secs(),
                            }
                        })
                        .collect();
                    IpcResponse::ServiceStatus { services }
                }

                IpcRequest::Shutdown { reason } => {
                    info!(reason = %reason, "shutdown requested via IPC");
                    let _ = self.shutdown_tx.send(true);
                    IpcResponse::Ok
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn crawler_included_only_when_enabled() {
        let mut cfg = ckbadger_config::CkbadgerConfig::default();
        assert!(!enabled_services(&cfg).contains(&"crawler"));
        cfg.crawler.enabled = true;
        assert!(enabled_services(&cfg).contains(&"crawler"));
        // Core services always present.
        assert!(enabled_services(&cfg).contains(&"indexer"));
    }

    /// Minimal valid `SyncProgressData` JSON at the given lag (camelCase, as the
    /// indexer writes it).
    fn progress_json(current_block: u64, target_block: u64) -> Vec<u8> {
        serde_json::to_vec(&ckbadger_common::SyncProgressData {
            current_block,
            target_block,
            last_batch_blocks: None,
            blocks_per_second: 0.0,
            ema_blocks_per_second: 0.0,
            txs_per_second: None,
            ema_txs_per_second: None,
            eta_seconds: None,
            eta_formatted: String::new(),
            progress_percentage: 0.0,
            updated_at: 0,
            startup_phase: None,
            is_direct_db_read: false,
            db_write_ms: None,
            db_commit_ms: None,
            rpc_fetch_ms: None,
            pipeline: None,
            pipeline_reset_epoch: None,
            pipeline_reset_reason: None,
            bulk_build: None,
        })
        .expect("SyncProgressData serializes")
    }

    fn test_sequenced_indexer(path: PathBuf) -> SequencedIndexer {
        test_sequenced_indexer_with_runtime(path, StoreRuntimeConfig::default())
    }

    fn test_sequenced_indexer_with_runtime(
        path: PathBuf,
        store_runtime_config: StoreRuntimeConfig,
    ) -> SequencedIndexer {
        SequencedIndexer {
            spec: ChildSpec {
                label: "testnet/indexer".to_string(),
                service: "indexer".to_string(),
                workdir: "/tmp/testnet".to_string(),
            },
            domain_data_path: path,
            bulk_sync_threshold: 1_000,
            store_runtime_config,
        }
    }

    #[test]
    fn sequencer_waits_only_while_domain_store_is_absent() {
        let dir = TempDir::new().unwrap();
        let idx = test_sequenced_indexer(dir.path().join("domain"));
        let mut reader = BulkStatusReader::new(&idx);
        assert_eq!(reader.read().unwrap().past_bulk, None);
        assert!(
            reader.store.is_none(),
            "no secondary may be opened before the primary's CURRENT exists"
        );
    }

    #[test]
    fn sequencer_surfaces_malformed_existing_progress() {
        let dir = TempDir::new().unwrap();
        let domain = dir.path().join("domain");
        let store = CkbadgerStore::open_domain_with_runtime(&domain, StoreRuntimeConfig::default())
            .unwrap();
        store.put_sync_progress(b"not-json").unwrap();
        let idx = test_sequenced_indexer(domain);

        let err = BulkStatusReader::new(&idx).read().unwrap_err().to_string();
        assert!(err.contains("invalid sync progress"));
        assert!(err.contains("domain"));
    }

    #[test]
    fn read_failures_on_an_existing_store_are_no_signal_not_a_teardown() {
        // The whole blast radius of the old behaviour lived on this line: a read
        // failure became an `Err`, which shut down every network's indexer/API and
        // the shared frontend. It must now be "no signal this round" and be
        // COUNTED, so the escalation ladder can get loud without going fatal.
        let dir = TempDir::new().unwrap();
        let domain = dir.path().join("domain");
        let primary =
            CkbadgerStore::open_domain_with_runtime(&domain, StoreRuntimeConfig::default())
                .unwrap();
        primary.put_sync_progress(b"not-json").unwrap();
        let idx = test_sequenced_indexer(domain);
        let mut reader = BulkStatusReader::new(&idx);

        assert_eq!(
            reader.poll().past_bulk,
            None,
            "undecodable progress yields no signal"
        );
        assert_eq!(reader.failures.consecutive(), 1);
        assert_eq!(reader.poll().past_bulk, None);
        assert_eq!(reader.failures.consecutive(), 2);

        // ...and once the record is valid again, the gate advances normally and
        // the streak resets.
        primary.put_sync_progress(&progress_json(10, 10)).unwrap();
        assert_eq!(reader.poll().past_bulk, Some(true));
        assert_eq!(reader.failures.consecutive(), 0);
    }

    #[test]
    fn a_missing_store_is_not_counted_as_a_read_failure() {
        // "Store not created yet" is the normal pre-spawn state of every deferred
        // network; conflating it with a failure would light the escalation ladder
        // on every healthy startup.
        let dir = TempDir::new().unwrap();
        let idx = test_sequenced_indexer(dir.path().join("domain"));
        let mut reader = BulkStatusReader::new(&idx);

        for _ in 0..5 {
            assert_eq!(reader.poll().past_bulk, None);
        }
        assert_eq!(reader.failures.consecutive(), 0);
    }

    #[test]
    fn read_failure_escalation_ladder_is_warn_then_error_then_persistent() {
        let mut tracker = ReadFailureTracker::default();
        for _ in 1..SEQUENCER_READ_FAILURE_ESCALATE {
            assert_eq!(tracker.record_failure(), ReadFailureLevel::Transient);
        }
        assert_eq!(tracker.record_failure(), ReadFailureLevel::Sustained);
        for _ in (SEQUENCER_READ_FAILURE_ESCALATE + 1)..SEQUENCER_READ_FAILURE_PERSISTENT {
            assert_eq!(tracker.record_failure(), ReadFailureLevel::Sustained);
        }
        assert_eq!(tracker.record_failure(), ReadFailureLevel::Persistent);
        assert_eq!(
            tracker.record_failure(),
            ReadFailureLevel::Persistent,
            "persistent is terminal, never resets on its own"
        );

        // A single good read clears the streak, so an hour-long healthy stretch
        // after a blip does not inherit its escalation.
        assert_eq!(
            tracker.record_success(),
            SEQUENCER_READ_FAILURE_PERSISTENT + 1
        );
        assert_eq!(tracker.consecutive(), 0);
        assert_eq!(tracker.record_failure(), ReadFailureLevel::Transient);
    }

    #[tokio::test]
    async fn spawn_retry_absorbs_transient_failures() {
        let attempts = std::cell::Cell::new(0u32);
        // Zero backoff keeps the test instant; `spawn_retry_backoff` is pinned
        // separately below.
        let value = spawn_with_retry(
            "testnet/indexer",
            |_| Duration::ZERO,
            || {
                attempts.set(attempts.get() + 1);
                if attempts.get() < 3 {
                    Err(anyhow::anyhow!("EMFILE"))
                } else {
                    Ok("started")
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(value, "started");
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn spawn_retry_gives_up_after_the_capped_attempts() {
        let attempts = std::cell::Cell::new(0u32);
        let err = spawn_with_retry(
            "testnet/indexer",
            |_| Duration::ZERO,
            || {
                attempts.set(attempts.get() + 1);
                Err::<(), _>(anyhow::anyhow!("EMFILE"))
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("EMFILE"));
        assert_eq!(
            attempts.get(),
            SPAWN_RETRY_ATTEMPTS,
            "retries are capped; the caller then parks instead of tearing down"
        );
    }

    #[test]
    fn spawn_retry_backoff_doubles_and_caps() {
        assert_eq!(spawn_retry_backoff(0), Duration::from_secs(1));
        assert_eq!(spawn_retry_backoff(1), Duration::from_secs(2));
        assert_eq!(spawn_retry_backoff(3), Duration::from_secs(8));
        assert_eq!(spawn_retry_backoff(30), MAX_BACKOFF);
    }

    // The escalation ladder must be ordered, and both rungs must be reachable
    // within a sane wait: at a 5s poll these are ~1min and ~2min.
    const _: () = assert!(SEQUENCER_READ_FAILURE_PERSISTENT > SEQUENCER_READ_FAILURE_ESCALATE);
    const _: () = assert!(SEQUENCER_READ_FAILURE_ESCALATE > 0);
    const _: () = assert!(SPAWN_RETRY_ATTEMPTS > 0);

    // A clean handoff is a once-per-bulk-completion event, so its limit must sit
    // BELOW the crash-restart cap: an exit-0 loop is pathological far sooner than
    // a crash loop, and it never reaches MAX_RESTART_ATTEMPTS on its own.
    const _: () = assert!(MAX_HANDOFFS_IN_WINDOW > 0);
    const _: () = assert!(MAX_HANDOFFS_IN_WINDOW < MAX_RESTART_ATTEMPTS as usize);
    // The window must be long enough that a fast respawn loop trips it: the
    // health check runs every HEALTH_CHECK_INTERVAL, so a loop produces
    // MAX_HANDOFFS_IN_WINDOW handoffs in seconds, well inside the window.
    const _: () = assert!(
        HANDOFF_RATE_WINDOW.as_secs()
            > HEALTH_CHECK_INTERVAL.as_secs() * MAX_HANDOFFS_IN_WINDOW as u64
    );

    #[test]
    fn sequencer_secondary_is_opened_once_and_only_refreshed_afterwards() {
        // The old reader opened and dropped a full 59-CF secondary on EVERY 5s
        // poll, for the hours a mainnet bulk sync runs. Pin the long-lived shape:
        // one handle, created on first sight of CURRENT and reused thereafter.
        let dir = TempDir::new().unwrap();
        let domain = dir.path().join("domain");
        let idx = test_sequenced_indexer(domain.clone());
        let mut reader = BulkStatusReader::new(&idx);

        assert_eq!(
            reader.read().unwrap().past_bulk,
            None,
            "store not created yet"
        );
        assert!(reader.store.is_none());

        let primary =
            CkbadgerStore::open_domain_with_runtime(&domain, StoreRuntimeConfig::default())
                .unwrap();
        primary.put_sync_progress(&progress_json(9, 10)).unwrap();

        assert_eq!(reader.read().unwrap().past_bulk, Some(true));
        let opened = reader.store.as_ref().expect("secondary opened") as *const CkbadgerStore;
        assert_eq!(reader.read().unwrap().past_bulk, Some(true));
        assert_eq!(
            reader.store.as_ref().unwrap() as *const CkbadgerStore,
            opened,
            "the secondary handle must be reused across polls, never reopened"
        );
    }

    #[test]
    fn sequencer_secondary_is_opened_with_the_networks_own_runtime_config() {
        // Guards the wiring, not the arithmetic: `StoreRuntimeConfig::default()`
        // here pins this process's shared RocksDB budget to UNDIVIDED host RAM and
        // silently drops an explicit `[store].memory_budget_gb`. The forwarded
        // config is all that stands between this fix and being inert.
        let dir = TempDir::new().unwrap();
        let domain = dir.path().join("domain");
        let _primary =
            CkbadgerStore::open_domain_with_runtime(&domain, StoreRuntimeConfig::default())
                .unwrap();

        let runtime = StoreRuntimeConfig {
            memory_budget_gb: Some(7),
            network_count: std::num::NonZeroUsize::new(3).unwrap(),
            ..StoreRuntimeConfig::default()
        };
        let idx = test_sequenced_indexer_with_runtime(domain, runtime);
        let mut reader = BulkStatusReader::new(&idx);
        reader.read().unwrap();

        let opened = reader
            .store
            .as_ref()
            .expect("secondary opened")
            .runtime_config();
        assert_eq!(opened.memory_budget_gb, Some(7));
        assert_eq!(opened.network_count.get(), 3);
    }

    #[tokio::test]
    async fn sequencer_task_error_is_returned_to_supervisor_waiter() {
        let mut handle = tokio::spawn(async { Err(anyhow::anyhow!("status read failed")) });
        let error = wait_for_sequencer_failure(Some(&mut handle)).await;
        assert!(error.to_string().contains("status read failed"));
    }

    #[test]
    fn test_backoff_calculation() {
        // Verify exponential backoff with cap
        let b0 = std::cmp::min(BASE_BACKOFF * 2u32.pow(0), MAX_BACKOFF);
        assert_eq!(b0, Duration::from_secs(1));

        let b1 = std::cmp::min(BASE_BACKOFF * 2u32.pow(1), MAX_BACKOFF);
        assert_eq!(b1, Duration::from_secs(2));

        let b5 = std::cmp::min(BASE_BACKOFF * 2u32.pow(5), MAX_BACKOFF);
        assert_eq!(b5, Duration::from_secs(32));

        let b6 = std::cmp::min(BASE_BACKOFF * 2u32.pow(6), MAX_BACKOFF);
        assert_eq!(b6, MAX_BACKOFF); // capped at 60
    }

    #[test]
    fn successful_indexer_exit_is_a_planned_process_handoff() {
        let indexer = ChildSpec {
            label: "testnet/indexer".to_string(),
            service: "indexer".to_string(),
            workdir: "/tmp/testnet".to_string(),
        };
        let api = ChildSpec {
            label: "testnet/api".to_string(),
            service: "api".to_string(),
            workdir: "/tmp/testnet".to_string(),
        };

        assert_eq!(
            classify_child_exit(&indexer, true, Some(0)),
            ChildExitAction::CleanHandoff
        );
        assert_eq!(
            classify_child_exit(&indexer, false, Some(1)),
            ChildExitAction::Restart
        );
        assert_eq!(
            classify_child_exit(&api, true, Some(0)),
            ChildExitAction::Restart
        );
    }

    #[test]
    fn rebuild_required_indexer_exit_is_blocked_instead_of_restarted() {
        let indexer = ChildSpec {
            label: "testnet/indexer".to_string(),
            service: "indexer".to_string(),
            workdir: "/tmp/testnet".to_string(),
        };
        let api = ChildSpec {
            label: "testnet/api".to_string(),
            service: "api".to_string(),
            workdir: "/tmp/testnet".to_string(),
        };
        let rebuild_code = i32::from(ckbadger_indexer::lifecycle::REBUILD_REQUIRED_EXIT_CODE);

        assert_eq!(
            classify_child_exit(&indexer, false, Some(rebuild_code)),
            ChildExitAction::BlockRebuildRequired
        );
        assert_eq!(
            classify_child_exit(&indexer, false, Some(1)),
            ChildExitAction::Restart
        );
        assert_eq!(
            classify_child_exit(&indexer, true, Some(0)),
            ChildExitAction::CleanHandoff
        );
        assert_eq!(
            classify_child_exit(&api, false, Some(rebuild_code)),
            ChildExitAction::Restart,
            "rebuild-required is an indexer-specific lifecycle state"
        );
    }

    #[test]
    fn clean_handoffs_are_allowed_up_to_the_window_limit_then_blocked() {
        // Regression for the unbounded exit-0 loop: `CleanHandoff` respawned with
        // no count, no backoff and no cap, so an indexer that exits 0 immediately
        // respawned every HEALTH_CHECK_INTERVAL forever, invisible to
        // MAX_RESTART_ATTEMPTS (which exit-0 short-circuits before any counting).
        for already in 0..MAX_HANDOFFS_IN_WINDOW {
            assert!(
                handoff_allowed(already),
                "a legitimate bulk->live handoff must never be blocked"
            );
        }
        assert!(!handoff_allowed(MAX_HANDOFFS_IN_WINDOW));
        assert!(!handoff_allowed(MAX_HANDOFFS_IN_WINDOW + 100));
    }

    #[tokio::test]
    async fn handoff_history_prunes_to_the_rate_window() {
        // Only handoffs INSIDE the window count, so a network that legitimately
        // completes bulk once a day never accumulates toward the limit.
        let dir = TempDir::new().unwrap();
        let exe = std::env::current_exe().unwrap();
        let spec = ChildSpec {
            label: "mainnet/indexer".to_string(),
            service: "indexer".to_string(),
            workdir: dir.path().to_string_lossy().to_string(),
        };
        let mut child = spawn_child(&exe, &spec, &dir.path().join("run/logs")).unwrap();

        let now = Instant::now();
        let stale = now - HANDOFF_RATE_WINDOW - Duration::from_secs(1);
        child.handoff_history = vec![stale, stale, now];
        child
            .handoff_history
            .retain(|t| now.duration_since(*t) < HANDOFF_RATE_WINDOW);

        assert_eq!(child.handoff_history.len(), 1, "stale handoffs pruned");
        assert!(handoff_allowed(child.handoff_history.len()));

        let _ = child.child.kill().await;
        let _ = child.child.wait().await;
    }

    #[test]
    fn indexer_gets_extended_cooperative_shutdown_timeout() {
        assert_eq!(
            graceful_shutdown_timeout("testnet/indexer"),
            INDEXER_GRACEFUL_SHUTDOWN_TIMEOUT
        );
        assert_eq!(
            graceful_shutdown_timeout("testnet/api"),
            GRACEFUL_SHUTDOWN_TIMEOUT
        );
    }

    // Compile-time checks for supervisor constants
    const _: () = assert!(MAX_RESTART_ATTEMPTS > 0);
    const _: () = assert!(HEALTH_CHECK_INTERVAL.as_secs() > 0);

    #[tokio::test]
    async fn observe_gate_reports_the_blocked_gating_indexer_with_its_reason() {
        // The MAJOR defect this closes: a blocked gating indexer (exit-78
        // rebuild-required, or past MAX_RESTART_ATTEMPTS) left the next network
        // deferred forever with no diagnostic anywhere. The gate must now SEE the
        // block, and see WHY, while still reporting "no signal" so it keeps
        // waiting rather than skipping ahead.
        let dir = TempDir::new().unwrap();
        let idx = test_sequenced_indexer(dir.path().join("domain"));
        let readers: BulkStatusReaders =
            Arc::new(std::sync::Mutex::new(vec![BulkStatusReader::new(&idx)]));

        let exe = std::env::current_exe().unwrap();
        let log_dir = dir.path().join("run/logs");
        let spec = ChildSpec {
            label: "mainnet/indexer".to_string(),
            service: "indexer".to_string(),
            workdir: dir.path().to_string_lossy().to_string(),
        };
        let mut child = spawn_child(&exe, &spec, &log_dir).unwrap();
        child.blocked = Some("exceeded max restart attempts (10)".to_string());
        assert!(child.is_blocked());
        let state = Arc::new(Mutex::new(SupervisorState {
            children: vec![child],
            shutdown_requested: false,
        }));

        let status = observe_gate(&readers, &state, 0, 0).await.unwrap();
        assert_eq!(
            status.past_bulk, None,
            "blocked gate still yields no signal"
        );
        assert_eq!(
            status.blocked.as_deref(),
            Some("exceeded max restart attempts (10)")
        );

        // Unblocking is visible on the very next observation.
        state.lock().await.children[0].blocked = None;
        let status = observe_gate(&readers, &state, 0, 0).await.unwrap();
        assert_eq!(status.blocked, None);

        let mut locked = state.lock().await;
        let _ = locked.children[0].child.kill().await;
        let _ = locked.children[0].child.wait().await;
    }

    #[tokio::test]
    async fn test_supervisor_ipc_handler_ping() {
        let state = Arc::new(Mutex::new(SupervisorState {
            children: vec![],
            shutdown_requested: false,
        }));
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let handler = SupervisorIpcHandler::new(state, shutdown_tx);

        let resp = handler.handle(IpcRequest::Ping).await;
        assert!(matches!(resp, IpcResponse::Pong));
    }

    #[tokio::test]
    async fn test_supervisor_ipc_handler_status_empty() {
        let state = Arc::new(Mutex::new(SupervisorState {
            children: vec![],
            shutdown_requested: false,
        }));
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let handler = SupervisorIpcHandler::new(state, shutdown_tx);

        let resp = handler.handle(IpcRequest::GetServiceStatus).await;
        match resp {
            IpcResponse::ServiceStatus { services } => {
                assert!(services.is_empty());
            }
            other => panic!("expected ServiceStatus, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_supervisor_ipc_handler_shutdown() {
        let state = Arc::new(Mutex::new(SupervisorState {
            children: vec![],
            shutdown_requested: false,
        }));
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let handler = SupervisorIpcHandler::new(state, shutdown_tx);

        let resp = handler
            .handle(IpcRequest::Shutdown {
                reason: "test".to_string(),
            })
            .await;
        assert!(matches!(resp, IpcResponse::Ok));

        // Verify the shutdown signal was sent
        assert!(shutdown_rx.changed().await.is_ok());
        assert!(*shutdown_rx.borrow());
    }

    #[test]
    fn child_spec_log_and_label() {
        let spec = ChildSpec {
            label: "testnet/indexer".to_string(),
            service: "indexer".to_string(),
            workdir: "/srv/ckb/testnet".to_string(),
        };
        // Log filename derives from the label with '/' replaced by '-'.
        assert_eq!(spec.log_file_name(), "testnet-indexer.log");
    }

    #[tokio::test]
    async fn spawn_child_creates_labeled_log_file() {
        let dir = TempDir::new().unwrap();
        let log_dir = dir.path().join("run/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        let exe = std::env::current_exe().unwrap();

        let spec = ChildSpec {
            label: "mainnet/indexer".to_string(),
            service: "indexer".to_string(),
            workdir: dir.path().to_string_lossy().to_string(),
        };
        let mut child = spawn_child(&exe, &spec, &log_dir).unwrap();
        assert!(log_dir.join("mainnet-indexer.log").exists());
        let _ = child.child.kill().await;
        let _ = child.child.wait().await;
    }

    #[test]
    fn test_stable_running_threshold_resets_restart_count() {
        // Simulate a service that ran beyond the stability threshold
        let uptime = STABLE_RUNNING_THRESHOLD + Duration::from_secs(1);
        let restart_count: u32 = 5;

        // Same logic as monitor_children: reset if uptime >= threshold
        let effective_count = if uptime >= STABLE_RUNNING_THRESHOLD && restart_count > 0 {
            0
        } else {
            restart_count
        };

        assert_eq!(
            effective_count, 0,
            "restart_count should reset to 0 after stable running"
        );
    }

    #[test]
    fn test_short_uptime_preserves_restart_count() {
        // Simulate a service that crashed quickly (before stability threshold)
        let uptime = STABLE_RUNNING_THRESHOLD - Duration::from_secs(1);
        let restart_count: u32 = 5;

        let effective_count = if uptime >= STABLE_RUNNING_THRESHOLD && restart_count > 0 {
            0
        } else {
            restart_count
        };

        assert_eq!(
            effective_count, 5,
            "restart_count should NOT reset for short-lived service"
        );
    }

    #[test]
    fn test_stable_running_zero_count_stays_zero() {
        // If restart_count is already 0, no reset needed
        let uptime = STABLE_RUNNING_THRESHOLD + Duration::from_secs(60);
        let restart_count: u32 = 0;

        let effective_count = if uptime >= STABLE_RUNNING_THRESHOLD && restart_count > 0 {
            0
        } else {
            restart_count
        };

        assert_eq!(effective_count, 0);
    }

    // Compile-time check for the new constant
    const _: () = assert!(STABLE_RUNNING_THRESHOLD.as_secs() > 0);

    #[tokio::test]
    async fn test_stop_child_gracefully_sends_sigterm() {
        let mut child = tokio::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        stop_child_gracefully("test-sleep", &mut child).await;
        // Child should have exited (SIGTERM default action is terminate)
        let status = child.try_wait().unwrap();
        assert!(
            status.is_some(),
            "child should have exited after graceful stop"
        );
    }
}
