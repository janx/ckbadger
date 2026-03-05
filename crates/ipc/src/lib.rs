//! IPC crate for ckbadger inter-process communication.
//!
//! Provides a JSON-over-Unix-socket protocol for the supervisor to
//! communicate with child services and for CLI commands (e.g. `status`)
//! to query the supervisor.
//!
//! Protocol: newline-delimited JSON. Each message is a single JSON object
//! followed by `\n`. The server reads one request, writes one response,
//! then closes the connection.

mod protocol;
mod server;

pub use protocol::{IpcRequest, IpcResponse, ServiceInfo, ServiceStatus};
pub use server::{IpcHandler, IpcServer};

use anyhow::{Context, Result};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Send a single IPC request and return the response.
///
/// Connects to the Unix socket at `socket_path`, writes the JSON-encoded
/// request followed by a newline, reads the JSON-encoded response, and
/// returns it. The connection is closed after each request-response cycle.
pub async fn ipc_request(socket_path: &Path, request: &IpcRequest) -> Result<IpcResponse> {
    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("failed to connect to IPC socket: {}", socket_path.display()))?;

    let (reader, mut writer) = stream.into_split();

    // Write request
    let mut msg = serde_json::to_string(request).context("failed to serialize IPC request")?;
    msg.push('\n');
    writer
        .write_all(msg.as_bytes())
        .await
        .context("failed to write IPC request")?;
    writer
        .shutdown()
        .await
        .context("failed to shutdown write half")?;

    // Read response
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader
        .read_line(&mut line)
        .await
        .context("failed to read IPC response")?;

    if line.is_empty() {
        anyhow::bail!("empty IPC response (server closed connection without reply)");
    }

    serde_json::from_str(line.trim()).context("failed to deserialize IPC response")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct EchoHandler;

    impl IpcHandler for EchoHandler {
        fn handle(
            &self,
            request: IpcRequest,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = IpcResponse> + Send + '_>> {
            Box::pin(async move {
                match request {
                    IpcRequest::Ping => IpcResponse::Pong,
                    IpcRequest::GetServiceStatus => IpcResponse::ServiceStatus {
                        services: vec![ServiceInfo {
                            name: "indexer".to_string(),
                            pid: 1234,
                            status: ServiceStatus::Running,
                            uptime_secs: 60,
                        }],
                    },
                    IpcRequest::Shutdown { .. } => IpcResponse::Ok,
                }
            })
        }
    }

    #[tokio::test]
    async fn test_ping_pong_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");

        let handler: Arc<dyn IpcHandler + Send + Sync> = Arc::new(EchoHandler);
        let server = IpcServer::new(sock.clone(), handler);

        let server_handle = tokio::spawn(async move {
            server.listen().await.unwrap();
        });

        // Give the server a moment to bind
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let resp = ipc_request(&sock, &IpcRequest::Ping).await.unwrap();
        assert!(matches!(resp, IpcResponse::Pong));

        // Server will continue listening; just abort it
        server_handle.abort();
    }

    #[tokio::test]
    async fn test_service_status_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");

        let handler: Arc<dyn IpcHandler + Send + Sync> = Arc::new(EchoHandler);
        let server = IpcServer::new(sock.clone(), handler);

        let server_handle = tokio::spawn(async move {
            server.listen().await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let resp = ipc_request(&sock, &IpcRequest::GetServiceStatus)
            .await
            .unwrap();

        match resp {
            IpcResponse::ServiceStatus { services } => {
                assert_eq!(services.len(), 1);
                assert_eq!(services[0].name, "indexer");
                assert_eq!(services[0].pid, 1234);
                assert!(matches!(services[0].status, ServiceStatus::Running));
                assert_eq!(services[0].uptime_secs, 60);
            }
            other => panic!("expected ServiceStatus, got: {:?}", other),
        }

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_shutdown_request() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");

        let handler: Arc<dyn IpcHandler + Send + Sync> = Arc::new(EchoHandler);
        let server = IpcServer::new(sock.clone(), handler);

        let server_handle = tokio::spawn(async move {
            server.listen().await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let resp = ipc_request(
            &sock,
            &IpcRequest::Shutdown {
                reason: "user requested".to_string(),
            },
        )
        .await
        .unwrap();

        assert!(matches!(resp, IpcResponse::Ok));

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_connection_to_nonexistent_socket_fails() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("nonexistent.sock");

        let result = ipc_request(&sock, &IpcRequest::Ping).await;
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("failed to connect"),
            "unexpected error: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_multiple_sequential_requests() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("test.sock");

        let handler: Arc<dyn IpcHandler + Send + Sync> = Arc::new(EchoHandler);
        let server = IpcServer::new(sock.clone(), handler);

        let server_handle = tokio::spawn(async move {
            server.listen().await.unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Multiple requests on sequential connections
        for _ in 0..5 {
            let resp = ipc_request(&sock, &IpcRequest::Ping).await.unwrap();
            assert!(matches!(resp, IpcResponse::Pong));
        }

        server_handle.abort();
    }
}
