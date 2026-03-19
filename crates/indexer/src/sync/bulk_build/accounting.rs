use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::BuildHasher;
use std::mem::size_of;

use serde::Serialize;

pub(crate) fn serialized_bytes<T: Serialize>(value: &T) -> u64 {
    bincode::serialized_size(value).unwrap_or(0)
}

pub(crate) fn bytes_vec_bytes(value: &Vec<u8>) -> u64 {
    size_of::<Vec<u8>>() as u64 + value.capacity() as u64
}

pub(crate) fn hash_map_bytes<K, V, S, F>(map: &HashMap<K, V, S>, entry_bytes: F) -> u64
where
    S: BuildHasher,
    F: Fn(&K, &V) -> u64,
{
    size_of::<HashMap<K, V, S>>() as u64
        + map.capacity() as u64 * size_of::<(K, V)>() as u64
        + map
            .iter()
            .map(|(key, value)| entry_bytes(key, value))
            .sum::<u64>()
}

pub(crate) fn hash_map_serialized_bytes<K, V, S>(map: &HashMap<K, V, S>) -> u64
where
    K: Serialize,
    V: Serialize,
    S: BuildHasher,
{
    hash_map_bytes(map, |key, value| {
        serialized_bytes(key) + serialized_bytes(value)
    })
}

pub(crate) fn hash_set_serialized_bytes<T, S>(set: &HashSet<T, S>) -> u64
where
    T: Serialize,
    S: BuildHasher,
{
    size_of::<HashSet<T, S>>() as u64
        + set.capacity() as u64 * size_of::<T>() as u64
        + set.iter().map(serialized_bytes).sum::<u64>()
}

pub(crate) fn btree_map_serialized_bytes<K, V>(map: &BTreeMap<K, V>) -> u64
where
    K: Serialize,
    V: Serialize,
{
    size_of::<BTreeMap<K, V>>() as u64
        + map.len() as u64 * size_of::<(K, V)>() as u64
        + map
            .iter()
            .map(|(key, value)| serialized_bytes(key) + serialized_bytes(value))
            .sum::<u64>()
}

pub(crate) fn btree_set_serialized_bytes<T>(set: &BTreeSet<T>) -> u64
where
    T: Serialize,
{
    size_of::<BTreeSet<T>>() as u64
        + set.len() as u64 * size_of::<T>() as u64
        + set.iter().map(serialized_bytes).sum::<u64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_bytes_returns_positive_for_nonempty_value() {
        let value = vec![1u8, 2, 3, 4, 5];
        assert!(serialized_bytes(&value) > 0);
    }

    #[test]
    fn bytes_vec_bytes_includes_heap_capacity() {
        let value = vec![0u8; 100];
        let bytes = bytes_vec_bytes(&value);
        assert!(bytes >= 100 + size_of::<Vec<u8>>() as u64);
    }

    #[test]
    fn hash_map_bytes_grows_with_entries() {
        let mut map = HashMap::new();
        let empty = hash_map_bytes(&map, |k, v| serialized_bytes(k) + serialized_bytes(v));
        map.insert(vec![1u8; 32], vec![2u8; 64]);
        let with_one = hash_map_bytes(&map, |k, v| serialized_bytes(k) + serialized_bytes(v));
        assert!(with_one > empty);
    }

    #[test]
    fn hash_map_serialized_bytes_matches_custom_fn() {
        let mut map = HashMap::new();
        map.insert(vec![0xaau8; 16], vec![0xbbu8; 32]);
        let auto = hash_map_serialized_bytes(&map);
        let manual = hash_map_bytes(&map, |k, v| serialized_bytes(k) + serialized_bytes(v));
        assert_eq!(auto, manual);
    }

    #[test]
    fn hash_set_serialized_bytes_grows_with_entries() {
        let mut set = HashSet::new();
        let empty = hash_set_serialized_bytes(&set);
        set.insert(vec![0u8; 32]);
        let with_one = hash_set_serialized_bytes(&set);
        assert!(with_one > empty);
    }

    #[test]
    fn btree_map_serialized_bytes_grows_with_entries() {
        let mut map = BTreeMap::new();
        let empty = btree_map_serialized_bytes(&map);
        map.insert(vec![1u8; 32], vec![2u8; 64]);
        let with_one = btree_map_serialized_bytes(&map);
        assert!(with_one > empty);
    }

    #[test]
    fn btree_set_serialized_bytes_grows_with_entries() {
        let mut set = BTreeSet::new();
        let empty = btree_set_serialized_bytes(&set);
        set.insert(vec![0u8; 32]);
        let with_one = btree_set_serialized_bytes(&set);
        assert!(with_one > empty);
    }
}
