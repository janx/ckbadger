//! Process supervisor for `ckbadger run`.
//!
//! Spawns child processes for each requested service via
//! `ckbadger internal {indexer,api}`, monitors them, and restarts
//! on crash with exponential backoff. Starts an IPC server on
//! `run/indexer.sock` for status queries and shutdown requests.

use anyhow::{Context, Result};
use ckbadger_config::{CkbadgerConfig, WorkDir};
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

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A managed child process.
struct ManagedChild {
    name: String,
    child: Child,
    restart_count: u32,
    started_at: Instant,
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
    for spec in &specs {
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
            info!(child = %managed.name, pid = managed.pid(), "stopping service");
            stop_child_gracefully(&managed.name, &mut managed.child).await;
        }
    }

    // Cleanup
    ipc_handle.abort();
    monitor_handle.abort();

    // Remove PID file and socket
    let _ = std::fs::remove_file(&root.supervisor_pid);
    let _ = std::fs::remove_file(&root.indexer_sock);

    info!("supervisor stopped");
    Ok(())
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
    })
}

/// Thin adapter: spawn a service in a single workdir under its own name.
///
/// Kept so single-network tests can spawn without constructing a [`ChildSpec`]
/// (label == service, so the log stays `<service>.log`). Production paths build
/// [`ChildSpec`]s and call [`spawn_child`] directly, so this is test-only.
#[cfg(test)]
fn spawn_service(
    exe: &PathBuf,
    workdir: &str,
    service: &str,
    log_dir: &Path,
) -> Result<ManagedChild> {
    let spec = ChildSpec {
        label: service.to_string(),
        service: service.to_string(),
        workdir: workdir.to_string(),
    };
    spawn_child(exe, &spec, log_dir)
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
