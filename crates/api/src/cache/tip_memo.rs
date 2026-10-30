//! One value computed for one canonical tip, held in API memory until the
//! tip moves. API memory only: nothing here is written to any store.

use std::sync::{Arc, Mutex};

/// `(tip number, tip hash, value)`.
type TipEntry<T> = (i64, Vec<u8>, Arc<T>);

/// The value of the newest tip it was computed for, keyed by the tip's number
/// AND hash, so a reorg to a different block at the same height is a miss.
///
/// A caller computes inside its request's pinned read view (tip and value
/// from one view), then [`TipMemo::set`]s it; the lock is never held while
/// computing, so a slow build never stalls other requests' pins.
pub struct TipMemo<T> {
    slot: Mutex<Option<TipEntry<T>>>,
}

impl<T> Default for TipMemo<T> {
    fn default() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }
}

impl<T> TipMemo<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The value computed for tip `(number, hash)`, if that is the memo's tip.
    pub fn get(&self, number: i64, hash: &[u8]) -> Option<Arc<T>> {
        let slot = self
            .slot
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        slot.as_ref()
            .filter(|(n, h, _)| *n == number && h.as_slice() == hash)
            .map(|(_, _, value)| value.clone())
    }

    /// Remember `value` as the one for tip `(number, hash)`.
    pub fn set(&self, number: i64, hash: Vec<u8>, value: Arc<T>) {
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        *slot = Some((number, hash, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memo_hits_only_its_exact_tip() {
        let memo = TipMemo::new();
        assert!(memo.get(5, &[1; 32]).is_none());
        memo.set(5, vec![1; 32], Arc::new("five"));
        assert_eq!(memo.get(5, &[1; 32]).as_deref(), Some(&"five"));
        assert!(memo.get(5, &[2; 32]).is_none(), "same height, other block");
        assert!(memo.get(6, &[1; 32]).is_none(), "another height");
        memo.set(6, vec![2; 32], Arc::new("six"));
        assert!(
            memo.get(5, &[1; 32]).is_none(),
            "only the newest tip is kept"
        );
    }
}
