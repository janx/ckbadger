use anyhow::{anyhow, bail, Result};
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
    /// Mutation and reclamation are only valid between frozen read views;
    /// attempting either while a view is alive is an invariant violation.
    storage: Mutex<InternerStorage>,
    intern_call_count: AtomicU64,
    intern_slow_path_count: AtomicU64,
}

#[derive(Debug)]
struct InternerStorage {
    values: Arc<Vec<Option<Arc<[u8]>>>>,
    free_ids: Vec<InternId>,
    payload_bytes: u64,
    lock_script_written: Vec<bool>,
    lock_script_written_count: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReclaimStats {
    pub(crate) identities: u64,
    pub(crate) payload_bytes: u64,
    pub(crate) invalidated_lock_script_markers: u64,
}

#[derive(Debug, Default)]
pub(crate) struct IdentityLiveness {
    live_refs: Vec<u32>,
    queued_for_reclaim: Vec<bool>,
    zero_candidates: Vec<InternId>,
}

impl Default for IdentityInterner {
    fn default() -> Self {
        Self {
            by_value: DashMap::new(),
            storage: Mutex::new(InternerStorage {
                values: Arc::new(Vec::new()),
                free_ids: Vec::new(),
                payload_bytes: 0,
                lock_script_written: Vec::new(),
                lock_script_written_count: 0,
            }),
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
            storage: Mutex::new(InternerStorage {
                values: Arc::new(Vec::with_capacity(capacity)),
                free_ids: Vec::new(),
                payload_bytes: 0,
                lock_script_written: Vec::with_capacity(capacity),
                lock_script_written_count: 0,
            }),
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
        // Slow path: acquire storage lock, double-check, insert.
        let mut storage = self.storage.lock().unwrap();
        if let Some(id) = self.by_value.get(bytes.as_slice()) {
            return *id;
        }
        let reused_id = storage.free_ids.pop();
        let shared: Arc<[u8]> = Arc::from(bytes);
        let payload_len = u64::try_from(shared.len()).unwrap_or_else(|_| {
            panic!(
                "intern identity payload length exceeds u64: payload_bytes={}",
                shared.len()
            )
        });
        storage.payload_bytes = storage
            .payload_bytes
            .checked_add(payload_len)
            .unwrap_or_else(|| {
                panic!(
                    "intern identity payload accounting overflow: current_bytes={} added_bytes={}",
                    storage.payload_bytes, payload_len
                )
            });

        let slots = storage.values.len();
        let active_identities = self.by_value.len();
        if let Some(id) = reused_id {
            assert!(
                !storage
                    .lock_script_written
                    .get(id.as_usize())
                    .copied()
                    .unwrap_or_else(|| panic!(
                        "free intern id is outside lock-script marker table: id={} marker_slots={}",
                        id.as_usize(),
                        storage.lock_script_written.len(),
                    )),
                "free intern id still has a lock-script write marker: id={}",
                id.as_usize()
            );
        }
        let Some(values) = Arc::get_mut(&mut storage.values) else {
            panic!(
                "cannot intern identity while a frozen identity view is alive: active_identities={} slots={}",
                active_identities, slots,
            );
        };
        let (id, appended_slot) = if let Some(id) = reused_id {
            let slots = values.len();
            let Some(slot) = values.get_mut(id.as_usize()) else {
                panic!(
                    "free intern id is outside the value table: id={} slots={}",
                    id.as_usize(),
                    slots,
                );
            };
            assert!(
                slot.is_none(),
                "free intern id still has a value: id={}",
                id.as_usize()
            );
            *slot = Some(Arc::clone(&shared));
            (id, false)
        } else {
            let id = InternId::new(values.len());
            values.push(Some(Arc::clone(&shared)));
            (id, true)
        };
        if appended_slot {
            storage.lock_script_written.push(false);
        }
        assert_eq!(
            storage.lock_script_written.len(),
            storage.values.len(),
            "intern value and lock-script marker tables diverged after insert"
        );
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
    /// this method. A later attempt to add a new identity while this view is
    /// alive fails immediately rather than cloning or exposing divergent tables.
    pub(crate) fn snapshot_for_reads(&self) -> FrozenIdentityView {
        let storage = self.storage.lock().unwrap();
        FrozenIdentityView {
            values: Arc::clone(&storage.values),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.by_value.len()
    }

    pub(crate) fn slot_len(&self) -> usize {
        self.storage.lock().unwrap().values.len()
    }

    pub(crate) fn free_len(&self) -> usize {
        self.storage.lock().unwrap().free_ids.len()
    }

    pub(crate) fn lock_script_written_count(&self) -> usize {
        self.storage.lock().unwrap().lock_script_written_count
    }

    pub(crate) fn mark_lock_script_written(&self, id: InternId) -> Result<bool> {
        let mut storage = self.storage.lock().unwrap();
        let slots = storage.values.len();
        let is_active = storage
            .values
            .get(id.as_usize())
            .and_then(Option::as_ref)
            .is_some();
        if !is_active {
            bail!(
                "cannot mark an inactive intern id as a written lock script: id={} slots={}",
                id.as_usize(),
                slots,
            );
        }
        let marker_slots = storage.lock_script_written.len();
        let marker = storage
            .lock_script_written
            .get_mut(id.as_usize())
            .ok_or_else(|| {
                anyhow!(
                    "lock-script marker table is missing an active intern id: id={} marker_slots={} value_slots={}",
                    id.as_usize(),
                    marker_slots,
                    slots,
                )
            })?;
        if *marker {
            return Ok(false);
        }
        *marker = true;
        storage.lock_script_written_count = storage
            .lock_script_written_count
            .checked_add(1)
            .ok_or_else(|| {
                anyhow!(
                    "written lock-script identity count overflow: id={}",
                    id.as_usize()
                )
            })?;
        Ok(true)
    }

    pub(crate) fn estimated_bytes(&self) -> u64 {
        let storage = self.storage.lock().unwrap();
        // The DashMap key and the ID table share one Arc payload. Count the
        // payload once, plus the allocated ID table and free-list capacities.
        let map_overhead = self.by_value.capacity() as u64
            * (64 + std::mem::size_of::<Arc<[u8]>>() + std::mem::size_of::<InternId>()) as u64;
        let value_table_bytes =
            storage.values.capacity() as u64 * std::mem::size_of::<Option<Arc<[u8]>>>() as u64;
        let free_list_bytes =
            storage.free_ids.capacity() as u64 * std::mem::size_of::<InternId>() as u64;
        let lock_script_marker_bytes = (storage.lock_script_written.capacity() as u64).div_ceil(8);
        std::mem::size_of::<Self>() as u64
            + map_overhead
            + value_table_bytes
            + free_list_bytes
            + lock_script_marker_bytes
            + storage.payload_bytes
    }

    pub(crate) fn reclaim_zero_ref_identities(
        &self,
        candidates: &[InternId],
    ) -> Result<ReclaimStats> {
        if candidates.is_empty() {
            return Ok(ReclaimStats::default());
        }

        let mut storage = self.storage.lock().unwrap();
        let strong_count = Arc::strong_count(&storage.values);
        if strong_count != 1 {
            bail!(
                "cannot reclaim interned identities while frozen views are alive: strong_count={} candidates={} active_identities={} slots={}",
                strong_count,
                candidates.len(),
                self.by_value.len(),
                storage.values.len(),
            );
        }

        let mut stats = ReclaimStats::default();
        for &id in candidates {
            let shared = {
                let strong_count = Arc::strong_count(&storage.values);
                let values = Arc::get_mut(&mut storage.values).ok_or_else(|| {
                    anyhow!(
                        "intern value table unexpectedly shared during reclaim: id={} strong_count={}",
                        id.as_usize(),
                        strong_count,
                    )
                })?;
                let slots = values.len();
                values
                    .get_mut(id.as_usize())
                    .ok_or_else(|| {
                        anyhow!(
                            "reclaim candidate is outside intern value table: id={} slots={}",
                            id.as_usize(),
                            slots,
                        )
                    })?
                    .take()
                    .ok_or_else(|| {
                        anyhow!("reclaim candidate is already free: id={}", id.as_usize())
                    })?
            };

            let removed = self.by_value.remove(shared.as_ref()).ok_or_else(|| {
                anyhow!(
                    "reclaim candidate is missing from identity lookup map: id={} payload=0x{}",
                    id.as_usize(),
                    hex::encode(shared.as_ref()),
                )
            })?;
            if removed.1 != id {
                bail!(
                    "identity lookup map returned the wrong id during reclaim: expected_id={} actual_id={} payload=0x{}",
                    id.as_usize(),
                    removed.1.as_usize(),
                    hex::encode(shared.as_ref()),
                );
            }

            let payload_len = u64::try_from(shared.len()).map_err(|_| {
                anyhow!(
                    "reclaimed identity payload length exceeds u64: id={} payload_bytes={}",
                    id.as_usize(),
                    shared.len(),
                )
            })?;
            storage.payload_bytes = storage
                .payload_bytes
                .checked_sub(payload_len)
                .ok_or_else(|| {
                    anyhow!(
                        "identity payload accounting underflow during reclaim: id={} current_bytes={} removed_bytes={}",
                        id.as_usize(),
                        storage.payload_bytes,
                        payload_len,
                    )
                })?;
            storage.free_ids.push(id);
            let marker_slots = storage.lock_script_written.len();
            let marker = storage
                .lock_script_written
                .get_mut(id.as_usize())
                .ok_or_else(|| {
                    anyhow!(
                        "lock-script marker table is missing a reclaimed intern id: id={} marker_slots={}",
                        id.as_usize(),
                        marker_slots,
                    )
                })?;
            if *marker {
                *marker = false;
                storage.lock_script_written_count = storage
                    .lock_script_written_count
                    .checked_sub(1)
                    .ok_or_else(|| {
                        anyhow!(
                            "written lock-script identity count underflow during reclaim: id={}",
                            id.as_usize()
                        )
                    })?;
                stats.invalidated_lock_script_markers = stats
                    .invalidated_lock_script_markers
                    .checked_add(1)
                    .ok_or_else(|| {
                        anyhow!(
                            "invalidated lock-script marker count overflow: id={}",
                            id.as_usize()
                        )
                    })?;
            }
            stats.identities = stats.identities.checked_add(1).ok_or_else(|| {
                anyhow!("reclaimed identity count overflow: id={}", id.as_usize())
            })?;
            stats.payload_bytes =
                stats
                    .payload_bytes
                    .checked_add(payload_len)
                    .ok_or_else(|| {
                        anyhow!(
                            "reclaimed identity byte count overflow: id={} payload_bytes={}",
                            id.as_usize(),
                            payload_len,
                        )
                    })?;
        }

        Ok(stats)
    }

    #[cfg(test)]
    fn storage_pointers_for_test(&self, bytes: &[u8], id: InternId) -> (*const u8, *const u8) {
        let map_ptr = self
            .by_value
            .get(bytes)
            .expect("test identity must exist in lookup map")
            .key()
            .as_ptr();
        let storage = self.storage.lock().unwrap();
        let table_ptr = storage
            .values
            .get(id.as_usize())
            .expect("test identity ID must exist in value table")
            .as_ref()
            .expect("test identity ID must be active")
            .as_ptr();
        (map_ptr, table_ptr)
    }
}

impl IdentityLiveness {
    pub(crate) fn ensure_slots(&mut self, slot_len: usize) {
        if self.live_refs.len() < slot_len {
            self.live_refs.resize(slot_len, 0);
            self.queued_for_reclaim.resize(slot_len, false);
        }
    }

    pub(crate) fn retain(&mut self, id: InternId) -> Result<()> {
        let refs_len = self.live_refs.len();
        let refs = self.live_refs.get_mut(id.as_usize()).ok_or_else(|| {
            anyhow!(
                "cannot retain unknown intern id: id={} liveness_slots={}",
                id.as_usize(),
                refs_len,
            )
        })?;
        *refs = refs.checked_add(1).ok_or_else(|| {
            anyhow!(
                "live identity reference count overflow: id={} current_refs={}",
                id.as_usize(),
                *refs,
            )
        })?;
        Ok(())
    }

    pub(crate) fn release(&mut self, id: InternId) -> Result<()> {
        let refs_len = self.live_refs.len();
        let refs = self.live_refs.get_mut(id.as_usize()).ok_or_else(|| {
            anyhow!(
                "cannot release unknown intern id: id={} liveness_slots={}",
                id.as_usize(),
                refs_len,
            )
        })?;
        *refs = refs.checked_sub(1).ok_or_else(|| {
            anyhow!(
                "live identity reference count underflow: id={} current_refs={}",
                id.as_usize(),
                *refs,
            )
        })?;
        if *refs == 0 && !self.queued_for_reclaim[id.as_usize()] {
            self.queued_for_reclaim[id.as_usize()] = true;
            self.zero_candidates.push(id);
        }
        Ok(())
    }

    pub(crate) fn drain_zero_candidates(&mut self) -> Vec<InternId> {
        let mut reclaimable = Vec::with_capacity(self.zero_candidates.len());
        for id in self.zero_candidates.drain(..) {
            self.queued_for_reclaim[id.as_usize()] = false;
            if self.live_refs[id.as_usize()] == 0 {
                reclaimable.push(id);
            }
        }
        reclaimable
    }

    #[cfg(test)]
    pub(crate) fn live_refs(&self, id: InternId) -> Option<u32> {
        self.live_refs.get(id.as_usize()).copied()
    }

    pub(crate) fn estimated_bytes(&self) -> u64 {
        std::mem::size_of::<Self>() as u64
            + self.live_refs.capacity() as u64 * std::mem::size_of::<u32>() as u64
            + (self.queued_for_reclaim.capacity() as u64).div_ceil(8)
            + self.zero_candidates.capacity() as u64 * std::mem::size_of::<InternId>() as u64
    }
}

/// Frozen snapshot for lock-free reads. Send + Sync safe.
#[derive(Debug)]
pub(crate) struct FrozenIdentityView {
    values: Arc<Vec<Option<Arc<[u8]>>>>,
}

impl FrozenIdentityView {
    pub(crate) fn resolve_bytes(&self, id: InternId) -> &[u8] {
        self.values
            .get(id.as_usize())
            .and_then(Option::as_deref)
            .unwrap_or_else(|| {
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
    use super::{IdentityInterner, IdentityLiveness};
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

    #[test]
    fn zero_ref_identity_is_reclaimed_and_its_slot_is_reused() {
        let interner = IdentityInterner::default();
        let old_id = interner.intern_bytes(vec![0x11; 32]);
        let mut liveness = IdentityLiveness::default();
        liveness.ensure_slots(interner.slot_len());
        liveness.retain(old_id).unwrap();
        liveness.release(old_id).unwrap();

        let candidates = liveness.drain_zero_candidates();
        let stats = interner.reclaim_zero_ref_identities(&candidates).unwrap();
        assert_eq!(stats.identities, 1);
        assert_eq!(stats.payload_bytes, 32);
        assert_eq!(interner.len(), 0);
        assert_eq!(interner.free_len(), 1);

        let new_id = interner.intern_bytes(vec![0x22; 20]);
        assert_eq!(new_id, old_id, "reclaimed ID slot must be reused");
        assert_eq!(interner.free_len(), 0);
        let frozen = interner.snapshot_for_reads();
        assert_eq!(frozen.resolve_bytes(new_id), &[0x22; 20]);
    }

    #[test]
    fn live_identity_is_preserved_and_reclaimed_lock_marker_is_invalidated() {
        let interner = IdentityInterner::default();
        let live_id = interner.intern_bytes(vec![0x33; 32]);
        let written_id = interner.intern_bytes(vec![0x44; 32]);
        let mut liveness = IdentityLiveness::default();
        liveness.ensure_slots(interner.slot_len());
        liveness.retain(live_id).unwrap();
        liveness.retain(written_id).unwrap();
        liveness.release(written_id).unwrap();
        assert!(interner.mark_lock_script_written(written_id).unwrap());

        let candidates = liveness.drain_zero_candidates();
        let stats = interner.reclaim_zero_ref_identities(&candidates).unwrap();
        assert_eq!(stats.identities, 1);
        assert_eq!(stats.payload_bytes, 32);
        assert_eq!(stats.invalidated_lock_script_markers, 1);
        assert_eq!(interner.lock_script_written_count(), 0);
        assert_eq!(interner.len(), 1);
        assert_eq!(liveness.live_refs(live_id), Some(1));

        let replacement_id = interner.intern_bytes(vec![0x66; 32]);
        assert_eq!(replacement_id, written_id);
        assert!(interner.mark_lock_script_written(replacement_id).unwrap());

        let frozen = interner.snapshot_for_reads();
        assert_eq!(frozen.resolve_bytes(live_id), &[0x33; 32]);
    }

    #[test]
    fn reclaim_fails_while_frozen_view_is_alive() {
        let interner = IdentityInterner::default();
        let id = interner.intern_bytes(vec![0x55; 32]);
        let mut liveness = IdentityLiveness::default();
        liveness.ensure_slots(interner.slot_len());
        liveness.retain(id).unwrap();
        liveness.release(id).unwrap();
        let candidates = liveness.drain_zero_candidates();
        let frozen = interner.snapshot_for_reads();

        let error = interner
            .reclaim_zero_ref_identities(&candidates)
            .unwrap_err();
        assert!(error.to_string().contains("frozen views are alive"));
        assert_eq!(frozen.resolve_bytes(id), &[0x55; 32]);
    }

    #[test]
    fn repeated_ephemeral_batches_reuse_slots_and_clear_write_markers() {
        let interner = IdentityInterner::default();
        let mut liveness = IdentityLiveness::default();

        for batch in 0..1_000u32 {
            let id = interner.intern_bytes(batch.to_le_bytes().to_vec());
            liveness.ensure_slots(interner.slot_len());
            liveness.retain(id).unwrap();
            liveness.release(id).unwrap();
            assert!(interner.mark_lock_script_written(id).unwrap());

            let candidates = liveness.drain_zero_candidates();
            let stats = interner.reclaim_zero_ref_identities(&candidates).unwrap();
            assert_eq!(stats.identities, 1, "batch={batch}");
            assert_eq!(stats.invalidated_lock_script_markers, 1, "batch={batch}");
            assert_eq!(interner.len(), 0, "batch={batch}");
            assert_eq!(interner.slot_len(), 1, "batch={batch}");
            assert_eq!(interner.free_len(), 1, "batch={batch}");
            assert_eq!(interner.lock_script_written_count(), 0, "batch={batch}");
        }
    }
}
