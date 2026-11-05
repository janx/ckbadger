//! Accumulates object lifecycle events within a batch and flushes them as
//! pre-computed `ObjectCollectionActivityEntry` rows into the
//! `CF_OBJECT_COLLECTION_ACTIVITIES` and `CF_IDENTITY_COLLECTION_ACTIVITIES` column families.
//!
//! Each `record()` call captures one raw event (Create or Consume) for a
//! single object.  `flush()` resolves per-object actions:
//!   - Create-only  → Mint
//!   - Both         → Transfer
//!   - Consume-only → Burn
//!
//! Then groups them by `(collection_id, tx_hash)` and writes one entry per
//! unique (collection, block, tx) triple.

use anyhow::{anyhow, bail, Result};
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    AssetAction, CollectionActivityDailyBlockDeltas, IdentityCollectionAggregate,
    ObjectCollectionActivityEntry, DOTBIT_SENTINEL_COLLECTION,
};
use ckbadger_store::CkbadgerStore;
use std::collections::{BTreeMap, HashMap};

use super::dotbit::DotbitBatchState;
use super::mnft::MnftBatchState;
use super::spore::SporeBatchState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawAction {
    Create,
    Consume,
}

/// Key: `(collection_id, tx_hash)`.
/// Value: block/tx metadata + per-object raw actions.
struct AccEntry {
    block_number: i64,
    tx_idx: i32,
    timestamp_ms: i64,
    block_hash: Vec<u8>,
    /// `(object_id, action)` pairs collected for this (collection, tx).
    object_actions: Vec<(Vec<u8>, RawAction)>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedCollectionActivityEntry {
    pub collection_id: Vec<u8>,
    pub block_number: i64,
    pub tx_idx: i32,
    pub entry: ObjectCollectionActivityEntry,
}

pub(crate) struct ObjectCollectionActivityAccumulator {
    entries: HashMap<(Vec<u8>, Vec<u8>), AccEntry>,
}

impl ObjectCollectionActivityAccumulator {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Record a raw object lifecycle event.
    ///
    /// `collection_id`: padded 32-byte collection identifier.
    /// `tx_hash`: the transaction where this event occurs.
    /// `object_id`: the individual object identifier.
    /// `block_number`, `tx_idx`, `timestamp_ms`: positional metadata.
    /// `is_create`: `true` for Create (insert), `false` for Consume.
    pub fn record(
        &mut self,
        collection_id: &[u8],
        tx_hash: &[u8],
        object_id: &[u8],
        block_hash: &[u8],
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
            block_hash: block_hash.to_vec(),
            object_actions: Vec::new(),
        });
        entry.object_actions.push((object_id.to_vec(), action));
    }

    /// Resolve raw actions into Mint/Transfer/Burn and write to the batch
    /// using the object collection activity CF, folding each row into its
    /// `collection_activity_daily` day under the row's block (`daily`, the
    /// day of a block from `day_of_block`).
    ///
    /// Returns inserted `(collection_id, block_number, tx_idx, block_hash, tx_hash)` rows so callers
    /// can update block-scoped reorg indexes in the domain store.
    pub fn flush(
        self,
        batch: &mut StoreBatch,
        daily: &mut CollectionActivityDailyBlockDeltas,
        day_of_block: &dyn Fn(i64) -> anyhow::Result<u32>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i64, i32, Vec<u8>, Vec<u8>)>> {
        Self::flush_inner(
            self.into_resolved_entries(),
            batch,
            daily,
            day_of_block,
            |b, cid, block_num, tx_idx, entry| {
                b.put_object_collection_activity(cid, block_num, tx_idx, entry);
            },
        )
    }

    /// Resolve raw actions into Mint/Transfer/Burn and write to the batch
    /// using the identity collection activity CF, folding each row into its
    /// `collection_activity_daily` day.
    ///
    /// Returns inserted `(collection_id, block_number, tx_idx, block_hash, tx_hash)` rows so callers
    /// can update block-scoped reorg indexes in the domain store.
    pub fn flush_identity(
        self,
        batch: &mut StoreBatch,
        daily: &mut CollectionActivityDailyBlockDeltas,
        day_of_block: &dyn Fn(i64) -> anyhow::Result<u32>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i64, i32, Vec<u8>, Vec<u8>)>> {
        Self::flush_inner(
            self.into_resolved_entries(),
            batch,
            daily,
            day_of_block,
            |b, cid, block_num, tx_idx, entry| {
                b.put_identity_collection_activity(cid, block_num, tx_idx, entry);
            },
        )
    }

    pub fn into_resolved_entries(self) -> Vec<ResolvedCollectionActivityEntry> {
        Self::resolve_entries(self.entries)
    }

    fn resolve_entries(
        entries: HashMap<(Vec<u8>, Vec<u8>), AccEntry>,
    ) -> Vec<ResolvedCollectionActivityEntry> {
        let mut inserted = Vec::new();
        for ((collection_id, tx_hash), entry) in entries {
            // Group by object_id to detect transfers (create + consume of same object)
            let mut per_object: HashMap<Vec<u8>, (bool, bool)> = HashMap::new();
            for (object_id, action) in &entry.object_actions {
                let pair = per_object
                    .entry(object_id.clone())
                    .or_insert((false, false));
                match action {
                    RawAction::Create => pair.0 = true,
                    RawAction::Consume => pair.1 = true,
                }
            }

            let mut actions = Vec::new();
            let mut has_mint = false;
            let mut has_transfer = false;
            let mut has_burn = false;
            for (created, consumed) in per_object.values() {
                match (*created, *consumed) {
                    (true, true) => has_transfer = true,
                    (true, false) => has_mint = true,
                    (false, true) => has_burn = true,
                    (false, false) => {}
                }
            }

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

            inserted.push(ResolvedCollectionActivityEntry {
                collection_id,
                block_number: entry.block_number,
                tx_idx: entry.tx_idx,
                entry: ObjectCollectionActivityEntry {
                    tx_hash,
                    block_hash: entry.block_hash,
                    timestamp_ms: entry.timestamp_ms,
                    actions,
                },
            });
        }
        inserted
    }

    /// Shared flush logic: writes pre-resolved entries via the provided
    /// `write_fn` and folds each into its day.
    fn flush_inner<F>(
        entries: Vec<ResolvedCollectionActivityEntry>,
        batch: &mut StoreBatch,
        daily: &mut CollectionActivityDailyBlockDeltas,
        day_of_block: &dyn Fn(i64) -> anyhow::Result<u32>,
        write_fn: F,
    ) -> anyhow::Result<Vec<(Vec<u8>, i64, i32, Vec<u8>, Vec<u8>)>>
    where
        F: Fn(&mut StoreBatch, &[u8], i64, i32, &ObjectCollectionActivityEntry),
    {
        let mut inserted = Vec::new();
        for resolved in entries {
            write_fn(
                batch,
                &resolved.collection_id,
                resolved.block_number,
                resolved.tx_idx,
                &resolved.entry,
            );
            daily.add_row(
                resolved.block_number,
                &resolved.collection_id,
                day_of_block(resolved.block_number)?,
                &resolved.entry.actions,
            )?;
            inserted.push((
                resolved.collection_id,
                resolved.block_number,
                resolved.tx_idx,
                resolved.entry.block_hash.clone(),
                resolved.entry.tx_hash.clone(),
            ));
        }
        Ok(inserted)
    }
}

/// One block's collection activity rows, counted per collection: what the
/// block adds to each collection aggregate's `activities_count`.
///
/// The live writer applies it at the end of the block — after the block's
/// protocol writes and its activity pass, which writes the `.cell` feed — so
/// every `mnft_collection_agg` / `identity_agg` row reaches the block overlay
/// in ascending block order, and its value at the end of every block carries
/// that block's activity count (store refactor lane B, B-3).
#[derive(Debug)]
pub(crate) struct BlockCollectionActivityCounts {
    block: i64,
    object: BTreeMap<Vec<u8>, i64>,
    identity: BTreeMap<Vec<u8>, i64>,
}

impl BlockCollectionActivityCounts {
    pub(crate) fn new(block: i64) -> Self {
        Self {
            block,
            object: BTreeMap::new(),
            identity: BTreeMap::new(),
        }
    }

    /// One `object_collection_activities` row of the block.
    pub(crate) fn add_object_row(&mut self, collection_id: &[u8], row_block: i64) -> Result<()> {
        Self::add_row(
            &mut self.object,
            self.block,
            collection_id,
            row_block,
            "object",
        )
    }

    /// One `identity_collection_activities` row of the block.
    pub(crate) fn add_identity_row(&mut self, collection_id: &[u8], row_block: i64) -> Result<()> {
        Self::add_row(
            &mut self.identity,
            self.block,
            collection_id,
            row_block,
            "identity",
        )
    }

    fn add_row(
        counts: &mut BTreeMap<Vec<u8>, i64>,
        block: i64,
        collection_id: &[u8],
        row_block: i64,
        what: &str,
    ) -> Result<()> {
        if row_block != block {
            bail!(
                "{what} collection activity row of block {row_block} counted in block {block}: collection_id=0x{}",
                hex::encode(collection_id)
            );
        }
        let count = counts.entry(collection_id.to_vec()).or_insert(0);
        *count = count.checked_add(1).ok_or_else(|| {
            anyhow!(
                "{what} collection activity row count overflow: collection_id=0x{}, block={block}",
                hex::encode(collection_id)
            )
        })?;
        Ok(())
    }

    /// Add the block's rows to their collections' `activities_count`, each
    /// on top of the aggregate the batch's writers left (their per-batch
    /// states hold it), in the block's scope (`batch`).
    ///
    /// Object rows of an mNFT class go to `mnft_collection_agg`; a Spore
    /// cluster's rows share the activity CF but have no such aggregate. The
    /// `.bit` AccountCell collection's identity aggregate is the `.bit`
    /// writer's, every other identity collection's (did:ckb, `.bit Cell`,
    /// `.cell`) the Spore writer's; a collection cached by the other writer
    /// is an error, never a silently stale count.
    pub(crate) fn apply(
        self,
        store: &CkbadgerStore,
        mnft_state: &mut MnftBatchState,
        spore_state: &mut SporeBatchState,
        dotbit_state: &mut DotbitBatchState,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let block = self.block;
        for (collection_id, rows) in &self.object {
            if mnft_state.add_collection_activities(store, collection_id, *rows, block, batch)? {
                continue;
            }
            if spore_state.knows_cluster(store, collection_id)? {
                continue;
            }
            bail!(
                "missing collection aggregate while applying activity_count delta: collection_id=0x{}, block={block}",
                hex::encode(collection_id)
            );
        }
        for (collection_id, rows) in &self.identity {
            let dotbit_owned = collection_id.as_slice() == DOTBIT_SENTINEL_COLLECTION.as_slice();
            let (owner, other) = if dotbit_owned {
                (".bit", spore_state.pending_identity_aggs())
            } else {
                ("Spore", dotbit_state.pending_identity_aggs())
            };
            if other.contains_key(collection_id) {
                bail!(
                    "identity collection aggregate cached outside its writer ({owner}): collection_id=0x{}, block={block}",
                    hex::encode(collection_id)
                );
            }
            if dotbit_owned {
                dotbit_state.add_identity_collection_activities(
                    store,
                    collection_id,
                    *rows,
                    block,
                    batch,
                )?;
            } else {
                spore_state.add_identity_collection_activities(
                    store,
                    collection_id,
                    *rows,
                    block,
                    batch,
                )?;
            }
        }
        Ok(())
    }
}

/// `current` activities count plus `rows` rows of `block`.
pub(crate) fn added_activities(
    current: i64,
    rows: i64,
    what: &str,
    collection_id: &[u8],
    block: i64,
) -> Result<i64> {
    if rows <= 0 {
        bail!(
            "{what} collection activity count of a block must be positive: collection_id=0x{}, block={block}, rows={rows}",
            hex::encode(collection_id)
        );
    }
    current.checked_add(rows).ok_or_else(|| {
        anyhow!(
            "{what} collection activities_count overflow: collection_id=0x{}, block={block}, current={current}, rows={rows}",
            hex::encode(collection_id)
        )
    })
}

/// An identity collection aggregate with `rows` more activity rows of
/// `block`: on top of `cached` (the batch's current value), else the
/// committed one, which must exist.
pub(crate) fn identity_aggregate_with_activities(
    cached: Option<&IdentityCollectionAggregate>,
    store: &CkbadgerStore,
    collection_id: &[u8],
    rows: i64,
    block: i64,
) -> Result<IdentityCollectionAggregate> {
    let mut agg = match cached {
        Some(agg) => agg.clone(),
        None => store
            .get_identity_collection_aggregate(collection_id)?
            .ok_or_else(|| {
                anyhow!(
                    "missing identity collection aggregate while applying activity_count delta: collection_id=0x{}, block={block}",
                    hex::encode(collection_id)
                )
            })?,
    };
    agg.activities_count =
        added_activities(agg.activities_count, rows, "identity", collection_id, block)?;
    Ok(agg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::CkbadgerStore;
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_mint_only() {
        let (_dir, store) = test_store();
        let mut acc = ObjectCollectionActivityAccumulator::new();
        let collection_id = [1u8; 32];
        let tx_hash = [2u8; 32];
        let block_hash = [0xA1u8; 32];
        let object_id = [3u8; 32];

        acc.record(
            &collection_id,
            &tx_hash,
            &object_id,
            &block_hash,
            100,
            1,
            1000,
            true,
        );

        let mut batch = StoreBatch::new(&store);
        acc.flush(
            &mut batch,
            &mut CollectionActivityDailyBlockDeltas::new(),
            &|_| Ok(20231115),
        )
        .unwrap();
        batch.commit().unwrap();

        let results = store
            .list_object_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 100); // block_number
        assert_eq!(results[0].1, 1); // tx_idx
        assert_eq!(results[0].2.actions.len(), 1);
        assert!(matches!(results[0].2.actions[0], AssetAction::Mint));
    }

    #[test]
    fn test_transfer_create_and_consume_same_object() {
        let (_dir, store) = test_store();
        let mut acc = ObjectCollectionActivityAccumulator::new();
        let collection_id = [1u8; 32];
        let tx_hash = [2u8; 32];
        let block_hash = [0xA2u8; 32];
        let object_id = [3u8; 32];

        // Same object created and consumed in same tx = Transfer
        acc.record(
            &collection_id,
            &tx_hash,
            &object_id,
            &block_hash,
            200,
            5,
            2000,
            false,
        );
        acc.record(
            &collection_id,
            &tx_hash,
            &object_id,
            &block_hash,
            200,
            5,
            2000,
            true,
        );

        let mut batch = StoreBatch::new(&store);
        acc.flush(
            &mut batch,
            &mut CollectionActivityDailyBlockDeltas::new(),
            &|_| Ok(20231115),
        )
        .unwrap();
        batch.commit().unwrap();

        let results = store
            .list_object_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2.actions.len(), 1);
        assert!(matches!(results[0].2.actions[0], AssetAction::Transfer));
    }

    #[test]
    fn test_burn_only() {
        let (_dir, store) = test_store();
        let mut acc = ObjectCollectionActivityAccumulator::new();
        let collection_id = [1u8; 32];
        let tx_hash = [2u8; 32];
        let block_hash = [0xA3u8; 32];
        let object_id = [3u8; 32];

        acc.record(
            &collection_id,
            &tx_hash,
            &object_id,
            &block_hash,
            300,
            2,
            3000,
            false,
        );

        let mut batch = StoreBatch::new(&store);
        acc.flush(
            &mut batch,
            &mut CollectionActivityDailyBlockDeltas::new(),
            &|_| Ok(20231115),
        )
        .unwrap();
        batch.commit().unwrap();

        let results = store
            .list_object_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2.actions.len(), 1);
        assert!(matches!(results[0].2.actions[0], AssetAction::Burn));
    }

    #[test]
    fn test_batch_mint_multiple_objects() {
        let (_dir, store) = test_store();
        let mut acc = ObjectCollectionActivityAccumulator::new();
        let collection_id = [1u8; 32];
        let tx_hash = [2u8; 32];
        let block_hash = [0xA4u8; 32];
        let object_a = [3u8; 32];
        let object_b = [4u8; 32];

        acc.record(
            &collection_id,
            &tx_hash,
            &object_a,
            &block_hash,
            400,
            0,
            4000,
            true,
        );
        acc.record(
            &collection_id,
            &tx_hash,
            &object_b,
            &block_hash,
            400,
            0,
            4000,
            true,
        );

        let mut batch = StoreBatch::new(&store);
        acc.flush(
            &mut batch,
            &mut CollectionActivityDailyBlockDeltas::new(),
            &|_| Ok(20231115),
        )
        .unwrap();
        batch.commit().unwrap();

        let results = store
            .list_object_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        // Both mints → single Mint action
        assert_eq!(results[0].2.actions.len(), 1);
        assert!(matches!(results[0].2.actions[0], AssetAction::Mint));
    }

    #[test]
    fn test_mixed_actions_same_tx() {
        let (_dir, store) = test_store();
        let mut acc = ObjectCollectionActivityAccumulator::new();
        let collection_id = [1u8; 32];
        let tx_hash = [2u8; 32];
        let block_hash = [0xA5u8; 32];
        let object_a = [3u8; 32]; // gets minted
        let object_b = [4u8; 32]; // gets burned

        acc.record(
            &collection_id,
            &tx_hash,
            &object_a,
            &block_hash,
            500,
            3,
            5000,
            true,
        );
        acc.record(
            &collection_id,
            &tx_hash,
            &object_b,
            &block_hash,
            500,
            3,
            5000,
            false,
        );

        let mut batch = StoreBatch::new(&store);
        acc.flush(
            &mut batch,
            &mut CollectionActivityDailyBlockDeltas::new(),
            &|_| Ok(20231115),
        )
        .unwrap();
        batch.commit().unwrap();

        let results = store
            .list_object_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2.actions.len(), 2);
        assert!(matches!(results[0].2.actions[0], AssetAction::Mint));
        assert!(matches!(results[0].2.actions[1], AssetAction::Burn));
    }

    #[test]
    fn test_flush_identity_mint() {
        let (_dir, store) = test_store();
        let mut acc = ObjectCollectionActivityAccumulator::new();
        let collection_id = *b"dotbit_collection_______________";
        let tx_hash = [2u8; 32];
        let block_hash = [0xA1u8; 32];
        let object_id = [3u8; 32];

        acc.record(
            &collection_id,
            &tx_hash,
            &object_id,
            &block_hash,
            100,
            1,
            1000,
            true,
        );

        let mut batch = StoreBatch::new(&store);
        let inserted = acc
            .flush_identity(
                &mut batch,
                &mut CollectionActivityDailyBlockDeltas::new(),
                &|_| Ok(20231115),
            )
            .unwrap();
        batch.commit().unwrap();

        assert_eq!(inserted.len(), 1);

        // Verify it was written to identity CF, not object CF
        let identity_results = store
            .list_identity_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(identity_results.len(), 1);
        assert!(matches!(
            identity_results[0].2.actions[0],
            AssetAction::Mint
        ));

        // Verify it was NOT written to object CF
        let object_results = store
            .list_object_collection_activities(&collection_id, 10, None, None)
            .unwrap();
        assert_eq!(object_results.len(), 0);
    }

    /// Regression test: verifies that flush_identity return value can be used
    /// to compute activity count deltas for the collection aggregate.
    /// This pattern is used in the pipeline bulk-sync path (T6a) where
    /// activities are committed separately from aggregates.
    #[test]
    fn test_flush_identity_returns_deltas_for_aggregate_update() {
        let (_dir, store) = test_store();
        let mut acc = ObjectCollectionActivityAccumulator::new();
        let coll = *b"did_ckb_collection______________";
        let tx1 = [10u8; 32];
        let tx2 = [11u8; 32];
        let block_hash = [0xC1u8; 32];
        let object_a = [3u8; 32];
        let object_b = [4u8; 32];

        // Two mint events across two transactions
        acc.record(&coll, &tx1, &object_a, &block_hash, 100, 1, 1000, true);
        acc.record(&coll, &tx2, &object_b, &block_hash, 100, 2, 1000, true);

        let mut batch = StoreBatch::new(&store);
        let inserted = acc
            .flush_identity(
                &mut batch,
                &mut CollectionActivityDailyBlockDeltas::new(),
                &|_| Ok(20231115),
            )
            .unwrap();
        batch.commit().unwrap();

        // Compute deltas from inserted tuples (mirrors pipeline T6a pattern)
        let mut deltas: std::collections::HashMap<Vec<u8>, i64> = std::collections::HashMap::new();
        for (collection_id, _, _, _, _) in &inserted {
            *deltas.entry(collection_id.clone()).or_insert(0) += 1;
        }

        // Two distinct txs → two activity entries → delta of 2
        assert_eq!(inserted.len(), 2);
        assert_eq!(*deltas.get(coll.as_slice()).unwrap(), 2);

        // Verify the activities are in the DB
        let results = store
            .list_identity_collection_activities(&coll, 10, None, None)
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_multiple_collections_multiple_txs() {
        let (_dir, store) = test_store();
        let mut acc = ObjectCollectionActivityAccumulator::new();
        let coll_a = [1u8; 32];
        let coll_b = [2u8; 32];
        let tx1 = [10u8; 32];
        let tx2 = [11u8; 32];
        let block_hash_a = [0xB1u8; 32];
        let block_hash_b = [0xB2u8; 32];
        let object = [3u8; 32];

        acc.record(&coll_a, &tx1, &object, &block_hash_a, 100, 0, 1000, true);
        acc.record(&coll_b, &tx2, &object, &block_hash_b, 200, 1, 2000, true);

        let mut batch = StoreBatch::new(&store);
        acc.flush(
            &mut batch,
            &mut CollectionActivityDailyBlockDeltas::new(),
            &|_| Ok(20231115),
        )
        .unwrap();
        batch.commit().unwrap();

        let results_a = store
            .list_object_collection_activities(&coll_a, 10, None, None)
            .unwrap();
        assert_eq!(results_a.len(), 1);

        let results_b = store
            .list_object_collection_activities(&coll_b, 10, None, None)
            .unwrap();
        assert_eq!(results_b.len(), 1);
    }

    // ---- BlockCollectionActivityCounts: one block's activity counts ----

    struct States {
        writer: super::super::BatchWriter,
        mnft: MnftBatchState,
        spore: SporeBatchState,
        dotbit: DotbitBatchState,
    }

    fn states(store: &std::sync::Arc<CkbadgerStore>) -> States {
        use super::super::entity_stats::SharedEntityStatsOverlay;
        let writer = super::super::BatchWriter::new(store.clone(), store.clone());
        let mnft = writer
            .new_mnft_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        let spore = writer
            .new_spore_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        let dotbit = writer
            .new_dotbit_batch_state(SharedEntityStatsOverlay::standalone(writer.store()).unwrap());
        States {
            writer,
            mnft,
            spore,
            dotbit,
        }
    }

    fn apply(
        counts: BlockCollectionActivityCounts,
        states: &mut States,
        batch: &mut StoreBatch,
    ) -> anyhow::Result<()> {
        counts.apply(
            states.writer.store(),
            &mut states.mnft,
            &mut states.spore,
            &mut states.dotbit,
            batch,
        )
    }

    /// Was `test_apply_object_collection_activity_count_deltas_updates_only_object_collections`
    /// (sync/batch.rs) before the counts became per block: an mNFT class
    /// counts its rows, a Spore cluster's rows leave it without an mNFT
    /// aggregate.
    #[test]
    fn test_block_activity_counts_update_only_object_collections() {
        let dir = TempDir::new().unwrap();
        let store = std::sync::Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let object_collection_id = vec![0x11; 32];
        let cluster_id = vec![0x22; 32];

        let mut seed = StoreBatch::new(&store);
        seed.put_mnft_collection_aggregate(
            &object_collection_id,
            &ckbadger_store::types::MnftCollectionAggregate {
                standard: ckbadger_store::types::ObjectStandard::MnftClass,
                activities_count: 3,
                ..Default::default()
            },
        );
        seed.put_cluster_aggregate(
            &cluster_id,
            &ckbadger_store::types::ClusterAggregate {
                name: Some("cluster".to_string()),
                ..Default::default()
            },
        );
        seed.commit().unwrap();

        let mut states = states(&store);
        let mut counts = BlockCollectionActivityCounts::new(7);
        for _ in 0..2 {
            counts.add_object_row(&object_collection_id, 7).unwrap();
        }
        for _ in 0..4 {
            counts.add_object_row(&cluster_id, 7).unwrap();
        }
        let mut batch = StoreBatch::new(&store);
        apply(counts, &mut states, &mut batch).unwrap();
        batch.commit().unwrap();

        let agg = store
            .get_mnft_collection_aggregate(&object_collection_id)
            .unwrap()
            .unwrap();
        assert_eq!(agg.activities_count, 5);
        assert!(store
            .get_mnft_collection_aggregate(&cluster_id)
            .unwrap()
            .is_none());
    }

    /// Was `test_apply_object_collection_activity_count_deltas_uses_pending_batch_aggregate`:
    /// the count lands on the aggregate this batch already wrote, never on a
    /// stale committed one.
    #[test]
    fn test_block_activity_counts_use_the_batch_aggregate() {
        let dir = TempDir::new().unwrap();
        let store = std::sync::Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let collection_id = vec![0x33; 32];
        let pending_agg = ckbadger_store::types::MnftCollectionAggregate {
            standard: ckbadger_store::types::ObjectStandard::MnftClass,
            name: Some("fresh collection".to_string()),
            ..Default::default()
        };

        let mut states = states(&store);
        let mut batch = StoreBatch::new(&store);
        states
            .mnft
            .seed_collection_aggregate(&collection_id, pending_agg, &mut batch);
        let mut counts = BlockCollectionActivityCounts::new(9);
        counts.add_object_row(&collection_id, 9).unwrap();
        apply(counts, &mut states, &mut batch).unwrap();
        batch.commit().unwrap();

        let agg = store
            .get_mnft_collection_aggregate(&collection_id)
            .unwrap()
            .unwrap();
        assert_eq!(agg.activities_count, 1);
        assert_eq!(agg.name.as_deref(), Some("fresh collection"));
    }

    /// Was `test_apply_object_collection_activity_count_deltas_uses_pending_cluster_ids`:
    /// a cluster this batch created is a cluster, not a missing aggregate.
    #[test]
    fn test_block_activity_counts_know_the_batch_clusters() {
        let dir = TempDir::new().unwrap();
        let store = std::sync::Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let cluster_id = vec![0x44; 32];
        let cluster = ckbadger_store::types::ClusterAggregate {
            name: Some("fresh cluster".to_string()),
            ..Default::default()
        };

        let mut states = states(&store);
        let mut batch = StoreBatch::new(&store);
        batch.put_cluster_aggregate(&cluster_id, &cluster);
        states.spore.seed_cluster_aggregate(&cluster_id, cluster);
        let mut counts = BlockCollectionActivityCounts::new(4);
        counts.add_object_row(&cluster_id, 4).unwrap();
        apply(counts, &mut states, &mut batch).unwrap();
        batch.commit().unwrap();

        let cluster = store.get_cluster_aggregate(&cluster_id).unwrap().unwrap();
        assert_eq!(cluster.name.as_deref(), Some("fresh cluster"));
        assert!(store
            .get_mnft_collection_aggregate(&cluster_id)
            .unwrap()
            .is_none());
    }

    /// A collection activity row of a collection with no aggregate at all is
    /// a lost fact: the batch fails with the collection and block.
    #[test]
    fn test_block_activity_counts_fail_on_a_collection_without_aggregate() {
        let dir = TempDir::new().unwrap();
        let store = std::sync::Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let mut states = states(&store);
        let mut batch = StoreBatch::new(&store);
        let mut counts = BlockCollectionActivityCounts::new(5);
        counts.add_object_row(&[0x55; 32], 5).unwrap();
        let err = apply(counts, &mut states, &mut batch)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("missing collection aggregate") && err.contains("block=5"),
            "{err}"
        );

        let mut counts = BlockCollectionActivityCounts::new(5);
        counts
            .add_identity_row(&DOTBIT_SENTINEL_COLLECTION, 5)
            .unwrap();
        let err = apply(counts, &mut states, &mut batch)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("missing identity collection aggregate") && err.contains("block=5"),
            "{err}"
        );
    }

    /// Identity rows count into their collection's committed aggregate, and
    /// a row of another block is refused.
    #[test]
    fn test_block_activity_counts_add_identity_rows_to_their_aggregates() {
        let dir = TempDir::new().unwrap();
        let store = std::sync::Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let did = ckbadger_store::types::DID_CKB_SENTINEL_COLLECTION;
        let mut seed = StoreBatch::new(&store);
        for (collection, count) in [(DOTBIT_SENTINEL_COLLECTION, 10), (did, 20)] {
            seed.put_identity_collection_aggregate(
                &collection,
                &IdentityCollectionAggregate {
                    activities_count: count,
                    ..Default::default()
                },
            );
        }
        seed.commit().unwrap();

        let mut states = states(&store);
        let mut counts = BlockCollectionActivityCounts::new(12);
        counts
            .add_identity_row(&DOTBIT_SENTINEL_COLLECTION, 12)
            .unwrap();
        counts.add_identity_row(&did, 12).unwrap();
        counts.add_identity_row(&did, 12).unwrap();
        assert!(counts.add_identity_row(&did, 13).is_err());
        let mut batch = StoreBatch::new(&store);
        apply(counts, &mut states, &mut batch).unwrap();
        batch.commit().unwrap();

        let count = |collection: &[u8]| {
            store
                .get_identity_collection_aggregate(collection)
                .unwrap()
                .unwrap()
                .activities_count
        };
        assert_eq!(count(&DOTBIT_SENTINEL_COLLECTION), 11);
        assert_eq!(count(&did), 22);
    }
}
