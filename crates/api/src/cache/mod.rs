mod mem_cache;

#[cfg(feature = "redis-cache")]
mod redis_cache;

#[cfg(feature = "redis-cache")]
pub use redis_cache::*;

pub use mem_cache::InMemoryCache;

use ckbadger_common::sync::{SyncStatusData, SYNC_STATUS_REDIS_KEY};
use ckbadger_store::CkbadgerStore;
use serde::{de::DeserializeOwned, Serialize};
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

    /// Get sync status from Redis, with fallback to store queries.
    pub async fn get_sync_status_from_store(&self, store: &CkbadgerStore) -> SyncStatusData {
        if let Some(status) = self.get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY).await {
            return status;
        }

        let sync = store.get_sync_status().unwrap_or_default();
        let tip = sync.tip_block_number;

        // Fallback should not depend on historical stats CF placement.
        // Use sync_status counters as the source of truth.
        let total_tx = sync.total_transactions;
        let total_cells = sync.total_cells_created;
        let total_live_cells = sync.total_cells_created - sync.total_cells_consumed;

        SyncStatusData {
            tip_block_number: tip,
            tip_block_hash: hex::encode(&sync.tip_block_hash),
            derived_tip_block_number: Some(sync.derived_tip_block_number),
            total_transactions: total_tx,
            total_cells,
            total_live_cells,
            total_addresses: 0,
            last_synced_at: sync.last_synced_at,
            derived_last_synced_at: Some(sync.derived_last_synced_at),
            derived_sync_in_progress: sync.derived_sync_in_progress,
            sync_started_at: None,
            sync_started_block: 0,
            sync_ema_rate: None,
            bulk_sync_completed_at: None,
            bulk_sync_completed_block: None,
        }
    }

    /// Get sync status tip block (lightweight, Redis-first with store fallback)
    pub async fn get_sync_tip_from_store(&self, store: &CkbadgerStore) -> i64 {
        if let Some(status) = self.get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY).await {
            return status.tip_block_number;
        }

        store
            .get_sync_status()
            .map(|s| s.tip_block_number)
            .unwrap_or(0)
    }
}

pub struct CacheKeys;

impl CacheKeys {
    pub const NETWORK_STATS: &'static str = "ckbadger:stats:network";
    pub const LATEST_BLOCKS: &'static str = "ckbadger:blocks:latest";

    pub fn block_by_hash(hash: &str) -> String {
        format!("ckbadger:block:hash:{}", hash)
    }

    pub fn transaction(hash: &str) -> String {
        format!("ckbadger:tx:{}", hash)
    }

    pub fn mining_reward(hash: &str) -> String {
        format!("ckbadger:mining-reward:{}", hash)
    }

    pub fn address_balance(lock_hash_hex: &str) -> String {
        format!("ckbadger:addr:balance:{}", lock_hash_hex)
    }
}

pub struct CacheTtl;

impl CacheTtl {
    pub const NETWORK_STATS: Duration = Duration::from_secs(10);
    pub const LATEST_BLOCKS: Duration = Duration::from_secs(5);
    pub const BLOCK: Duration = Duration::from_secs(300);
    pub const TRANSACTION: Duration = Duration::from_secs(300);
    pub const MEMPOOL_INFO: Duration = Duration::from_secs(2);
    pub const MINING_REWARD: Duration = Duration::from_secs(86400);
    pub const ADDRESS_BALANCE: Duration = Duration::from_secs(30);
    /// Chart data is primarily historical and changes slowly (new data only at current day)
    pub const CHART: Duration = Duration::from_secs(21600);
    /// Assets/tokens/NFT cached data TTL (refreshed every 30s by background loop)
    pub const ASSETS: Duration = Duration::from_secs(45);
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

    #[test]
    fn test_cache_key_address_balance_format() {
        let key = CacheKeys::address_balance("abc123");
        assert_eq!(key, "ckbadger:addr:balance:abc123");
    }

    #[test]
    fn test_cache_key_address_balance_with_hex_hash() {
        let key = CacheKeys::address_balance("0xdeadbeef");
        assert!(key.starts_with("ckbadger:addr:balance:"));
        assert!(key.ends_with("0xdeadbeef"));
    }

    #[test]
    fn test_cache_key_mining_reward_format() {
        let key = CacheKeys::mining_reward("0xabc");
        assert_eq!(key, "ckbadger:mining-reward:0xabc");
    }

    #[test]
    fn test_cache_ttl_address_balance_is_short() {
        // Address balance TTL should be short enough for responsiveness
        // but long enough to avoid excessive DB queries
        assert!(CacheTtl::ADDRESS_BALANCE.as_secs() >= 10);
        assert!(CacheTtl::ADDRESS_BALANCE.as_secs() <= 120);
    }

    #[test]
    fn test_cache_ttl_mining_reward_is_long() {
        // Mining rewards are immutable once confirmed
        assert!(CacheTtl::MINING_REWARD.as_secs() >= 3600);
    }

    #[test]
    fn test_cache_keys_are_unique() {
        // Ensure different key generators produce non-overlapping keys
        let addr_key = CacheKeys::address_balance("test");
        let block_key = CacheKeys::block_by_hash("test");
        let tx_key = CacheKeys::transaction("test");
        assert_ne!(addr_key, block_key);
        assert_ne!(addr_key, tx_key);
        assert_ne!(block_key, tx_key);
    }

    #[test]
    fn test_cache_ttl_chart_is_six_hours() {
        assert_eq!(CacheTtl::CHART.as_secs(), 21600);
        assert_eq!(CacheTtl::CHART.as_secs() / 3600, 6);
    }

    #[test]
    fn test_cache_ttl_chart_longer_than_block() {
        // Chart data changes slowly, should cache much longer than block/tx data
        assert!(CacheTtl::CHART > CacheTtl::BLOCK);
        assert!(CacheTtl::CHART > CacheTtl::TRANSACTION);
        assert!(CacheTtl::CHART > CacheTtl::NETWORK_STATS);
    }

    #[tokio::test]
    async fn test_sync_status_fallback_uses_sync_totals() {
        let cache = CacheBackend::None;
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path().to_str().unwrap()).unwrap();

        store
            .set_sync_status(&ckbadger_store::types::SyncStatus {
                tip_block_number: 42,
                tip_block_hash: vec![0xab; 32],
                total_transactions: 1234,
                total_cells_created: 300,
                total_cells_consumed: 120,
                last_synced_at: 1_700_000_000,
                ..Default::default()
            })
            .unwrap();

        let status = cache.get_sync_status_from_store(&store).await;
        assert_eq!(status.tip_block_number, 42);
        assert_eq!(status.total_transactions, 1234);
        assert_eq!(status.total_cells, 300);
        assert_eq!(status.total_live_cells, 180);
    }
}
