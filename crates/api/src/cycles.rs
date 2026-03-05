use ckbadger_common::cycles_task::{normalize_tx_hash, CyclesTaskResult};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum CyclesStatus {
    Done,
    Calculating,
    Queued,
    Failed,
    NotFound,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CyclesStatusResponse {
    pub status: CyclesStatus,
    pub cycles: Option<i64>,
    pub error: Option<String>,
}

/// In-memory store for cycles task results, shared between API and indexer worker.
#[derive(Clone)]
pub struct CyclesResultStore {
    results: Arc<Mutex<HashMap<String, CyclesTaskResult>>>,
}

impl CyclesResultStore {
    pub fn new() -> Self {
        Self {
            results: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn get(&self, tx_hash: &str) -> Option<CyclesTaskResult> {
        let normalized = normalize_tx_hash(tx_hash);
        let map = self.results.lock().await;
        map.get(&normalized).cloned()
    }
}

impl Default for CyclesResultStore {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CyclesClient {
    task_tx: Option<mpsc::Sender<String>>,
    result_store: CyclesResultStore,
    wait_timeout: Duration,
    poll_interval: Duration,
}

impl CyclesClient {
    /// Create a new CyclesClient.
    /// If task_tx is None, the cycles feature is disabled.
    pub fn new(
        task_tx: Option<mpsc::Sender<String>>,
        result_store: CyclesResultStore,
    ) -> Arc<Self> {
        Arc::new(Self {
            task_tx,
            result_store,
            wait_timeout: Duration::from_secs(12),
            poll_interval: Duration::from_millis(250),
        })
    }

    /// Create a disabled CyclesClient (no worker connected).
    pub fn disabled() -> Arc<Self> {
        Arc::new(Self {
            task_tx: None,
            result_store: CyclesResultStore::new(),
            wait_timeout: Duration::from_secs(12),
            poll_interval: Duration::from_millis(250),
        })
    }

    pub fn wait_timeout(&self) -> Duration {
        self.wait_timeout
    }

    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    pub fn is_enabled(&self) -> bool {
        self.task_tx.is_some()
    }

    pub async fn enqueue_task(&self, tx_hash: &str) -> Result<(), String> {
        let Some(ref tx) = self.task_tx else {
            return Err("Cycles task dispatch unavailable: worker not connected".to_string());
        };

        let normalized = normalize_tx_hash(tx_hash);
        tx.send(normalized)
            .await
            .map_err(|e| format!("failed to enqueue cycles task: {}", e))
    }

    pub async fn get_task_result(&self, tx_hash: &str) -> Result<Option<CyclesTaskResult>, String> {
        Ok(self.result_store.get(tx_hash).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cycles_client_is_disabled_without_channel() {
        let client = CyclesClient::disabled();
        assert!(!client.is_enabled());
    }

    #[tokio::test]
    async fn test_get_task_result_returns_none_without_data() {
        let client = CyclesClient::disabled();
        let result = client.get_task_result("0x1234").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_enqueue_task_fails_without_channel() {
        let client = CyclesClient::disabled();
        let err = client.enqueue_task("0x1234").await.unwrap_err();
        assert!(err.contains("worker not connected"));
    }

    #[tokio::test]
    async fn test_enqueue_task_succeeds_with_channel() {
        let (tx, mut rx) = mpsc::channel(10);
        let client = CyclesClient::new(Some(tx), CyclesResultStore::new());
        assert!(client.is_enabled());

        client.enqueue_task("0xABCD").await.unwrap();
        let received = rx.recv().await.unwrap();
        assert_eq!(received, "0xabcd");
    }
}
