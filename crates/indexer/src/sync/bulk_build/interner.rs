use dashmap::DashMap;
use std::sync::{Arc, Mutex};

use crate::sync::types::InternId;

/// Thread-safe identity interner for concurrent use during facts building.
#[derive(Debug)]
pub(crate) struct IdentityInterner {
    by_value: DashMap<Vec<u8>, InternId>,
    values: Mutex<Vec<Arc<[u8]>>>,
}

impl Default for IdentityInterner {
    fn default() -> Self {
        Self {
            by_value: DashMap::new(),
            values: Mutex::new(Vec::new()),
        }
    }
}

impl IdentityInterner {
    /// Create an interner with pre-allocated capacity to reduce reallocations.
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            by_value: DashMap::with_capacity(capacity),
            values: Mutex::new(Vec::with_capacity(capacity)),
        }
    }

    /// Intern a byte sequence. Thread-safe for concurrent callers.
    pub(crate) fn intern_bytes(&self, bytes: Vec<u8>) -> InternId {
        // Fast path: lock-free DashMap read
        if let Some(id) = self.by_value.get(&bytes) {
            return *id;
        }
        // Slow path: acquire values lock, double-check, insert
        let mut values = self.values.lock().unwrap();
        if let Some(id) = self.by_value.get(&bytes) {
            return *id;
        }
        let id = InternId::new(values.len());
        values.push(Arc::from(bytes.as_slice()));
        self.by_value.insert(bytes, id);
        id
    }

    /// Create a frozen snapshot for read-only access during reduce phase.
    ///
    /// Cloning the inner `Vec<Arc<[u8]>>` is cheap: it copies Arc pointers
    /// and bumps reference counts, without deep-copying the byte data.
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
            values: values.clone(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.by_value.len()
    }

    pub(crate) fn estimated_bytes(&self) -> u64 {
        let values = self.values.lock().unwrap();
        let map_overhead = self.by_value.len() as u64 * 80;
        let values_bytes: u64 = values.iter().map(|v| v.len() as u64 + 24).sum();
        std::mem::size_of::<Self>() as u64 + map_overhead + values_bytes
    }
}

/// Frozen snapshot for lock-free reads. Send + Sync safe.
#[derive(Debug)]
pub(crate) struct FrozenIdentityView {
    values: Vec<Arc<[u8]>>,
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
}
