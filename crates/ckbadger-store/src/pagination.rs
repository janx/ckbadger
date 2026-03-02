//! Cursor-based pagination over prefix iterators.

use rocksdb::{ColumnFamily, IteratorMode};

use crate::store::CkbadgerStore;
use crate::types::Cursor;

/// Paginated result with items and optional next cursor.
#[derive(Debug)]
pub struct PaginatedResult<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<Cursor>,
    pub total_hint: Option<usize>,
}

impl CkbadgerStore {
    /// Iterate a CF with a prefix, returning up to `limit` items.
    /// If `cursor` is provided, start after that key.
    /// The `map_fn` converts (key, value) -> Option<T>; returning None skips the entry.
    pub fn paginate_prefix<T, F>(
        &self,
        cf: &ColumnFamily,
        prefix: &[u8],
        cursor: Option<&Cursor>,
        limit: usize,
        map_fn: F,
    ) -> PaginatedResult<T>
    where
        F: Fn(&[u8], &[u8]) -> Option<T>,
    {
        // Always use iterator_cf with From mode for consistency
        let start_key;
        let mode = match cursor {
            Some(c) => {
                start_key = c.last_key.clone();
                IteratorMode::From(&start_key, rocksdb::Direction::Forward)
            }
            None => {
                start_key = prefix.to_vec();
                IteratorMode::From(&start_key, rocksdb::Direction::Forward)
            }
        };

        let iter = self.iterator_cf(cf, mode);

        let mut items = Vec::with_capacity(limit);
        let mut last_key = None;
        let mut skip_first = cursor.is_some();

        for item in iter.flatten() {
            let (key, value) = item;

            // Check prefix match
            if !key.starts_with(prefix) {
                break;
            }

            // Skip cursor key itself
            if skip_first {
                if cursor
                    .map(|c| c.last_key.as_slice() == key.as_ref())
                    .unwrap_or(false)
                {
                    skip_first = false;
                    continue;
                }
                skip_first = false;
            }

            if let Some(mapped) = map_fn(&key, &value) {
                last_key = Some(key.to_vec());
                items.push(mapped);
                if items.len() >= limit {
                    break;
                }
            }
        }

        // Check if there are more items after the last one
        let has_more = if items.len() >= limit {
            if let Some(ref lk) = last_key {
                // Peek ahead to see if there's another item with matching prefix
                let peek_iter =
                    self.iterator_cf(cf, IteratorMode::From(lk, rocksdb::Direction::Forward));
                if let Some(peek_item) = peek_iter.flatten().nth(1) {
                    let (pk, _) = peek_item;
                    pk.starts_with(prefix)
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        let next_cursor = if has_more {
            last_key.map(|k| Cursor { last_key: k })
        } else {
            None
        };

        PaginatedResult {
            items,
            next_cursor,
            total_hint: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_paginate_prefix() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let cf = store.cf_stats_chain();
        for i in 0u32..10 {
            let key = [&[0x01u8][..], &i.to_be_bytes()].concat();
            store.put_cf(cf, &key, &i.to_le_bytes()).unwrap();
        }
        // Different prefix
        store.put_cf(cf, &[0x02, 0, 0, 0, 0], b"other").unwrap();

        let result: PaginatedResult<u32> =
            store.paginate_prefix(cf, &[0x01], None, 5, |_key, value| {
                Some(u32::from_le_bytes(value[..4].try_into().ok()?))
            });

        assert_eq!(result.items.len(), 5);
        assert!(result.next_cursor.is_some());
        assert_eq!(result.items, vec![0, 1, 2, 3, 4]);

        // Next page
        let result2: PaginatedResult<u32> = store.paginate_prefix(
            cf,
            &[0x01],
            result.next_cursor.as_ref(),
            5,
            |_key, value| Some(u32::from_le_bytes(value[..4].try_into().ok()?)),
        );

        assert_eq!(result2.items.len(), 5);
        assert!(result2.next_cursor.is_none());
        assert_eq!(result2.items, vec![5, 6, 7, 8, 9]);
    }
}
