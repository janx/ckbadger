//! A count-bounded in-memory cache for values that never change once known.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::Mutex;

/// At most `capacity` entries; inserting past it evicts the oldest insert.
///
/// For immutable facts keyed by what fixes them (e.g. a block hash), so there
/// is no TTL: an entry is only ever evicted, never stale. First in, first out
/// suits keys that arrive in chain order, where the oldest key is the one
/// least likely to be asked for again.
pub struct BoundedCache<K, V> {
    capacity: NonZeroUsize,
    inner: Mutex<Entries<K, V>>,
}

struct Entries<K, V> {
    values: HashMap<K, V>,
    /// Keys in insertion order, oldest first; exactly the keys of `values`.
    order: VecDeque<K>,
}

impl<K: Eq + Hash + Clone, V: Clone> BoundedCache<K, V> {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            inner: Mutex::new(Entries {
                values: HashMap::with_capacity(capacity.get()),
                order: VecDeque::with_capacity(capacity.get()),
            }),
        }
    }

    pub fn capacity(&self) -> NonZeroUsize {
        self.capacity
    }

    pub fn get(&self, key: &K) -> Option<V> {
        self.inner
            .lock()
            .expect("bounded cache poisoned")
            .values
            .get(key)
            .cloned()
    }

    /// Insert `value` under `key`. A key already present keeps its place in
    /// the eviction order and takes the new value.
    pub fn insert(&self, key: K, value: V) {
        let mut entries = self.inner.lock().expect("bounded cache poisoned");
        if entries.values.insert(key.clone(), value).is_some() {
            return;
        }
        entries.order.push_back(key);
        if entries.order.len() > self.capacity.get() {
            let oldest = entries
                .order
                .pop_front()
                .expect("order holds more keys than the capacity");
            entries.values.remove(&oldest);
        }
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("bounded cache poisoned")
            .values
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(capacity: usize) -> BoundedCache<u32, &'static str> {
        BoundedCache::new(NonZeroUsize::new(capacity).unwrap())
    }

    #[test]
    fn serves_what_was_inserted() {
        let c = cache(2);
        assert!(c.is_empty());
        c.insert(1, "one");
        assert_eq!(c.get(&1), Some("one"));
        assert_eq!(c.get(&2), None);
    }

    #[test]
    fn never_holds_more_than_its_capacity_and_evicts_the_oldest() {
        let c = cache(3);
        for key in 1..=5 {
            c.insert(key, "v");
        }
        assert_eq!(c.len(), 3);
        assert_eq!(c.get(&1), None);
        assert_eq!(c.get(&2), None);
        for key in 3..=5 {
            assert_eq!(c.get(&key), Some("v"), "key {key}");
        }
    }

    #[test]
    fn reinserting_a_key_neither_grows_nor_reorders_it() {
        let c = cache(2);
        c.insert(1, "a");
        c.insert(2, "b");
        c.insert(1, "a2");
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(&1), Some("a2"));
        // 1 is still the oldest insert, so it goes first.
        c.insert(3, "c");
        assert_eq!(c.get(&1), None);
        assert_eq!(c.get(&2), Some("b"));
        assert_eq!(c.get(&3), Some("c"));
    }
}
