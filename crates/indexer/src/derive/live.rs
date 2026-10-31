//! Live maintenance of derived aggregates: one call per live batch, staged
//! into the batch's own domain write so the rows and the sync tip are
//! durable together.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use ckbadger_store::derive_ops::{encode_derived_value, DeriveMeta, DeriveState};
use ckbadger_store::keys::UndoSeqScope;
use ckbadger_store::types::{UndoLogEntry, UndoLogStoreTarget};
use ckbadger_store::{CkbadgerStore, StoreBatch};
use tracing::error;

use super::{failed_meta, sum_with, BlockDeltas, DerivedAggregate};
use crate::sync::undo::SharedUndoSeq;

/// One block of a live batch, as the derivations see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveBlockPoint {
    pub number: i64,
    pub epoch_number: i64,
    pub epoch_index: i32,
    pub epoch_length: i32,
}

/// What one live batch changed, for the derivations that maintain rows from
/// it. Each field is filled by the live writer that owns those source rows.
pub struct LiveDeriveCtx<'a> {
    /// The committed store the batch builds on.
    pub store: &'a CkbadgerStore,
    /// Every block of the batch, ascending.
    pub blocks: &'a [LiveBlockPoint],
}

/// Add this batch's deltas to every aggregate live sync maintains (state
/// `Running` or `Complete` at the current version), staging rows, undo
/// pre-images and — on an invariant violation — the aggregate's `Failed`
/// state into `batch`.
///
/// A failing aggregate does not fail the batch: its rows are a rebuildable
/// view, so its own writes are dropped, its state becomes `Failed` with the
/// error (the API serves that error) and canonical sync continues. The
/// failures are returned so the caller can report them once the batch has
/// committed ([`report_live_failures`]).
pub(crate) fn apply_live_batch(
    store: &CkbadgerStore,
    aggregates: &[Box<dyn DerivedAggregate>],
    ctx: &LiveDeriveCtx<'_>,
    batch: &mut StoreBatch<'_>,
    undo_seq: &SharedUndoSeq,
) -> Result<Vec<LiveFailure>> {
    let mut failures = Vec::new();
    for aggregate in aggregates {
        let id = aggregate.id();
        let Some(meta) = store.get_derive_meta(id.name)? else {
            continue;
        };
        if !meta.is_maintained(&id) {
            continue;
        }
        match stage_one(store, aggregate.as_ref(), &meta, ctx) {
            Ok(staged) => stage_into(staged, id.cf_name, batch, undo_seq)?,
            Err(err) => {
                let failed = report_failure(id.name, &meta, &err);
                batch.put_derive_meta(id.name, &failed);
                failures.push(LiveFailure {
                    name: id.name,
                    error: format!("{err:#}"),
                });
            }
        }
    }
    Ok(failures)
}

/// One aggregate a live batch moved to `Failed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveFailure {
    pub name: &'static str,
    pub error: String,
}

/// Mirror live failures into the background task registry (visibility only;
/// the meta committed with the batch is the source of truth).
pub fn report_live_failures(store: &CkbadgerStore, failures: &[LiveFailure]) -> Result<()> {
    for failure in failures {
        store.update_background_task(&super::task_name(failure.name), |entry| {
            entry.state = ckbadger_common::BackgroundTaskState::Failed;
            entry.error = Some(failure.error.clone());
            entry.message = Some("live maintenance found an invariant violation".to_string());
        })?;
    }
    Ok(())
}

/// The rows and undo pre-images one aggregate's live deltas produce.
struct StagedAggregate {
    /// `(block, key, value at the end of the previous block)`, ascending block.
    undo: Vec<(i64, Vec<u8>, Option<Vec<u8>>)>,
    /// Final value per touched key (`None` = row removed).
    rows: HashMap<Vec<u8>, Option<Vec<i128>>>,
}

fn stage_one(
    store: &CkbadgerStore,
    aggregate: &dyn DerivedAggregate,
    meta: &DeriveMeta,
    ctx: &LiveDeriveCtx<'_>,
) -> Result<StagedAggregate> {
    let id = aggregate.id();
    let mut deltas = BlockDeltas::new();
    aggregate.live_deltas(ctx, &mut deltas)?;
    let mut rows: HashMap<Vec<u8>, Option<Vec<i128>>> = HashMap::new();
    let mut undo = Vec::new();
    for (block, keyed) in deltas {
        for (key, delta) in keyed {
            let current = match rows.get(&key) {
                Some(value) => value.clone(),
                None => store.get_derived_value(id.cf_name, &key)?,
            };
            // A block's deltas are one map entry per key, so this is the
            // block's first touch of `key`: record its pre-block value.
            undo.push((
                block,
                key.clone(),
                current.as_deref().map(encode_derived_value),
            ));
            let next = sum_with(current, &delta).map_err(|e| {
                anyhow!(
                    "{} live delta at block {block}, key=0x{}: {e}",
                    id.name,
                    hex::encode(&key)
                )
            })?;
            rows.insert(key, next);
        }
    }
    if matches!(meta.state, DeriveState::Complete { .. }) {
        for (key, value) in &rows {
            if let Some(value) = value {
                aggregate.check_row(key, value).map_err(|e| {
                    anyhow!(
                        "{} row 0x{} after live batch {}..={}: {e:#}",
                        id.name,
                        hex::encode(key),
                        ctx.blocks.first().map_or(-1, |b| b.number),
                        ctx.blocks.last().map_or(-1, |b| b.number)
                    )
                })?;
            }
        }
    }
    Ok(StagedAggregate { undo, rows })
}

fn stage_into(
    staged: StagedAggregate,
    cf_name: &str,
    batch: &mut StoreBatch<'_>,
    undo_seq: &SharedUndoSeq,
) -> Result<()> {
    for (block, key, previous_value) in staged.undo {
        let seq = undo_seq.next(block, UndoSeqScope::Derived);
        batch.put_reorg_undo_log_by_block(
            block,
            seq,
            &UndoLogEntry::KeyMutation {
                target_store: UndoLogStoreTarget::Domain,
                cf_name: cf_name.to_string(),
                key,
                previous_value,
            },
        );
    }
    for (key, value) in staged.rows {
        batch.put_derived_value(cf_name, &key, value.as_deref())?;
    }
    Ok(())
}

fn report_failure(name: &str, meta: &DeriveMeta, err: &anyhow::Error) -> DeriveMeta {
    error!(aggregate = name, error = %format!("{err:#}"), "derived aggregate live maintenance failed");
    failed_meta(meta, err)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::test_support::*;
    use super::super::{DeriveScheduler, DerivedAggregate};
    use super::*;

    fn blocks(range: std::ops::RangeInclusive<i64>) -> Vec<LiveBlockPoint> {
        range
            .map(|number| LiveBlockPoint {
                number,
                epoch_number: epoch_of(number),
                epoch_index: 0,
                epoch_length: 1,
            })
            .collect()
    }

    /// The live writer's shape: one domain batch carries the batch's headers
    /// and the derived deltas, and commits once.
    pub(crate) fn write_live_batch(
        store: &CkbadgerStore,
        aggregates: &[Box<dyn DerivedAggregate>],
        range: std::ops::RangeInclusive<i64>,
    ) {
        let points = blocks(range.clone());
        let mut batch = StoreBatch::new(store);
        for number in range {
            batch.put_block_header(number, &header(number, epoch_of(number)));
        }
        let ctx = LiveDeriveCtx {
            store,
            blocks: &points,
        };
        let failures = apply_live_batch(
            store,
            aggregates,
            &ctx,
            &mut batch,
            &SharedUndoSeq::default(),
        )
        .unwrap();
        batch.commit().unwrap();
        report_live_failures(store, &failures).unwrap();
    }

    fn toy() -> Vec<Box<dyn DerivedAggregate>> {
        vec![Box::new(ToyHeadersPerEpoch::default())]
    }

    fn complete(store: &CkbadgerStore) {
        let mut scheduler = DeriveScheduler::with_aggregates(store, toy());
        scheduler.normalize_on_startup().unwrap();
        while scheduler.has_pending_work().unwrap() {
            scheduler.step().unwrap();
        }
        assert!(store
            .get_derive_meta(TOY.name)
            .unwrap()
            .unwrap()
            .is_complete(&TOY));
    }

    #[test]
    fn live_batches_maintain_a_complete_aggregate_exactly() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=10);
        complete(&store);
        write_live_batch(&store, &toy(), 11..=11);
        write_live_batch(&store, &toy(), 12..=17);
        assert_eq!(stored_rows(&store, &TOY), expected_rows(17));
    }

    #[test]
    fn live_batches_do_not_touch_an_aggregate_that_has_not_started() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=10);
        write_live_batch(&store, &toy(), 11..=12);
        assert!(stored_rows(&store, &TOY).is_empty());
    }

    /// The fork point may land inside a multi-block live batch: undo replay
    /// must restore the value at the end of the fork block, not the value
    /// before the batch (per-block pre-images).
    #[test]
    fn undo_replay_restores_the_rows_at_any_fork_point_inside_a_batch() {
        for fork_point in [10, 12, 13, 15] {
            let (_dir, store) = toy_store();
            commit_headers(&store, 0..=10);
            complete(&store);
            write_live_batch(&store, &toy(), 11..=16);
            store.rollback_via_undo_log(&store, fork_point).unwrap();
            assert_eq!(
                stored_rows(&store, &TOY),
                expected_rows(fork_point),
                "fork_point={fork_point}"
            );
        }
    }

    #[test]
    fn an_invariant_violation_fails_only_the_aggregate_and_drops_its_writes() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=10);
        complete(&store);
        let before = stored_rows(&store, &TOY);
        // Epoch 3 holds blocks 9..=11; a cap of 2 per epoch is violated by 11.
        let strict: Vec<Box<dyn DerivedAggregate>> = vec![Box::new(ToyHeadersPerEpoch {
            id: TOY,
            max_per_epoch: 2,
        })];
        write_live_batch(&store, &strict, 11..=11);
        let meta = store.get_derive_meta(TOY.name).unwrap().unwrap();
        match &meta.state {
            DeriveState::Failed { error } => assert!(
                error.contains("toy row") && error.contains("11..=11"),
                "{error}"
            ),
            other => panic!("expected Failed, got {other:?}"),
        }
        assert_eq!(
            stored_rows(&store, &TOY),
            before,
            "the failing batch wrote no rows"
        );
        assert!(
            store.get_block_header(11).unwrap().is_some(),
            "canonical writes committed"
        );
    }
}
