use std::collections::HashMap;

use crate::sync::types::InternId;

#[derive(Debug, Default)]
pub(crate) struct IdentityInterner {
    by_value: HashMap<Vec<u8>, InternId>,
    values: Vec<Vec<u8>>,
}

impl IdentityInterner {
    pub(crate) fn intern_bytes(&mut self, bytes: Vec<u8>) -> InternId {
        if let Some(existing) = self.by_value.get(&bytes) {
            return *existing;
        }

        let id = InternId::new(self.values.len());
        self.by_value.insert(bytes.clone(), id);
        self.values.push(bytes);
        id
    }

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
        let mut interner = IdentityInterner::default();
        let first = interner.intern_bytes(vec![1, 2, 3]);
        let second = interner.intern_bytes(vec![1, 2, 3]);

        assert_eq!(first, second);
    }

    #[test]
    fn script_identity_interner_assigns_new_ids_for_new_values() {
        let mut interner = IdentityInterner::default();
        let first = interner.intern_bytes(vec![1, 2, 3]);
        let second = interner.intern_bytes(vec![4, 5, 6]);

        assert_ne!(first, second);
        assert_eq!(interner.intern_bytes(vec![1, 2, 3]), first);
        assert_eq!(interner.intern_bytes(vec![4, 5, 6]), second);
    }

    #[test]
    fn script_identity_interner_resolves_interned_bytes_by_id() {
        let mut interner = IdentityInterner::default();
        let id = interner.intern_bytes(vec![0x11, 0x22, 0x33]);

        assert_eq!(interner.resolve_bytes(id), &[0x11, 0x22, 0x33]);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    #[should_panic(expected = "intern id overflow: index")]
    fn script_identity_intern_id_overflows_u32() {
        InternId::new(u32::MAX as usize + 1);
    }
}
