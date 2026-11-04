//! Live maintenance of derived aggregates: one call per live batch, staged
//! into the batch's own domain write so the rows and the sync tip are
//! durable together.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use ckbadger_store::derive_ops::{encode_derived_value, DeriveMeta};
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
    /// Every NervosDAO lifecycle transition of the batch, in chain order
    /// (`LiveDaoBatch`, the live path's one resolution of DAO inputs).
    pub dao_transitions: &'a [crate::db::writer::DaoTransition],
    /// Every `object_collection_activities` row the batch writes, as
    /// `(collection id, block)`.
    pub object_collection_activity_rows: &'a [(Vec<u8>, i64)],
    /// Every `.bit Cell` identity entry transition of the batch, in write
    /// order (`insert_bit_cell`, the identity branch of `consume_spore`).
    pub identity_transitions: &'a [crate::db::writer::IdentityTransition],
}

/// Add this batch's deltas to every aggregate live sync maintains (state
/// `Running` or `Complete` at the current version), staging rows and undo
/// pre-images into `batch`.
///
/// Fail fast: an error — a live delta that cannot be computed, or a row that
/// breaks its aggregate's invariant — fails the batch like any other writer
/// invariant, with the aggregate, key and blocks in its context; the caller
/// stops the indexer. Before returning it, the aggregate is marked `Failed`
/// (its own small write; the batch itself is never committed) so the TUI and
/// the API can say why the indexer stopped. The next start re-derives it.
pub(crate) fn apply_live_batch(
    store: &CkbadgerStore,
    aggregates: &[Box<dyn DerivedAggregate>],
    ctx: &LiveDeriveCtx<'_>,
    batch: &mut StoreBatch<'_>,
    undo_seq: &SharedUndoSeq,
) -> Result<()> {
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
                let err = err.context(format!(
                    "derived aggregate {} live maintenance for blocks {}..={}",
                    id.name,
                    ctx.blocks.first().map_or(-1, |b| b.number),
                    ctx.blocks.last().map_or(-1, |b| b.number)
                ));
                record_failure_before_stop(store, id.name, &meta, &err)?;
                return Err(err);
            }
        }
    }
    Ok(())
}

/// Persist `Failed{error}` and mirror it to the background task — what a
/// stopping indexer leaves behind for the TUI and the API.
pub(crate) fn record_failure_before_stop(
    store: &CkbadgerStore,
    name: &'static str,
    meta: &DeriveMeta,
    err: &anyhow::Error,
) -> Result<()> {
    error!(aggregate = name, error = %format!("{err:#}"), "derived aggregate invariant violated; the indexer stops");
    let mut marker = StoreBatch::new(store);
    marker.put_derive_meta(name, &failed_meta(meta, err));
    marker.commit()?;
    store.update_background_task(&super::task_name(name), |entry| {
        entry.state = ckbadger_common::BackgroundTaskState::Failed;
        entry.error = Some(format!("{err:#}"));
        entry.message = Some("invariant violated; the indexer stopped".to_string());
    })?;
    Ok(())
}

/// The rows and undo pre-images one aggregate's live deltas produce.
struct StagedAggregate {
    /// `(block, key, value at the end of the previous block)`, ascending block.
    undo: Vec<(i64, Vec<u8>, Option<Vec<u8>>)>,
    /// `(block, key, value at the end of the block)` for every block that
    /// moves a key, ascending block (`None` = row removed): each is written in
    /// its block's scope, so the block journal restores the row as of the
    /// end of any block of the batch.
    rows: Vec<(i64, Vec<u8>, Option<Vec<i128>>)>,
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
    // Each touched key's value as of the last block that moved it.
    let mut current_rows: HashMap<Vec<u8>, Option<Vec<i128>>> = HashMap::new();
    let mut rows = Vec::new();
    let mut undo = Vec::new();
    for (block, keyed) in deltas {
        for (key, delta) in keyed {
            let current = match current_rows.get(&key) {
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
            rows.push((block, key.clone(), next.clone()));
            current_rows.insert(key, next);
        }
    }
    if meta.checks_rows() {
        for (key, value) in &current_rows {
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
    for (block, key, value) in staged.rows {
        batch
            .at(block)
            .put_derived_value(cf_name, &key, value.as_deref())?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::test_support::*;
    use super::super::{DeriveScheduler, DerivedAggregate};
    use super::*;
    use ckbadger_store::derive_ops::DeriveState;

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
            dao_transitions: &[],
            object_collection_activity_rows: &[],
            identity_transitions: &[],
        };
        apply_live_batch(
            store,
            aggregates,
            &ctx,
            &mut batch,
            &SharedUndoSeq::default(),
        )
        .unwrap();
        batch.commit().unwrap();
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

    /// Through the live batch's block overlay, every block that moves a row
    /// writes it in its own scope: the block journal holds the row's value at
    /// the end of the previous block, so replaying it restores the rows as of
    /// any block of the batch (store refactor lane B, B-3).
    #[test]
    fn a_journaled_live_batch_journals_each_row_at_its_block() {
        use crate::db::writer::journal_test_support::journal_entries;
        use ckbadger_store::block_overlay::SharedBlockOverlay;

        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=10);
        complete(&store);
        // Epoch 3 (blocks 9..=11) gains block 11; epoch 4 (12..=14) is
        // created by block 12 and moved by block 13.
        let points = blocks(11..=13);
        let overlay = SharedBlockOverlay::new(&store);
        let mut batch = StoreBatch::journaled(&store, &overlay, 0);
        for number in 11..=13 {
            batch.put_block_header(number, &header(number, epoch_of(number)));
        }
        let ctx = LiveDeriveCtx {
            store: &store,
            blocks: &points,
            dao_transitions: &[],
            object_collection_activity_rows: &[],
            identity_transitions: &[],
        };
        let undo_seq = SharedUndoSeq::default();
        apply_live_batch(&store, &toy(), &ctx, &mut batch, &undo_seq).unwrap();
        batch
            .stage_journal(&mut |block| Ok(undo_seq.next(block, UndoSeqScope::Journal)))
            .unwrap();
        batch.commit().unwrap();

        let row = |count: i128| Some(encode_derived_value(&[count]));
        assert_eq!(
            journal_entries(&store, TOY.cf_name),
            vec![
                (11, epoch_key(3), row(2)),
                (12, epoch_key(4), None),
                (13, epoch_key(4), row(1)),
            ]
        );
        assert_eq!(stored_rows(&store, &TOY), expected_rows(13));
    }

    /// Ruling A: a live invariant violation fails the batch like any other
    /// writer invariant — the error names the aggregate, the key and the
    /// blocks; nothing of the batch is committed; the `Failed` marker that is
    /// persisted is only what the stopping process leaves for the TUI/API.
    #[test]
    fn a_live_invariant_violation_fails_the_batch() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=10);
        complete(&store);
        let before = stored_rows(&store, &TOY);
        // Epoch 3 holds blocks 9..=11; a cap of 2 per epoch is violated by 11.
        let strict: Vec<Box<dyn DerivedAggregate>> = vec![Box::new(ToyHeadersPerEpoch {
            max_per_epoch: 2,
            ..Default::default()
        })];
        let points = blocks(11..=11);
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(11, &header(11, epoch_of(11)));
        let ctx = LiveDeriveCtx {
            store: &store,
            blocks: &points,
            dao_transitions: &[],
            object_collection_activity_rows: &[],
            identity_transitions: &[],
        };
        let result = apply_live_batch(&store, &strict, &ctx, &mut batch, &SharedUndoSeq::default());
        let err = format!("{:#}", result.expect_err("the batch must fail"));
        assert!(
            err.contains("toy_headers_per_epoch")
                && err.contains("toy row")
                && err.contains("11..=11"),
            "{err}"
        );
        drop(batch);
        assert!(
            store.get_block_header(11).unwrap().is_none(),
            "nothing of the batch committed"
        );
        assert_eq!(stored_rows(&store, &TOY), before);
        assert!(matches!(
            store.get_derive_meta(TOY.name).unwrap().unwrap().state,
            DeriveState::Failed { .. }
        ));
    }
}
