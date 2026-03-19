use dashmap::DashMap;
use std::sync::Mutex;

use crate::sync::types::InternId;

/// Thread-safe identity interner for concurrent use during facts building.
#[derive(Debug)]
pub(crate) struct IdentityInterner {
    by_value: DashMap<Vec<u8>, InternId>,
    values: Mutex<Vec<Vec<u8>>>,
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
        values.push(bytes.clone());
        self.by_value.insert(bytes, id);
        id
    }

    /// Create a frozen snapshot for zero-copy reads during reduce phase.
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

/// Frozen snapshot for lock-free, zero-copy reads. Send + Sync safe.
#[derive(Debug)]
pub(crate) struct FrozenIdentityView {
    values: Vec<Vec<u8>>,
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

    #[cfg(target_pointer_width = "64")]
    #[test]
    #[should_panic(expected = "intern id overflow: index")]
    fn script_identity_intern_id_overflows_u32() {
        InternId::new(u32::MAX as usize + 1);
    }
}
