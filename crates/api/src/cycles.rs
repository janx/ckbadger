use ckbadger_common::cycles_task::{
    cycles_task_lock_key, cycles_task_result_key, normalize_tx_hash, CyclesTaskResult,
    CYCLES_TASK_LOCK_TTL_SECS, CYCLES_TASK_QUEUE_KEY,
};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

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
    conn: Option<ConnectionManager>,
    wait_timeout: Duration,
    poll_interval: Duration,
}

impl CyclesClient {
    pub async fn new(redis_url: Option<&str>) -> Arc<Self> {
        let conn = if let Some(url) = redis_url {
            match redis::Client::open(url) {
                Ok(client) => match redis::aio::ConnectionManager::new(client).await {
                    Ok(conn) => {
                        info!("Connected to Redis for cycles task dispatch");
                        Some(conn)
                    }
                    Err(e) => {
                        warn!("Failed to connect Redis for cycles task dispatch: {}", e);
                        None
                    }
                },
                Err(e) => {
                    warn!("Invalid Redis URL for cycles task dispatch: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Arc::new(Self {
            conn,
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
        self.conn.is_some()
    }

    pub async fn enqueue_task(&self, tx_hash: &str) -> Result<(), String> {
        let Some(conn) = self.conn.as_ref() else {
            return Err("Cycles task dispatch unavailable: Redis is not configured".to_string());
        };

        let normalized = normalize_tx_hash(tx_hash);
        let lock_key = cycles_task_lock_key(&normalized);

        let mut conn = conn.clone();
        let lock_result: Option<String> = redis::cmd("SET")
            .arg(&lock_key)
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(CYCLES_TASK_LOCK_TTL_SECS)
            .query_async(&mut conn)
            .await
            .map_err(|e| format!("failed to acquire cycles task lock: {}", e))?;

        if lock_result.is_some() {
            let enqueue_result: Result<(), redis::RedisError> = redis::cmd("LPUSH")
                .arg(CYCLES_TASK_QUEUE_KEY)
                .arg(&normalized)
                .query_async(&mut conn)
                .await;

            if let Err(e) = enqueue_result {
                let _: Result<(), _> = redis::cmd("DEL")
                    .arg(&lock_key)
                    .query_async(&mut conn)
                    .await;
                return Err(format!("failed to enqueue cycles task: {}", e));
            }
        }

        Ok(())
    }

    pub async fn get_task_result(&self, tx_hash: &str) -> Result<Option<CyclesTaskResult>, String> {
        let Some(conn) = self.conn.as_ref() else {
            return Ok(None);
        };

        let result_key = cycles_task_result_key(tx_hash);
        let mut conn = conn.clone();
        let raw: Option<String> = conn
            .get(&result_key)
            .await
            .map_err(|e| format!("failed to read cycles task result: {}", e))?;

        raw.map(|json| {
            serde_json::from_str::<CyclesTaskResult>(&json)
                .map_err(|e| format!("invalid cycles task result payload: {}", e))
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cycles_client_is_disabled_without_redis() {
        let client = CyclesClient::new(None).await;
        assert!(!client.is_enabled());
    }

    #[tokio::test]
    async fn test_get_task_result_returns_none_without_redis() {
        let client = CyclesClient::new(None).await;
        let result = client.get_task_result("0x1234").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_enqueue_task_fails_without_redis() {
        let client = CyclesClient::new(None).await;
        let err = client.enqueue_task("0x1234").await.unwrap_err();
        assert!(err.contains("Redis is not configured"));
    }
}
