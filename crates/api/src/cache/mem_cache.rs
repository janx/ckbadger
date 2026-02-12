//! Simple in-memory TTL cache using sync RwLock.
//!
//! Designed for use from `spawn_blocking` contexts where async is not needed.

use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

struct CacheEntry {
    data: Vec<u8>,
    expires_at: Instant,
}

#[derive(Clone)]
pub struct InMemoryCache {
    inner: Arc<RwLock<HashMap<String, CacheEntry>>>,
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
        }
    }

    /// Get a cached value (sync). Returns None if missing or expired.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let map = self.inner.read().ok()?;
        let entry = map.get(key)?;
        if Instant::now() > entry.expires_at {
            return None;
        }
        serde_json::from_slice(&entry.data).ok()
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
    fn test_overwrite() {
        let cache = InMemoryCache::new();
        cache.set("k", &1i64, Duration::from_secs(60));
        cache.set("k", &2i64, Duration::from_secs(60));
        assert_eq!(cache.get::<i64>("k"), Some(2));
    }
}
