//! Process supervisor for `ckbadger run`.
//!
//! Spawns child processes for each requested service via
//! `ckbadger internal {indexer,api}`, monitors them, and restarts
//! on crash with exponential backoff. Starts an IPC server on
//! `run/indexer.sock` for status queries and shutdown requests.

use anyhow::{Context, Result};
use ckbadger_config::{CkbadgerConfig, WorkDir};
use ckbadger_indexer::entry::EXIT_CODE_UNRECOVERABLE;
use ckbadger_ipc::{IpcHandler, IpcRequest, IpcResponse, IpcServer, ServiceInfo, ServiceStatus};
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

/// Sliding window for the crash-loop rate limit.
const RESTART_RATE_WINDOW: Duration = Duration::from_secs(3600);

/// More than this many restarts within `RESTART_RATE_WINDOW` is a persistent
/// crash-loop: give up even if every individual run outlived
/// `STABLE_RUNNING_THRESHOLD`. Without this backstop, a service that crashes
/// just after the stable threshold resets the consecutive counter on every
/// iteration and never reaches `MAX_RESTART_ATTEMPTS` — the exact shape of a
/// ~9h, ~250-restart loop observed in the field. Set above `MAX_RESTART_ATTEMPTS`
/// so fast startup-failure loops still trip the consecutive cap first.
const RESTART_RATE_LIMIT: u32 = 15;

// ---------------------------------------------------------------------------
// Restart decision
// ---------------------------------------------------------------------------

/// Why the supervisor stopped restarting a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HaltReason {
    /// Child exited with [`EXIT_CODE_UNRECOVERABLE`] — a restart cannot fix it.
    UnrecoverableExit,
    /// Too many restarts within [`RESTART_RATE_WINDOW`] — persistent crash-loop.
    CrashLoop,
    /// Consecutive restart attempts reached [`MAX_RESTART_ATTEMPTS`].
    MaxConsecutive,
}

/// Outcome of [`decide_restart`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartDecision {
    /// Stop restarting this service.
    Halt(HaltReason),
    /// Restart after `backoff`, carrying `next_count` as the new consecutive count.
    Restart { backoff: Duration, next_count: u32 },
}

/// Decide whether to restart a service that just exited.
///
/// Pure (no I/O, no clock) so it can be unit-tested exhaustively; the caller
/// supplies the observed state.
///
/// - `exit_code`: the child's process exit code (`None` if killed by a signal).
/// - `uptime`: how long the child ran before exiting.
/// - `consecutive_restart_count`: restarts since the last stable run.
/// - `restarts_in_window`: restarts within the last [`RESTART_RATE_WINDOW`].
fn decide_restart(
    exit_code: Option<i32>,
    uptime: Duration,
    consecutive_restart_count: u32,
    restarts_in_window: u32,
) -> RestartDecision {
    // Unrecoverable exit: a restart cannot fix it (e.g. a corrupted DB /
    // cross-store inconsistency). Halt immediately — never burn retries.
    if exit_code == Some(EXIT_CODE_UNRECOVERABLE) {
        return RestartDecision::Halt(HaltReason::UnrecoverableExit);
    }

    // Persistent crash-loop: give up regardless of per-run uptime. This catches
    // slow loops where each run outlives STABLE_RUNNING_THRESHOLD and would
    // otherwise reset the consecutive counter on every iteration.
    if restarts_in_window >= RESTART_RATE_LIMIT {
        return RestartDecision::Halt(HaltReason::CrashLoop);
    }

    // A run that lasted long enough clears accumulated consecutive startup
    // failures — a one-off crash after stable operation isn't penalised.
    let effective = if uptime >= STABLE_RUNNING_THRESHOLD {
        0
    } else {
        consecutive_restart_count
    };

    if effective >= MAX_RESTART_ATTEMPTS {
        return RestartDecision::Halt(HaltReason::MaxConsecutive);
    }

    let backoff = std::cmp::min(BASE_BACKOFF * 2u32.saturating_pow(effective), MAX_BACKOFF);
    RestartDecision::Restart {
        backoff,
        next_count: effective + 1,
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A managed child process.
struct ManagedChild {
    name: String,
    child: Child,
    restart_count: u32,
    started_at: Instant,
    /// Timestamps of recent restarts, pruned to `RESTART_RATE_WINDOW`. Carried
    /// across respawns so a persistent crash-loop can be detected by rate even
    /// when each individual run outlives `STABLE_RUNNING_THRESHOLD`.
    restart_history: Vec<Instant>,
}

impl ManagedChild {
    fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
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

    match tokio::time::timeout(GRACEFUL_SHUTDOWN_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => {
            info!(service = %name, pid, ?status, "service stopped gracefully");
        }
        Ok(Err(e)) => {
            warn!(service = %name, pid, error = %e, "error waiting for service, sending SIGKILL");
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        Err(_) => {
            warn!(service = %name, pid, timeout_secs = GRACEFUL_SHUTDOWN_TIMEOUT.as_secs(),
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
/// `internal <name>` subcommand via [`spawn_service`] (e.g. `"crawler"` ->
/// `internal crawler`). An explicit `--only` flag overrides this default.
pub fn enabled_services(cfg: &CkbadgerConfig) -> Vec<&'static str> {
    let mut services = vec!["indexer", "api", "frontend-server"];
    if cfg.crawler.enabled {
        services.push("crawler");
    }
    services
}

/// Run the supervisor: spawn services, start IPC server, monitor children.
///
/// Blocks until Ctrl+C or IPC shutdown request. Returns after all
/// children have been stopped.
pub async fn run_supervisor(
    work_dir: &WorkDir,
    _config: &CkbadgerConfig,
    services: Vec<String>,
) -> Result<()> {
    // Ensure run directory exists
    std::fs::create_dir_all(&work_dir.run_dir).with_context(|| {
        format!(
            "failed to create run directory: {}",
            work_dir.run_dir.display()
        )
    })?;
    std::fs::create_dir_all(&work_dir.log_dir).with_context(|| {
        format!(
            "failed to create log directory: {}",
            work_dir.log_dir.display()
        )
    })?;

    // Write PID file
    let pid = std::process::id();
    std::fs::write(&work_dir.supervisor_pid, pid.to_string()).with_context(|| {
        format!(
            "failed to write PID file: {}",
            work_dir.supervisor_pid.display()
        )
    })?;

    // Spawn initial children
    let exe = std::env::current_exe().context("failed to determine executable path")?;
    let workdir_str = work_dir.root.to_string_lossy().to_string();

    let mut children = Vec::new();
    for service in &services {
        let child = spawn_service(&exe, &workdir_str, service, &work_dir.log_dir)?;
        info!(service = %service, pid = child.pid(), "started service");
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
    let sock_path = work_dir.indexer_sock.clone();
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
    let monitor_workdir = workdir_str.clone();
    let monitor_log_dir = work_dir.log_dir.clone();
    let monitor_services = services.clone();
    let monitor_shutdown = shutdown_rx.clone();
    let monitor_handle = tokio::spawn(async move {
        monitor_children(
            monitor_state,
            &monitor_exe,
            &monitor_workdir,
            &monitor_log_dir,
            &monitor_services,
            monitor_shutdown,
        )
        .await;
    });

    // Wait for Ctrl+C or IPC shutdown
    let mut ctrl_c_rx = shutdown_rx.clone();
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
    }

    // Signal shutdown
    let _ = shutdown_tx.send(true);

    // Stop all children gracefully (SIGTERM + timeout + SIGKILL)
    {
        let mut locked = state.lock().await;
        locked.shutdown_requested = true;
        for managed in &mut locked.children {
            info!(service = %managed.name, pid = managed.pid(), "stopping service");
            stop_child_gracefully(&managed.name, &mut managed.child).await;
        }
    }

    // Cleanup
    ipc_handle.abort();
    monitor_handle.abort();

    // Remove PID file and socket
    let _ = std::fs::remove_file(&work_dir.supervisor_pid);
    let _ = std::fs::remove_file(&work_dir.indexer_sock);

    info!("supervisor stopped");
    Ok(())
}

// ---------------------------------------------------------------------------
// Service spawning
// ---------------------------------------------------------------------------

fn spawn_service(
    exe: &PathBuf,
    workdir: &str,
    service: &str,
    log_dir: &Path,
) -> Result<ManagedChild> {
    std::fs::create_dir_all(log_dir)
        .with_context(|| format!("failed to create log directory: {}", log_dir.display()))?;

    let log_file_path = log_dir.join(format!("{service}.log"));
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
        .with_context(|| format!("failed to clone log handle for service {}", service))?;

    let child = Command::new(exe)
        .arg("internal")
        .arg(service)
        .arg("-C")
        .arg(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(stderr_log))
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to spawn {} subprocess", service))?;

    Ok(ManagedChild {
        name: service.to_string(),
        child,
        restart_count: 0,
        started_at: Instant::now(),
        restart_history: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Health monitoring
// ---------------------------------------------------------------------------

async fn monitor_children(
    state: Arc<Mutex<SupervisorState>>,
    exe: &PathBuf,
    workdir: &str,
    log_dir: &Path,
    services: &[String],
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
            // Check if child has exited
            let exited = match locked.children[i].child.try_wait() {
                Ok(Some(status)) => Some(status),
                Ok(None) => None,
                Err(e) => {
                    warn!(
                        service = %locked.children[i].name,
                        error = %e,
                        "failed to check child status"
                    );
                    None
                }
            };

            if let Some(status) = exited {
                let name = locked.children[i].name.clone();
                let uptime = locked.children[i].started_at.elapsed();
                let restart_count = locked.children[i].restart_count;

                // Prune the restart history to the rate window and count it, so
                // decide_restart can catch a persistent crash-loop even when each
                // run outlives STABLE_RUNNING_THRESHOLD (which resets the
                // consecutive counter every iteration).
                let now = Instant::now();
                locked.children[i]
                    .restart_history
                    .retain(|t| now.duration_since(*t) < RESTART_RATE_WINDOW);
                let restarts_in_window = locked.children[i].restart_history.len() as u32;

                let (backoff, next_count) = match decide_restart(
                    status.code(),
                    uptime,
                    restart_count,
                    restarts_in_window,
                ) {
                    RestartDecision::Halt(reason) => {
                        match reason {
                            HaltReason::UnrecoverableExit => error!(
                                service = %name,
                                exit_status = %status,
                                "service exited with an UNRECOVERABLE error and will not be \
                                 restarted; the DB is corrupted (cross-store inconsistency). \
                                 Purge the RocksDB data dirs and re-sync from genesis."
                            ),
                            HaltReason::CrashLoop => error!(
                                service = %name,
                                restarts_in_window,
                                window_secs = RESTART_RATE_WINDOW.as_secs(),
                                "service is in a persistent crash-loop; giving up (restart rate \
                                 limit exceeded). Investigate the logs before restarting."
                            ),
                            HaltReason::MaxConsecutive => error!(
                                service = %name,
                                restarts = restart_count,
                                "service exceeded max restart attempts, giving up"
                            ),
                        }
                        continue;
                    }
                    RestartDecision::Restart {
                        backoff,
                        next_count,
                    } => (backoff, next_count),
                };

                warn!(
                    service = %name,
                    exit_status = %status,
                    restart_count = next_count,
                    backoff_secs = backoff.as_secs(),
                    "service exited, restarting..."
                );

                // Drop lock during sleep
                drop(locked);
                tokio::time::sleep(backoff).await;
                locked = state.lock().await;

                if locked.shutdown_requested {
                    return;
                }

                // Carry the pruned restart history across the respawn and record
                // this restart so the rate window spans process lifetimes.
                let mut carried_history = std::mem::take(&mut locked.children[i].restart_history);
                carried_history.push(Instant::now());

                // Respawn
                let service_name = &services[i];
                match spawn_service(exe, workdir, service_name, log_dir) {
                    Ok(mut new_child) => {
                        new_child.restart_count = next_count;
                        new_child.restart_history = carried_history;
                        info!(
                            service = %service_name,
                            pid = new_child.pid(),
                            restart_count = next_count,
                            "service restarted"
                        );
                        locked.children[i] = new_child;
                    }
                    Err(e) => {
                        error!(
                            service = %service_name,
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
                            let status = match c.child.id() {
                                Some(_) => ServiceStatus::Running,
                                None => ServiceStatus::Stopped,
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

    // Compile-time checks for supervisor constants
    const _: () = assert!(MAX_RESTART_ATTEMPTS > 0);
    const _: () = assert!(HEALTH_CHECK_INTERVAL.as_secs() > 0);
    // Rate limit must sit above the consecutive cap so fast startup-failure
    // loops trip MaxConsecutive first and the rate window stays a slow-loop
    // backstop.
    const _: () = assert!(RESTART_RATE_LIMIT > MAX_RESTART_ATTEMPTS);

    #[test]
    fn decide_restart_halts_on_unrecoverable_exit_code() {
        // A corrupted-DB / cross-store-inconsistency exit must never be retried,
        // no matter how healthy the counts/uptime look.
        let d = decide_restart(
            Some(EXIT_CODE_UNRECOVERABLE),
            Duration::from_secs(10_000),
            0,
            0,
        );
        assert_eq!(d, RestartDecision::Halt(HaltReason::UnrecoverableExit));
    }

    #[test]
    fn decide_restart_halts_on_crash_loop_even_when_each_run_looks_stable() {
        // Regression for the ~9h / ~250-restart loop: each run lasted ~131s
        // (> the 120s stable threshold), so the consecutive counter reset every
        // iteration and MAX_RESTART_ATTEMPTS was never reached. The rate-window
        // backstop must still catch it.
        let stable_uptime = STABLE_RUNNING_THRESHOLD + Duration::from_secs(11);
        let d = decide_restart(Some(1), stable_uptime, 0, RESTART_RATE_LIMIT);
        assert_eq!(d, RestartDecision::Halt(HaltReason::CrashLoop));
    }

    #[test]
    fn decide_restart_halts_after_max_consecutive_fast_failures() {
        // Fast startup-failure loop: runs shorter than the stable threshold, so
        // the consecutive counter climbs to the cap. Window count kept below the
        // rate limit to isolate the consecutive cap.
        let d = decide_restart(
            Some(1),
            Duration::from_secs(2),
            MAX_RESTART_ATTEMPTS,
            MAX_RESTART_ATTEMPTS,
        );
        assert_eq!(d, RestartDecision::Halt(HaltReason::MaxConsecutive));
    }

    #[test]
    fn decide_restart_restarts_with_exponential_backoff() {
        let d = decide_restart(Some(1), Duration::from_secs(2), 3, 3);
        assert_eq!(
            d,
            RestartDecision::Restart {
                backoff: Duration::from_secs(8), // 1s * 2^3
                next_count: 4,
            }
        );
    }

    #[test]
    fn decide_restart_resets_consecutive_counter_after_stable_run() {
        // A single crash after a long stable run isn't penalised by earlier
        // transient failures: the consecutive counter resets to 0.
        let d = decide_restart(Some(1), STABLE_RUNNING_THRESHOLD, 5, 1);
        assert_eq!(
            d,
            RestartDecision::Restart {
                backoff: Duration::from_secs(1), // 1s * 2^0
                next_count: 1,
            }
        );
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

    #[tokio::test]
    async fn test_spawn_service_creates_log_file() {
        let dir = TempDir::new().unwrap();
        let workdir = dir.path();
        let log_dir = workdir.join("run/logs");
        std::fs::create_dir_all(&log_dir).unwrap();

        let exe = std::env::current_exe().unwrap();
        let mut child =
            spawn_service(&exe, &workdir.to_string_lossy(), "indexer", &log_dir).unwrap();
        let log_file = log_dir.join("indexer.log");

        assert!(
            log_file.exists(),
            "service log file should be created at spawn time"
        );

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
