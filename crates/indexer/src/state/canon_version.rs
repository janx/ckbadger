use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Manages monotonically increasing version numbers for canonical state.
///
/// The canon_version is used in ReplacingMergeTree tables to determine the latest
/// state of canonical blocks and cells. It must be strictly monotonic across restarts
/// to ensure correct reorg handling.
///
/// # Design
///
/// - Uses `AtomicU64` with `SeqCst` ordering for thread-safe atomic operations
/// - Recovered from ClickHouse on startup via `recover_from_db_sync()`
/// - Each call to `next()` atomically increments and returns the new version
/// - Independent of block numbers (can have gaps)
///
/// # Example
///
/// ```ignore
/// // Query max version from ClickHouse
/// let max_version = ch_client.fetch_max_canon_version().await?;
/// let version_mgr = CanonVersionManager::recover_from_db(max_version);
/// let v1 = version_mgr.next(); // Returns 1
/// let v2 = version_mgr.next(); // Returns 2
/// // After restart:
/// let max_version = ch_client.fetch_max_canon_version().await?;
/// let version_mgr = CanonVersionManager::recover_from_db(max_version);
/// let v3 = version_mgr.next(); // Returns 3 (monotonic!)
/// ```
#[derive(Clone)]
pub struct CanonVersionManager {
    counter: Arc<AtomicU64>,
}

impl CanonVersionManager {
    /// Create a new version manager starting at version 0.
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Create a new version manager starting at a specific version.
    ///
    /// Used when recovering from database to ensure monotonic ordering.
    pub fn new_from(version: u64) -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(version)),
        }
    }

    /// Get the next version number.
    ///
    /// Atomically increments the counter and returns the new value.
    /// Uses `SeqCst` ordering to ensure strict monotonic ordering across threads.
    pub fn next(&self) -> u64 {
        // fetch_add returns the OLD value, so we add 1 to get the new version
        self.counter.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Get the current version without incrementing.
    #[allow(dead_code)]
    pub fn current(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }

    /// Recover the version manager from ClickHouse on startup.
    ///
    /// Queries the maximum canon_version from the canonical_blocks table.
    /// If the table is empty (fresh database), starts at version 0.
    ///
    /// # Arguments
    ///
    /// * `max_version` - Maximum version from database, or None if table is empty
    ///
    /// # Returns
    ///
    /// A new CanonVersionManager initialized with the recovered version
    pub fn recover_from_db(max_version: Option<u64>) -> Self {
        match max_version {
            Some(version) => {
                tracing::info!("Recovered canon_version from database: {}", version);
                Self::new_from(version)
            }
            None => {
                tracing::info!("No canon_version found in database, starting at 0");
                Self::new()
            }
        }
    }
}

impl Default for CanonVersionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canon_version_monotonic() {
        let mgr = CanonVersionManager::new();
        let v1 = mgr.next();
        let v2 = mgr.next();
        let v3 = mgr.next();

        assert_eq!(v1, 1);
        assert_eq!(v2, 2);
        assert_eq!(v3, 3);
        assert!(v1 < v2 && v2 < v3);
    }

    #[test]
    fn test_canon_version_from_specific() {
        let mgr = CanonVersionManager::new_from(100);
        let v1 = mgr.next();
        let v2 = mgr.next();

        assert_eq!(v1, 101);
        assert_eq!(v2, 102);
    }

    #[test]
    fn test_canon_version_current() {
        let mgr = CanonVersionManager::new();
        assert_eq!(mgr.current(), 0);

        mgr.next();
        assert_eq!(mgr.current(), 1);

        mgr.next();
        assert_eq!(mgr.current(), 2);
    }

    #[test]
    fn test_canon_version_clone_shared() {
        let mgr1 = CanonVersionManager::new();
        let mgr2 = mgr1.clone();

        let v1 = mgr1.next();
        let v2 = mgr2.next();

        // Both should see the same counter
        assert_eq!(v1, 1);
        assert_eq!(v2, 2);
        assert_eq!(mgr1.current(), 2);
        assert_eq!(mgr2.current(), 2);
    }

    #[test]
    fn test_canon_version_thread_safe() {
        use std::thread;

        let mgr = CanonVersionManager::new();
        let mut handles = vec![];

        for _ in 0..10 {
            let mgr_clone = mgr.clone();
            let handle = thread::spawn(move || {
                let mut versions = vec![];
                for _ in 0..100 {
                    versions.push(mgr_clone.next());
                }
                versions
            });
            handles.push(handle);
        }

        let mut all_versions = vec![];
        for handle in handles {
            all_versions.extend(handle.join().unwrap());
        }

        // All versions should be unique and in range [1, 1000]
        all_versions.sort();
        all_versions.dedup();
        assert_eq!(all_versions.len(), 1000);
        assert_eq!(all_versions[0], 1);
        assert_eq!(all_versions[999], 1000);
    }
}
