//! IPC message protocol types.
//!
//! All messages are JSON-serializable with a `type` tag for enum dispatch.

use serde::{Deserialize, Serialize};

/// Request sent from client to supervisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcRequest {
    /// Health check.
    Ping,
    /// Query status of all managed services.
    GetServiceStatus,
    /// Request graceful shutdown of all services.
    Shutdown { reason: String },
}

/// Response sent from supervisor to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcResponse {
    /// Response to Ping.
    Pong,
    /// Generic success.
    Ok,
    /// Status of all managed services.
    ServiceStatus { services: Vec<ServiceInfo> },
    /// Error response.
    Error { message: String },
}

/// Information about a single managed service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    /// Service name (e.g. "indexer", "api").
    pub name: String,
    /// OS process ID.
    pub pid: u32,
    /// Current status.
    pub status: ServiceStatus,
    /// Seconds since the service was (re)started.
    pub uptime_secs: u64,
}

/// Status of a managed service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceStatus {
    /// Service is running normally.
    Running,
    /// Service has stopped (exited).
    Stopped,
    /// Service is being restarted after a crash.
    Restarting,
    /// Service hit a persistent state that requires operator action.
    Blocked,
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
            Self::Restarting => write!(f, "restarting"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_ping_serialization() {
        let req = IpcRequest::Ping;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"Ping\""));

        let parsed: IpcRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, IpcRequest::Ping));
    }

    #[test]
    fn test_request_shutdown_serialization() {
        let req = IpcRequest::Shutdown {
            reason: "test".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"Shutdown\""));
        assert!(json.contains("\"reason\":\"test\""));

        let parsed: IpcRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            IpcRequest::Shutdown { reason } => assert_eq!(reason, "test"),
            _ => panic!("expected Shutdown"),
        }
    }

    #[test]
    fn test_request_get_service_status_serialization() {
        let req = IpcRequest::GetServiceStatus;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: IpcRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, IpcRequest::GetServiceStatus));
    }

    #[test]
    fn test_response_pong_serialization() {
        let resp = IpcResponse::Pong;
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: IpcResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, IpcResponse::Pong));
    }

    #[test]
    fn test_response_ok_serialization() {
        let resp = IpcResponse::Ok;
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: IpcResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, IpcResponse::Ok));
    }

    #[test]
    fn test_response_error_serialization() {
        let resp = IpcResponse::Error {
            message: "something went wrong".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: IpcResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            IpcResponse::Error { message } => assert_eq!(message, "something went wrong"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_response_service_status_serialization() {
        let resp = IpcResponse::ServiceStatus {
            services: vec![
                ServiceInfo {
                    name: "indexer".to_string(),
                    pid: 1000,
                    status: ServiceStatus::Running,
                    uptime_secs: 120,
                },
                ServiceInfo {
                    name: "api".to_string(),
                    pid: 1001,
                    status: ServiceStatus::Stopped,
                    uptime_secs: 0,
                },
                ServiceInfo {
                    name: "testnet/indexer".to_string(),
                    pid: 0,
                    status: ServiceStatus::Blocked,
                    uptime_secs: 0,
                },
            ],
        };

        let json = serde_json::to_string(&resp).unwrap();
        let parsed: IpcResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            IpcResponse::ServiceStatus { services } => {
                assert_eq!(services.len(), 3);
                assert_eq!(services[0].name, "indexer");
                assert_eq!(services[0].pid, 1000);
                assert_eq!(services[0].status, ServiceStatus::Running);
                assert_eq!(services[0].uptime_secs, 120);
                assert_eq!(services[1].name, "api");
                assert_eq!(services[1].status, ServiceStatus::Stopped);
                assert_eq!(services[2].name, "testnet/indexer");
                assert_eq!(services[2].status, ServiceStatus::Blocked);
            }
            _ => panic!("expected ServiceStatus"),
        }
    }

    #[test]
    fn test_service_status_display() {
        assert_eq!(format!("{}", ServiceStatus::Running), "running");
        assert_eq!(format!("{}", ServiceStatus::Stopped), "stopped");
        assert_eq!(format!("{}", ServiceStatus::Restarting), "restarting");
        assert_eq!(format!("{}", ServiceStatus::Blocked), "blocked");
    }
}
