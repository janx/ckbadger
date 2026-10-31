//! The derivation scheduler: runs on the live pipeline's writer task, between
//! live batches, one bounded chunk per call.

use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use ckbadger_common::BackgroundTaskState;
use ckbadger_store::derive_ops::{DeriveMeta, DeriveState, ReadSnapshot};
use ckbadger_store::{CkbadgerStore, StoreBatch};
use tracing::info;

use super::live::record_failure_before_stop;
use super::{
    committed_tip, registry, stage_reset, sum_with, task_name, ChunkBudget, DeltaMap,
    DerivedAggregate,
};

/// Wall-clock bound of one chunk: the longest a live batch can wait behind
/// the derivation.
pub const CHUNK_MAX_TIME: Duration = Duration::from_millis(150);
/// Source rows per chunk at most.
pub const CHUNK_MAX_ROWS: u64 = 20_000;
/// Distinct derived keys one chunk may read-modify-write at most.
pub const CHUNK_MAX_KEYS: usize = 4_096;

/// What one [`DeriveScheduler::step`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// Every aggregate is complete (or failed in this process).
    Idle,
    /// A derivation started (rows cleared, snapshot taken).
    Started,
    /// One chunk was folded; more follow.
    Chunk,
    /// The last chunk was folded and the aggregate is complete.
    Completed,
}

/// The rebuild in flight: its snapshot, and where it stands.
struct ActiveRebuild<'s> {
    index: usize,
    run: u64,
    snapshot: ReadSnapshot<'s>,
    derived_from_tip: i64,
    cursor: Option<Vec<u8>>,
    rows_done: u64,
    started: Instant,
}

/// Owns the rebuilds of every registered aggregate, one at a time in registry
/// order. It must live on the task that writes live batches, so a chunk and a
/// batch never run at the same time.
pub struct DeriveScheduler<'s> {
    store: &'s CkbadgerStore,
    aggregates: Vec<Box<dyn DerivedAggregate>>,
    active: Option<ActiveRebuild<'s>>,
    chunk_time: Duration,
    chunk_rows: u64,
    #[allow(dead_code)]
    chunk_keys: usize,
}

impl<'s> DeriveScheduler<'s> {
    pub fn new(store: &'s CkbadgerStore) -> Self {
        Self::with_aggregates(store, registry())
    }

    pub fn with_aggregates(
        store: &'s CkbadgerStore,
        aggregates: Vec<Box<dyn DerivedAggregate>>,
    ) -> Self {
        Self {
            store,
            aggregates,
            active: None,
            chunk_time: CHUNK_MAX_TIME,
            chunk_rows: CHUNK_MAX_ROWS,
            chunk_keys: CHUNK_MAX_KEYS,
        }
    }

    /// Smaller chunks, for tests that interleave live batches between them.
    pub fn with_chunk_rows(mut self, rows: u64) -> Self {
        self.chunk_rows = rows;
        self
    }

    /// Fewer distinct output keys per chunk.
    pub fn with_chunk_keys(mut self, keys: usize) -> Self {
        self.chunk_keys = keys;
        self
    }

    /// The startup rule, applied once before the first live batch: a missing
    /// meta, an old version, an interrupted (`Running`) or a `Failed`
    /// derivation is reset (rows cleared) and derived again; a `Complete` one
    /// is kept — and must not claim a completion above the sync tip, which
    /// only a rollback that skipped [`super::invalidate_for_rollback`] leaves.
    pub fn normalize_on_startup(&mut self) -> Result<()> {
        let tip = committed_tip(self.store)?;
        let mut batch = StoreBatch::new(self.store);
        let mut reset = Vec::new();
        for aggregate in &self.aggregates {
            let id = aggregate.id();
            let meta = self.store.get_derive_meta(id.name)?;
            let keep = match &meta {
                Some(meta) if meta.version == id.version => match meta.state {
                    DeriveState::Complete {
                        completed_at_tip, ..
                    } => {
                        if completed_at_tip > tip {
                            bail!(
                                "derived aggregate {} completed at block {completed_at_tip}, above the \
                                 sync tip {tip}: a rollback below its completion did not reset it",
                                id.name
                            );
                        }
                        true
                    }
                    DeriveState::NotStarted => false,
                    DeriveState::Running { .. } | DeriveState::Failed { .. } => false,
                },
                _ => false,
            };
            if !keep {
                stage_reset(&mut batch, &id, meta.as_ref())?;
                reset.push(id.name);
            }
        }
        batch.commit()?;
        self.active = None;
        if !reset.is_empty() {
            info!(target: "derive", aggregates = ?reset, tip, "derived aggregates reset at startup; they derive once sync is near the tip");
            for name in reset {
                self.store
                    .update_background_task(&task_name(name), |entry| {
                        entry.state = BackgroundTaskState::Waiting;
                        entry.message = Some("Waiting for sync to catch up".to_string());
                        entry.progress_current = None;
                        entry.progress_total = None;
                        entry.rate = None;
                        entry.error = None;
                    })?;
            }
        }
        Ok(())
    }

    /// The first aggregate, in registry order, that still needs rebuild work,
    /// with its meta.
    fn next_pending(&self) -> Result<Option<(usize, Option<DeriveMeta>)>> {
        for (index, aggregate) in self.aggregates.iter().enumerate() {
            let id = aggregate.id();
            let meta = self.store.get_derive_meta(id.name)?;
            match &meta {
                Some(meta) if meta.is_complete(&id) => continue,
                _ => return Ok(Some((index, meta))),
            }
        }
        Ok(None)
    }

    /// Whether any aggregate still needs rebuild work in this process.
    pub fn has_pending_work(&self) -> Result<bool> {
        Ok(self.next_pending()?.is_some())
    }

    /// Do one bounded unit of rebuild work: start the next derivation, or
    /// fold one chunk of the one in flight.
    pub fn step(&mut self) -> Result<StepOutcome> {
        let Some((index, meta)) = self.next_pending()? else {
            self.active = None;
            return Ok(StepOutcome::Idle);
        };
        let continues = matches!(
            (&self.active, &meta),
            (Some(active), Some(DeriveMeta { state: DeriveState::Running { .. }, run, .. }))
                if active.index == index && active.run == *run
        );
        if !continues {
            return self.start(index, meta.as_ref());
        }
        self.chunk()
    }

    fn start(&mut self, index: usize, previous: Option<&DeriveMeta>) -> Result<StepOutcome> {
        let id = self.aggregates[index].id();
        // Drop any abandoned snapshot before taking the new one.
        self.active = None;
        let tip = committed_tip(self.store)?;
        let run = previous.map_or(0, |meta| meta.run) + 1;
        let started_at_ms = chrono::Utc::now().timestamp_millis();
        let meta = DeriveMeta {
            version: id.version,
            state: DeriveState::Running {
                derived_from_tip: tip,
                cursor: None,
                rows_done: 0,
            },
            run,
            started_at_ms: Some(started_at_ms),
            finished_at_ms: None,
        };
        let mut batch = StoreBatch::new(self.store);
        batch.clear_derived_cf(id.cf_name)?;
        batch.put_derive_meta(id.name, &meta);
        batch.commit()?;
        // Taken after the Running meta committed and before any live batch
        // can run: every block above `tip` reaches the rows as a live delta,
        // every block at or below it through this snapshot, and none twice.
        let snapshot = self.store.read_snapshot()?;
        let snapshot_tip = snapshot.tip()?.map_or(-1, |(number, _)| number);
        if snapshot_tip != tip {
            bail!(
                "derived aggregate {}: snapshot tip {snapshot_tip} differs from the sync tip {tip} \
                 it was started at; a live batch committed concurrently with the scheduler",
                id.name
            );
        }
        info!(target: "derive", aggregate = id.name, run, derived_from_tip = tip, "derivation started");
        self.store
            .update_background_task(&task_name(id.name), |entry| {
                entry.state = BackgroundTaskState::Running;
                entry.message = Some(format!("Deriving from a snapshot at block {tip}"));
                entry.progress_current = Some(0);
                entry.progress_total = None;
                entry.rate = None;
                entry.eta_seconds = None;
                entry.started_at = Some(started_at_ms / 1000);
                entry.elapsed_ms = Some(0.0);
                entry.error = None;
                entry.last_trigger_reason =
                    Some("derived rows missing, outdated or reset".to_string());
            })?;
        self.active = Some(ActiveRebuild {
            index,
            run,
            snapshot,
            derived_from_tip: tip,
            cursor: None,
            rows_done: 0,
            started: Instant::now(),
        });
        Ok(StepOutcome::Started)
    }

    fn chunk(&mut self) -> Result<StepOutcome> {
        let active = self
            .active
            .as_mut()
            .expect("chunk() is only called with a rebuild in flight");
        let aggregate = &self.aggregates[active.index];
        let id = aggregate.id();
        let mut budget = ChunkBudget::new(self.chunk_time, self.chunk_rows);
        let mut out = DeltaMap::new();
        let next = match aggregate.rebuild_chunk(
            &active.snapshot,
            active.cursor.as_deref(),
            &mut budget,
            &mut out,
        ) {
            Ok(next) => next,
            Err(err) => return self.fail(err.context(format!("{} rebuild chunk", id.name))),
        };
        // Fold the chunk into the rows. Live deltas may already sit on them
        // (signed sums commute), so read-modify-write against the primary.
        let mut batch = StoreBatch::new(self.store);
        for (key, delta) in &out {
            let current = self.store.get_derived_value(id.cf_name, key)?;
            let value = sum_with(current, delta)?;
            batch.put_derived_value(id.cf_name, key, value.as_deref())?;
        }
        active.rows_done += budget.rows();
        active.cursor = next.clone();
        let running = DeriveMeta {
            version: id.version,
            state: DeriveState::Running {
                derived_from_tip: active.derived_from_tip,
                cursor: next.clone(),
                rows_done: active.rows_done,
            },
            run: active.run,
            started_at_ms: self
                .store
                .get_derive_meta(id.name)?
                .and_then(|meta| meta.started_at_ms),
            finished_at_ms: None,
        };
        batch.put_derive_meta(id.name, &running);
        batch.commit()?;
        let elapsed = active.started.elapsed();
        let rows_done = active.rows_done;
        if next.is_some() {
            self.store
                .update_background_task(&task_name(id.name), |entry| {
                    entry.progress_current = Some(rows_done);
                    entry.elapsed_ms = Some(elapsed.as_secs_f64() * 1000.0);
                    entry.rate = (elapsed.as_secs_f64() > 0.0)
                        .then(|| rows_done as f64 / elapsed.as_secs_f64());
                })?;
            return Ok(StepOutcome::Chunk);
        }
        // Source exhausted: every row must satisfy the aggregate's invariant
        // before the rows are served.
        if let Err(err) = self.validate_rows(active_index(&self.active)) {
            return self.fail(err);
        }
        let active = self.active.take().expect("rebuild in flight");
        let completed_at_tip = committed_tip(self.store)?;
        let finished_at_ms = chrono::Utc::now().timestamp_millis();
        let mut batch = StoreBatch::new(self.store);
        batch.put_derive_meta(
            id.name,
            &DeriveMeta {
                version: id.version,
                state: DeriveState::Complete {
                    derived_from_tip: active.derived_from_tip,
                    completed_at_tip,
                    rows_done: active.rows_done,
                },
                run: active.run,
                started_at_ms: running.started_at_ms,
                finished_at_ms: Some(finished_at_ms),
            },
        );
        batch.commit()?;
        info!(
            target: "derive",
            aggregate = id.name,
            rows = active.rows_done,
            derived_from_tip = active.derived_from_tip,
            completed_at_tip,
            elapsed_ms = elapsed.as_millis() as u64,
            "derivation complete"
        );
        self.store
            .update_background_task(&task_name(id.name), |entry| {
                entry.state = BackgroundTaskState::Completed;
                entry.message = Some(format!(
                    "Derived from block {} ({} source rows), exact since block {completed_at_tip}",
                    active.derived_from_tip, active.rows_done
                ));
                entry.progress_current = Some(active.rows_done);
                entry.progress_total = Some(active.rows_done);
                entry.elapsed_ms = Some(elapsed.as_secs_f64() * 1000.0);
                entry.last_success_at = Some(finished_at_ms / 1000);
                entry.error = None;
            })?;
        Ok(StepOutcome::Completed)
    }

    fn validate_rows(&self, index: usize) -> Result<()> {
        let aggregate = &self.aggregates[index];
        let id = aggregate.id();
        self.store
            .scan_derived_prefix(id.cf_name, &[], |key, value| {
                aggregate.check_row(key, &value).map_err(|e| {
                    e.context(format!(
                        "{} row 0x{} at completion",
                        id.name,
                        hex::encode(key)
                    ))
                })
            })
    }

    /// Fail fast: mark the aggregate `Failed` (what the stopping indexer
    /// leaves for the TUI and the API) and return the error, which stops the
    /// writer loop and the indexer.
    fn fail(&mut self, err: anyhow::Error) -> Result<StepOutcome> {
        let active = self.active.take().expect("a failing rebuild is in flight");
        let id = self.aggregates[active.index].id();
        let previous = self.store.get_derive_meta(id.name)?.ok_or_else(|| {
            anyhow::anyhow!("derive meta for {} vanished while it was running", id.name)
        })?;
        record_failure_before_stop(self.store, id.name, &previous, &err)?;
        Err(err.context(format!(
            "background derivation of {} stopped the indexer",
            id.name
        )))
    }
}

fn active_index(active: &Option<ActiveRebuild<'_>>) -> usize {
    active.as_ref().expect("rebuild in flight").index
}

/// `derive_to_completion` over `aggregates` (tests).
#[cfg(test)]
pub(crate) fn derive_to_completion_with(
    store: &CkbadgerStore,
    aggregates: Vec<Box<dyn DerivedAggregate>>,
) -> Result<()> {
    let mut scheduler = DeriveScheduler::with_aggregates(store, aggregates);
    scheduler.normalize_on_startup()?;
    while scheduler.has_pending_work()? {
        scheduler.step()?;
    }
    Ok(())
}

/// `rederive_to_completion` over `aggregates` (tests).
#[cfg(test)]
pub(crate) fn rederive_to_completion_with(
    store: &CkbadgerStore,
    aggregates: Vec<Box<dyn DerivedAggregate>>,
) -> Result<()> {
    let mut batch = StoreBatch::new(store);
    for aggregate in &aggregates {
        let id = aggregate.id();
        let previous = store.get_derive_meta(id.name)?;
        stage_reset(&mut batch, &id, previous.as_ref())?;
    }
    batch.commit()?;
    derive_to_completion_with(store, aggregates)
}

/// Reset every registered derivation and derive it again from the store as it
/// stands now (tests and tools that change source rows directly, outside the
/// live writer). Returns each aggregate's final state.
pub fn rederive_to_completion(store: &CkbadgerStore) -> Result<Vec<(&'static str, DeriveState)>> {
    let mut batch = StoreBatch::new(store);
    for aggregate in registry() {
        let id = aggregate.id();
        let previous = store.get_derive_meta(id.name)?;
        stage_reset(&mut batch, &id, previous.as_ref())?;
    }
    batch.commit()?;
    derive_to_completion(store)
}

/// Run every registered derivation to completion on a primary store, with no
/// live sync alongside. For tests and tools that build a store and then read
/// it; the indexer itself runs [`DeriveScheduler`] between live batches.
/// Returns each aggregate's final state; an invariant violation is an error
/// (after the aggregate was marked `Failed`), exactly as in the indexer.
pub fn derive_to_completion(store: &CkbadgerStore) -> Result<Vec<(&'static str, DeriveState)>> {
    let mut scheduler = DeriveScheduler::new(store);
    scheduler.normalize_on_startup()?;
    while scheduler.has_pending_work()? {
        scheduler.step()?;
    }
    registry()
        .iter()
        .map(|aggregate| {
            let id = aggregate.id();
            let meta = store.get_derive_meta(id.name)?.ok_or_else(|| {
                anyhow::anyhow!("derive meta for {} missing after derivation", id.name)
            })?;
            Ok((id.name, meta.state))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    fn toy() -> Vec<Box<dyn DerivedAggregate>> {
        vec![Box::new(ToyHeadersPerEpoch::default())]
    }

    fn run_to_completion(scheduler: &mut DeriveScheduler<'_>) -> Vec<StepOutcome> {
        let mut outcomes = Vec::new();
        while scheduler.has_pending_work().unwrap() {
            outcomes.push(scheduler.step().unwrap());
            assert!(outcomes.len() < 10_000, "derivation does not converge");
        }
        outcomes
    }

    fn put_meta(store: &CkbadgerStore, meta: &DeriveMeta, junk_row: bool) {
        let mut batch = StoreBatch::new(store);
        batch.put_derive_meta(TOY.name, meta);
        if junk_row {
            batch
                .put_derived_value(TOY.cf_name, &epoch_key(999), Some(&[42]))
                .unwrap();
        }
        batch.commit().unwrap();
    }

    fn meta(version: u32, state: DeriveState) -> DeriveMeta {
        DeriveMeta {
            version,
            state,
            run: 3,
            started_at_ms: Some(1),
            finished_at_ms: None,
        }
    }

    #[test]
    fn a_rebuild_equals_the_direct_recomputation_and_records_its_tips() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=40);
        let mut scheduler = DeriveScheduler::with_aggregates(&store, toy()).with_chunk_rows(7);
        scheduler.normalize_on_startup().unwrap();
        let outcomes = run_to_completion(&mut scheduler);
        assert_eq!(outcomes.first(), Some(&StepOutcome::Started));
        assert_eq!(outcomes.last(), Some(&StepOutcome::Completed));
        assert_eq!(stored_rows(&store, &TOY), expected_rows(40));
        let meta = store.get_derive_meta(TOY.name).unwrap().unwrap();
        assert_eq!(
            meta.state,
            DeriveState::Complete {
                derived_from_tip: 40,
                completed_at_tip: 40,
                rows_done: 41,
            }
        );
        let task = store
            .get_background_tasks()
            .unwrap()
            .tasks
            .into_iter()
            .find(|task| task.name == "derive:toy_headers_per_epoch")
            .expect("progress is reported through the background task registry");
        assert_eq!(task.state, ckbadger_common::BackgroundTaskState::Completed);
        assert_eq!(task.progress_current, Some(41));
    }

    /// Missing meta, an old version, an interrupted rebuild and a failure from
    /// an earlier process each clear the rows and derive again; a complete
    /// derivation is a no-op.
    #[test]
    fn startup_resets_everything_but_a_complete_derivation() {
        let cases: Vec<(&str, Option<DeriveMeta>, bool)> = vec![
            ("missing", None, true),
            ("old version", Some(meta(0, DeriveState::NotStarted)), true),
            (
                "interrupted",
                Some(meta(
                    TOY.version,
                    DeriveState::Running {
                        derived_from_tip: 5,
                        cursor: Some(vec![1]),
                        rows_done: 2,
                    },
                )),
                true,
            ),
            (
                "failed earlier",
                Some(meta(
                    TOY.version,
                    DeriveState::Failed {
                        error: "boom".into(),
                    },
                )),
                true,
            ),
            (
                "complete",
                Some(meta(
                    TOY.version,
                    DeriveState::Complete {
                        derived_from_tip: 9,
                        completed_at_tip: 9,
                        rows_done: 1,
                    },
                )),
                false,
            ),
        ];
        for (label, seeded, rebuilds) in cases {
            let (_dir, store) = toy_store();
            commit_headers(&store, 0..=9);
            if let Some(seeded) = &seeded {
                put_meta(&store, seeded, true);
            } else {
                let mut batch = StoreBatch::new(&store);
                batch
                    .put_derived_value(TOY.cf_name, &epoch_key(999), Some(&[42]))
                    .unwrap();
                batch.commit().unwrap();
            }
            let mut scheduler = DeriveScheduler::with_aggregates(&store, toy());
            scheduler.normalize_on_startup().unwrap();
            assert_eq!(
                scheduler.has_pending_work().unwrap(),
                rebuilds,
                "{label}: pending work"
            );
            if rebuilds {
                assert_eq!(
                    store.get_derive_meta(TOY.name).unwrap().unwrap().state,
                    DeriveState::NotStarted,
                    "{label}"
                );
                assert!(
                    stored_rows(&store, &TOY).is_empty(),
                    "{label}: rows cleared"
                );
                run_to_completion(&mut scheduler);
                assert_eq!(stored_rows(&store, &TOY), expected_rows(9), "{label}");
            } else {
                assert_eq!(
                    stored_rows(&store, &TOY),
                    vec![(epoch_key(999), vec![42])],
                    "{label}: a complete aggregate is not touched"
                );
            }
        }
    }

    #[test]
    fn startup_refuses_a_complete_derivation_above_the_sync_tip() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=9);
        put_meta(
            &store,
            &meta(
                TOY.version,
                DeriveState::Complete {
                    derived_from_tip: 12,
                    completed_at_tip: 12,
                    rows_done: 1,
                },
            ),
            false,
        );
        let err = DeriveScheduler::with_aggregates(&store, toy())
            .normalize_on_startup()
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("completed at block 12")
                && format!("{err:#}").contains("tip 9"),
            "{err:#}"
        );
    }

    /// Chunks between live batches: the live tip advances on every iteration,
    /// blocks above the snapshot tip arrive through live deltas only, and the
    /// result equals a from-scratch derivation at the final tip (no double
    /// counting, nothing missed).
    #[test]
    fn chunks_interleaved_with_live_batches_neither_stall_live_sync_nor_double_count() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=30);
        let mut scheduler = DeriveScheduler::with_aggregates(&store, toy()).with_chunk_rows(4);
        scheduler.normalize_on_startup().unwrap();
        let mut tip = 30;
        let mut iterations = 0;
        while scheduler.has_pending_work().unwrap() {
            scheduler.step().unwrap();
            // One live batch per iteration, as the writer loop does.
            tip += 1;
            super::super::live::tests::write_live_batch(&store, &toy(), tip..=tip);
            assert_eq!(super::super::committed_tip(&store).unwrap(), tip);
            iterations += 1;
            assert!(iterations < 1_000);
        }
        assert!(iterations > 3, "the rebuild spanned several live batches");
        assert_eq!(stored_rows(&store, &TOY), expected_rows(tip));
        match store.get_derive_meta(TOY.name).unwrap().unwrap().state {
            DeriveState::Complete {
                derived_from_tip,
                completed_at_tip,
                ..
            } => {
                assert_eq!(derived_from_tip, 30);
                assert!(completed_at_tip > 30 && completed_at_tip <= tip);
            }
            other => panic!("{other:?}"),
        }
    }

    /// A rollback that resets a running derivation restarts it from a new
    /// snapshot; the abandoned snapshot is never folded again.
    #[test]
    fn a_reset_while_running_restarts_from_a_new_snapshot() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=30);
        let mut scheduler = DeriveScheduler::with_aggregates(&store, toy()).with_chunk_rows(4);
        scheduler.normalize_on_startup().unwrap();
        assert_eq!(scheduler.step().unwrap(), StepOutcome::Started);
        assert_eq!(scheduler.step().unwrap(), StepOutcome::Chunk);
        super::super::invalidate_registered_for_rollback(&store, &toy(), 25).unwrap();
        assert_eq!(scheduler.step().unwrap(), StepOutcome::Started);
        run_to_completion(&mut scheduler);
        assert_eq!(stored_rows(&store, &TOY), expected_rows(30));
        assert_eq!(store.get_derive_meta(TOY.name).unwrap().unwrap().run, 2);
    }

    /// Ruling A: a rebuild invariant violation stops the scheduler (the
    /// error propagates out of `step`, so the indexer stops); the `Failed`
    /// marker is what it leaves behind.
    #[test]
    fn a_rebuild_invariant_violation_stops_the_scheduler() {
        let (_dir, store) = toy_store();
        commit_headers(&store, 0..=9);
        let strict: Vec<Box<dyn DerivedAggregate>> = vec![Box::new(ToyHeadersPerEpoch {
            max_per_epoch: 2,
            ..Default::default()
        })];
        let mut scheduler = DeriveScheduler::with_aggregates(&store, strict);
        scheduler.normalize_on_startup().unwrap();
        let err = loop {
            match scheduler.step() {
                Ok(StepOutcome::Idle) => panic!("the derivation completed over an invalid row"),
                Ok(_) => continue,
                Err(err) => break format!("{err:#}"),
            }
        };
        assert!(err.contains("toy row"), "{err}");
        match store.get_derive_meta(TOY.name).unwrap().unwrap().state {
            DeriveState::Failed { error } => assert!(error.contains("toy row"), "{error}"),
            other => panic!("{other:?}"),
        }
    }
}
