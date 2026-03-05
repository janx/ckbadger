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
use std::path::PathBuf;
use std::pin::Pin;
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

/// How often to check child process health.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(2);

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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

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
        let child = spawn_service(&exe, &workdir_str, service)?;
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
    let monitor_services = services.clone();
    let monitor_shutdown = shutdown_rx.clone();
    let monitor_handle = tokio::spawn(async move {
        monitor_children(
            monitor_state,
            &monitor_exe,
            &monitor_workdir,
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

    // Stop all children
    {
        let mut locked = state.lock().await;
        locked.shutdown_requested = true;
        for managed in &mut locked.children {
            info!(service = %managed.name, pid = managed.pid(), "stopping service");
            let _ = managed.child.kill().await;
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

fn spawn_service(exe: &PathBuf, workdir: &str, service: &str) -> Result<ManagedChild> {
    let child = Command::new(exe)
        .arg("internal")
        .arg(service)
        .arg("-C")
        .arg(workdir)
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to spawn {} subprocess", service))?;

    Ok(ManagedChild {
        name: service.to_string(),
        child,
        restart_count: 0,
        started_at: Instant::now(),
    })
}

// ---------------------------------------------------------------------------
// Health monitoring
// ---------------------------------------------------------------------------

async fn monitor_children(
    state: Arc<Mutex<SupervisorState>>,
    exe: &PathBuf,
    workdir: &str,
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
                let name = &locked.children[i].name;
                let restart_count = locked.children[i].restart_count;

                if restart_count >= MAX_RESTART_ATTEMPTS {
                    error!(
                        service = %name,
                        restarts = restart_count,
                        "service exceeded max restart attempts, giving up"
                    );
                    continue;
                }

                warn!(
                    service = %name,
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

                // Respawn
                let service_name = &services[i];
                match spawn_service(exe, workdir, service_name) {
                    Ok(mut new_child) => {
                        new_child.restart_count = restart_count + 1;
                        info!(
                            service = %service_name,
                            pid = new_child.pid(),
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
}
