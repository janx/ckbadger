//! IPC Unix socket server.
//!
//! The server listens on a Unix domain socket and handles one
//! request-response cycle per connection. Each connection is handled
//! in a separate tokio task.

use crate::protocol::{IpcRequest, IpcResponse};
use anyhow::{Context, Result};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{debug, warn};

/// Handler trait for processing IPC requests.
///
/// Implementors produce an [`IpcResponse`] for each [`IpcRequest`].
pub trait IpcHandler: Send + Sync + 'static {
    fn handle(&self, request: IpcRequest)
        -> Pin<Box<dyn Future<Output = IpcResponse> + Send + '_>>;
}

/// Unix socket IPC server.
///
/// Listens on `socket_path`, accepting connections and dispatching
/// requests to the handler. Each connection is served by a spawned task.
pub struct IpcServer {
    socket_path: PathBuf,
    handler: Arc<dyn IpcHandler + Send + Sync>,
}

impl IpcServer {
    pub fn new(socket_path: PathBuf, handler: Arc<dyn IpcHandler + Send + Sync>) -> Self {
        Self {
            socket_path,
            handler,
        }
    }

    /// Listen for connections until the task is cancelled.
    ///
    /// Removes any stale socket file before binding. On each accepted
    /// connection, spawns a task that reads one request, calls the handler,
    /// writes the response, and closes the connection.
    pub async fn listen(self) -> Result<()> {
        // Remove stale socket if it exists
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path).with_context(|| {
                format!(
                    "failed to remove stale socket: {}",
                    self.socket_path.display()
                )
            })?;
        }

        let listener = UnixListener::bind(&self.socket_path).with_context(|| {
            format!("failed to bind IPC socket: {}", self.socket_path.display())
        })?;

        debug!(path = %self.socket_path.display(), "IPC server listening");

        loop {
            let (stream, _addr) = listener.accept().await.with_context(|| {
                format!(
                    "failed to accept on IPC socket: {}",
                    self.socket_path.display()
                )
            })?;

            let handler = self.handler.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, handler).await {
                    warn!("IPC connection error: {:#}", e);
                }
            });
        }
    }
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    handler: Arc<dyn IpcHandler + Send + Sync>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    buf_reader
        .read_line(&mut line)
        .await
        .context("failed to read IPC request from connection")?;

    if line.is_empty() {
        // Client disconnected without sending anything
        return Ok(());
    }

    let request: IpcRequest = serde_json::from_str(line.trim())
        .with_context(|| format!("failed to parse IPC request: {}", line.trim()))?;

    let response = handler.handle(request).await;

    let mut resp_json =
        serde_json::to_string(&response).context("failed to serialize IPC response")?;
    resp_json.push('\n');

    writer
        .write_all(resp_json.as_bytes())
        .await
        .context("failed to write IPC response")?;

    Ok(())
}
