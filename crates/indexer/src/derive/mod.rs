//! Background derivation of persisted aggregates (round 3, lane L).
//!
//! A derived aggregate is computed from rows the indexer has already written,
//! after bulk sync hands off to live sync, and is then maintained by the live
//! writer only. Bulk build never runs any of this: the column families are
//! `BulkDisabled`, the scheduler runs inside the live pipeline's writer loop,
//! and live maintenance runs inside `write_parsed_batch` only when the batch
//! is not a bulk batch.
//!
//! - [`scheduler::DeriveScheduler`] rebuilds one aggregate at a time, in
//!   [`registry`] order, through one RocksDB snapshot taken at the sync tip
//!   `H`, one bounded chunk per writer-loop iteration. The live writer and the
//!   rebuild are therefore serialised by construction: nothing else writes the
//!   aggregate's rows while a chunk runs, and no chunk runs while a batch is
//!   being written.
//! - [`live::apply_live_batch`] adds each live block's signed deltas
//!   `f(new) - f(old)` to every aggregate whose derivation is running or
//!   complete, in the batch's own atomic write, each moved row in its block's
//!   scope. Rollback is therefore the block journal's, like every other
//!   `Journal`-class row.
//! - [`invalidate_for_rollback`] runs before every rollback (reorg, startup
//!   cleanup, partial-batch cleanup): a running derivation, or a complete one
//!   whose rows became exact above the rollback target, is reset and derived
//!   again.
//!
//! The persisted state machine is `ckbadger_store::derive_ops::DeriveMeta`.

pub mod bit_cell;
pub mod collection_activity;
pub mod dao;
pub mod live;
pub mod scheduler;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use ckbadger_store::derive_ops::{DeriveMeta, DeriveState, DerivedAggregateId, ReadSnapshot};
use ckbadger_store::{CkbadgerStore, OutsideWriter, StoreBatch};
use tracing::warn;

pub(crate) use live::apply_live_batch;
pub use live::{LiveBlockPoint, LiveDeriveCtx};
pub use scheduler::{derive_to_completion, rederive_to_completion, DeriveScheduler, StepOutcome};
#[cfg(test)]
pub(crate) use scheduler::{derive_to_completion_with, rederive_to_completion_with};

/// Derived key → signed contribution, one map per chunk or per block.
pub type DeltaMap = BTreeMap<Vec<u8>, Vec<i128>>;

/// Live contributions by block, ascending.
pub type BlockDeltas = BTreeMap<i64, DeltaMap>;

/// Add `delta` to `key`'s pending contribution, component-wise and checked.
pub fn add_delta(map: &mut DeltaMap, key: Vec<u8>, delta: &[i128]) -> Result<()> {
    match map.get_mut(&key) {
        Some(existing) => add_components(existing, delta)
            .map_err(|e| anyhow!("derived delta for key=0x{}: {e}", hex::encode(&key))),
        None => {
            map.insert(key, delta.to_vec());
            Ok(())
        }
    }
}

/// `value += delta`, component-wise, checked, with equal arity.
pub fn add_components(value: &mut [i128], delta: &[i128]) -> Result<()> {
    anyhow::ensure!(
        value.len() == delta.len(),
        "derived value arity {} differs from delta arity {}",
        value.len(),
        delta.len()
    );
    for (component, change) in value.iter_mut().zip(delta) {
        *component = component
            .checked_add(*change)
            .ok_or_else(|| anyhow!("derived value overflow: {component} + {change}"))?;
    }
    Ok(())
}

/// `current + delta`, or `None` when the sum is all zeros (a zero sum has no
/// row, so a rebuild and a live-maintained aggregate store identical bytes).
pub fn sum_with(current: Option<Vec<i128>>, delta: &[i128]) -> Result<Option<Vec<i128>>> {
    let mut value = current.unwrap_or_else(|| vec![0; delta.len()]);
    add_components(&mut value, delta)?;
    Ok(if value.iter().all(|component| *component == 0) {
        None
    } else {
        Some(value)
    })
}

/// How much one rebuild chunk may do before it yields back to live sync.
pub struct ChunkBudget {
    started: Instant,
    max_time: Duration,
    max_rows: u64,
    max_keys: usize,
    rows: u64,
}

impl ChunkBudget {
    pub fn new(max_time: Duration, max_rows: u64) -> Self {
        Self {
            started: Instant::now(),
            max_time,
            max_rows,
            max_keys: usize::MAX,
            rows: 0,
        }
    }

    /// At most this many distinct output keys per chunk.
    pub fn with_max_keys(mut self, max_keys: usize) -> Self {
        self.max_keys = max_keys;
        self
    }

    /// Count one folded source row, with `keys` distinct output keys pending
    /// so far; `true` once the chunk must stop. The key bound is what bounds
    /// the chunk's read-modify-write of the rows, which runs after the fold.
    pub fn tick(&mut self, keys: usize) -> bool {
        self.rows += 1;
        self.rows >= self.max_rows
            || keys >= self.max_keys
            || (self.rows.is_multiple_of(64) && self.started.elapsed() >= self.max_time)
    }

    pub fn rows(&self) -> u64 {
        self.rows
    }
}

/// The first source key strictly after `cursor` (a chunk resumes after the
/// last key it folded).
pub fn resume_key(cursor: Option<&[u8]>) -> Vec<u8> {
    match cursor {
        Some(last) => {
            let mut next = last.to_vec();
            next.push(0);
            next
        }
        None => Vec::new(),
    }
}

/// One derived aggregate. Every implementation is a signed sum over source
/// rows (see `ckbadger_store::derive_ops`): the rebuild folds the rows of a
/// snapshot, live sync folds `f(new) - f(old)` for each row a block changes.
/// Clearing and rollback are the framework's: `clear` is a range delete of
/// [`DerivedAggregateId::cf_name`], and rollback writes back the block
/// journal entries of the rows [`live::apply_live_batch`] writes per block.
pub trait DerivedAggregate: Send + Sync {
    fn id(&self) -> DerivedAggregateId;

    /// Fold source rows after `cursor` from `snapshot` into `out` until the
    /// budget is spent. Returns the last key folded, or `None` when the source
    /// is exhausted.
    fn rebuild_chunk(
        &self,
        snapshot: &ReadSnapshot<'_>,
        cursor: Option<&[u8]>,
        budget: &mut ChunkBudget,
        out: &mut DeltaMap,
    ) -> Result<Option<Vec<u8>>>;

    /// The signed contributions one live batch makes, by block.
    fn live_deltas(&self, ctx: &LiveDeriveCtx<'_>, out: &mut BlockDeltas) -> Result<()>;

    /// The invariant every stored row satisfies once the derivation is
    /// complete (checked at completion and on every later live write).
    fn check_row(&self, key: &[u8], value: &[i128]) -> Result<()>;
}

/// Every registered aggregate, in derivation order.
pub fn registry() -> Vec<Box<dyn DerivedAggregate>> {
    vec![
        Box::new(dao::DaoLiveAgg),
        Box::new(collection_activity::ObjectCollectionActivityCount),
        Box::new(bit_cell::BitCellByAccount),
    ]
}

/// Reset every derivation a rollback to `rollback_to` would corrupt, BEFORE
/// the rollback writes anything.
///
/// - A running derivation is reset by any rollback that removes a block: its
///   rows mix snapshot contributions with live deltas, and the journal
///   entries recorded while chunks were still writing would put stale
///   values back.
/// - A complete derivation is reset when `rollback_to` is below the tip it
///   completed at, for the same reason; at or above it every journal entry
///   was recorded after the rows became exact, so the rollback restores them.
///
/// The reset (meta `NotStarted` + range delete) commits before the journal
/// rollback, so a crash anywhere in the rollback leaves the aggregate reset,
/// never half-restored. The scheduler derives it again.
pub fn invalidate_for_rollback(store: &CkbadgerStore, rollback_to: i64) -> Result<()> {
    invalidate_registered_for_rollback(store, &registry(), rollback_to)
}

pub(crate) fn invalidate_registered_for_rollback(
    store: &CkbadgerStore,
    aggregates: &[Box<dyn DerivedAggregate>],
    rollback_to: i64,
) -> Result<()> {
    let tip = committed_tip(store)?;
    let mut batch = StoreBatch::outside_journal(store, OutsideWriter::Derive);
    let mut reset = Vec::new();
    // The journal owns an aggregate's rollback only while it is complete at
    // or below the target; every other aggregate's journal entries above the
    // target would write stale rows into a CF that is (or is about to be)
    // empty, so they are dropped in the same write as the reset.
    let mut not_replayed: Vec<&'static str> = Vec::new();
    for aggregate in aggregates {
        let id = aggregate.id();
        let meta = store.get_derive_meta(id.name)?;
        let current = meta.as_ref().filter(|meta| meta.version == id.version);
        let (corrupted, replay_owns) = match current.map(|meta| &meta.state) {
            Some(DeriveState::Running { .. }) => (rollback_to < tip, false),
            Some(DeriveState::Complete {
                completed_at_tip, ..
            }) => (
                rollback_to < *completed_at_tip,
                rollback_to >= *completed_at_tip,
            ),
            Some(DeriveState::NotStarted | DeriveState::Failed { .. }) | None => (false, false),
        };
        if corrupted {
            stage_reset(&mut batch, &id, meta.as_ref())?;
            reset.push(id.name);
        }
        if !replay_owns {
            not_replayed.push(id.cf_name);
        }
    }
    let dropped = store.stage_delete_journal_above(&mut batch, &not_replayed, rollback_to)?;
    if !reset.is_empty() || dropped > 0 {
        batch.commit()?;
        tracing::info!(
            target: "derive",
            rollback_to,
            tip,
            aggregates = ?reset,
            dropped_journal_entries = dropped,
            "derived aggregates reset before rollback; they are derived again"
        );
    }
    Ok(())
}

/// Reset one aggregate: meta `NotStarted` (keeping its run counter) and every
/// row removed, in one write.
pub(crate) fn stage_reset(
    batch: &mut StoreBatch<'_>,
    id: &DerivedAggregateId,
    previous: Option<&DeriveMeta>,
) -> Result<()> {
    let run = previous.map_or(0, |meta| meta.run);
    batch.clear_derived_cf(id.cf_name)?;
    batch.put_derive_meta(id.name, &DeriveMeta::not_started(id.version, run));
    Ok(())
}

/// The background task a derivation reports its progress through.
pub fn task_name(aggregate: &str) -> String {
    format!("derive:{aggregate}")
}

/// The committed sync tip (the highest stored header), `-1` on an empty store.
pub(crate) fn committed_tip(store: &CkbadgerStore) -> Result<i64> {
    Ok(match store.get_sync_tip_block()? {
        Some((number, _)) => number,
        None => -1,
    })
}

pub(crate) fn failed_meta(previous: &DeriveMeta, error: &anyhow::Error) -> DeriveMeta {
    warn!(
        target: "derive",
        error = %format!("{error:#}"),
        "derived aggregate failed"
    );
    DeriveMeta {
        version: previous.version,
        state: DeriveState::Failed {
            error: format!("{error:#}"),
        },
        run: previous.run,
        started_at_ms: previous.started_at_ms,
        finished_at_ms: Some(chrono::Utc::now().timestamp_millis()),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A toy aggregate for the framework tests: block headers per epoch
    //! number. Source rows are `block_headers`; live sync adds one per block.

    use super::*;
    use ckbadger_store::types::CachedBlockHeader;

    pub(crate) const TOY: DerivedAggregateId = DerivedAggregateId {
        name: "toy_headers_per_epoch",
        cf_name: ckbadger_store::CF_OBJECT_COLLECTION_ACTIVITY_COUNT,
        version: 1,
    };

    pub(crate) struct ToyHeadersPerEpoch {
        pub(crate) id: DerivedAggregateId,
        /// Rows with a count above this violate the toy invariant.
        pub(crate) max_per_epoch: i128,
        /// Key each header by its block number instead of its epoch (one
        /// distinct output key per source row).
        pub(crate) key_by_block: bool,
        /// `check_row` calls, so a test can see how much one step validated.
        pub(crate) checks: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Default for ToyHeadersPerEpoch {
        fn default() -> Self {
            Self {
                id: TOY,
                max_per_epoch: i128::MAX,
                key_by_block: false,
                checks: Default::default(),
            }
        }
    }

    impl ToyHeadersPerEpoch {
        fn key_of(&self, number: i64, epoch: i64) -> Vec<u8> {
            if self.key_by_block {
                number.to_be_bytes().to_vec()
            } else {
                epoch_key(epoch)
            }
        }
    }

    impl DerivedAggregate for ToyHeadersPerEpoch {
        fn id(&self) -> DerivedAggregateId {
            self.id
        }

        fn rebuild_chunk(
            &self,
            snapshot: &ReadSnapshot<'_>,
            cursor: Option<&[u8]>,
            budget: &mut ChunkBudget,
            out: &mut DeltaMap,
        ) -> Result<Option<Vec<u8>>> {
            let start = resume_key(cursor);
            for item in snapshot.iter_from(ckbadger_store::CF_BLOCK_HEADERS, &start)? {
                let (key, value) = item?;
                if key.len() != 8 {
                    continue;
                }
                let header: CachedBlockHeader = bincode::deserialize(&value)?;
                let number = ckbadger_store::keys::decode_block_num(&key);
                add_delta(out, self.key_of(number, header.epoch_number), &[1])?;
                if budget.tick(out.len()) {
                    return Ok(Some(key.to_vec()));
                }
            }
            Ok(None)
        }

        fn live_deltas(&self, ctx: &LiveDeriveCtx<'_>, out: &mut BlockDeltas) -> Result<()> {
            for block in ctx.blocks {
                add_delta(
                    out.entry(block.number).or_default(),
                    self.key_of(block.number, block.epoch_number),
                    &[1],
                )?;
            }
            Ok(())
        }

        fn check_row(&self, key: &[u8], value: &[i128]) -> Result<()> {
            self.checks
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            anyhow::ensure!(
                value.len() == 1 && value[0] > 0 && value[0] <= self.max_per_epoch,
                "toy row 0x{} out of range: {value:?}",
                hex::encode(key)
            );
            Ok(())
        }
    }

    pub(crate) fn epoch_key(epoch: i64) -> Vec<u8> {
        epoch.to_be_bytes().to_vec()
    }

    pub(crate) fn header(number: i64, epoch: i64) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![(number % 251) as u8; 32],
            parent_hash: vec![0; 32],
            timestamp: number * 1_000,
            epoch_number: epoch,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
            chain_time_ms: number * 1_000,
        }
    }

    /// Blocks `0..=tip`, epoch = number / 3.
    pub(crate) fn epoch_of(number: i64) -> i64 {
        number / 3
    }

    pub(crate) fn toy_store() -> (tempfile::TempDir, CkbadgerStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        (dir, store)
    }

    pub(crate) fn commit_headers(store: &CkbadgerStore, numbers: std::ops::RangeInclusive<i64>) {
        let mut batch = StoreBatch::new(store);
        for number in numbers {
            batch.put_block_header(number, &header(number, epoch_of(number)));
        }
        batch.commit().unwrap();
    }

    /// The toy's from-scratch value at `tip`.
    pub(crate) fn expected_rows(tip: i64) -> Vec<(Vec<u8>, Vec<i128>)> {
        let mut map = DeltaMap::new();
        for number in 0..=tip {
            add_delta(&mut map, epoch_key(epoch_of(number)), &[1]).unwrap();
        }
        map.into_iter().collect()
    }

    pub(crate) fn stored_rows(
        store: &CkbadgerStore,
        id: &DerivedAggregateId,
    ) -> Vec<(Vec<u8>, Vec<i128>)> {
        let mut rows = Vec::new();
        store
            .scan_derived_prefix(id.cf_name, &[], |key, value| {
                rows.push((key.to_vec(), value));
                Ok(())
            })
            .unwrap();
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    fn complete_meta(completed_at_tip: i64) -> DeriveMeta {
        DeriveMeta {
            version: TOY.version,
            state: DeriveState::Complete {
                derived_from_tip: completed_at_tip,
                completed_at_tip,
                rows_done: 1,
            },
            run: 4,
            started_at_ms: None,
            finished_at_ms: None,
        }
    }

    fn seed(store: &CkbadgerStore, meta: &DeriveMeta) {
        let mut batch = StoreBatch::new(store);
        batch.put_derive_meta(TOY.name, meta);
        batch
            .put_derived_value(TOY.cf_name, &epoch_key(99), Some(&[7]))
            .unwrap();
        batch.commit().unwrap();
    }

    fn toy() -> Vec<Box<dyn DerivedAggregate>> {
        vec![Box::new(ToyHeadersPerEpoch::default())]
    }

    #[test]
    fn a_rollback_below_the_completion_tip_resets_a_complete_derivation() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=20);
        seed(&store, &complete_meta(15));
        invalidate_registered_for_rollback(&store, &toy(), 14).unwrap();
        let meta = store.get_derive_meta(TOY.name).unwrap().unwrap();
        assert_eq!(meta.state, DeriveState::NotStarted);
        assert_eq!(meta.run, 4, "a reset keeps the run counter");
        assert!(
            stored_rows(&store, &TOY).is_empty(),
            "a reset clears every row"
        );
    }

    #[test]
    fn a_rollback_at_or_above_the_completion_tip_leaves_it_to_the_undo_log() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=20);
        seed(&store, &complete_meta(15));
        invalidate_registered_for_rollback(&store, &toy(), 15).unwrap();
        assert!(store
            .get_derive_meta(TOY.name)
            .unwrap()
            .unwrap()
            .is_complete(&TOY));
        assert_eq!(stored_rows(&store, &TOY).len(), 1);
    }

    #[test]
    fn any_real_rollback_resets_a_running_derivation_but_a_no_op_cleanup_does_not() {
        for (target, reset) in [(19, true), (20, false)] {
            let (_dir, store) = toy_store();
            commit_headers(&store, 0..=20);
            seed(
                &store,
                &DeriveMeta {
                    version: TOY.version,
                    state: DeriveState::Running {
                        derived_from_tip: 10,
                        phase: ckbadger_store::derive_ops::RebuildPhase::Folding,
                        cursor: None,
                        rows_done: 0,
                    },
                    run: 1,
                    started_at_ms: None,
                    finished_at_ms: None,
                },
            );
            invalidate_registered_for_rollback(&store, &toy(), target).unwrap();
            let meta = store.get_derive_meta(TOY.name).unwrap().unwrap();
            assert_eq!(
                meta.state == DeriveState::NotStarted,
                reset,
                "target={target}: {meta:?}"
            );
        }
    }

    /// Review P2: a reset must leave the aggregate's CF empty through the
    /// rollback that follows it — the rollback must not write the aggregate's
    /// journal entries above the target back into the cleared CF.
    #[test]
    fn a_reset_aggregate_stays_empty_through_the_rollback() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=10);
        crate::derive::derive_to_completion_with(&store, toy()).unwrap();
        super::live::tests::write_live_batch(&store, &toy(), 11..=16);
        // Derived again at 16: a rollback to 13 is below the completion tip.
        crate::derive::rederive_to_completion_with(&store, toy()).unwrap();
        invalidate_registered_for_rollback(&store, &toy(), 13).unwrap();
        assert!(
            stored_rows(&store, &TOY).is_empty(),
            "the reset cleared the rows"
        );
        store.rollback_to_block(13).unwrap();
        assert_eq!(
            stored_rows(&store, &TOY),
            Vec::new(),
            "the rollback wrote stale derived rows into a reset aggregate"
        );
    }

    #[test]
    fn sums_drop_all_zero_rows() {
        assert_eq!(sum_with(Some(vec![2, 1]), &[-2, -1]).unwrap(), None);
        assert_eq!(sum_with(None, &[-1, 0]).unwrap(), Some(vec![-1, 0]));
        assert!(
            sum_with(Some(vec![1]), &[1, 1]).is_err(),
            "arity must match"
        );
        assert!(sum_with(Some(vec![i128::MAX]), &[1]).is_err());
    }
}
