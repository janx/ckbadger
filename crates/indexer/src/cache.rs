use ckbadger_common::{CachedProposal, MemoryStatsData, SyncProgressData, SyncStatusData};
#[cfg(feature = "redis-cache")]
use ckbadger_common::{
    MEMORY_STATS_REDIS_KEY, PENDING_PROPOSALS_REDIS_KEY, PROPOSAL_CACHE_TTL_SECS,
    SYNC_PROGRESS_REDIS_KEY, SYNC_STATUS_REDIS_KEY,
};
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
    "chart:block-time-distribution:v2",
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

    pub async fn get_sync_status(&self) -> Option<SyncStatusData> {
        #[cfg(feature = "redis-cache")]
        {
            let conn = self.conn.as_ref()?;

            let mut conn = conn.clone();
            let result: Result<Option<String>, _> = conn.get(SYNC_STATUS_REDIS_KEY).await;
            match result {
                Ok(Some(json)) => serde_json::from_str(&json).ok(),
                Ok(None) => None,
                Err(e) => {
                    warn!("Failed to get sync status: {}", e);
                    None
                }
            }
        }

        #[cfg(not(feature = "redis-cache"))]
        {
            let _ = self;
            None
        }
    }

    pub async fn set_sync_status(&self, data: &SyncStatusData) {
        #[cfg(feature = "redis-cache")]
        {
            let Some(ref conn) = self.conn else {
                return;
            };

            let mut conn = conn.clone();
            match serde_json::to_string(data) {
                Ok(json) => {
                    let result: Result<(), _> = conn.set(SYNC_STATUS_REDIS_KEY, json).await;
                    if let Err(e) = result {
                        warn!("Failed to set sync status: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to serialize sync status: {}", e);
                }
            }
        }

        #[cfg(not(feature = "redis-cache"))]
        {
            let _ = (self, data);
        }
    }

    pub async fn update_sync_status<F>(&self, updater: F) -> Option<SyncStatusData>
    where
        F: FnOnce(&mut SyncStatusData),
    {
        #[cfg(feature = "redis-cache")]
        {
            let mut status = self.get_sync_status().await.unwrap_or_default();
            updater(&mut status);
            self.set_sync_status(&status).await;
            Some(status)
        }

        #[cfg(not(feature = "redis-cache"))]
        {
            let _ = (self, updater);
            None
        }
    }

    pub async fn cache_proposals(&self, proposals: &[CachedProposal]) {
        #[cfg(feature = "redis-cache")]
        {
            let Some(ref conn) = self.conn else {
                return;
            };

            if proposals.is_empty() {
                return;
            }

            let mut conn = conn.clone();

            for proposal in proposals {
                match serde_json::to_string(proposal) {
                    Ok(json) => {
                        let result: Result<(), _> = conn
                            .hset(PENDING_PROPOSALS_REDIS_KEY, &proposal.proposal_id, json)
                            .await;
                        if let Err(e) = result {
                            warn!("Failed to cache proposal {}: {}", &proposal.proposal_id, e);
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to serialize proposal {}: {}",
                            &proposal.proposal_id, e
                        );
                    }
                }
            }

            let _: Result<(), _> = conn
                .expire(PENDING_PROPOSALS_REDIS_KEY, PROPOSAL_CACHE_TTL_SECS as i64)
                .await;
        }

        #[cfg(not(feature = "redis-cache"))]
        {
            let _ = (self, proposals);
        }
    }

    pub async fn remove_committed_proposals(&self, proposal_ids: &[String]) {
        #[cfg(feature = "redis-cache")]
        {
            let Some(ref conn) = self.conn else {
                return;
            };

            if proposal_ids.is_empty() {
                return;
            }

            let mut conn = conn.clone();

            for proposal_id in proposal_ids {
                let result: Result<i64, _> =
                    conn.hdel(PENDING_PROPOSALS_REDIS_KEY, proposal_id).await;
                if let Err(e) = result {
                    warn!("Failed to remove committed proposal {}: {}", proposal_id, e);
                }
            }
        }

        #[cfg(not(feature = "redis-cache"))]
        {
            let _ = (self, proposal_ids);
        }
    }

    pub async fn cleanup_expired_proposals(&self, current_tip: i64) {
        #[cfg(feature = "redis-cache")]
        {
            use ckbadger_common::PROPOSAL_WINDOW_FARTHEST;

            let Some(ref conn) = self.conn else {
                return;
            };

            let mut conn = conn.clone();

            let all_proposals: Result<std::collections::HashMap<String, String>, _> =
                conn.hgetall(PENDING_PROPOSALS_REDIS_KEY).await;

            match all_proposals {
                Ok(proposals) => {
                    let mut expired = Vec::new();
                    for (proposal_id, json) in proposals {
                        if let Ok(cached) = serde_json::from_str::<CachedProposal>(&json) {
                            let expiry_block =
                                cached.proposed_at_block + PROPOSAL_WINDOW_FARTHEST as i64;
                            if current_tip > expiry_block {
                                expired.push(proposal_id);
                            }
                        }
                    }

                    if !expired.is_empty() {
                        info!("Cleaning up {} expired proposals", expired.len());
                        for proposal_id in &expired {
                            let _: Result<i64, _> =
                                conn.hdel(PENDING_PROPOSALS_REDIS_KEY, proposal_id).await;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to get proposals for cleanup: {}", e);
                }
            }
        }

        #[cfg(not(feature = "redis-cache"))]
        {
            let _ = (self, current_tip);
        }
    }

    pub async fn get_pending_proposals(&self) -> Vec<CachedProposal> {
        #[cfg(feature = "redis-cache")]
        {
            let Some(ref conn) = self.conn else {
                return Vec::new();
            };

            let mut conn = conn.clone();

            let all_proposals: Result<std::collections::HashMap<String, String>, _> =
                conn.hgetall(PENDING_PROPOSALS_REDIS_KEY).await;

            match all_proposals {
                Ok(proposals) => {
                    let mut result = Vec::new();
                    for (_proposal_id, json) in proposals {
                        if let Ok(cached) = serde_json::from_str::<CachedProposal>(&json) {
                            result.push(cached);
                        }
                    }
                    result.sort_by(|a, b| {
                        b.proposed_at_block
                            .cmp(&a.proposed_at_block)
                            .then(a.proposed_at_index.cmp(&b.proposed_at_index))
                    });
                    result
                }
                Err(e) => {
                    warn!("Failed to get pending proposals: {}", e);
                    Vec::new()
                }
            }
        }

        #[cfg(not(feature = "redis-cache"))]
        {
            let _ = self;
            Vec::new()
        }
    }

    pub async fn publish_memory_stats(&self, data: &MemoryStatsData) {
        #[cfg(feature = "redis-cache")]
        {
            let Some(ref conn) = self.conn else {
                return;
            };

            let mut conn = conn.clone();
            match serde_json::to_string(data) {
                Ok(json) => {
                    let result: Result<(), _> = conn.set_ex(MEMORY_STATS_REDIS_KEY, json, 30).await;
                    if let Err(e) = result {
                        warn!("Failed to publish memory stats: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to serialize memory stats: {}", e);
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
            last_batch_blocks: Some(64),
            blocks_per_second: 100.0,
            ema_blocks_per_second: 95.0,
            txs_per_second: Some(2_000.0),
            ema_txs_per_second: Some(1_900.0),
            eta_seconds: Some(90.0),
            eta_formatted: "1m 30s".to_string(),
            progress_percentage: 10.0,
            updated_at: 1234567890,
            startup_phase: None,
            is_direct_db_read: false,
            db_write_ms: None,
            db_commit_ms: None,
            rpc_fetch_ms: None,
            pipeline: None,
            pipeline_reset_epoch: None,
            pipeline_reset_reason: None,
            adaptive_target_batch_txs: None,
            adaptive_inflight_limit: None,
            adaptive_min_target_batch_txs: None,
            adaptive_cooldown_steps: None,
            adaptive_last_reason: None,
            adaptive_adjustment_seq: None,
            adaptive_backoff_streak: None,
            adaptive_last_adjusted_at: None,
        };
        invalidator.publish_sync_progress(&data).await;
    }

    #[tokio::test]
    async fn test_get_sync_status_returns_none_when_disabled() {
        let invalidator = CacheInvalidator::new(None).await;
        assert!(invalidator.get_sync_status().await.is_none());
    }

    #[tokio::test]
    async fn test_set_sync_status_does_not_panic_when_disabled() {
        let invalidator = CacheInvalidator::new(None).await;
        let status = SyncStatusData::default();
        invalidator.set_sync_status(&status).await;
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
                last_batch_blocks: Some(128),
                blocks_per_second: 200.0,
                ema_blocks_per_second: 180.0,
                txs_per_second: Some(8_000.0),
                ema_txs_per_second: Some(7_200.0),
                eta_seconds: Some(27.78),
                eta_formatted: "27s".to_string(),
                progress_percentage: 50.0,
                updated_at: chrono::Utc::now().timestamp(),
                startup_phase: Some("rollback_cleanup".to_string()),
                is_direct_db_read: false,
                db_write_ms: None,
                db_commit_ms: None,
                rpc_fetch_ms: None,
                pipeline: None,
                pipeline_reset_epoch: Some(7),
                pipeline_reset_reason: Some("pipeline batch mismatch".to_string()),
                adaptive_target_batch_txs: Some(40_000),
                adaptive_inflight_limit: Some(3),
                adaptive_min_target_batch_txs: Some(10_000),
                adaptive_cooldown_steps: Some(2),
                adaptive_last_reason: Some("pressure_backoff".to_string()),
                adaptive_adjustment_seq: Some(9),
                adaptive_backoff_streak: Some(4),
                adaptive_last_adjusted_at: Some(1_700_000_456),
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
            assert_eq!(stored.startup_phase.as_deref(), Some("rollback_cleanup"));
            assert_eq!(stored.txs_per_second, Some(8_000.0));
            assert_eq!(stored.ema_txs_per_second, Some(7_200.0));
            assert_eq!(stored.pipeline_reset_epoch, Some(7));
            assert_eq!(
                stored.pipeline_reset_reason.as_deref(),
                Some("pipeline batch mismatch")
            );
            assert_eq!(stored.last_batch_blocks, Some(128));
            assert_eq!(stored.adaptive_target_batch_txs, Some(40_000));
            assert_eq!(stored.adaptive_inflight_limit, Some(3));
            assert_eq!(stored.adaptive_min_target_batch_txs, Some(10_000));
            assert_eq!(stored.adaptive_cooldown_steps, Some(2));
            assert_eq!(
                stored.adaptive_last_reason.as_deref(),
                Some("pressure_backoff")
            );
            assert_eq!(stored.adaptive_adjustment_seq, Some(9));
            assert_eq!(stored.adaptive_backoff_streak, Some(4));
            assert_eq!(stored.adaptive_last_adjusted_at, Some(1_700_000_456));
        }

        #[tokio::test]
        async fn test_sync_status_roundtrip() {
            let redis_url = std::env::var("TEST_REDIS_URL").ok();
            if redis_url.is_none() {
                eprintln!("Skipping: TEST_REDIS_URL not set");
                return;
            }

            let invalidator = CacheInvalidator::new(redis_url.as_deref()).await;
            let status = SyncStatusData {
                tip_block_number: 12345,
                tip_block_hash: "0xabc123".to_string(),
                total_transactions: 1000,
                total_cells: 500,
                total_live_cells: 300,
                total_addresses: 100,
                last_synced_at: chrono::Utc::now().timestamp(),
                sync_started_at: None,
                sync_started_block: 0,
                sync_ema_rate: Some(500.0),
                bulk_sync_completed_at: None,
                bulk_sync_completed_block: None,
                ..Default::default()
            };

            invalidator.set_sync_status(&status).await;
            let retrieved = invalidator.get_sync_status().await;

            assert!(retrieved.is_some());
            let retrieved = retrieved.unwrap();
            assert_eq!(retrieved.tip_block_number, 12345);
            assert_eq!(retrieved.tip_block_hash, "0xabc123");
            assert_eq!(retrieved.total_transactions, 1000);
        }

        #[tokio::test]
        async fn test_update_sync_status() {
            let redis_url = std::env::var("TEST_REDIS_URL").ok();
            if redis_url.is_none() {
                eprintln!("Skipping: TEST_REDIS_URL not set");
                return;
            }

            let invalidator = CacheInvalidator::new(redis_url.as_deref()).await;

            let initial = SyncStatusData {
                tip_block_number: 100,
                ..Default::default()
            };
            invalidator.set_sync_status(&initial).await;

            invalidator
                .update_sync_status(|s| {
                    s.tip_block_number = 200;
                    s.total_transactions += 50;
                })
                .await;

            let updated = invalidator.get_sync_status().await.unwrap();
            assert_eq!(updated.tip_block_number, 200);
            assert_eq!(updated.total_transactions, 50);
        }
    }
}
