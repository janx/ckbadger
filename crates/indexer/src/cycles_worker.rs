use ckbadger_store::CkbadgerStore;
use std::sync::Arc;
use tracing::warn;

#[cfg(feature = "redis-cache")]
use ckbadger_common::cycles_task::{
    cycles_task_lock_key, cycles_task_result_key, normalize_tx_hash, CYCLES_TASK_QUEUE_KEY,
    CYCLES_TASK_RESULT_TTL_SECS,
};
#[cfg(any(feature = "redis-cache", test))]
use ckbadger_common::cycles_task::{CyclesTaskResult, CyclesTaskStatus};
#[cfg(feature = "redis-cache")]
use tracing::{error, info};

#[cfg(feature = "redis-cache")]
use redis::aio::ConnectionManager;

pub fn spawn_cycles_task_worker(
    store: Arc<CkbadgerStore>,
    ckb_rpc_url: String,
    redis_url: Option<String>,
) {
    #[cfg(feature = "redis-cache")]
    {
        let Some(redis_url) = redis_url else {
            warn!("Cycles task worker disabled: REDIS_URL is not configured");
            return;
        };

        tokio::spawn(async move {
            run_cycles_task_worker(store, ckb_rpc_url, redis_url).await;
        });
    }

    #[cfg(not(feature = "redis-cache"))]
    {
        let _ = (store, ckb_rpc_url, redis_url);
        warn!("Cycles task worker disabled: indexer built without redis-cache feature");
    }
}

#[cfg(feature = "redis-cache")]
async fn run_cycles_task_worker(store: Arc<CkbadgerStore>, ckb_rpc_url: String, redis_url: String) {
    let client = match redis::Client::open(redis_url.clone()) {
        Ok(client) => client,
        Err(e) => {
            error!("Cycles task worker failed to open Redis client: {}", e);
            return;
        }
    };

    let mut conn = match ConnectionManager::new(client).await {
        Ok(conn) => conn,
        Err(e) => {
            error!("Cycles task worker failed to connect Redis: {}", e);
            return;
        }
    };

    info!("Cycles task worker started");

    loop {
        let popped: Result<Option<(String, String)>, _> = redis::cmd("BRPOP")
            .arg(CYCLES_TASK_QUEUE_KEY)
            .arg(1)
            .query_async(&mut conn)
            .await;

        let popped_item = match popped {
            Ok(item) => item,
            Err(e) => {
                warn!("Cycles task worker BRPOP failed: {}", e);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        let Some((_key, tx_hash)) = popped_item else {
            continue;
        };

        let normalized = normalize_tx_hash(&tx_hash);
        let result = process_cycles_task(store.as_ref(), &ckb_rpc_url, &normalized).await;
        if let Err(e) = publish_task_result(&mut conn, &normalized, &result).await {
            warn!(
                "Failed to publish cycles task result for {}: {}",
                normalized, e
            );
        }

        if let Err(e) = release_task_lock(&mut conn, &normalized).await {
            warn!(
                "Failed to release cycles task lock for {}: {}",
                normalized, e
            );
        }
    }
}

#[cfg(feature = "redis-cache")]
async fn release_task_lock(conn: &mut ConnectionManager, tx_hash: &str) -> Result<(), String> {
    let lock_key = cycles_task_lock_key(tx_hash);
    let _: () = redis::cmd("DEL")
        .arg(lock_key)
        .query_async(conn)
        .await
        .map_err(|e| format!("redis DEL lock failed: {}", e))?;
    Ok(())
}

#[cfg(feature = "redis-cache")]
async fn publish_task_result(
    conn: &mut ConnectionManager,
    tx_hash: &str,
    result: &CyclesTaskResult,
) -> Result<(), String> {
    let result_key = cycles_task_result_key(tx_hash);
    let payload = serde_json::to_string(result)
        .map_err(|e| format!("serialize result payload failed: {}", e))?;

    let _: () = redis::cmd("SET")
        .arg(result_key)
        .arg(payload)
        .arg("EX")
        .arg(CYCLES_TASK_RESULT_TTL_SECS)
        .query_async(conn)
        .await
        .map_err(|e| format!("redis SET result failed: {}", e))?;

    Ok(())
}

#[cfg(any(feature = "redis-cache", test))]
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
        if existing == -1 {
            return CyclesTaskResult {
                status: CyclesTaskStatus::Failed,
                cycles: None,
                error: Some("calculation previously failed".to_string()),
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

    #[tokio::test]
    async fn test_process_cycles_task_invalid_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let result = process_cycles_task(&store, "http://localhost:8114", "0xzz").await;

        assert_eq!(result.status, CyclesTaskStatus::Failed);
        assert!(result.error.unwrap_or_default().contains("invalid tx hash"));
    }
}
