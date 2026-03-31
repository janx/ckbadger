use ckbadger_common::cycles_task::CyclesTaskResult;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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

pub struct CyclesClient {
    request_dir: Option<PathBuf>,
    wait_timeout: Duration,
    poll_interval: Duration,
}

impl CyclesClient {
    pub fn new(request_dir: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            request_dir,
            wait_timeout: Duration::from_secs(12),
            poll_interval: Duration::from_millis(250),
        })
    }

    pub fn disabled() -> Arc<Self> {
        Arc::new(Self {
            request_dir: None,
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
        self.request_dir.is_some()
    }

    pub async fn enqueue_task(&self, tx_hash: &str) -> Result<(), String> {
        let dir = self
            .request_dir
            .as_ref()
            .ok_or("Cycles request dir not configured")?;
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| format!("create dir: {}", e))?;
        let file_path = dir.join(format!("tx_{}", tx_hash));
        tokio::fs::write(&file_path, b"")
            .await
            .map_err(|e| format!("write request: {}", e))
    }

    pub async fn enqueue_block(&self, block_number: i64) -> Result<(), String> {
        let dir = self
            .request_dir
            .as_ref()
            .ok_or("Cycles request dir not configured")?;
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| format!("create dir: {}", e))?;
        let file_path = dir.join(format!("blk_{}", block_number));
        tokio::fs::write(&file_path, b"")
            .await
            .map_err(|e| format!("write request: {}", e))
    }

    /// No in-memory result store — API polls DB directly.
    pub async fn get_task_result(
        &self,
        _tx_hash: &str,
    ) -> Result<Option<CyclesTaskResult>, String> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_disabled_client() {
        let client = CyclesClient::disabled();
        assert!(!client.is_enabled());
        assert!(client.enqueue_task("0x1234").await.is_err());
        assert!(client.enqueue_block(100).await.is_err());
    }

    #[tokio::test]
    async fn test_writes_tx_request_file() {
        let dir = tempfile::tempdir().unwrap();
        let client = CyclesClient::new(Some(dir.path().to_path_buf()));
        client.enqueue_task("0xabcd").await.unwrap();
        assert!(dir.path().join("tx_0xabcd").exists());
    }

    #[tokio::test]
    async fn test_writes_blk_request_file() {
        let dir = tempfile::tempdir().unwrap();
        let client = CyclesClient::new(Some(dir.path().to_path_buf()));
        client.enqueue_block(18974).await.unwrap();
        assert!(dir.path().join("blk_18974").exists());
    }
}
