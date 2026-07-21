use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::sync::types::InternId;

/// Thread-safe identity interner for concurrent use during facts building.
#[derive(Debug)]
pub(crate) struct IdentityInterner {
    by_value: DashMap<Arc<[u8]>, InternId>,
    /// Wrapped in `Arc` so `snapshot_for_reads()` is O(1) (single atomic
    /// ref-count bump) instead of O(n) (cloning millions of Arc pointers).
    /// `intern_bytes()` uses `Arc::make_mut` for COW semantics: when no
    /// snapshot is alive the inner Vec is mutated in-place (no clone);
    /// if a snapshot is still alive (refcount > 1) it clones first.
    values: Mutex<Arc<Vec<Arc<[u8]>>>>,
    intern_call_count: AtomicU64,
    intern_slow_path_count: AtomicU64,
}

impl Default for IdentityInterner {
    fn default() -> Self {
        Self {
            by_value: DashMap::new(),
            values: Mutex::new(Arc::new(Vec::new())),
            intern_call_count: AtomicU64::new(0),
            intern_slow_path_count: AtomicU64::new(0),
        }
    }
}

impl IdentityInterner {
    /// Create an interner with pre-allocated capacity to reduce reallocations.
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            by_value: DashMap::with_capacity(capacity),
            values: Mutex::new(Arc::new(Vec::with_capacity(capacity))),
            intern_call_count: AtomicU64::new(0),
            intern_slow_path_count: AtomicU64::new(0),
        }
    }

    /// Intern a byte sequence. Thread-safe for concurrent callers.
    pub(crate) fn intern_bytes(&self, bytes: Vec<u8>) -> InternId {
        self.intern_call_count.fetch_add(1, Ordering::Relaxed);
        // Fast path: lock-free DashMap read
        if let Some(id) = self.by_value.get(bytes.as_slice()) {
            return *id;
        }
        // Counts Mutex acquisitions, not new-identity insertions. The double-check inside the
        // Mutex may find the value already inserted by another thread, but the contention
        // (Mutex wait) still occurred.
        self.intern_slow_path_count.fetch_add(1, Ordering::Relaxed);
        // Slow path: acquire values lock, double-check, insert
        let mut values = self.values.lock().unwrap();
        if let Some(id) = self.by_value.get(bytes.as_slice()) {
            return *id;
        }
        let vec = Arc::make_mut(&mut values);
        let id = InternId::new(vec.len());
        let shared: Arc<[u8]> = Arc::from(bytes);
        vec.push(Arc::clone(&shared));
        self.by_value.insert(shared, id);
        id
    }

    /// Read and reset per-batch intern counters. Called once per batch.
    pub(crate) fn drain_counters(&self) -> (u64, u64) {
        let total = self.intern_call_count.swap(0, Ordering::Relaxed);
        let slow = self.intern_slow_path_count.swap(0, Ordering::Relaxed);
        (total, slow)
    }

    /// Create a frozen snapshot for read-only access during reduce phase.
    ///
    /// O(1): clones the outer `Arc` (single atomic increment), not the
    /// millions of inner `Arc<[u8]>` pointers.
    ///
    /// # Precondition
    ///
    /// All concurrent `intern_bytes()` calls must have completed before calling
    /// this method. Calling it while `intern_bytes()` is still active on another
    /// thread may produce a snapshot with fewer entries than have been interned,
    /// causing `FrozenIdentityView::resolve_bytes()` to panic on missing IDs.
    pub(crate) fn snapshot_for_reads(&self) -> FrozenIdentityView {
        let values = self.values.lock().unwrap();
        FrozenIdentityView {
            values: Arc::clone(&values),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.by_value.len()
    }

    pub(crate) fn estimated_bytes(&self) -> u64 {
        let values = self.values.lock().unwrap();
        // The DashMap key and the ID table share one Arc payload. Count the
        // payload once, plus one Arc handle in each container.
        let map_overhead = self.by_value.len() as u64
            * (64 + std::mem::size_of::<Arc<[u8]>>() + std::mem::size_of::<InternId>()) as u64;
        let values_bytes: u64 = values
            .iter()
            .map(|v| v.len() as u64 + std::mem::size_of::<Arc<[u8]>>() as u64)
            .sum();
        std::mem::size_of::<Self>() as u64 + map_overhead + values_bytes
    }

    #[cfg(test)]
    fn storage_pointers_for_test(&self, bytes: &[u8], id: InternId) -> (*const u8, *const u8) {
        let map_ptr = self
            .by_value
            .get(bytes)
            .expect("test identity must exist in lookup map")
            .key()
            .as_ptr();
        let values = self.values.lock().unwrap();
        let table_ptr = values
            .get(id.as_usize())
            .expect("test identity ID must exist in value table")
            .as_ptr();
        (map_ptr, table_ptr)
    }
}

/// Frozen snapshot for lock-free reads. Send + Sync safe.
#[derive(Debug)]
pub(crate) struct FrozenIdentityView {
    values: Arc<Vec<Arc<[u8]>>>,
}

impl FrozenIdentityView {
    pub(crate) fn resolve_bytes(&self, id: InternId) -> &[u8] {
        self.values.get(id.as_usize()).unwrap_or_else(|| {
            panic!(
                "missing interned identity bytes for id={} values_len={}",
                id.as_usize(),
                self.values.len()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::IdentityInterner;
    use crate::sync::types::InternId;

    #[test]
    fn interner_with_capacity_works() {
        let interner = IdentityInterner::with_capacity(100);
        let id = interner.intern_bytes(vec![1, 2, 3]);
        let frozen = interner.snapshot_for_reads();
        assert_eq!(frozen.resolve_bytes(id), &[1, 2, 3]);
    }

    #[test]
    fn script_identity_interner_reuses_existing_id() {
        let interner = IdentityInterner::default();
        let first = interner.intern_bytes(vec![1, 2, 3]);
        let second = interner.intern_bytes(vec![1, 2, 3]);

        assert_eq!(first, second);
    }

    #[test]
    fn interner_map_and_id_table_share_one_identity_payload_allocation() {
        let interner = IdentityInterner::default();
        let bytes = vec![0xAB; 32];
        let id = interner.intern_bytes(bytes.clone());

        let (map_ptr, table_ptr) = interner.storage_pointers_for_test(&bytes, id);
        assert_eq!(
            map_ptr, table_ptr,
            "the lookup key and ID table must share the same Arc<[u8]> payload"
        );
    }

    #[test]
    fn script_identity_interner_assigns_new_ids_for_new_values() {
        let interner = IdentityInterner::default();
        let first = interner.intern_bytes(vec![1, 2, 3]);
        let second = interner.intern_bytes(vec![4, 5, 6]);

        assert_ne!(first, second);
        assert_eq!(interner.intern_bytes(vec![1, 2, 3]), first);
        assert_eq!(interner.intern_bytes(vec![4, 5, 6]), second);
    }

    #[test]
    fn script_identity_interner_resolves_interned_bytes_by_id() {
        let interner = IdentityInterner::default();
        let id = interner.intern_bytes(vec![0x11, 0x22, 0x33]);
        let frozen = interner.snapshot_for_reads();

        assert_eq!(frozen.resolve_bytes(id), &[0x11, 0x22, 0x33]);
    }

    #[test]
    fn concurrent_intern_bytes_assigns_unique_ids() {
        use std::collections::HashSet;
        use std::sync::Arc;

        let interner = Arc::new(IdentityInterner::default());
        let num_threads = 8;
        let values_per_thread = 500;
        // Each thread interns both shared (overlapping) and unique values.
        let shared_values: Vec<Vec<u8>> = (0u16..100).map(|i| i.to_le_bytes().to_vec()).collect();

        std::thread::scope(|s| {
            let handles: Vec<_> = (0..num_threads)
                .map(|t| {
                    let interner = Arc::clone(&interner);
                    let shared = shared_values.clone();
                    s.spawn(move || {
                        let mut ids = Vec::new();
                        // Intern shared values (all threads compete for these)
                        for v in &shared {
                            ids.push((v.clone(), interner.intern_bytes(v.clone())));
                        }
                        // Intern thread-unique values
                        for i in 0..values_per_thread {
                            let v = format!("thread-{}-val-{}", t, i).into_bytes();
                            ids.push((v.clone(), interner.intern_bytes(v)));
                        }
                        ids
                    })
                })
                .collect();

            let all_results: Vec<Vec<(Vec<u8>, InternId)>> =
                handles.into_iter().map(|h| h.join().unwrap()).collect();

            // Verify: same bytes always get the same ID across all threads.
            let mut canonical: std::collections::HashMap<Vec<u8>, InternId> =
                std::collections::HashMap::new();
            for results in &all_results {
                for (bytes, id) in results {
                    if let Some(&expected) = canonical.get(bytes) {
                        assert_eq!(
                            *id,
                            expected,
                            "same bytes got different IDs: bytes=0x{}",
                            hex::encode(bytes)
                        );
                    } else {
                        canonical.insert(bytes.clone(), *id);
                    }
                }
            }

            // Verify: no two different byte sequences share the same ID.
            let unique_ids: HashSet<u32> =
                canonical.values().map(|id| id.as_usize() as u32).collect();
            assert_eq!(
                unique_ids.len(),
                canonical.len(),
                "duplicate IDs assigned to different byte sequences"
            );

            // Verify: snapshot resolves all interned values correctly.
            let frozen = interner.snapshot_for_reads();
            for (bytes, id) in &canonical {
                assert_eq!(
                    frozen.resolve_bytes(*id),
                    bytes.as_slice(),
                    "snapshot mismatch for id={}",
                    id.as_usize()
                );
            }

            // Verify: expected total count.
            let expected_unique = shared_values.len() + num_threads * values_per_thread;
            assert_eq!(canonical.len(), expected_unique);
        });
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    #[should_panic(expected = "intern id overflow: index")]
    fn script_identity_intern_id_overflows_u32() {
        InternId::new(u32::MAX as usize + 1);
    }

    #[test]
    fn drain_counters_returns_counts_and_resets() {
        let interner = IdentityInterner::default();
        // First intern: new value → slow path (Mutex)
        interner.intern_bytes(vec![1, 2, 3]);
        // Second intern: same value → fast path (DashMap hit)
        interner.intern_bytes(vec![1, 2, 3]);
        // Third intern: new value → slow path
        interner.intern_bytes(vec![4, 5, 6]);

        let (total, slow) = interner.drain_counters();
        assert_eq!(total, 3, "3 total intern_bytes calls");
        assert_eq!(slow, 2, "2 slow-path Mutex acquisitions (new identities)");

        // After drain, counters reset to zero
        let (total2, slow2) = interner.drain_counters();
        assert_eq!(total2, 0);
        assert_eq!(slow2, 0);
    }
}
