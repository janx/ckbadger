#[cfg(feature = "redis-cache")]
mod redis_cache;

#[cfg(feature = "redis-cache")]
pub use redis_cache::*;

use ckbadger_common::sync::{SyncStatusData, SYNC_STATUS_REDIS_KEY};
use serde::{de::DeserializeOwned, Serialize};
use sqlx::PgPool;
use std::time::Duration;

#[derive(Clone)]
pub enum CacheBackend {
    #[cfg(feature = "redis-cache")]
    Redis(Box<RedisCache>),
    None,
}

impl CacheBackend {
    pub async fn get<T: DeserializeOwned>(&self, _key: &str) -> Option<T> {
        match self {
            #[cfg(feature = "redis-cache")]
            CacheBackend::Redis(cache) => cache.get(_key).await,
            CacheBackend::None => None,
        }
    }

    pub async fn set<T: Serialize>(&self, _key: &str, _value: &T, _ttl: Duration) {
        match self {
            #[cfg(feature = "redis-cache")]
            CacheBackend::Redis(cache) => cache.set(_key, _value, _ttl).await,
            CacheBackend::None => {}
        }
    }

    pub async fn delete(&self, _key: &str) {
        match self {
            #[cfg(feature = "redis-cache")]
            CacheBackend::Redis(cache) => cache.delete(_key).await,
            CacheBackend::None => {}
        }
    }

    pub async fn hgetall<T: serde::de::DeserializeOwned>(&self, _key: &str) -> Vec<T> {
        match self {
            #[cfg(feature = "redis-cache")]
            CacheBackend::Redis(cache) => cache.hgetall(_key).await,
            CacheBackend::None => Vec::new(),
        }
    }

    /// Get sync status from Redis, with fallback to database queries
    pub async fn get_sync_status(&self, pool: &PgPool) -> SyncStatusData {
        if let Some(status) = self.get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY).await {
            return status;
        }

        let tip: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

        let (total_tx, total_cells, total_live_cells, total_addresses): (i64, i64, i64, i64) =
            sqlx::query_as(
                r#"SELECT 
                    COALESCE((SELECT COUNT(*) FROM transactions), 0),
                    COALESCE((SELECT COUNT(*) FROM cells), 0),
                    COALESCE((SELECT COUNT(*) FROM live_cells), 0),
                    COALESCE((SELECT COUNT(*) FROM addresses), 0)
                "#,
            )
            .fetch_one(pool)
            .await
            .unwrap_or((0, 0, 0, 0));

        SyncStatusData {
            tip_block_number: tip,
            tip_block_hash: String::new(),
            total_transactions: total_tx,
            total_cells,
            total_live_cells,
            total_addresses,
            last_synced_at: 0,
            sync_started_at: None,
            sync_started_block: 0,
            sync_ema_rate: None,
            bulk_sync_completed_at: None,
            bulk_sync_completed_block: None,
            indexes_deferred: false,
            indexes_dropped_at: None,
            indexes_rebuild_started_at: None,
            indexes_rebuild_completed_at: None,
            indexes_rebuild_progress: None,
        }
    }

    /// Get sync status tip block (lightweight, Redis-first with DB fallback)
    pub async fn get_sync_tip(&self, pool: &PgPool) -> i64 {
        if let Some(status) = self.get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY).await {
            return status.tip_block_number;
        }

        sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
            .fetch_one(pool)
            .await
            .unwrap_or(0)
    }
}

pub struct CacheKeys;

impl CacheKeys {
    pub const NETWORK_STATS: &'static str = "ckbadger:stats:network";
    pub const LATEST_BLOCKS: &'static str = "ckbadger:blocks:latest";

    pub fn block_by_number(number: i64) -> String {
        format!("ckbadger:block:{}", number)
    }

    pub fn block_by_hash(hash: &str) -> String {
        format!("ckbadger:block:hash:{}", hash)
    }

    pub fn transaction(hash: &str) -> String {
        format!("ckbadger:tx:{}", hash)
    }
}

pub struct CacheTtl;

impl CacheTtl {
    pub const NETWORK_STATS: Duration = Duration::from_secs(10);
    pub const LATEST_BLOCKS: Duration = Duration::from_secs(5);
    pub const BLOCK: Duration = Duration::from_secs(300);
    pub const TRANSACTION: Duration = Duration::from_secs(300);
    pub const MEMPOOL_INFO: Duration = Duration::from_secs(2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_backend_none_hgetall_returns_empty_vec() {
        let cache = CacheBackend::None;
        let result: Vec<String> = cache.hgetall("any_key").await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_cache_backend_none_get_returns_none() {
        let cache = CacheBackend::None;
        let result: Option<String> = cache.get("any_key").await;
        assert!(result.is_none());
    }
}
