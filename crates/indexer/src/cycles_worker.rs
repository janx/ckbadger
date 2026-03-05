use ckbadger_common::cycles_task::{normalize_tx_hash, CyclesTaskResult, CyclesTaskStatus};
use ckbadger_store::CkbadgerStore;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::info;

/// In-memory store for cycles task results, shared between worker and API.
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

    pub async fn insert(&self, tx_hash: &str, result: CyclesTaskResult) {
        let normalized = normalize_tx_hash(tx_hash);
        let mut map = self.results.lock().await;
        map.insert(normalized, result);
        // Evict old entries if the map grows too large
        if map.len() > 10_000 {
            let now = chrono::Utc::now().timestamp();
            map.retain(|_, v| now - v.updated_at < 300);
        }
    }
}

impl Default for CyclesResultStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn the cycles task worker that processes tasks from an mpsc channel.
/// Returns the sender and result store for the API to use.
pub fn spawn_cycles_task_worker(
    store: Arc<CkbadgerStore>,
    ckb_rpc_url: String,
) -> (mpsc::Sender<String>, CyclesResultStore) {
    let (tx, rx) = mpsc::channel::<String>(256);
    let result_store = CyclesResultStore::new();
    let worker_result_store = result_store.clone();

    tokio::spawn(async move {
        run_cycles_task_worker(store, ckb_rpc_url, rx, worker_result_store).await;
    });

    (tx, result_store)
}

async fn run_cycles_task_worker(
    store: Arc<CkbadgerStore>,
    ckb_rpc_url: String,
    mut rx: mpsc::Receiver<String>,
    result_store: CyclesResultStore,
) {
    info!("Cycles task worker started");

    while let Some(tx_hash) = rx.recv().await {
        let normalized = normalize_tx_hash(&tx_hash);
        let result = process_cycles_task(store.as_ref(), &ckb_rpc_url, &normalized).await;
        result_store.insert(&normalized, result).await;
    }

    info!("Cycles task worker stopped (channel closed)");
}

async fn process_cycles_task(
    store: &CkbadgerStore,
    ckb_rpc_url: &str,
    tx_hash: &str,
) -> CyclesTaskResult {
    let updated_at = chrono::Utc::now().timestamp();
    let hash_bytes = match hex::decode(tx_hash.strip_prefix("0x").unwrap_or(tx_hash)) {
        Ok(bytes) => bytes,
        Err(e) => {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!("invalid tx hash: {}", e)),
                updated_at,
            };
        }
    };

    let (block_num, tx_idx, entry) = match store.get_tx_by_hash(&hash_bytes) {
        Ok(Some(row)) => row,
        Ok(None) => {
            return CyclesTaskResult {
                status: CyclesTaskStatus::NotFound,
                cycles: None,
                error: Some("transaction not found".to_string()),
                updated_at,
            };
        }
        Err(e) => {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!("store read failed: {}", e)),
                updated_at,
            };
        }
    };

    if entry.is_cellbase {
        if let Err(e) = store.update_tx_cycles(block_num, tx_idx, 0) {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!("failed to persist cellbase cycles: {}", e)),
                updated_at,
            };
        }
        return CyclesTaskResult {
            status: CyclesTaskStatus::Done,
            cycles: Some(0),
            error: None,
            updated_at,
        };
    }

    if let Some(existing) = entry.cycles {
        if existing > 0 {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Done,
                cycles: Some(existing),
                error: None,
                updated_at,
            };
        }
    }

    match ckbadger_common::cycles::calculate_cycles(ckb_rpc_url, tx_hash).await {
        Ok(cycles) => {
            if let Err(e) = store.update_tx_cycles(block_num, tx_idx, cycles) {
                return CyclesTaskResult {
                    status: CyclesTaskStatus::Failed,
                    cycles: None,
                    error: Some(format!("failed to persist calculated cycles: {}", e)),
                    updated_at,
                };
            }
            CyclesTaskResult {
                status: CyclesTaskStatus::Done,
                cycles: Some(cycles),
                error: None,
                updated_at,
            }
        }
        Err(e) => {
            let marker_err = store
                .update_tx_cycles(block_num, tx_idx, 0)
                .err()
                .map(|write_err| format!(" (failed to persist failure marker: {})", write_err))
                .unwrap_or_default();
            CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!("ckb-debugger failed: {}{}", e, marker_err)),
                updated_at,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::{StoreBatch, TxIndexEntry};

    fn insert_tx_with_cycles(store: &CkbadgerStore, tx_hash: &[u8; 32], cycles: Option<i64>) {
        let mut batch = StoreBatch::new(store);
        batch.put_tx_hash_map(tx_hash, 123, 0);
        batch.put_tx_index(
            123,
            0,
            &TxIndexEntry {
                is_cellbase: false,
                timestamp: 1_700_000_000_000,
                inputs_count: 1,
                outputs_count: 1,
                fee: 1_000,
                tx_size: 200,
                cycles,
            },
        );
        batch.commit().unwrap();
    }

    #[tokio::test]
    async fn test_process_cycles_task_invalid_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let result = process_cycles_task(&store, "http://localhost:8114", "0xzz").await;

        assert_eq!(result.status, CyclesTaskStatus::Failed);
        assert!(result.error.unwrap_or_default().contains("invalid tx hash"));
    }

    #[tokio::test]
    async fn test_process_cycles_task_failure_marker_is_retryable() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let tx_hash = [0x11u8; 32];
        insert_tx_with_cycles(&store, &tx_hash, Some(-1));
        let tx_hash_hex = format!("0x{}", hex::encode(tx_hash));

        let result = process_cycles_task(&store, "http://127.0.0.1:1", &tx_hash_hex).await;

        assert_eq!(result.status, CyclesTaskStatus::Failed);
        assert!(!result
            .error
            .unwrap_or_default()
            .contains("calculation previously failed"));

        let (_, _, updated) = store.get_tx_by_hash(&tx_hash).unwrap().unwrap();
        assert_eq!(updated.cycles, Some(0));
    }

    #[tokio::test]
    async fn test_cycles_result_store_roundtrip() {
        let store = CyclesResultStore::new();
        let result = CyclesTaskResult {
            status: CyclesTaskStatus::Done,
            cycles: Some(100),
            error: None,
            updated_at: 1700000000,
        };

        store.insert("0xabc", result.clone()).await;
        let retrieved = store.get("0xabc").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().cycles, Some(100));
    }

    #[tokio::test]
    async fn test_cycles_result_store_returns_none_for_missing() {
        let store = CyclesResultStore::new();
        assert!(store.get("0xmissing").await.is_none());
    }
}
