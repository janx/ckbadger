use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::mem::size_of;

use serde::Serialize;

pub(crate) fn serialized_bytes<T: Serialize>(value: &T) -> u64 {
    bincode::serialized_size(value).unwrap_or(0)
}

pub(crate) fn bytes_vec_bytes(value: &Vec<u8>) -> u64 {
    size_of::<Vec<u8>>() as u64 + value.capacity() as u64
}

pub(crate) fn hash_map_bytes<K, V, F>(map: &HashMap<K, V>, entry_bytes: F) -> u64
where
    F: Fn(&K, &V) -> u64,
{
    size_of::<HashMap<K, V>>() as u64
        + map.capacity() as u64 * size_of::<(K, V)>() as u64
        + map
            .iter()
            .map(|(key, value)| entry_bytes(key, value))
            .sum::<u64>()
}

pub(crate) fn hash_map_serialized_bytes<K, V>(map: &HashMap<K, V>) -> u64
where
    K: Serialize,
    V: Serialize,
{
    hash_map_bytes(map, |key, value| {
        serialized_bytes(key) + serialized_bytes(value)
    })
}

pub(crate) fn hash_set_serialized_bytes<T>(set: &HashSet<T>) -> u64
where
    T: Serialize,
{
    size_of::<HashSet<T>>() as u64
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
