#![allow(dead_code)]

//! LRU cache for live cell lookups.
//!
//! Provides O(1) cell lookups during blockchain synchronization using an in-memory
//! LRU cache. This reduces database queries for frequently accessed cells.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐
//! │  Block Parser   │────▶│  CellInfoCache   │
//! └─────────────────┘     └──────────────────┘
//!         │                        │
//!         │ insert()               │ get_batch()
//!         ▼                        ▼
//! ┌─────────────────┐     ┌──────────────────┐
//! │   LRU Cache     │     │  (hits, misses)  │
//! └─────────────────┘     └──────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! let cache = CellInfoCache::new(1_000_000);
//!
//! // Insert newly created cells
//! cache.insert(tx_hash, output_index, cell_info);
//!
//! // Batch lookup with cache hits/misses
//! let (hits, misses) = cache.get_batch(&outpoints);
//! // Query ClickHouse for misses separately
//!
//! // Invalidate on reorg
//! cache.invalidate(&tx_hash, output_index);
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lru::LruCache;

use crate::db::live_cell_storage::LiveCellInfo;

/// Type alias for cache batch result: (hits, misses)
type CacheBatchResult = (HashMap<(Vec<u8>, i16), LiveCellInfo>, Vec<(Vec<u8>, i16)>);

/// Configuration for the cell cache.
#[derive(Debug, Clone)]
pub struct CellCacheConfig {
    /// Maximum number of entries in the cache.
    /// Default: 1,000,000 entries (~200MB with typical cell sizes).
    pub capacity: usize,
}

impl Default for CellCacheConfig {
    fn default() -> Self {
        Self {
            capacity: 1_000_000,
        }
    }
}

/// Statistics for cache performance monitoring.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Total number of cache hits.
    pub hits: u64,
    /// Total number of cache misses.
    pub misses: u64,
    /// Total number of insertions.
    pub insertions: u64,
    /// Total number of invalidations.
    pub invalidations: u64,
}

impl CacheStats {
    /// Calculate the cache hit rate as a percentage.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }
}

/// LRU cache for live cell information.
///
/// Thread-safe cache that stores cell metadata indexed by (tx_hash, output_index).
/// Uses LRU eviction policy to maintain bounded memory usage.
///
/// # Thread Safety
///
/// All methods are thread-safe and can be called from multiple threads concurrently.
/// The cache uses a `Mutex` internally for synchronization.
#[derive(Clone)]
pub struct CellInfoCache {
    inner: Arc<Mutex<CellInfoCacheInner>>,
}

struct CellInfoCacheInner {
    cache: LruCache<(Vec<u8>, i16), LiveCellInfo>,
    stats: CacheStats,
}

impl CellInfoCache {
    /// Create a new cell cache with the specified capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of entries in the cache.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let cache = CellInfoCache::new(1_000_000);
    /// ```
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CellInfoCacheInner {
                cache: LruCache::new(
                    std::num::NonZeroUsize::new(capacity).expect("capacity must be non-zero"),
                ),
                stats: CacheStats::default(),
            })),
        }
    }

    /// Create a new cell cache from configuration.
    pub fn from_config(config: &CellCacheConfig) -> Self {
        Self::new(config.capacity)
    }

    /// Get a single cell from the cache.
    ///
    /// # Arguments
    ///
    /// * `tx_hash` - Transaction hash (32 bytes).
    /// * `output_index` - Output index within the transaction.
    ///
    /// # Returns
    ///
    /// `Some(LiveCellInfo)` if the cell is in the cache, `None` otherwise.
    pub fn get(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo> {
        let mut inner = self.inner.lock().unwrap();
        let key = (tx_hash.to_vec(), output_index);
        let result = inner.cache.get(&key).cloned();

        if result.is_some() {
            inner.stats.hits += 1;
        } else {
            inner.stats.misses += 1;
        }

        result
    }

    /// Get multiple cells from the cache in a single operation.
    ///
    /// # Arguments
    ///
    /// * `outpoints` - Slice of (tx_hash, output_index) tuples to look up.
    ///
    /// # Returns
    ///
    /// A tuple of:
    /// - `hits`: HashMap of found cells indexed by (tx_hash, output_index).
    /// - `misses`: Vec of outpoints that were not found in the cache.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let outpoints = vec![(&tx_hash1, 0), (&tx_hash2, 1)];
    /// let (hits, misses) = cache.get_batch(&outpoints);
    ///
    /// // Use hits directly
    /// for ((tx_hash, idx), info) in hits {
    ///     println!("Found cell: {}:{}", hex::encode(tx_hash), idx);
    /// }
    ///
    /// // Query ClickHouse for misses
    /// if !misses.is_empty() {
    ///     let db_results = query_clickhouse(&misses).await?;
    /// }
    /// ```
    pub fn get_batch(&self, outpoints: &[(&[u8], i16)]) -> CacheBatchResult {
        let mut inner = self.inner.lock().unwrap();
        let mut hits = HashMap::new();
        let mut misses = Vec::new();

        for (tx_hash, output_index) in outpoints {
            let key = (tx_hash.to_vec(), *output_index);
            if let Some(info) = inner.cache.get(&key) {
                hits.insert(key, info.clone());
                inner.stats.hits += 1;
            } else {
                misses.push(key);
                inner.stats.misses += 1;
            }
        }

        (hits, misses)
    }

    /// Insert a cell into the cache.
    ///
    /// # Arguments
    ///
    /// * `tx_hash` - Transaction hash (32 bytes).
    /// * `output_index` - Output index within the transaction.
    /// * `info` - Cell metadata to cache.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// cache.insert(tx_hash, 0, LiveCellInfo {
    ///     capacity: 10000000000,
    ///     created_at_block: 12345,
    ///     lock_script_hash: vec![...],
    ///     // ... other fields
    /// });
    /// ```
    pub fn insert(&self, tx_hash: Vec<u8>, output_index: i16, info: LiveCellInfo) {
        let mut inner = self.inner.lock().unwrap();
        let key = (tx_hash, output_index);
        inner.cache.put(key, info);
        inner.stats.insertions += 1;
    }

    /// Insert multiple cells into the cache in a single operation.
    ///
    /// # Arguments
    ///
    /// * `cells` - Iterator of (tx_hash, output_index, info) tuples.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let cells = vec![
    ///     (tx_hash1, 0, info1),
    ///     (tx_hash2, 1, info2),
    /// ];
    /// cache.insert_batch(cells.into_iter());
    /// ```
    pub fn insert_batch(&self, cells: impl Iterator<Item = (Vec<u8>, i16, LiveCellInfo)>) {
        let mut inner = self.inner.lock().unwrap();
        for (tx_hash, output_index, info) in cells {
            let key = (tx_hash, output_index);
            inner.cache.put(key, info);
            inner.stats.insertions += 1;
        }
    }

    /// Remove a cell from the cache (e.g., on reorg disconnect).
    ///
    /// # Arguments
    ///
    /// * `tx_hash` - Transaction hash (32 bytes).
    /// * `output_index` - Output index within the transaction.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // On reorg, invalidate cells from disconnected blocks
    /// cache.invalidate(&tx_hash, output_index);
    /// ```
    pub fn invalidate(&self, tx_hash: &[u8], output_index: i16) {
        let mut inner = self.inner.lock().unwrap();
        let key = (tx_hash.to_vec(), output_index);
        if inner.cache.pop(&key).is_some() {
            inner.stats.invalidations += 1;
        }
    }

    /// Get the current number of entries in the cache.
    pub fn len(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.cache.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.cache.is_empty()
    }

    /// Clear all entries from the cache.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.cache.clear();
    }

    /// Get cache statistics.
    ///
    /// # Returns
    ///
    /// A snapshot of the current cache statistics including hits, misses, and hit rate.
    pub fn stats(&self) -> CacheStats {
        let inner = self.inner.lock().unwrap();
        inner.stats.clone()
    }

    /// Reset cache statistics to zero.
    pub fn reset_stats(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.stats = CacheStats::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_cell_info(capacity: i64) -> LiveCellInfo {
        LiveCellInfo {
            capacity,
            created_at_block: 12345,
            lock_script_hash: vec![1; 32],
            lock_code_hash: vec![2; 32],
            lock_args: vec![3; 20],
            type_script_hash: None,
            type_code_hash: None,
            data_size: 0,
        }
    }

    #[test]
    fn test_new_cache() {
        let cache = CellInfoCache::new(100);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_insert_and_get() {
        let cache = CellInfoCache::new(100);
        let tx_hash = vec![1; 32];
        let info = make_test_cell_info(10000000000);

        cache.insert(tx_hash.clone(), 0, info.clone());

        let result = cache.get(&tx_hash, 0);
        assert!(result.is_some());
        assert_eq!(result.unwrap().capacity, 10000000000);
    }

    #[test]
    fn test_get_nonexistent() {
        let cache = CellInfoCache::new(100);
        let tx_hash = vec![1; 32];

        let result = cache.get(&tx_hash, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_batch() {
        let cache = CellInfoCache::new(100);
        let tx_hash1 = vec![1; 32];
        let tx_hash2 = vec![2; 32];
        let tx_hash3 = vec![3; 32];

        cache.insert(tx_hash1.clone(), 0, make_test_cell_info(100));
        cache.insert(tx_hash2.clone(), 1, make_test_cell_info(200));

        let outpoints = vec![
            (tx_hash1.as_slice(), 0),
            (tx_hash2.as_slice(), 1),
            (tx_hash3.as_slice(), 2),
        ];

        let (hits, misses) = cache.get_batch(&outpoints);

        assert_eq!(hits.len(), 2);
        assert_eq!(misses.len(), 1);
        assert!(hits.contains_key(&(tx_hash1.clone(), 0)));
        assert!(hits.contains_key(&(tx_hash2.clone(), 1)));
        assert_eq!(misses[0], (tx_hash3, 2));
    }

    #[test]
    fn test_insert_batch() {
        let cache = CellInfoCache::new(100);
        let cells = vec![
            (vec![1; 32], 0, make_test_cell_info(100)),
            (vec![2; 32], 1, make_test_cell_info(200)),
            (vec![3; 32], 2, make_test_cell_info(300)),
        ];

        cache.insert_batch(cells.into_iter());

        assert_eq!(cache.len(), 3);
        assert!(cache.get(&[1; 32], 0).is_some());
        assert!(cache.get(&[2; 32], 1).is_some());
        assert!(cache.get(&[3; 32], 2).is_some());
    }

    #[test]
    fn test_invalidate() {
        let cache = CellInfoCache::new(100);
        let tx_hash = vec![1; 32];

        cache.insert(tx_hash.clone(), 0, make_test_cell_info(100));
        assert!(cache.get(&tx_hash, 0).is_some());

        cache.invalidate(&tx_hash, 0);
        assert!(cache.get(&tx_hash, 0).is_none());
    }

    #[test]
    fn test_lru_eviction() {
        let cache = CellInfoCache::new(2);

        cache.insert(vec![1; 32], 0, make_test_cell_info(100));
        cache.insert(vec![2; 32], 0, make_test_cell_info(200));
        cache.insert(vec![3; 32], 0, make_test_cell_info(300));

        // First entry should be evicted
        assert!(cache.get(&[1; 32], 0).is_none());
        assert!(cache.get(&[2; 32], 0).is_some());
        assert!(cache.get(&[3; 32], 0).is_some());
    }

    #[test]
    fn test_clear() {
        let cache = CellInfoCache::new(100);
        cache.insert(vec![1; 32], 0, make_test_cell_info(100));
        cache.insert(vec![2; 32], 1, make_test_cell_info(200));

        assert_eq!(cache.len(), 2);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_stats() {
        let cache = CellInfoCache::new(100);
        let tx_hash = vec![1; 32];

        cache.insert(tx_hash.clone(), 0, make_test_cell_info(100));

        // Hit
        cache.get(&tx_hash, 0);
        // Miss
        cache.get(&[2; 32], 0);

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.insertions, 1);
        assert!((stats.hit_rate() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_reset_stats() {
        let cache = CellInfoCache::new(100);
        cache.insert(vec![1; 32], 0, make_test_cell_info(100));
        cache.get(&[1; 32], 0);

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);

        cache.reset_stats();

        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.insertions, 0);
    }

    #[test]
    fn test_from_config() {
        let config = CellCacheConfig { capacity: 500 };
        let cache = CellInfoCache::from_config(&config);

        for i in 0..600 {
            cache.insert(vec![i as u8; 32], 0, make_test_cell_info(i as i64));
        }

        let len = cache.len();
        assert!(
            len < 600,
            "Cache should have evicted some entries, got {}",
            len
        );
        assert!(len > 0, "Cache should not be empty");
    }

    #[test]
    fn test_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let cache = Arc::new(CellInfoCache::new(1000));
        let mut handles = vec![];

        for i in 0..10 {
            let cache_clone = Arc::clone(&cache);
            let handle = thread::spawn(move || {
                for j in 0..100 {
                    let tx_hash = vec![i as u8; 32];
                    cache_clone.insert(
                        tx_hash.clone(),
                        j,
                        make_test_cell_info(i as i64 * 100 + j as i64),
                    );
                    cache_clone.get(&tx_hash, j);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Verify cache is still functional
        assert!(cache.len() <= 1000);
    }
}
