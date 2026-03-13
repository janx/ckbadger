//! Simple in-memory TTL cache using sync RwLock.
//!
//! Designed for use from `spawn_blocking` contexts where async is not needed.

use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

struct CacheEntry {
    data: Vec<u8>,
    expires_at: Instant,
}

/// Number of mutating operations (get-with-expired-eviction + set) between
/// full expired-entry cleanup sweeps.
const CLEANUP_INTERVAL: u64 = 100;

#[derive(Clone)]
pub struct InMemoryCache {
    inner: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Counts mutating operations to trigger periodic cleanup.
    ops_counter: Arc<AtomicU64>,
}

impl Default for InMemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            ops_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Get a cached value (sync). Returns None if missing or expired.
    /// Expired entries are removed eagerly on access.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        {
            let map = self.inner.read().ok()?;
            let entry = map.get(key)?;
            if Instant::now() <= entry.expires_at {
                return serde_json::from_slice(&entry.data).ok();
            }
        }
        // Entry is expired -- upgrade to write lock and remove it.
        if let Ok(mut map) = self.inner.write() {
            map.remove(key);
        }
        self.maybe_cleanup();
        None
    }

    /// Delete a cached value (sync).
    pub fn delete(&self, key: &str) {
        if let Ok(mut map) = self.inner.write() {
            map.remove(key);
        }
    }

    /// Set a cached value with TTL (sync).
    pub fn set<T: Serialize>(&self, key: &str, value: &T, ttl: Duration) {
        if let Ok(data) = serde_json::to_vec(value) {
            if let Ok(mut map) = self.inner.write() {
                map.insert(
                    key.to_string(),
                    CacheEntry {
                        data,
                        expires_at: Instant::now() + ttl,
                    },
                );
            }
        }
        self.maybe_cleanup();
    }

    /// Remove all expired entries from the cache.
    pub fn cleanup(&self) {
        if let Ok(mut map) = self.inner.write() {
            let now = Instant::now();
            map.retain(|_, entry| now <= entry.expires_at);
        }
    }

    /// Trigger a full cleanup sweep every `CLEANUP_INTERVAL` operations.
    fn maybe_cleanup(&self) {
        let count = self.ops_counter.fetch_add(1, Ordering::Relaxed);
        if count.is_multiple_of(CLEANUP_INTERVAL) {
            self.cleanup();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_get() {
        let cache = InMemoryCache::new();
        cache.set("key1", &42i64, Duration::from_secs(60));
        assert_eq!(cache.get::<i64>("key1"), Some(42));
    }

    #[test]
    fn test_missing_key() {
        let cache = InMemoryCache::new();
        assert_eq!(cache.get::<i64>("missing"), None);
    }

    #[test]
    fn test_expired_entry() {
        let cache = InMemoryCache::new();
        cache.set("key", &"hello", Duration::from_millis(0));
        // Entry is already expired
        std::thread::sleep(Duration::from_millis(1));
        assert_eq!(cache.get::<String>("key"), None);
    }

    #[test]
    fn test_expired_entry_is_removed_from_map() {
        let cache = InMemoryCache::new();
        cache.set("key", &"hello", Duration::from_millis(0));
        std::thread::sleep(Duration::from_millis(1));
        // First get returns None and removes the entry
        assert_eq!(cache.get::<String>("key"), None);
        // Verify entry is actually removed from the underlying map
        let map = cache.inner.read().unwrap();
        assert!(
            !map.contains_key("key"),
            "expired entry should be removed from the map"
        );
    }

    #[test]
    fn test_cleanup_removes_all_expired() {
        let cache = InMemoryCache::new();
        cache.set("alive", &1i64, Duration::from_secs(60));
        cache.set("dead1", &2i64, Duration::from_millis(0));
        cache.set("dead2", &3i64, Duration::from_millis(0));
        std::thread::sleep(Duration::from_millis(1));

        cache.cleanup();

        let map = cache.inner.read().unwrap();
        assert!(map.contains_key("alive"));
        assert!(!map.contains_key("dead1"));
        assert!(!map.contains_key("dead2"));
    }

    #[test]
    fn test_overwrite() {
        let cache = InMemoryCache::new();
        cache.set("k", &1i64, Duration::from_secs(60));
        cache.set("k", &2i64, Duration::from_secs(60));
        assert_eq!(cache.get::<i64>("k"), Some(2));
    }
}
