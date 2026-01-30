use ckbadger_common::SyncProgressData;
#[cfg(feature = "redis-cache")]
use ckbadger_common::SYNC_PROGRESS_REDIS_KEY;
#[cfg(feature = "redis-cache")]
use redis::AsyncCommands;
#[cfg(feature = "redis-cache")]
use tracing::{info, warn};

#[cfg(feature = "redis-cache")]
const CHART_CACHE_KEYS: &[&str] = &[
    "chart:average-block-time",
    "chart:hash-rate",
    "chart:difficulty",
    "chart:uncle-rate",
    "chart:block-time-distribution",
    "chart:epoch-time-distribution",
    "chart:epoch-time-length",
    "chart:miner-address-distribution",
    "chart:total-supply",
    "chart:secondary-issuance",
    "chart:dao-total-deposit",
    "chart:dao-daily-deposit",
    "chart:dao-circulation-ratio",
];

#[derive(Clone)]
pub struct CacheInvalidator {
    #[cfg(feature = "redis-cache")]
    conn: Option<redis::aio::ConnectionManager>,
    #[cfg(not(feature = "redis-cache"))]
    _phantom: std::marker::PhantomData<()>,
}

impl CacheInvalidator {
    #[cfg(feature = "redis-cache")]
    pub async fn new(redis_url: Option<&str>) -> Self {
        let conn = if let Some(url) = redis_url {
            match redis::Client::open(url) {
                Ok(client) => match redis::aio::ConnectionManager::new(client).await {
                    Ok(conn) => {
                        info!("Connected to Redis for cache invalidation");
                        Some(conn)
                    }
                    Err(e) => {
                        warn!(
                            "Failed to connect to Redis: {}. Cache invalidation disabled.",
                            e
                        );
                        None
                    }
                },
                Err(e) => {
                    warn!("Invalid Redis URL: {}. Cache invalidation disabled.", e);
                    None
                }
            }
        } else {
            None
        };
        Self { conn }
    }

    #[cfg(not(feature = "redis-cache"))]
    pub async fn new(_redis_url: Option<&str>) -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }

    pub async fn invalidate_chart_caches(&self) {
        #[cfg(feature = "redis-cache")]
        {
            let Some(ref conn) = self.conn else {
                return;
            };

            let mut conn = conn.clone();
            let mut deleted = 0;

            for key in CHART_CACHE_KEYS {
                match conn.del::<_, i64>(*key).await {
                    Ok(1) => deleted += 1,
                    Ok(_) => {}
                    Err(e) => {
                        warn!("Failed to delete cache key {}: {}", key, e);
                    }
                }
            }

            if deleted > 0 {
                info!("Invalidated {} chart cache entries", deleted);
            }
        }

        #[cfg(not(feature = "redis-cache"))]
        {
            let _ = self;
        }
    }

    pub fn is_enabled(&self) -> bool {
        #[cfg(feature = "redis-cache")]
        {
            self.conn.is_some()
        }
        #[cfg(not(feature = "redis-cache"))]
        {
            false
        }
    }

    pub async fn publish_sync_progress(&self, data: &SyncProgressData) {
        #[cfg(feature = "redis-cache")]
        {
            let Some(ref conn) = self.conn else {
                return;
            };

            let mut conn = conn.clone();
            match serde_json::to_string(data) {
                Ok(json) => {
                    let result: Result<(), _> =
                        conn.set_ex(SYNC_PROGRESS_REDIS_KEY, json, 30).await;
                    if let Err(e) = result {
                        warn!("Failed to publish sync progress: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to serialize sync progress: {}", e);
                }
            }
        }

        #[cfg(not(feature = "redis-cache"))]
        {
            let _ = (self, data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_invalidator_disabled_without_redis_url() {
        let invalidator = CacheInvalidator::new(None).await;
        assert!(!invalidator.is_enabled());
    }

    #[tokio::test]
    async fn test_invalidate_does_not_panic_when_disabled() {
        let invalidator = CacheInvalidator::new(None).await;
        invalidator.invalidate_chart_caches().await;
    }

    #[tokio::test]
    async fn test_publish_sync_progress_does_not_panic_when_disabled() {
        let invalidator = CacheInvalidator::new(None).await;
        let data = SyncProgressData {
            current_block: 1000,
            target_block: 10000,
            blocks_per_second: 100.0,
            ema_blocks_per_second: 95.0,
            eta_seconds: Some(90.0),
            eta_formatted: "1m 30s".to_string(),
            progress_percentage: 10.0,
            updated_at: 1234567890,
        };
        invalidator.publish_sync_progress(&data).await;
    }

    #[cfg(feature = "redis-cache")]
    #[tokio::test]
    async fn test_cache_invalidator_disabled_with_invalid_url() {
        let invalidator = CacheInvalidator::new(Some("invalid://url")).await;
        assert!(!invalidator.is_enabled());
    }

    #[cfg(feature = "redis-cache")]
    mod redis_tests {
        use super::*;

        #[tokio::test]
        async fn test_cache_invalidator_connects_to_redis() {
            let redis_url = std::env::var("TEST_REDIS_URL").ok();
            if redis_url.is_none() {
                eprintln!("Skipping: TEST_REDIS_URL not set");
                return;
            }

            let invalidator = CacheInvalidator::new(redis_url.as_deref()).await;
            assert!(invalidator.is_enabled());
        }

        #[tokio::test]
        async fn test_invalidate_chart_caches_deletes_keys() {
            let redis_url = std::env::var("TEST_REDIS_URL").ok();
            if redis_url.is_none() {
                eprintln!("Skipping: TEST_REDIS_URL not set");
                return;
            }

            let client = redis::Client::open(redis_url.as_ref().unwrap().as_str()).unwrap();
            let mut conn = client.get_multiplexed_async_connection().await.unwrap();

            let _: () = redis::cmd("SET")
                .arg("chart:test-key")
                .arg("test-value")
                .arg("EX")
                .arg(60)
                .query_async(&mut conn)
                .await
                .unwrap();

            let invalidator = CacheInvalidator::new(redis_url.as_deref()).await;
            invalidator.invalidate_chart_caches().await;

            let result: Option<String> = redis::cmd("GET")
                .arg("chart:secondary-issuance")
                .query_async(&mut conn)
                .await
                .unwrap();
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn test_publish_sync_progress_writes_to_redis() {
            let redis_url = std::env::var("TEST_REDIS_URL").ok();
            if redis_url.is_none() {
                eprintln!("Skipping: TEST_REDIS_URL not set");
                return;
            }

            let invalidator = CacheInvalidator::new(redis_url.as_deref()).await;
            let data = SyncProgressData {
                current_block: 5000,
                target_block: 10000,
                blocks_per_second: 200.0,
                ema_blocks_per_second: 180.0,
                eta_seconds: Some(27.78),
                eta_formatted: "27s".to_string(),
                progress_percentage: 50.0,
                updated_at: chrono::Utc::now().timestamp(),
            };
            invalidator.publish_sync_progress(&data).await;

            let client = redis::Client::open(redis_url.as_ref().unwrap().as_str()).unwrap();
            let mut conn = client.get_multiplexed_async_connection().await.unwrap();
            let result: Option<String> = redis::cmd("GET")
                .arg(SYNC_PROGRESS_REDIS_KEY)
                .query_async(&mut conn)
                .await
                .unwrap();

            assert!(result.is_some(), "Sync progress should be stored in Redis");
            let stored: SyncProgressData = serde_json::from_str(&result.unwrap()).unwrap();
            assert_eq!(stored.current_block, 5000);
            assert_eq!(stored.target_block, 10000);
            assert!((stored.progress_percentage - 50.0).abs() < 0.01);
        }
    }
}
