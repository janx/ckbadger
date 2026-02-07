mod redis_cache;

#[cfg(feature = "redis-cache")]
pub use redis_cache::*;

use ckbadger_common::sync::{SyncStatusData, SYNC_STATUS_REDIS_KEY};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

use crate::db::DbPool;

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

    pub async fn hgetall<T: DeserializeOwned>(&self, _key: &str) -> Vec<T> {
        match self {
            #[cfg(feature = "redis-cache")]
            CacheBackend::Redis(cache) => cache.hgetall(_key).await,
            CacheBackend::None => Vec::new(),
        }
    }

    pub async fn get_sync_status(&self, _pool: &DbPool) -> SyncStatusData {
        if let Some(status) = self.get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY).await {
            return status;
        }

        SyncStatusData::default()
    }

    pub async fn get_sync_tip(&self, _pool: &DbPool) -> i64 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_backend_none_get_returns_none() {
        let cache = CacheBackend::None;
        let result: Option<String> = cache.get("any_key").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_backend_none_set_does_not_panic() {
        let cache = CacheBackend::None;
        cache
            .set("key", &"value".to_string(), Duration::from_secs(60))
            .await;
    }

    #[tokio::test]
    async fn test_cache_backend_none_delete_does_not_panic() {
        let cache = CacheBackend::None;
        cache.delete("key").await;
    }

    #[tokio::test]
    async fn test_cache_backend_none_hgetall_returns_empty() {
        let cache = CacheBackend::None;
        let result: Vec<String> = cache.hgetall("any_key").await;
        assert!(result.is_empty());
    }
}
