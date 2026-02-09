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

    /// Get sync status from Redis, with fallback to database queries.
    /// Uses pre-computed totals from daily_statistics instead of expensive COUNT(*) scans.
    pub async fn get_sync_status(&self, pool: &PgPool) -> SyncStatusData {
        if let Some(status) = self.get::<SyncStatusData>(SYNC_STATUS_REDIS_KEY).await {
            return status;
        }

        let tip: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(number), 0) FROM blocks")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

        // Use pre-computed totals from daily_statistics (latest row) instead of COUNT(*)
        // which would scan all partitions of transactions/cells tables (30s+).
        // Address count query runs in parallel using the idx_address_balances_balance index.
        let stats_fut = sqlx::query_as::<_, (i64, i64, i64)>(
            r#"SELECT
                COALESCE(ds.total_transactions, 0)::bigint,
                COALESCE(ds.total_all_cells, 0)::bigint,
                COALESCE(ds.total_live_cells, 0)::bigint
            FROM daily_statistics ds
            ORDER BY ds.date DESC
            LIMIT 1
            "#,
        )
        .fetch_one(pool);

        let addr_count_fut =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM address_balances WHERE balance > 0")
                .fetch_one(pool);

        let (stats_result, addr_result) = tokio::join!(stats_fut, addr_count_fut);
        let (total_tx, total_cells, total_live_cells) = stats_result.unwrap_or((0, 0, 0));
        let total_addresses = addr_result.unwrap_or(0);

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
            activities_deferred: false,
            activities_deferred_at: None,
            activities_rebuild_started_at: None,
            activities_rebuild_completed_at: None,
            address_balances_deferred: false,
            address_balances_deferred_at: None,
            address_balances_rebuild_completed_at: None,
            token_deferred: false,
            token_deferred_at: None,
            token_rebuild_completed_at: None,
            spore_deferred: false,
            spore_deferred_at: None,
            spore_rebuild_completed_at: None,
            tx_block_map_deferred: false,
            tx_block_map_deferred_at: None,
            tx_block_map_rebuild_completed_at: None,
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
}
