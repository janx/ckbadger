//! Accumulates NFT lifecycle events within a batch and flushes them as
//! pre-computed `NftCollectionActivityEntry` rows into the
//! `CF_NFT_COLLECTION_ACTIVITIES` column family.
//!
//! Each `record()` call captures one raw event (Create or Consume) for a
//! single NFT.  `flush()` resolves per-NFT actions:
//!   - Create-only  → Mint
//!   - Both         → Transfer
//!   - Consume-only → Burn
//!
//! Then groups them by `(collection_id, tx_hash)` and writes one entry per
//! unique (collection, block, tx) triple.

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{AssetAction, NftCollectionActivityEntry};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawAction {
    Create,
    Consume,
}

/// Key: `(collection_id, tx_hash)`.
/// Value: block/tx metadata + per-NFT raw actions.
struct AccEntry {
    block_number: i64,
    tx_idx: i32,
    timestamp_ms: i64,
    /// `(nft_id, action)` pairs collected for this (collection, tx).
    nft_actions: Vec<(Vec<u8>, RawAction)>,
}

pub(crate) struct NftCollectionActivityAccumulator {
    entries: HashMap<(Vec<u8>, Vec<u8>), AccEntry>,
}

impl NftCollectionActivityAccumulator {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Record a raw NFT lifecycle event.
    ///
    /// `collection_id`: padded 32-byte collection identifier.
    /// `tx_hash`: the transaction where this event occurs.
    /// `nft_id`: the individual NFT identifier.
    /// `block_number`, `tx_idx`, `timestamp_ms`: positional metadata.
    /// `is_create`: `true` for Create (insert), `false` for Consume.
    pub fn record(
        &mut self,
        collection_id: &[u8],
        tx_hash: &[u8],
        nft_id: &[u8],
        block_number: i64,
        tx_idx: i32,
        timestamp_ms: i64,
        is_create: bool,
    ) {
        let key = (collection_id.to_vec(), tx_hash.to_vec());
        let action = if is_create {
            RawAction::Create
        } else {
            RawAction::Consume
        };
        let entry = self.entries.entry(key).or_insert_with(|| AccEntry {
            block_number,
            tx_idx,
            timestamp_ms,
            nft_actions: Vec::new(),
        });
        entry.nft_actions.push((nft_id.to_vec(), action));
    }

    /// Resolve raw actions into Mint/Transfer/Burn and write to the batch.
    pub fn flush(self, batch: &mut StoreBatch) {
        for ((collection_id, _tx_hash), entry) in self.entries {
            // Group by nft_id to detect transfers (create + consume of same NFT)
            let mut per_nft: HashMap<Vec<u8>, (bool, bool)> = HashMap::new();
            for (nft_id, action) in &entry.nft_actions {
                let pair = per_nft.entry(nft_id.clone()).or_insert((false, false));
                match action {
                    RawAction::Create => pair.0 = true,
                    RawAction::Consume => pair.1 = true,
                }
            }

            let mut actions = Vec::new();
            let mut has_mint = false;
            let mut has_transfer = false;
            let mut has_burn = false;
            for (created, consumed) in per_nft.values() {
                match (*created, *consumed) {
                    (true, true) => has_transfer = true,
                    (true, false) => has_mint = true,
                    (false, true) => has_burn = true,
                    (false, false) => {} // unreachable
                }
            }

            // Deterministic order: Mint, Transfer, Burn
            if has_mint {
                actions.push(AssetAction::Mint);
            }
            if has_transfer {
                actions.push(AssetAction::Transfer);
            }
            if has_burn {
                actions.push(AssetAction::Burn);
            }

            if actions.is_empty() {
                continue;
            }

            let activity_entry = NftCollectionActivityEntry {
                tx_hash: _tx_hash,
                timestamp_ms: entry.timestamp_ms,
                actions,
            };

            batch.put_nft_collection_activity(
                &collection_id,
                entry.block_number,
                entry.tx_idx,
                &activity_entry,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::CkbadgerStore;
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_mint_only() {
        let (_dir, store) = test_store();
        let mut acc = NftCollectionActivityAccumulator::new();
        let collection_id = [1u8; 32];
        let tx_hash = [2u8; 32];
        let nft_id = [3u8; 32];

        acc.record(&collection_id, &tx_hash, &nft_id, 100, 1, 1000, true);

        let mut batch = StoreBatch::new(&store);
        acc.flush(&mut batch);
        batch.commit().unwrap();

        let results = store
            .list_nft_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 100); // block_number
        assert_eq!(results[0].1, 1); // tx_idx
        assert_eq!(results[0].2.actions.len(), 1);
        assert!(matches!(results[0].2.actions[0], AssetAction::Mint));
    }

    #[test]
    fn test_transfer_create_and_consume_same_nft() {
        let (_dir, store) = test_store();
        let mut acc = NftCollectionActivityAccumulator::new();
        let collection_id = [1u8; 32];
        let tx_hash = [2u8; 32];
        let nft_id = [3u8; 32];

        // Same NFT created and consumed in same tx = Transfer
        acc.record(&collection_id, &tx_hash, &nft_id, 200, 5, 2000, false);
        acc.record(&collection_id, &tx_hash, &nft_id, 200, 5, 2000, true);

        let mut batch = StoreBatch::new(&store);
        acc.flush(&mut batch);
        batch.commit().unwrap();

        let results = store
            .list_nft_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2.actions.len(), 1);
        assert!(matches!(results[0].2.actions[0], AssetAction::Transfer));
    }

    #[test]
    fn test_burn_only() {
        let (_dir, store) = test_store();
        let mut acc = NftCollectionActivityAccumulator::new();
        let collection_id = [1u8; 32];
        let tx_hash = [2u8; 32];
        let nft_id = [3u8; 32];

        acc.record(&collection_id, &tx_hash, &nft_id, 300, 2, 3000, false);

        let mut batch = StoreBatch::new(&store);
        acc.flush(&mut batch);
        batch.commit().unwrap();

        let results = store
            .list_nft_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2.actions.len(), 1);
        assert!(matches!(results[0].2.actions[0], AssetAction::Burn));
    }

    #[test]
    fn test_batch_mint_multiple_nfts() {
        let (_dir, store) = test_store();
        let mut acc = NftCollectionActivityAccumulator::new();
        let collection_id = [1u8; 32];
        let tx_hash = [2u8; 32];
        let nft_a = [3u8; 32];
        let nft_b = [4u8; 32];

        acc.record(&collection_id, &tx_hash, &nft_a, 400, 0, 4000, true);
        acc.record(&collection_id, &tx_hash, &nft_b, 400, 0, 4000, true);

        let mut batch = StoreBatch::new(&store);
        acc.flush(&mut batch);
        batch.commit().unwrap();

        let results = store
            .list_nft_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        // Both mints → single Mint action
        assert_eq!(results[0].2.actions.len(), 1);
        assert!(matches!(results[0].2.actions[0], AssetAction::Mint));
    }

    #[test]
    fn test_mixed_actions_same_tx() {
        let (_dir, store) = test_store();
        let mut acc = NftCollectionActivityAccumulator::new();
        let collection_id = [1u8; 32];
        let tx_hash = [2u8; 32];
        let nft_a = [3u8; 32]; // gets minted
        let nft_b = [4u8; 32]; // gets burned

        acc.record(&collection_id, &tx_hash, &nft_a, 500, 3, 5000, true);
        acc.record(&collection_id, &tx_hash, &nft_b, 500, 3, 5000, false);

        let mut batch = StoreBatch::new(&store);
        acc.flush(&mut batch);
        batch.commit().unwrap();

        let results = store
            .list_nft_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2.actions.len(), 2);
        assert!(matches!(results[0].2.actions[0], AssetAction::Mint));
        assert!(matches!(results[0].2.actions[1], AssetAction::Burn));
    }

    #[test]
    fn test_multiple_collections_multiple_txs() {
        let (_dir, store) = test_store();
        let mut acc = NftCollectionActivityAccumulator::new();
        let coll_a = [1u8; 32];
        let coll_b = [2u8; 32];
        let tx1 = [10u8; 32];
        let tx2 = [11u8; 32];
        let nft = [3u8; 32];

        acc.record(&coll_a, &tx1, &nft, 100, 0, 1000, true);
        acc.record(&coll_b, &tx2, &nft, 200, 1, 2000, true);

        let mut batch = StoreBatch::new(&store);
        acc.flush(&mut batch);
        batch.commit().unwrap();

        let results_a = store
            .list_nft_collection_activities(&coll_a, 10, None, None)
            .unwrap();
        assert_eq!(results_a.len(), 1);

        let results_b = store
            .list_nft_collection_activities(&coll_b, 10, None, None)
            .unwrap();
        assert_eq!(results_b.len(), 1);
    }
}
