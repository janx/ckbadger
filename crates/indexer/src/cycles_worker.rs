use ckbadger_common::cycles_task::{CyclesTaskResult, CyclesTaskStatus, TxGroupCyclesRecord};
use ckbadger_common::{BackgroundTaskKind, BackgroundTaskState};
use ckbadger_store::{CkbadgerStore, TxCyclesWrite};
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
        entry.kind = BackgroundTaskKind::Watcher;
        entry.state = BackgroundTaskState::Waiting;
        entry.message = Some("Polling for requests".to_string());
    });

    loop {
        let requests = read_requests(&request_dir).await;

        if requests.is_empty() {
            tokio::time::sleep(POLL_INTERVAL).await;
            continue;
        }

        let total = requests.len();
        info!("Cycles worker: processing {} request(s)", total);

        let _ = store.update_background_task(TASK_NAME, |entry| {
            entry.kind = BackgroundTaskKind::Watcher;
            entry.state = BackgroundTaskState::Running;
            entry.progress_current = Some(0);
            entry.progress_total = Some(total as u64);
            entry.message = Some(format!("Processing {} request(s)", total));
        });

        let mut processed = 0u64;
        for request in requests {
            match request {
                CyclesRequest::Tx { hash, path } => {
                    let result = process_cycles_task(store.as_ref(), &ckb_rpc_url, &hash).await;
                    if result.status == CyclesTaskStatus::Failed {
                        warn!(
                            "Cycles worker: tx {hash} group enrichment failed: {:?}",
                            result.error
                        );
                    }
                    let _ = tokio::fs::remove_file(&path).await;
                    processed += 1;
                }
                CyclesRequest::Block { number, path } => {
                    let count = process_block_request(store.as_ref(), &ckb_rpc_url, number).await;
                    let _ = tokio::fs::remove_file(&path).await;
                    processed += count;
                }
            }

            let _ = store.update_background_task(TASK_NAME, |entry| {
                entry.progress_current = Some(processed);
            });
        }

        let _ = store.update_background_task(TASK_NAME, |entry| {
            entry.kind = BackgroundTaskKind::Watcher;
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

/// Expand a block request using the persisted canonical transaction identities.
/// Returns the number of individual tx tasks processed.
async fn process_block_request(store: &CkbadgerStore, ckb_rpc_url: &str, block_number: u64) -> u64 {
    let block_num = match i64::try_from(block_number) {
        Ok(number) => number,
        Err(_) => {
            warn!(
                "Cycles worker: block_number exceeds i64 range: {}",
                block_number
            );
            return 0;
        }
    };
    let block_header = match store.get_block_header(block_num) {
        Ok(Some(header)) => header,
        Ok(None) => {
            warn!("Cycles worker: canonical block header missing: block={block_number}");
            return 0;
        }
        Err(error) => {
            warn!("Cycles worker: failed to read block {block_number} header: {error}");
            return 0;
        }
    };
    let block_txs = match store.list_block_txs(block_num) {
        Ok(txs) => txs,
        Err(error) => {
            warn!("Cycles worker: failed to list block {block_number} txs: {error}");
            return 0;
        }
    };
    let expected_txs: Vec<(i32, [u8; 32])> = block_txs
        .iter()
        .map(|(index, entry)| (*index, entry.tx_hash))
        .collect();
    let mut processed = 0;
    let mut all_ok = true;
    for (_, entry) in &block_txs {
        if entry.is_cellbase {
            continue;
        }
        if block_num != 0 {
            match store.get_tx_group_cycles_record(&entry.tx_hash) {
                Ok(Some(TxGroupCyclesRecord::Complete(_))) => continue,
                Ok(Some(TxGroupCyclesRecord::Failed { .. })) | Ok(None) => {}
                Err(error) => {
                    warn!("Cycles worker: invalid group result: {error}");
                    all_ok = false;
                    continue;
                }
            }
        }
        let hash = format!("0x{}", hex::encode(entry.tx_hash));
        let result = process_cycles_task(store, ckb_rpc_url, &hash).await;
        processed += 1;
        if result.status != CyclesTaskStatus::Done {
            all_ok = false;
            warn!(
                "Cycles worker: block {block_number} tx {hash} cycles unavailable: {:?}",
                result.error
            );
        }
    }
    if all_ok {
        if let Err(error) =
            store.update_block_cycles_if_complete(block_num, &block_header.hash, &expected_txs)
        {
            warn!(
                "Cycles worker: cannot publish complete cycles for block {block_number}: {error}"
            );
        }
    }
    processed
}

async fn process_cycles_task(
    store: &CkbadgerStore,
    ckb_rpc_url: &str,
    tx_hash: &str,
) -> CyclesTaskResult {
    let updated_at = chrono::Utc::now().timestamp();
    let hash_bytes: [u8; 32] = match hex::decode(tx_hash.strip_prefix("0x").unwrap_or(tx_hash)) {
        Ok(bytes) if bytes.len() == 32 => bytes.try_into().expect("validated length"),
        Ok(bytes) => {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!("invalid tx hash length: {}", bytes.len())),
                updated_at,
            };
        }
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

    if entry.is_cellbase || block_num == 0 {
        if entry.cycles.is_some_and(|cycles| cycles != 0) {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!(
                    "execution-exempt transaction 0x{} at block {block_num}:{tx_idx} has nonzero stored cycles {:?}",
                    hex::encode(hash_bytes),
                    entry.cycles,
                )),
                updated_at,
            };
        }
        if let Err(e) = store.persist_tx_cycles(
            block_num,
            tx_idx,
            &hash_bytes,
            TxCyclesWrite::ExecutionExempt,
        ) {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!("failed to persist execution-exempt cycles: {}", e)),
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

    match store.get_tx_cycles_result(&hash_bytes) {
        Ok(Some(result)) => {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Done,
                cycles: Some(result.cycles),
                error: None,
                updated_at,
            };
        }
        Ok(None) => {}
        Err(error) => {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!("invalid persisted group cycles: {error}")),
                updated_at,
            }
        }
    }

    let block_hash: [u8; 32] = match store.get_block_header(block_num) {
        Ok(Some(header)) => match header.hash.as_slice().try_into() {
            Ok(hash) => hash,
            Err(_) => {
                return CyclesTaskResult {
                    status: CyclesTaskStatus::Failed,
                    cycles: None,
                    error: Some(format!(
                        "invalid canonical block hash length at block {block_num}: {}",
                        header.hash.len()
                    )),
                    updated_at,
                }
            }
        },
        Ok(None) => {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!(
                    "missing canonical block header at block {block_num}"
                )),
                updated_at,
            }
        }
        Err(error) => {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!(
                    "failed to read canonical block header at block {block_num}: {error}"
                )),
                updated_at,
            }
        }
    };

    match ckbadger_common::cycles::calculate_cycles(ckb_rpc_url, tx_hash).await {
        Ok(cycles) => {
            if let Err(e) = store.persist_tx_cycles(
                block_num,
                tx_idx,
                &hash_bytes,
                TxCyclesWrite::Groups(&cycles),
            ) {
                return CyclesTaskResult {
                    status: CyclesTaskStatus::Failed,
                    cycles: None,
                    error: Some(format!("failed to persist calculated cycles: {}", e)),
                    updated_at,
                };
            }
            CyclesTaskResult {
                status: CyclesTaskStatus::Done,
                cycles: Some(cycles.cycles),
                error: None,
                updated_at,
            }
        }
        Err(e) => {
            let marker_err = store
                .persist_tx_cycles(
                    block_num,
                    tx_idx,
                    &hash_bytes,
                    TxCyclesWrite::ReplayFailed {
                        block_hash,
                        error: &e,
                    },
                )
                .err()
                .map(|err| format!(" (failed to persist failure marker: {err})"))
                .unwrap_or_default();
            CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some(format!(
                    "CKB script verification failed: {}{}",
                    e, marker_err
                )),
                updated_at,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::{CachedBlockHeader, StoreBatch, TxIndexEntry};

    #[tokio::test]
    async fn genesis_non_cellbase_is_persisted_as_zero_without_vm_execution() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let hash = [0x68; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_hash_map(&hash, 0, 1);
        batch.put_tx_index(
            0,
            1,
            &TxIndexEntry {
                tx_hash: hash,
                is_cellbase: false,
                timestamp: 0,
                inputs_count: 1,
                outputs_count: 1,
                fee: 0,
                tx_size: 200,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.commit().unwrap();
        let result = process_cycles_task(
            &store,
            "http://127.0.0.1:1",
            &format!("0x{}", hex::encode(hash)),
        )
        .await;
        assert_eq!(result.status, CyclesTaskStatus::Done, "{result:?}");
        assert_eq!(result.cycles, Some(0));
        assert_eq!(store.get_tx_index(0, 1).unwrap().unwrap().cycles, Some(0));
        assert!(store.get_tx_group_cycles_record(&hash).unwrap().is_none());
        let mut batch = StoreBatch::new(&store);
        let mut entry = store.get_tx_index(0, 1).unwrap().unwrap();
        entry.cycles = Some(5);
        batch.put_tx_index(0, 1, &entry);
        batch.commit().unwrap();
        let invalid = process_cycles_task(
            &store,
            "http://127.0.0.1:1",
            &format!("0x{}", hex::encode(hash)),
        )
        .await;
        assert_eq!(invalid.status, CyclesTaskStatus::Failed, "{invalid:?}");
        assert!(invalid
            .error
            .as_deref()
            .unwrap()
            .contains("execution-exempt"));
        assert_eq!(store.get_tx_index(0, 1).unwrap().unwrap().cycles, Some(5));
    }

    #[tokio::test]
    async fn genesis_block_request_publishes_zero_and_rejects_cached_positive_groups() {
        use ckbadger_common::cycles_task::{ScriptGroupCycles, ScriptGroupKind, TransactionCycles};
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let header = CachedBlockHeader {
            hash: vec![0x69; 32],
            parent_hash: vec![0; 32],
            timestamp: 1,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 2,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1,
        };
        let cellbase_hash = [0x70; 32];
        let hash = [0x71; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &header);
        for (index, hash, is_cellbase) in [(0, cellbase_hash, true), (1, hash, false)] {
            batch.put_tx_hash_map(&hash, 0, index);
            batch.put_tx_index(
                0,
                index,
                &TxIndexEntry {
                    tx_hash: hash,
                    is_cellbase,
                    timestamp: 1,
                    inputs_count: 1,
                    outputs_count: 1,
                    fee: 0,
                    tx_size: 200,
                    cycles: None,
                    semantic_tags: 0,
                },
            );
        }
        batch.commit().unwrap();
        assert_eq!(
            process_block_request(&store, "http://127.0.0.1:1", 0).await,
            1
        );
        assert_eq!(store.get_block_header(0).unwrap().unwrap().cycles, Some(0));
        assert_eq!(store.get_tx_index(0, 1).unwrap().unwrap().cycles, Some(0));
        assert!(store.get_tx_group_cycles_record(&hash).unwrap().is_none());
        store
            .persist_tx_cycles(
                0,
                1,
                &hash,
                TxCyclesWrite::Groups(&TransactionCycles {
                    block_hash: [0x69; 32],
                    cycles: 5,
                    groups: vec![ScriptGroupCycles {
                        script_hash: [0x72; 32],
                        group_type: ScriptGroupKind::Lock,
                        cycles: 5,
                    }],
                }),
            )
            .unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &header);
        batch.commit().unwrap();
        assert_eq!(
            process_block_request(&store, "http://127.0.0.1:1", 0).await,
            1,
            "genesis must not accept cached execution groups"
        );
        assert_eq!(store.get_block_header(0).unwrap().unwrap().cycles, None);
        assert_eq!(store.get_tx_index(0, 1).unwrap().unwrap().cycles, Some(5));
    }

    #[tokio::test]
    async fn block_request_does_not_publish_zero_when_canonical_tx_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let hash = [0x44; 32];
        let header = CachedBlockHeader {
            hash: vec![0x42; 32],
            parent_hash: vec![0; 32],
            timestamp: 1,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: 1,
        };
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(123, &header);
        batch.put_tx_index(
            123,
            0,
            &TxIndexEntry {
                tx_hash: hash,
                is_cellbase: false,
                timestamp: 1,
                inputs_count: 1,
                outputs_count: 1,
                fee: 0,
                tx_size: 1,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.commit().unwrap();

        assert_eq!(
            process_block_request(&store, "http://127.0.0.1:1", 123).await,
            1
        );
        assert_eq!(store.get_block_header(123).unwrap().unwrap().cycles, None);
    }

    fn insert_tx_with_cycles(store: &CkbadgerStore, tx_hash: &[u8; 32], cycles: Option<i64>) {
        let mut batch = StoreBatch::new(store);
        batch.put_block_header(
            123,
            &CachedBlockHeader {
                hash: vec![0x77; 32],
                parent_hash: vec![0; 32],
                timestamp: 1,
                epoch_number: 1,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: 1,
            },
        );
        batch.put_tx_hash_map(tx_hash, 123, 0);
        batch.put_tx_index(
            123,
            0,
            &TxIndexEntry {
                tx_hash: *tx_hash,
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
    async fn failed_group_enrichment_preserves_authoritative_scalar_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let hash = [0x56; 32];
        insert_tx_with_cycles(&store, &hash, Some(42));
        let result = process_cycles_task(
            &store,
            "http://127.0.0.1:1",
            &format!("0x{}", hex::encode(hash)),
        )
        .await;

        assert_eq!(result.status, CyclesTaskStatus::Failed);
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("CKB script verification failed"));
        assert_eq!(
            store.get_tx_index(123, 0).unwrap().unwrap().cycles,
            Some(42)
        );
        assert!(matches!(
            store.get_tx_group_cycles_record(&hash).unwrap(),
            Some(TxGroupCyclesRecord::Failed { block_hash, .. }) if block_hash == [0x77; 32]
        ));
    }

    #[tokio::test]
    async fn cached_group_result_is_reused_and_total_only_result_is_recalculated() {
        use ckbadger_common::cycles_task::{ScriptGroupCycles, ScriptGroupKind, TransactionCycles};
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let hash = [0x55; 32];
        insert_tx_with_cycles(&store, &hash, None);
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(
            123,
            &CachedBlockHeader {
                hash: vec![0x77; 32],
                parent_hash: vec![0; 32],
                timestamp: 1,
                epoch_number: 1,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: 1,
            },
        );
        batch.commit().unwrap();
        store
            .persist_tx_cycles(
                123,
                0,
                &hash,
                TxCyclesWrite::Groups(&TransactionCycles {
                    block_hash: [0x77; 32],
                    cycles: 11,
                    groups: vec![ScriptGroupCycles {
                        script_hash: [3; 32],
                        group_type: ScriptGroupKind::Lock,
                        cycles: 11,
                    }],
                }),
            )
            .unwrap();
        let tx_hash = format!("0x{}", hex::encode(hash));
        let cached = process_cycles_task(&store, "http://127.0.0.1:1", &tx_hash).await;
        assert_eq!(cached.status, CyclesTaskStatus::Done);
        assert_eq!(cached.cycles, Some(11));
        // A pre-existing scalar cannot masquerade as a complete group result.
        let mut batch = StoreBatch::new(&store);
        let mut entry = store.get_tx_index(123, 0).unwrap().unwrap();
        entry.cycles = Some(12);
        batch.put_tx_index(123, 0, &entry);
        batch.commit().unwrap();
        store
            .delete_cf(
                store.cf_sync_meta(),
                &ckbadger_store::keys::encode_tx_group_cycles_key(&hash),
            )
            .unwrap();
        let retry = process_cycles_task(&store, "http://127.0.0.1:1", &tx_hash).await;
        assert_eq!(retry.status, CyclesTaskStatus::Failed);
        assert!(retry
            .error
            .unwrap()
            .contains("CKB script verification failed"));
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
