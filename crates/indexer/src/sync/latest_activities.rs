//! In-memory ring buffer for global latest-activities feed.
//!
//! Maintains the most recent N activities across all addresses.
//! Serialized to CF_SYNC_META after each batch commit for cross-process
//! access by the API.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Mutex;

#[cfg(test)]
use ckbadger_store::types::ActivityEntry;
use ckbadger_store::types::LatestActivityItem;

use crate::db::writer::activities::OwnerActivity;
use crate::parser::cell::ParsedCell;

/// (code_hash, hash_type, args) tuple for lock script identification.
type LockScriptInfo = (Vec<u8>, i16, Vec<u8>);

/// Maximum items in the ring buffer.
const RING_BUFFER_CAPACITY: usize = 64;

pub struct LatestActivitiesBuffer {
    items: Mutex<VecDeque<LatestActivityItem>>,
}

impl Default for LatestActivitiesBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LatestActivitiesBuffer {
    pub fn new() -> Self {
        Self {
            items: Mutex::new(VecDeque::with_capacity(RING_BUFFER_CAPACITY)),
        }
    }

    /// Push new activities (newest first). Evicts oldest when full.
    pub fn push_batch(&self, new_items: Vec<LatestActivityItem>) {
        let mut buf = self.items.lock().expect("ring buffer lock poisoned");
        for item in new_items {
            if buf.len() >= RING_BUFFER_CAPACITY {
                buf.pop_back();
            }
            buf.push_front(item);
        }
    }

    /// Snapshot current buffer contents (newest first).
    pub fn snapshot(&self) -> Vec<LatestActivityItem> {
        let buf = self.items.lock().expect("ring buffer lock poisoned");
        buf.iter().cloned().collect()
    }
}

/// Build lock_hash -> (code_hash, hash_type, args) mapping from parsed output cells.
/// Input cells (InputCellView) don't carry lock script components, so input-only
/// addresses won't have CKB address info — API falls back to hex display for those.
pub fn collect_lock_scripts_from_outputs(
    outputs: &[ParsedCell],
) -> HashMap<Vec<u8>, LockScriptInfo> {
    let mut map = HashMap::new();
    for cell in outputs {
        if cell.lock_script_hash.len() == 32 {
            map.entry(cell.lock_script_hash.clone()).or_insert_with(|| {
                (
                    cell.lock_code_hash.clone(),
                    cell.lock_hash_type,
                    cell.lock_args.clone(),
                )
            });
        }
    }
    map
}

/// Convert activity triples + lock script map into LatestActivityItems.
pub fn to_latest_items(
    activities: &[OwnerActivity],
    lock_scripts: &HashMap<Vec<u8>, LockScriptInfo>,
) -> Vec<LatestActivityItem> {
    activities
        .iter()
        .filter(|(_, _, entry)| !entry.is_cellbase)
        .map(|(lock_hash, _, entry)| {
            let (code_hash, hash_type, args) = lock_scripts
                .get(lock_hash)
                .cloned()
                .unwrap_or_else(|| (Vec::new(), 0, Vec::new()));
            LatestActivityItem {
                lock_hash: lock_hash.clone(),
                lock_code_hash: code_hash,
                lock_hash_type: hash_type,
                lock_args: args,
                entry: entry.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(block_num: i64) -> LatestActivityItem {
        LatestActivityItem {
            lock_hash: vec![block_num as u8; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            entry: ActivityEntry {
                tx_hash: vec![block_num as u8; 32],
                block_hash: vec![0xA0; 32],
                block_number: block_num,
                tx_index: 0,
                timestamp: 1_700_000_000 + block_num,
                ckb_delta: 100_00000000,
                used_delta: 0,
                is_cellbase: false,
                has_type_script: false,
                asset_changes: vec![],
                script_calls: None,
                peers: vec![],
            },
        }
    }

    #[test]
    fn test_ring_buffer_push_and_snapshot() {
        let buf = LatestActivitiesBuffer::new();
        buf.push_batch(vec![make_item(1), make_item(2)]);
        let snap = buf.snapshot();
        assert_eq!(snap.len(), 2);
        // Newest first (push_front)
        assert_eq!(snap[0].entry.block_number, 2);
        assert_eq!(snap[1].entry.block_number, 1);
    }

    #[test]
    fn test_ring_buffer_evicts_oldest() {
        let buf = LatestActivitiesBuffer::new();
        for i in 0..70 {
            buf.push_batch(vec![make_item(i)]);
        }
        let snap = buf.snapshot();
        assert_eq!(snap.len(), 64);
        assert_eq!(snap[0].entry.block_number, 69);
        assert_eq!(snap[63].entry.block_number, 6);
    }

    #[test]
    fn test_collect_lock_scripts_from_outputs() {
        let cells = vec![
            ParsedCell {
                capacity: 100,
                lock_code_hash: vec![0x11; 32],
                lock_hash_type: 1,
                lock_args: vec![0x22; 20],
                lock_script_hash: vec![0xAA; 32],
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                type_script_hash: None,
                data_hash: vec![0; 32],
                data_size: 0,
                data: vec![],
            },
            ParsedCell {
                capacity: 200,
                lock_code_hash: vec![0x33; 32],
                lock_hash_type: 0,
                lock_args: vec![0x44; 20],
                lock_script_hash: vec![0xBB; 32],
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                type_script_hash: None,
                data_hash: vec![0; 32],
                data_size: 0,
                data: vec![],
            },
        ];
        let map = collect_lock_scripts_from_outputs(&cells);
        assert_eq!(map.len(), 2);
        let (code_hash, hash_type, args) = map.get(&vec![0xAA; 32]).unwrap();
        assert_eq!(code_hash, &vec![0x11; 32]);
        assert_eq!(*hash_type, 1);
        assert_eq!(args, &vec![0x22; 20]);
    }

    #[test]
    fn test_to_latest_items_with_missing_lock_script() {
        let activities = vec![(
            vec![0xFF; 32],
            vec![vec![0x11; 32]],
            ActivityEntry {
                tx_hash: vec![0x01; 32],
                block_hash: vec![0x02; 32],
                block_number: 100,
                tx_index: 0,
                timestamp: 1_700_000_000,
                ckb_delta: 50,
                used_delta: 0,
                is_cellbase: false,
                has_type_script: false,
                asset_changes: vec![],
                script_calls: None,
                peers: vec![],
            },
        )];
        let lock_scripts = HashMap::new(); // empty — no lock script info
        let items = to_latest_items(&activities, &lock_scripts);
        assert_eq!(items.len(), 1);
        assert!(items[0].lock_code_hash.is_empty());
        assert_eq!(items[0].lock_hash_type, 0);
        assert!(items[0].lock_args.is_empty());
        assert_eq!(items[0].entry.block_number, 100);
    }

    #[test]
    fn test_to_latest_items_filters_cellbase() {
        let activities = vec![
            (
                vec![0xAA; 32],
                vec![vec![0x11; 32]],
                ActivityEntry {
                    tx_hash: vec![0x01; 32],
                    block_hash: vec![0x02; 32],
                    block_number: 100,
                    tx_index: 0,
                    timestamp: 1_700_000_000,
                    ckb_delta: 1000,
                    used_delta: 0,
                    is_cellbase: true,
                    has_type_script: false,
                    asset_changes: vec![],
                    script_calls: None,
                    peers: vec![],
                },
            ),
            (
                vec![0xBB; 32],
                vec![vec![0x11; 32]],
                ActivityEntry {
                    tx_hash: vec![0x03; 32],
                    block_hash: vec![0x02; 32],
                    block_number: 100,
                    tx_index: 1,
                    timestamp: 1_700_000_000,
                    ckb_delta: 500,
                    used_delta: 0,
                    is_cellbase: false,
                    has_type_script: false,
                    asset_changes: vec![],
                    script_calls: None,
                    peers: vec![],
                },
            ),
        ];
        let lock_scripts = HashMap::new();
        let items = to_latest_items(&activities, &lock_scripts);
        assert_eq!(items.len(), 1, "cellbase activity should be filtered out");
        assert_eq!(items[0].lock_hash, vec![0xBB; 32]);
    }
}
