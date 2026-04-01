use ckbadger_common::cycles_task::{CyclesTaskResult, CyclesTaskStatus};
use ckbadger_common::{BackgroundTaskKind, BackgroundTaskState};
use ckbadger_store::CkbadgerStore;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);
const TASK_NAME: &str = "cycles_calculate";

/// Spawn the cycles worker as a background task that polls a directory for requests.
///
/// Two file name patterns are recognized:
/// - `tx_{hash}` — calculate cycles for one transaction
/// - `blk_{number}` — calculate cycles for all txs in a block, then write block total
pub fn spawn_cycles_worker(store: Arc<CkbadgerStore>, ckb_rpc_url: String, request_dir: PathBuf) {
    tokio::spawn(async move {
        run_cycles_worker(store, ckb_rpc_url, request_dir).await;
    });
}

async fn run_cycles_worker(store: Arc<CkbadgerStore>, ckb_rpc_url: String, request_dir: PathBuf) {
    info!("Cycles worker started, polling {:?}", request_dir);

    // Ensure request directory exists.
    if let Err(e) = tokio::fs::create_dir_all(&request_dir).await {
        warn!(
            "Failed to create cycles request dir {:?}: {}",
            request_dir, e
        );
        return;
    }

    // Register as Waiting.
    let _ = store.update_background_task(TASK_NAME, |entry| {
        entry.kind = BackgroundTaskKind::Job;
        entry.state = BackgroundTaskState::Waiting;
        entry.message = Some("Polling for requests".to_string());
    });

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("failed to build HTTP client for cycles worker");

    loop {
        let requests = read_requests(&request_dir).await;

        if requests.is_empty() {
            tokio::time::sleep(POLL_INTERVAL).await;
            continue;
        }

        let total = requests.len();
        info!("Cycles worker: processing {} request(s)", total);

        let _ = store.update_background_task(TASK_NAME, |entry| {
            entry.kind = BackgroundTaskKind::Job;
            entry.state = BackgroundTaskState::Running;
            entry.progress_current = Some(0);
            entry.progress_total = Some(total as u64);
            entry.message = Some(format!("Processing {} request(s)", total));
        });

        let mut processed = 0u64;
        for request in requests {
            match request {
                CyclesRequest::Tx { hash, path } => {
                    let _result = process_cycles_task(store.as_ref(), &ckb_rpc_url, &hash).await;
                    let _ = tokio::fs::remove_file(&path).await;
                    processed += 1;
                }
                CyclesRequest::Block { number, path } => {
                    let count =
                        process_block_request(store.as_ref(), &ckb_rpc_url, &http_client, number)
                            .await;
                    let _ = tokio::fs::remove_file(&path).await;
                    processed += count;
                }
            }

            let _ = store.update_background_task(TASK_NAME, |entry| {
                entry.progress_current = Some(processed);
            });
        }

        let _ = store.update_background_task(TASK_NAME, |entry| {
            entry.kind = BackgroundTaskKind::Job;
            entry.state = BackgroundTaskState::Waiting;
            entry.progress_current = None;
            entry.progress_total = None;
            entry.message = Some("Polling for requests".to_string());
        });
    }
}

/// A parsed request from the directory.
enum CyclesRequest {
    Tx { hash: String, path: PathBuf },
    Block { number: u64, path: PathBuf },
}

/// Read and categorize request files from the directory.
async fn read_requests(dir: &Path) -> Vec<CyclesRequest> {
    let mut requests = Vec::new();

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Failed to read cycles request dir: {}", e);
            return requests;
        }
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        if let Some(hash) = file_name.strip_prefix("tx_") {
            requests.push(CyclesRequest::Tx {
                hash: hash.to_string(),
                path,
            });
        } else if let Some(num_str) = file_name.strip_prefix("blk_") {
            if let Ok(number) = num_str.parse::<u64>() {
                requests.push(CyclesRequest::Block { number, path });
            } else {
                warn!("Invalid block number in cycles request: {}", file_name);
                let _ = tokio::fs::remove_file(&path).await;
            }
        } else {
            debug!("Ignoring unrecognized cycles request file: {}", file_name);
        }
    }

    requests
}

/// Expand a block request: find txs with missing cycles, calculate each, then write block total.
///
/// Returns the number of individual tx tasks processed.
async fn process_block_request(
    store: &CkbadgerStore,
    ckb_rpc_url: &str,
    http_client: &reqwest::Client,
    block_number: u64,
) -> u64 {
    let block_num = match i64::try_from(block_number) {
        Ok(n) => n,
        Err(_) => {
            warn!(
                "Cycles worker: block_number exceeds i64 range: {}",
                block_number
            );
            return 0;
        }
    };

    // 1. List all txs in this block.
    let block_txs = match store.list_block_txs(block_num) {
        Ok(txs) => txs,
        Err(e) => {
            warn!(
                "Cycles worker: failed to list block txs for block {}: {}",
                block_number, e
            );
            return 0;
        }
    };

    // 2. Find which txs need cycles (cycles == None and not cellbase).
    let missing_indices: Vec<i32> = block_txs
        .iter()
        .filter(|(_, entry)| entry.cycles.is_none() && !entry.is_cellbase)
        .map(|(tx_idx, _)| *tx_idx)
        .collect();

    if missing_indices.is_empty() {
        // All txs already have cycles — compute block total from existing data.
        let any_negative = block_txs
            .iter()
            .any(|(_, entry)| matches!(entry.cycles, Some(c) if c < 0));
        if any_negative {
            warn!(
                "Cycles worker: block {} has failed tx cycles (early exit), skipping block total",
                block_number
            );
        } else {
            let total: i64 = block_txs.iter().filter_map(|(_, entry)| entry.cycles).sum();
            if let Err(e) = store.update_block_cycles(block_num, total) {
                warn!(
                    "Cycles worker: failed to write block cycles for block {}: {}",
                    block_number, e
                );
            }
        }
        return 0;
    }

    // 3. Fetch block from CKB RPC to get tx hashes.
    let tx_hashes = match fetch_block_tx_hashes(http_client, ckb_rpc_url, block_number).await {
        Ok(hashes) => hashes,
        Err(e) => {
            warn!(
                "Cycles worker: failed to fetch block {} tx hashes from RPC: {}",
                block_number, e
            );
            return 0;
        }
    };

    // 4. Calculate cycles for each missing tx.
    let mut all_ok = true;
    let mut count = 0u64;

    for tx_idx in &missing_indices {
        let idx = *tx_idx as usize;
        if idx >= tx_hashes.len() {
            warn!(
                "Cycles worker: tx_idx {} out of range for block {} (has {} txs from RPC)",
                tx_idx,
                block_number,
                tx_hashes.len()
            );
            all_ok = false;
            continue;
        }

        let tx_hash = &tx_hashes[idx];
        let result = process_cycles_task(store, ckb_rpc_url, tx_hash).await;
        count += 1;

        if result.status == CyclesTaskStatus::Failed {
            all_ok = false;
        }
    }

    // 5. If all txs succeeded, write block total.
    if all_ok {
        // Re-read to get updated cycles values.
        match store.list_block_txs(block_num) {
            Ok(updated_txs) => {
                let any_negative = updated_txs
                    .iter()
                    .any(|(_, entry)| matches!(entry.cycles, Some(c) if c < 0));

                if any_negative {
                    warn!(
                        "Cycles worker: block {} has failed tx cycles, skipping block total",
                        block_number
                    );
                } else {
                    let total: i64 = updated_txs
                        .iter()
                        .filter_map(|(_, entry)| entry.cycles)
                        .sum();
                    if let Err(e) = store.update_block_cycles(block_num, total) {
                        warn!(
                            "Cycles worker: failed to write block cycles for block {}: {}",
                            block_number, e
                        );
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Cycles worker: failed to re-read block txs for block {}: {}",
                    block_number, e
                );
            }
        }
    }

    count
}

/// Fetch tx hashes for a block from the CKB RPC node.
///
/// Uses verbosity `0x1` (hex format) and `with_cycles=false` since we only need hashes.
async fn fetch_block_tx_hashes(
    client: &reqwest::Client,
    rpc_url: &str,
    block_number: u64,
) -> anyhow::Result<Vec<String>> {
    let hex_number = format!("0x{:x}", block_number);
    let body = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "get_block_by_number",
        "params": [hex_number, "0x2", false]
    });

    let resp = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("RPC request failed: {}", e))?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("RPC response parse failed: {}", e))?;

    if let Some(err) = json.get("error") {
        return Err(anyhow::anyhow!("RPC error: {}", err));
    }

    // Verbosity 0x2 returns { header, transactions, uncles, proposals } at result level
    let txs = json["result"]["transactions"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("RPC response missing transactions array"))?;

    let hashes: Vec<String> = txs
        .iter()
        .map(|tx| tx["hash"].as_str().unwrap_or_default().to_string())
        .collect();

    Ok(hashes)
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
                .update_tx_cycles(block_num, tx_idx, -1)
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
                semantic_tags: 0,
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
        assert_eq!(updated.cycles, Some(-1));
    }

    #[tokio::test]
    async fn test_read_requests_parses_tx_and_blk_files() {
        let dir = tempfile::tempdir().unwrap();
        let req_dir = dir.path().join("requests");
        tokio::fs::create_dir_all(&req_dir).await.unwrap();

        // Create tx request file
        tokio::fs::write(req_dir.join("tx_0xabc123"), "")
            .await
            .unwrap();
        // Create block request file
        tokio::fs::write(req_dir.join("blk_12345"), "")
            .await
            .unwrap();
        // Create unrecognized file (should be ignored)
        tokio::fs::write(req_dir.join("unknown_file"), "")
            .await
            .unwrap();

        let requests = read_requests(&req_dir).await;
        assert_eq!(requests.len(), 2);

        let has_tx = requests
            .iter()
            .any(|r| matches!(r, CyclesRequest::Tx { hash, .. } if hash == "0xabc123"));
        let has_blk = requests
            .iter()
            .any(|r| matches!(r, CyclesRequest::Block { number: 12345, .. }));
        assert!(has_tx, "should find tx request");
        assert!(has_blk, "should find block request");
    }

    #[tokio::test]
    async fn test_read_requests_removes_invalid_blk_files() {
        let dir = tempfile::tempdir().unwrap();
        let req_dir = dir.path().join("requests");
        tokio::fs::create_dir_all(&req_dir).await.unwrap();

        // Invalid block number
        tokio::fs::write(req_dir.join("blk_notanumber"), "")
            .await
            .unwrap();

        let requests = read_requests(&req_dir).await;
        assert!(requests.is_empty());

        // File should have been removed
        assert!(!req_dir.join("blk_notanumber").exists());
    }

    #[tokio::test]
    async fn test_read_requests_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let req_dir = dir.path().join("requests");
        tokio::fs::create_dir_all(&req_dir).await.unwrap();

        let requests = read_requests(&req_dir).await;
        assert!(requests.is_empty());
    }
}
