//! HODL Wave tracker — tracks live cell capacity distribution by age.
//!
//! Maintains an in-memory `HashMap<NaiveDate, i128>` mapping cell creation dates
//! to their total live capacity. At each day boundary, computes age-band snapshots
//! for the HODL wave chart.
//!
//! Persisted as rows (store refactor lane B, B-4): one `hodl_capacity_by_date`
//! row per creation day and the `hodl_cursor` row for the scalars. The live
//! writer stages, in each block's scope, the rows that block moved
//! ([`HodlWaveTracker::stage_block_rows`]); bulk finalize stages them all
//! ([`HodlWaveTracker::stage_all_rows`]). The tracker never maps a block to its
//! day itself: the caller passes the creation day of a consumed cell, derived
//! from the creating block's canonical header (`sync::block_days`).

use std::collections::{BTreeSet, HashMap};

use anyhow::{bail, Result};
use chrono::NaiveDate;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::{CkbadgerStore, DailyHodlWave, HodlTrackerCursor};

/// Tracks live cell capacity by creation date for HODL wave chart computation.
#[derive(Debug, Clone)]
pub struct HodlWaveTracker {
    /// Total live capacity (shannons) per cell creation date. A day whose
    /// capacity reaches zero is removed (and its row deleted).
    capacity_by_creation_date: HashMap<NaiveDate, i128>,
    /// Number of addresses with at least one live cell.
    pub holder_count: i64,
    /// The day of the last processed block: the open day, sealed by the first
    /// block of the next day.
    pub last_snapshot_date: Option<NaiveDate>,
    /// The last block number processed.
    last_processed_block: Option<i64>,
}

impl HodlWaveTracker {
    pub fn new() -> Self {
        Self {
            capacity_by_creation_date: HashMap::new(),
            holder_count: 0,
            last_snapshot_date: None,
            last_processed_block: None,
        }
    }

    /// The tracker its persisted rows describe: `hodl_capacity_by_date` and
    /// the `hodl_cursor` (both validated by the store's row codecs).
    pub fn from_rows(rows: Vec<(NaiveDate, i128)>, cursor: HodlTrackerCursor) -> Result<Self> {
        let last_snapshot_date = NaiveDate::parse_from_str(&cursor.last_snapshot_date, "%Y%m%d")
            .map_err(|e| {
                super::corrupt_stored_row(format!(
                    "corrupt hodl cursor last_snapshot_date '{}': {e}",
                    cursor.last_snapshot_date
                ))
            })?;
        let mut capacity_by_creation_date = HashMap::with_capacity(rows.len());
        for (day, capacity) in rows {
            if capacity <= 0 {
                return Err(super::corrupt_stored_row(format!(
                    "hodl_capacity_by_date row {day} holds non-positive capacity {capacity}"
                )));
            }
            if capacity_by_creation_date.insert(day, capacity).is_some() {
                return Err(super::corrupt_stored_row(format!(
                    "hodl_capacity_by_date row {day} appears twice"
                )));
            }
        }
        if cursor.holder_count < 0 {
            return Err(super::corrupt_stored_row(format!(
                "invalid hodl cursor: negative holder_count={}",
                cursor.holder_count
            )));
        }
        Ok(Self {
            capacity_by_creation_date,
            holder_count: cursor.holder_count,
            last_snapshot_date: Some(last_snapshot_date),
            last_processed_block: Some(cursor.last_processed_block),
        })
    }

    /// The tracker as persisted in `store`: its rows and cursor, or a fresh
    /// tracker when nothing was ever processed (no cursor and no row).
    pub fn load(store: &CkbadgerStore) -> Result<Self> {
        let rows = store.list_hodl_capacity_by_date()?;
        match store.get_hodl_cursor()? {
            Some(cursor) => Self::from_rows(rows, cursor),
            None if rows.is_empty() => Ok(Self::new()),
            None => Err(super::corrupt_stored_row(format!(
                "hodl_capacity_by_date holds {} rows but there is no hodl_cursor. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
                rows.len()
            ))),
        }
    }

    /// The last block this tracker processed, `None` for a fresh tracker.
    pub fn last_processed_block(&self) -> Option<i64> {
        self.last_processed_block
    }

    /// The scalar state as its `hodl_cursor` row; defined once a block was
    /// processed.
    pub fn cursor(&self) -> Result<HodlTrackerCursor> {
        let (Some(last_processed_block), Some(last_snapshot_date)) =
            (self.last_processed_block, self.last_snapshot_date)
        else {
            bail!("hodl tracker has processed no block: no cursor to persist");
        };
        Ok(HodlTrackerCursor {
            holder_count: self.holder_count,
            last_snapshot_date: last_snapshot_date.format("%Y%m%d").to_string(),
            last_processed_block,
        })
    }

    /// The live capacity of one creation day, `None` when it holds none.
    pub fn capacity_on(&self, day: NaiveDate) -> Option<i128> {
        self.capacity_by_creation_date.get(&day).copied()
    }

    /// Stage the rows one block moved — the current value of each day in
    /// `moved_days` (deleted when it holds no capacity any more) — and the
    /// cursor, into `batch` (the caller scopes it to the block). Returns the
    /// value bytes staged.
    pub fn stage_block_rows(
        &self,
        batch: &mut StoreBatch<'_>,
        moved_days: &BTreeSet<NaiveDate>,
    ) -> Result<usize> {
        let mut bytes = 0;
        for &day in moved_days {
            match self.capacity_on(day) {
                Some(capacity) => bytes += batch.put_hodl_capacity_by_date(day, capacity)?,
                None => batch.delete_hodl_capacity_by_date(day)?,
            }
        }
        Ok(bytes + batch.put_hodl_cursor(&self.cursor()?)?)
    }

    /// Stage every row and the cursor (bulk finalize, onto a store without
    /// tracker rows). Returns the value bytes staged.
    pub fn stage_all_rows(&self, batch: &mut StoreBatch<'_>) -> Result<usize> {
        let mut bytes = 0;
        for (&day, &capacity) in &self.capacity_by_creation_date {
            bytes += batch.put_hodl_capacity_by_date(day, capacity)?;
        }
        Ok(bytes + batch.put_hodl_cursor(&self.cursor()?)?)
    }

    /// Record that `block_number` is being processed. Blocks arrive in
    /// ascending order.
    pub fn record_block(&mut self, block_number: i64) -> Result<()> {
        if let Some(last) = self.last_processed_block {
            if block_number <= last {
                bail!(
                    "hodl tracker received block {block_number} after block {last}: blocks must ascend"
                );
            }
        }
        self.last_processed_block = Some(block_number);
        Ok(())
    }

    /// A cell was created at the given date with the given capacity.
    pub fn cell_created(&mut self, block_date: NaiveDate, capacity: i64) {
        *self
            .capacity_by_creation_date
            .entry(block_date)
            .or_insert(0) += capacity as i128;
    }

    /// A cell created on `creation_date` was consumed: subtract its capacity
    /// from that day.
    pub fn cell_consumed(&mut self, creation_date: NaiveDate, capacity: i64) -> Result<()> {
        if capacity <= 0 {
            bail!(
                "invalid consumed cell capacity: creation_date={}, capacity={}",
                creation_date,
                capacity
            );
        }

        // The per-day capacities are stored aggregates: a consume they cannot
        // explain (no capacity on the day, less than the cell) is
        // rebuild-required (D-4c). The subtraction of a positive capacity can
        // only overflow below `i128::MIN`, an underflow too.
        let current = self
            .capacity_by_creation_date
            .get(&creation_date)
            .copied()
            .ok_or_else(|| {
                super::aggregate_underflow(format!(
                    "missing live capacity bucket for consumed cell: creation_date={}",
                    creation_date
                ))
            })?;

        let next = current.checked_sub(capacity as i128).ok_or_else(|| {
            super::aggregate_underflow(format!(
                "capacity subtraction overflow: current={}, capacity={}",
                current, capacity
            ))
        })?;

        if next < 0 {
            return Err(super::aggregate_underflow(format!(
                "live capacity underflow on consume: creation_date={}, current={}, capacity={}",
                creation_date, current, capacity
            )));
        }

        if next == 0 {
            self.capacity_by_creation_date.remove(&creation_date);
        } else {
            self.capacity_by_creation_date.insert(creation_date, next);
        }

        Ok(())
    }

    /// Track holder count transitions based on live cell count changes.
    /// old_live=0, new_live>0 → new holder (+1)
    /// old_live>0, new_live=0 → lost holder (-1)
    pub fn update_holder_count(&mut self, old_live_cells: i32, new_live_cells: i32) -> Result<()> {
        if old_live_cells == 0 && new_live_cells > 0 {
            self.holder_count = self
                .holder_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("holder_count overflow"))?;
        } else if old_live_cells > 0 && new_live_cells == 0 {
            if self.holder_count <= 0 {
                return Err(super::aggregate_underflow(format!(
                    "holder_count underflow: holder_count={}, old_live_cells={}, new_live_cells={}",
                    self.holder_count, old_live_cells, new_live_cells
                )));
            }
            self.holder_count -= 1;
        }
        Ok(())
    }

    /// Check if a day boundary was crossed. If so, compute and return a snapshot
    /// for the previous day along with its date.
    pub fn maybe_snapshot(
        &mut self,
        current_date: NaiveDate,
    ) -> Option<(NaiveDate, DailyHodlWave)> {
        match self.last_snapshot_date {
            Some(last) if last < current_date => {
                let snapshot = self.compute_snapshot(last);
                self.last_snapshot_date = Some(current_date);
                Some((last, snapshot))
            }
            None => {
                // First block ever — set the date but don't produce a snapshot yet
                self.last_snapshot_date = Some(current_date);
                None
            }
            _ => None,
        }
    }

    /// Compute the HODL wave snapshot for the given date.
    pub fn compute_snapshot(&self, snapshot_date: NaiveDate) -> DailyHodlWave {
        let mut wave = DailyHodlWave {
            holder_count: self.holder_count,
            ..Default::default()
        };

        for (&creation_date, &capacity) in &self.capacity_by_creation_date {
            if capacity <= 0 {
                continue;
            }
            let age_days = (snapshot_date - creation_date).num_days();
            match age_days {
                0 => wave.band_24h += capacity,
                1..=6 => wave.band_1d_1w += capacity,
                7..=29 => wave.band_1w_1m += capacity,
                30..=89 => wave.band_1m_3m += capacity,
                90..=179 => wave.band_3m_6m += capacity,
                180..=364 => wave.band_6m_1y += capacity,
                365..=1094 => wave.band_1y_3y += capacity,
                _ => wave.band_gt_3y += capacity,
            }
        }

        wave
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_created_updates_capacity_map() {
        let mut tracker = HodlWaveTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.cell_created(date, 100_00000000);
        tracker.cell_created(date, 50_00000000);
        assert_eq!(tracker.capacity_by_creation_date[&date], 150_00000000_i128);
    }

    #[test]
    fn test_cell_consumed_reduces_capacity() {
        let mut tracker = HodlWaveTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.cell_created(date, 200_00000000);
        tracker.cell_consumed(date, 50_00000000).unwrap();
        assert_eq!(tracker.capacity_by_creation_date[&date], 150_00000000_i128);
    }

    #[test]
    fn test_cell_consumed_removes_zero_entries() {
        let mut tracker = HodlWaveTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.cell_created(date, 100_00000000);
        tracker.cell_consumed(date, 100_00000000).unwrap();
        assert!(!tracker.capacity_by_creation_date.contains_key(&date));
    }

    #[test]
    fn test_cell_consumed_errors_on_underflow() {
        let mut tracker = HodlWaveTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.cell_created(date, 100_00000000);

        let err = tracker.cell_consumed(date, 150_00000000).unwrap_err();
        assert!(err.to_string().contains("underflow"));
    }

    #[test]
    fn test_cell_consumed_errors_when_no_matching_live_bucket() {
        let mut tracker = HodlWaveTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let err = tracker.cell_consumed(date, 10_00000000).unwrap_err();
        assert!(err.to_string().contains("missing live capacity bucket"));
    }

    #[test]
    fn test_compute_snapshot_age_bands() {
        let mut tracker = HodlWaveTracker::new();
        let snapshot_date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        // band_24h: same day
        tracker.cell_created(snapshot_date, 10);
        // band_1d_1w: 3 days ago
        tracker.cell_created(NaiveDate::from_ymd_opt(2024, 6, 12).unwrap(), 20);
        // band_1w_1m: 14 days ago
        tracker.cell_created(NaiveDate::from_ymd_opt(2024, 6, 1).unwrap(), 30);
        // band_1m_3m: 60 days ago
        tracker.cell_created(NaiveDate::from_ymd_opt(2024, 4, 16).unwrap(), 40);
        // band_3m_6m: 120 days ago
        tracker.cell_created(NaiveDate::from_ymd_opt(2024, 2, 16).unwrap(), 50);
        // band_6m_1y: 200 days ago
        tracker.cell_created(NaiveDate::from_ymd_opt(2023, 11, 28).unwrap(), 60);
        // band_1y_3y: 400 days ago
        tracker.cell_created(NaiveDate::from_ymd_opt(2023, 5, 12).unwrap(), 70);
        // band_gt_3y: 1200 days ago
        tracker.cell_created(NaiveDate::from_ymd_opt(2021, 2, 27).unwrap(), 80);
        tracker.holder_count = 42;

        let wave = tracker.compute_snapshot(snapshot_date);
        assert_eq!(wave.band_24h, 10);
        assert_eq!(wave.band_1d_1w, 20);
        assert_eq!(wave.band_1w_1m, 30);
        assert_eq!(wave.band_1m_3m, 40);
        assert_eq!(wave.band_3m_6m, 50);
        assert_eq!(wave.band_6m_1y, 60);
        assert_eq!(wave.band_1y_3y, 70);
        assert_eq!(wave.band_gt_3y, 80);
        assert_eq!(wave.holder_count, 42);
    }

    #[test]
    fn test_holder_count_transitions() {
        let mut tracker = HodlWaveTracker::new();
        assert_eq!(tracker.holder_count, 0);
        // New holder: 0 → 1
        tracker.update_holder_count(0, 1).unwrap();
        assert_eq!(tracker.holder_count, 1);
        // Existing holder gets more cells: 1 → 5 (no change)
        tracker.update_holder_count(1, 5).unwrap();
        assert_eq!(tracker.holder_count, 1);
        // Another new holder
        tracker.update_holder_count(0, 3).unwrap();
        assert_eq!(tracker.holder_count, 2);
        // First holder loses all cells: 5 → 0
        tracker.update_holder_count(5, 0).unwrap();
        assert_eq!(tracker.holder_count, 1);
        // 0 → 0 (no change)
        tracker.update_holder_count(0, 0).unwrap();
        assert_eq!(tracker.holder_count, 1);
    }

    #[test]
    fn test_holder_count_underflow_errors() {
        let mut tracker = HodlWaveTracker::new();
        let err = tracker.update_holder_count(3, 0).unwrap_err();
        assert!(err.to_string().contains("underflow"));
    }

    #[test]
    fn test_maybe_snapshot_day_boundary() {
        let mut tracker = HodlWaveTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let jan16 = NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
        tracker.cell_created(jan15, 100);
        // First call: sets last_snapshot_date, no snapshot produced
        assert!(tracker.maybe_snapshot(jan15).is_none());
        // Same day: no snapshot
        assert!(tracker.maybe_snapshot(jan15).is_none());
        // Day boundary crossed: produces snapshot for jan15
        let result = tracker.maybe_snapshot(jan16);
        assert!(result.is_some());
        let (date, wave) = result.unwrap();
        assert_eq!(date, jan15);
        assert_eq!(wave.band_24h, 100);
    }

    #[test]
    fn record_block_requires_ascending_blocks() {
        let mut tracker = HodlWaveTracker::new();
        tracker.record_block(100).unwrap();
        tracker.record_block(101).unwrap();
        let err = tracker.record_block(101).unwrap_err();
        assert!(err.to_string().contains("must ascend"), "{err}");
    }

    /// The tracker's persisted form is its rows and cursor: staging them all
    /// and loading them back yields the same tracker, and a block's rows
    /// delete an emptied day.
    #[test]
    fn rows_and_cursor_round_trip_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        assert!(HodlWaveTracker::new().cursor().is_err());
        assert_eq!(
            HodlWaveTracker::load(&store)
                .unwrap()
                .last_processed_block(),
            None
        );

        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let jan16 = NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
        let mut tracker = HodlWaveTracker::new();
        tracker.record_block(100).unwrap();
        assert!(tracker.maybe_snapshot(jan15).is_none());
        tracker.cell_created(jan15, 500_00000000);
        tracker.record_block(200).unwrap();
        assert!(tracker.maybe_snapshot(jan16).is_some());
        tracker.cell_created(jan16, 300_00000000);
        tracker.holder_count = 42;

        let mut batch = StoreBatch::new(&store);
        tracker.stage_all_rows(&mut batch).unwrap();
        batch.commit().unwrap();
        let restored = HodlWaveTracker::load(&store).unwrap();
        assert_eq!(restored.holder_count, 42);
        assert_eq!(restored.last_snapshot_date, Some(jan16));
        assert_eq!(restored.last_processed_block(), Some(200));
        assert_eq!(restored.capacity_on(jan15), Some(500_00000000));
        assert_eq!(restored.capacity_on(jan16), Some(300_00000000));

        let mut tracker = restored;
        tracker.record_block(201).unwrap();
        tracker.cell_consumed(jan15, 500_00000000).unwrap();
        let mut batch = StoreBatch::new(&store);
        tracker
            .stage_block_rows(&mut batch, &BTreeSet::from([jan15]))
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(
            store.list_hodl_capacity_by_date().unwrap(),
            vec![(jan16, 300_00000000)]
        );
        assert_eq!(
            store
                .get_hodl_cursor()
                .unwrap()
                .unwrap()
                .last_processed_block,
            201
        );
    }

    fn assert_rebuild_required(err: &anyhow::Error, what: &str) {
        assert!(
            crate::lifecycle::is_rebuild_required(err),
            "{what}: {err:#}"
        );
        assert!(err.to_string().contains(what), "{what}: {err:#}");
    }

    fn cursor(holder_count: i64, last_snapshot_date: &str) -> HodlTrackerCursor {
        HodlTrackerCursor {
            holder_count,
            last_snapshot_date: last_snapshot_date.to_string(),
            last_processed_block: 1,
        }
    }

    /// D-4c: stored tracker rows the tracker cannot hold (an undecodable
    /// cursor date, a non-positive day, a day listed twice, a negative holder
    /// count) are corruption the chain cannot repair: rebuild-required.
    #[test]
    fn corrupt_stored_rows_are_rebuild_required() {
        let day = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let err = HodlWaveTracker::from_rows(Vec::new(), cursor(0, "2024-13-01")).unwrap_err();
        assert_rebuild_required(&err, "corrupt hodl cursor last_snapshot_date");
        let err = HodlWaveTracker::from_rows(vec![(day, 0)], cursor(0, "20240115")).unwrap_err();
        assert_rebuild_required(&err, "non-positive capacity");
        let err = HodlWaveTracker::from_rows(vec![(day, 1), (day, 2)], cursor(0, "20240115"))
            .unwrap_err();
        assert_rebuild_required(&err, "appears twice");
        let err = HodlWaveTracker::from_rows(Vec::new(), cursor(-1, "20240115")).unwrap_err();
        assert_rebuild_required(&err, "negative holder_count");
    }

    /// D-4c: a consume or holder transition the stored totals cannot explain
    /// (no capacity on the creation day, less capacity than the cell, a total
    /// so negative the subtraction overflows, no holder left) is
    /// rebuild-required.
    #[test]
    fn consume_and_holder_underflows_are_rebuild_required() {
        let day = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let mut tracker = HodlWaveTracker::new();
        let err = tracker.cell_consumed(day, 10_00000000).unwrap_err();
        assert_rebuild_required(&err, "missing live capacity bucket");
        tracker.cell_created(day, 100_00000000);
        let err = tracker.cell_consumed(day, 150_00000000).unwrap_err();
        assert_rebuild_required(&err, "live capacity underflow on consume");
        tracker.capacity_by_creation_date.insert(day, i128::MIN);
        let err = tracker.cell_consumed(day, 1).unwrap_err();
        assert_rebuild_required(&err, "capacity subtraction overflow");
        let err = HodlWaveTracker::new()
            .update_holder_count(3, 0)
            .unwrap_err();
        assert_rebuild_required(&err, "holder_count underflow");
    }

    /// D-4c: the caller's invariants stay plain errors: they are not stored
    /// state (blocks out of order, a non-positive consumed capacity, a cursor
    /// before any block, an overflow).
    #[test]
    fn caller_invariants_stay_plain_errors() {
        let day = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let mut tracker = HodlWaveTracker::new();
        tracker.record_block(7).unwrap();
        let err = tracker.record_block(7).unwrap_err();
        assert!(!crate::lifecycle::is_rebuild_required(&err), "{err:#}");
        let err = tracker.cell_consumed(day, 0).unwrap_err();
        assert!(!crate::lifecycle::is_rebuild_required(&err), "{err:#}");
        let err = HodlWaveTracker::new().cursor().unwrap_err();
        assert!(!crate::lifecycle::is_rebuild_required(&err), "{err:#}");
        tracker.holder_count = i64::MAX;
        let err = tracker.update_holder_count(0, 1).unwrap_err();
        assert!(err.to_string().contains("overflow"), "{err:#}");
        assert!(!crate::lifecycle::is_rebuild_required(&err), "{err:#}");
    }

    #[test]
    fn rows_without_a_cursor_fail_to_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch
            .put_hodl_capacity_by_date(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(), 5)
            .unwrap();
        batch.commit().unwrap();
        let err = HodlWaveTracker::load(&store).unwrap_err();
        assert!(err.to_string().contains("no hodl_cursor"), "{err}");
        // D-4c: rows without their cursor are not a state any sync leaves.
        assert!(crate::lifecycle::is_rebuild_required(&err), "{err:#}");
    }
}
