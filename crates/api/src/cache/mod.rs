mod mem_cache;

pub use mem_cache::InMemoryCache;

use ckbadger_store::CkbadgerStore;
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

/// Cache backend using in-memory TTL cache.
/// Previously supported Redis; now always uses in-memory storage.
#[derive(Clone)]
pub struct CacheBackend {
    inner: InMemoryCache,
}

impl CacheBackend {
    pub fn new() -> Self {
        Self {
            inner: InMemoryCache::new(),
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.inner.get(key)
    }

    pub async fn set<T: Serialize>(&self, key: &str, value: &T, ttl: Duration) {
        self.inner.set(key, value, ttl);
    }

    pub async fn delete(&self, key: &str) {
        self.inner.delete(key);
    }

    /// Get sync status from store queries (no Redis).
    pub async fn get_sync_status_from_store(
        &self,
        store: &CkbadgerStore,
    ) -> ckbadger_common::sync::SyncStatusData {
        let sync = store.get_sync_status().unwrap_or_default();
        let tip = sync.tip_block_number;
        let total_tx = sync.total_transactions;
        let total_cells = sync.total_cells_created;
        let total_live_cells = sync.total_cells_created - sync.total_cells_consumed;

        ckbadger_common::sync::SyncStatusData {
            tip_block_number: tip,
            tip_block_hash: hex::encode(&sync.tip_block_hash),
            total_transactions: total_tx,
            total_cells,
            total_live_cells,
            total_addresses: 0,
            last_synced_at: sync.last_synced_at,
            sync_started_at: sync.sync_started_at,
            sync_started_block: sync.sync_started_block,
            sync_ema_rate: sync.sync_ema_rate,
            bulk_sync_completed_at: sync.bulk_sync_completed_at,
            bulk_sync_completed_block: sync.bulk_sync_completed_block,
        }
    }

    /// Get sync status tip block from store.
    pub async fn get_sync_tip_from_store(&self, store: &CkbadgerStore) -> i64 {
        store
            .get_sync_status()
            .map(|s| s.tip_block_number)
            .unwrap_or(0)
    }
}

impl Default for CacheBackend {
    fn default() -> Self {
        Self::new()
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
    /// Asset ecosystem overview (homepage panel)
    pub const ASSET_ECOSYSTEM: Duration = Duration::from_secs(30);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_backend_get_returns_none_for_missing() {
        let cache = CacheBackend::new();
        let result: Option<String> = cache.get("any_key").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_backend_set_and_get() {
        let cache = CacheBackend::new();
        cache.set("key1", &42i64, Duration::from_secs(60)).await;
        let result: Option<i64> = cache.get("key1").await;
        assert_eq!(result, Some(42));
    }

    #[tokio::test]
    async fn test_cache_backend_delete() {
        let cache = CacheBackend::new();
        cache.set("key1", &42i64, Duration::from_secs(60)).await;
        cache.delete("key1").await;
        let result: Option<i64> = cache.get("key1").await;
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
        assert!(CacheTtl::ADDRESS_BALANCE.as_secs() >= 10);
        assert!(CacheTtl::ADDRESS_BALANCE.as_secs() <= 120);
    }

    #[test]
    fn test_cache_ttl_mining_reward_is_long() {
        assert!(CacheTtl::MINING_REWARD.as_secs() >= 3600);
    }

    #[test]
    fn test_cache_keys_are_unique() {
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
        assert!(CacheTtl::CHART > CacheTtl::BLOCK);
        assert!(CacheTtl::CHART > CacheTtl::TRANSACTION);
        assert!(CacheTtl::CHART > CacheTtl::NETWORK_STATS);
    }

    #[tokio::test]
    async fn test_sync_status_fallback_uses_sync_totals() {
        let cache = CacheBackend::new();
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
                sync_started_at: Some(1_699_999_000),
                sync_started_block: 1,
                sync_ema_rate: Some(66.6),
                bulk_sync_completed_at: Some(1_700_000_100),
                bulk_sync_completed_block: Some(42),
                ..Default::default()
            })
            .unwrap();

        let status = cache.get_sync_status_from_store(&store).await;
        assert_eq!(status.tip_block_number, 42);
        assert_eq!(status.total_transactions, 1234);
        assert_eq!(status.total_cells, 300);
        assert_eq!(status.total_live_cells, 180);
        assert_eq!(status.sync_started_at, Some(1_699_999_000));
        assert_eq!(status.sync_ema_rate, Some(66.6));
        assert_eq!(status.bulk_sync_completed_block, Some(42));
    }
}
