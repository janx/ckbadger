//! Cell distribution tracker — tracks live cell capacity by size bucket.
//!
//! Maintains global live cell counts/capacities per size bucket and materializes a daily
//! snapshot for the size distribution chart. The same tracker also owns the cohort
//! accumulation for address retention snapshots.
//!
//! Persisted as rows (store refactor lane B, B-4): one `cell_dist_cohort_by_month`
//! row per cohort month and the `cell_dist_cursor` row for the bucket totals and
//! the scalars. The live writer stages, in each block's scope, the cohort months
//! that block moved plus the cursor ([`CellDistributionTracker::stage_block_rows`]);
//! bulk finalize stages them all. The caller passes an address's first-seen day,
//! derived from the first-seen block's canonical header (`sync::block_days`).

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Result};
use chrono::NaiveDate;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::{CellDistTrackerCursor, CkbadgerStore, DailyCellDistribution};

/// Number of size buckets for cell distribution.
const NUM_BUCKETS: usize = 6;

/// CKB per shannon.
const CKB: i64 = 100_000_000;

/// Determine the size bucket index for a given occupied capacity (in shannons).
///
/// Bucket 0: <100 CKB
/// Bucket 1: 100-999 CKB
/// Bucket 2: 1,000-9,999 CKB
/// Bucket 3: 10,000-99,999 CKB
/// Bucket 4: 100,000-999,999 CKB
/// Bucket 5: >=1,000,000 CKB
fn size_bucket(occupied_capacity: i64) -> usize {
    let ckb = occupied_capacity / CKB;
    match ckb {
        0..=99 => 0,
        100..=999 => 1,
        1_000..=9_999 => 2,
        10_000..=99_999 => 3,
        100_000..=999_999 => 4,
        _ => 5,
    }
}

/// The cohort month (`YYYY-MM`) of an address first seen on `first_seen_day`:
/// the key of its `cell_dist_cohort_by_month` row.
pub fn cohort_month(first_seen_day: NaiveDate) -> String {
    first_seen_day.format("%Y-%m").to_string()
}

/// Tracks live cell capacity by size bucket for cell distribution chart computation.
#[derive(Debug, Clone)]
pub struct CellDistributionTracker {
    /// Cell count per size bucket (global totals).
    count_by_bucket: [i64; NUM_BUCKETS],
    /// Total capacity per size bucket (global totals, shannons).
    total_capacity_by_bucket: [i128; NUM_BUCKETS],
    /// The day of the last processed block (to detect day boundaries).
    last_snapshot_date: Option<NaiveDate>,
    /// Incremental address cohort accumulator: cohort_month → (used_capacity, balance).
    cohort_accum: BTreeMap<String, (i128, i128)>,
    /// The last block number processed.
    last_processed_block: Option<i64>,
}

impl CellDistributionTracker {
    pub fn new() -> Self {
        Self {
            count_by_bucket: [0; NUM_BUCKETS],
            total_capacity_by_bucket: [0; NUM_BUCKETS],
            last_snapshot_date: None,
            cohort_accum: BTreeMap::new(),
            last_processed_block: None,
        }
    }

    /// The tracker its persisted rows describe: `cell_dist_cohort_by_month`
    /// and the `cell_dist_cursor` (both validated by the store's row codecs).
    pub fn from_rows(
        cohorts: Vec<(String, i128, i128)>,
        cursor: CellDistTrackerCursor,
    ) -> Result<Self> {
        let last_snapshot_date = NaiveDate::parse_from_str(&cursor.last_snapshot_date, "%Y%m%d")
            .map_err(|e| {
                super::corrupt_stored_row(format!(
                    "corrupt cell_dist cursor last_snapshot_date '{}': {e}",
                    cursor.last_snapshot_date
                ))
            })?;
        let mut cohort_accum = BTreeMap::new();
        for (month, used, bal) in cohorts {
            if used < 0 || bal < 0 {
                return Err(super::corrupt_stored_row(format!(
                    "negative cohort accumulator in stored cell_dist tracker: month={month}, used={used}, balance={bal}"
                )));
            }
            if cohort_accum.insert(month.clone(), (used, bal)).is_some() {
                return Err(super::corrupt_stored_row(format!(
                    "duplicate cohort month in stored cell_dist tracker: month={month}"
                )));
            }
        }

        Ok(Self {
            count_by_bucket: cursor.count_by_bucket,
            total_capacity_by_bucket: cursor.total_capacity_by_bucket,
            last_snapshot_date: Some(last_snapshot_date),
            cohort_accum,
            last_processed_block: Some(cursor.last_processed_block),
        })
    }

    /// The tracker as persisted in `store`: its rows and cursor, or a fresh
    /// tracker when nothing was ever processed (no cursor and no row).
    pub fn load(store: &CkbadgerStore) -> Result<Self> {
        let cohorts = store.list_cell_dist_cohorts()?;
        match store.get_cell_dist_cursor()? {
            Some(cursor) => Self::from_rows(cohorts, cursor),
            None if cohorts.is_empty() => Ok(Self::new()),
            None => Err(super::corrupt_stored_row(format!(
                "cell_dist_cohort_by_month holds {} rows but there is no cell_dist_cursor. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
                cohorts.len()
            ))),
        }
    }

    /// The last block this tracker processed, `None` for a fresh tracker.
    pub fn last_processed_block(&self) -> Option<i64> {
        self.last_processed_block
    }

    /// The bucket totals and scalars as the `cell_dist_cursor` row; defined
    /// once a block was processed.
    pub fn cursor(&self) -> Result<CellDistTrackerCursor> {
        let (Some(last_processed_block), Some(last_snapshot_date)) =
            (self.last_processed_block, self.last_snapshot_date)
        else {
            bail!("cell_dist tracker has processed no block: no cursor to persist");
        };
        Ok(CellDistTrackerCursor {
            count_by_bucket: self.count_by_bucket,
            total_capacity_by_bucket: self.total_capacity_by_bucket,
            last_snapshot_date: last_snapshot_date.format("%Y%m%d").to_string(),
            last_processed_block,
        })
    }

    /// One cohort month's `(used_capacity, balance)`.
    pub fn cohort(&self, month: &str) -> Option<(i128, i128)> {
        self.cohort_accum.get(month).copied()
    }

    /// Stage the rows one block moved — the current value of each month in
    /// `moved_months` — and the cursor, into `batch` (the caller scopes it to
    /// the block). Returns the value bytes staged.
    pub fn stage_block_rows(
        &self,
        batch: &mut StoreBatch<'_>,
        moved_months: &BTreeSet<String>,
    ) -> Result<usize> {
        let mut bytes = 0;
        for month in moved_months {
            let (used, balance) = self.cohort(month).ok_or_else(|| {
                anyhow!("cell_dist tracker moved cohort month {month} but holds no row for it")
            })?;
            bytes += batch.put_cell_dist_cohort(month, used, balance)?;
        }
        Ok(bytes + batch.put_cell_dist_cursor(&self.cursor()?)?)
    }

    /// Stage every cohort row and the cursor (bulk finalize, onto a store
    /// without tracker rows). Returns the value bytes staged.
    pub fn stage_all_rows(&self, batch: &mut StoreBatch<'_>) -> Result<usize> {
        let mut bytes = 0;
        for (month, &(used, balance)) in &self.cohort_accum {
            bytes += batch.put_cell_dist_cohort(month, used, balance)?;
        }
        Ok(bytes + batch.put_cell_dist_cursor(&self.cursor()?)?)
    }

    /// Record that `block_number` is being processed. Blocks arrive in
    /// ascending order.
    pub fn record_block(&mut self, block_number: i64) -> Result<()> {
        if let Some(last) = self.last_processed_block {
            if block_number <= last {
                bail!(
                    "cell_dist tracker received block {block_number} after block {last}: blocks must ascend"
                );
            }
        }
        self.last_processed_block = Some(block_number);
        Ok(())
    }

    /// A cell was created with the given occupied capacity.
    pub fn cell_created(&mut self, occupied_capacity: i64) {
        let bucket = size_bucket(occupied_capacity);
        self.count_by_bucket[bucket] += 1;
        self.total_capacity_by_bucket[bucket] += occupied_capacity as i128;
    }

    /// A cell was consumed; subtract its occupied capacity from the matching size bucket.
    pub fn cell_consumed(&mut self, occupied_capacity: i64) -> Result<()> {
        if occupied_capacity <= 0 {
            bail!(
                "invalid consumed cell occupied_capacity: {}",
                occupied_capacity
            );
        }

        let bucket = size_bucket(occupied_capacity);
        // The bucket totals are stored aggregates: a consume they cannot
        // explain is rebuild-required (D-4c). The subtraction of a positive
        // capacity can only overflow below `i128::MIN`, an underflow too.
        if self.count_by_bucket[bucket] <= 0 {
            return Err(super::aggregate_underflow(format!(
                "live cell count underflow on consume: bucket={}, count={}, occupied_capacity={}",
                bucket, self.count_by_bucket[bucket], occupied_capacity
            )));
        }

        let current = self.total_capacity_by_bucket[bucket];
        let next = current
            .checked_sub(occupied_capacity as i128)
            .ok_or_else(|| {
                super::aggregate_underflow(format!(
                    "capacity subtraction overflow: bucket={}, current={}, occupied_capacity={}",
                    bucket, current, occupied_capacity
                ))
            })?;

        if next < 0 {
            return Err(super::aggregate_underflow(format!(
                "live capacity underflow on consume: bucket={}, current={}, occupied_capacity={}",
                bucket, current, occupied_capacity
            )));
        }

        // Update global totals
        self.count_by_bucket[bucket] -= 1;
        self.total_capacity_by_bucket[bucket] = next;

        Ok(())
    }

    /// Check if a day boundary was crossed. If so, compute and return a snapshot
    /// for the previous day along with its date.
    ///
    /// The snapshot is the tracker's state *right now*, labelled with the
    /// previous day, so it must be taken before `current_date`'s block is
    /// applied — otherwise day D's row silently gains the first block of D+1.
    /// Both sync paths go through `sync::reorg::begin_cell_distribution_block`,
    /// which enforces that ordering; call this directly only where that ordering
    /// is already guaranteed.
    pub fn maybe_snapshot(
        &mut self,
        current_date: NaiveDate,
    ) -> Option<(NaiveDate, DailyCellDistribution)> {
        match self.last_snapshot_date {
            Some(last) if last < current_date => {
                let snapshot = self.compute_snapshot();
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

    /// Compute the current cell distribution snapshot.
    pub fn compute_snapshot(&self) -> DailyCellDistribution {
        DailyCellDistribution {
            size_bucket_counts: self.count_by_bucket,
            size_bucket_capacities: self.total_capacity_by_bucket,
        }
    }

    /// Apply a single cohort delta for an address first seen on
    /// `first_seen_day`; it moves the row of [`cohort_month`]`(first_seen_day)`.
    ///
    /// Bulk-build uses this to update cohort state from the same address delta path
    /// that materializes `AddressBalance`, without re-reading RocksDB.
    pub fn apply_cohort_delta(
        &mut self,
        first_seen_day: NaiveDate,
        used_capacity_delta: i128,
        balance_delta: i128,
    ) -> Result<()> {
        let cohort_month = cohort_month(first_seen_day);
        let current = self
            .cohort_accum
            .get(&cohort_month)
            .copied()
            .unwrap_or((0, 0));
        if current.0 < 0 || current.1 < 0 {
            return Err(super::aggregate_underflow(format!(
                "negative cohort accumulator before delta: cohort_month={cohort_month}, used={}, balance={}",
                current.0, current.1
            )));
        }
        let used = current.0.checked_add(used_capacity_delta).ok_or_else(|| {
            anyhow::anyhow!(
                "address cohort used_capacity overflow: cohort_month={}, current={}, delta={}",
                cohort_month,
                current.0,
                used_capacity_delta
            )
        })?;
        let balance = current.1.checked_add(balance_delta).ok_or_else(|| {
            anyhow::anyhow!(
                "address cohort balance overflow: cohort_month={}, current={}, delta={}",
                cohort_month,
                current.1,
                balance_delta
            )
        })?;
        if used < 0 || balance < 0 {
            return Err(super::aggregate_underflow(format!(
                "address cohort accumulator underflow: cohort_month={cohort_month}, used={used}, balance={balance}, used_delta={used_capacity_delta}, balance_delta={balance_delta}"
            )));
        }
        self.cohort_accum.insert(cohort_month, (used, balance));
        Ok(())
    }

    /// Produce address cohort snapshot from incremental accumulator.
    pub fn cohort_snapshot(&self) -> ckbadger_store::DailyAddressCohort {
        use ckbadger_store::types::AddressCohortEntry;
        let entries: Vec<AddressCohortEntry> = self
            .cohort_accum
            .iter()
            .filter(|(_, (used, bal))| *used != 0 || *bal != 0)
            .map(|(month, (used, bal))| AddressCohortEntry {
                cohort_month: month.clone(),
                used_capacity: *used,
                total_balance: *bal,
            })
            .collect();
        ckbadger_store::DailyAddressCohort { cohorts: entries }
    }
}

#[cfg(test)]
#[allow(clippy::inconsistent_digit_grouping)]
mod tests {
    use super::*;

    #[test]
    fn test_size_bucket_boundaries() {
        // Bucket 0: <100 CKB
        assert_eq!(size_bucket(0), 0);
        assert_eq!(size_bucket(6_100_000_000), 0); // 61 CKB (minimum cell)
        assert_eq!(size_bucket(9_999_999_999), 0); // Just under 100 CKB

        // Bucket 1: 100-999 CKB
        assert_eq!(size_bucket(10_000_000_000), 1); // 100 CKB
        assert_eq!(size_bucket(50_000_000_000), 1);
        assert_eq!(size_bucket(99_999_999_999), 1);

        // Bucket 2: 1,000-9,999 CKB
        assert_eq!(size_bucket(100_000_000_000), 2); // 1,000 CKB
        assert_eq!(size_bucket(999_999_999_999), 2);

        // Bucket 3: 10,000-99,999 CKB
        assert_eq!(size_bucket(1_000_000_000_000), 3); // 10,000 CKB
        assert_eq!(size_bucket(9_999_999_999_999), 3);

        // Bucket 4: 100,000-999,999 CKB
        assert_eq!(size_bucket(10_000_000_000_000), 4); // 100,000 CKB
        assert_eq!(size_bucket(99_999_999_999_999), 4);

        // Bucket 5: >=1,000,000 CKB
        assert_eq!(size_bucket(100_000_000_000_000), 5); // 1,000,000 CKB
        assert_eq!(size_bucket(1_000_000_000_000_000), 5);
    }

    #[test]
    fn test_cell_created_consumed() {
        let mut tracker = CellDistributionTracker::new();
        // Create a cell: 500 CKB → bucket 1
        tracker.cell_created(500_00000000);
        assert_eq!(tracker.count_by_bucket[1], 1);
        assert_eq!(tracker.total_capacity_by_bucket[1], 500_00000000_i128);

        // Create another cell: 50 CKB → bucket 0
        tracker.cell_created(50_00000000);
        assert_eq!(tracker.count_by_bucket[0], 1);
        assert_eq!(tracker.total_capacity_by_bucket[0], 50_00000000_i128);

        // Consume the 500 CKB cell
        tracker.cell_consumed(500_00000000).unwrap();
        assert_eq!(tracker.count_by_bucket[1], 0);
        assert_eq!(tracker.total_capacity_by_bucket[1], 0);

        // Consume the 50 CKB cell
        tracker.cell_consumed(50_00000000).unwrap();
        assert_eq!(tracker.count_by_bucket[0], 0);
        assert_eq!(tracker.total_capacity_by_bucket[0], 0);
    }

    #[test]
    fn test_compute_snapshot_returns_size_totals() {
        let mut tracker = CellDistributionTracker::new();
        tracker.cell_created(61_00000000);
        tracker.cell_created(200_00000000);
        tracker.cell_created(5_000_00000000);

        let dist = tracker.compute_snapshot();
        assert_eq!(dist.size_bucket_counts, [1, 1, 1, 0, 0, 0]);
        assert_eq!(
            dist.size_bucket_capacities,
            [61_00000000, 200_00000000, 5_000_00000000, 0, 0, 0]
        );
    }

    #[test]
    fn test_maybe_snapshot_day_boundary() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let jan16 = NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
        tracker.cell_created(100_00000000); // 100 CKB → bucket 1

        // First call: sets last_snapshot_date, no snapshot produced
        assert!(tracker.maybe_snapshot(jan15).is_none());
        // Same day: no snapshot
        assert!(tracker.maybe_snapshot(jan15).is_none());
        // Day boundary crossed: produces snapshot for jan15
        let result = tracker.maybe_snapshot(jan16);
        assert!(result.is_some());
        let (date, dist) = result.unwrap();
        assert_eq!(date, jan15);
        assert_eq!(dist.size_bucket_counts, [0, 1, 0, 0, 0, 0]);
        assert_eq!(dist.size_bucket_capacities, [0, 100_00000000, 0, 0, 0, 0]);
        // Same day again: no snapshot
        assert!(tracker.maybe_snapshot(jan16).is_none());
    }

    #[test]
    fn test_consumed_updates_totals() {
        let mut tracker = CellDistributionTracker::new();

        // Create two cells on the same date, same bucket
        tracker.cell_created(61_00000000); // bucket 0
        tracker.cell_created(80_00000000); // bucket 0

        // Consume both
        tracker.cell_consumed(61_00000000).unwrap();
        tracker.cell_consumed(80_00000000).unwrap();
        assert_eq!(tracker.count_by_bucket[0], 0);
        assert_eq!(tracker.total_capacity_by_bucket[0], 0);
    }

    #[test]
    fn test_cell_consumed_errors_on_underflow() {
        let mut tracker = CellDistributionTracker::new();
        tracker.cell_created(61_00000000); // bucket 0, 61 CKB

        let err = tracker.cell_consumed(80_00000000).unwrap_err();
        assert!(err.to_string().contains("underflow"));
    }

    #[test]
    fn test_cell_consumed_errors_when_bucket_count_is_zero() {
        let mut tracker = CellDistributionTracker::new();
        let err = tracker.cell_consumed(61_00000000).unwrap_err();
        assert!(err.to_string().contains("live cell count underflow"));
    }

    #[test]
    fn test_multiple_buckets_same_date() {
        let mut tracker = CellDistributionTracker::new();

        // Create cells in different buckets on the same date
        tracker.cell_created(61_00000000); // bucket 0 (61 CKB)
        tracker.cell_created(500_00000000); // bucket 1 (500 CKB)
        tracker.cell_created(5_000_00000000); // bucket 2 (5000 CKB)

        assert_eq!(tracker.count_by_bucket[0], 1);
        assert_eq!(tracker.count_by_bucket[1], 1);
        assert_eq!(tracker.count_by_bucket[2], 1);
        assert_eq!(tracker.total_capacity_by_bucket[0], 61_00000000_i128);
        assert_eq!(tracker.total_capacity_by_bucket[1], 500_00000000_i128);
        assert_eq!(tracker.total_capacity_by_bucket[2], 5_000_00000000_i128);

        // Consume only bucket 1 cell — date entry should remain (other buckets still non-zero)
        tracker.cell_consumed(500_00000000).unwrap();
        assert_eq!(tracker.count_by_bucket[1], 0);
        assert_eq!(tracker.total_capacity_by_bucket[1], 0);
    }

    #[test]
    fn test_incremental_cohort_new_address() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();

        // New address first seen at block 100: used=61 CKB, balance=500 CKB
        tracker
            .apply_cohort_delta(jan15, 61_00000000, 500_00000000)
            .unwrap();

        let snapshot = tracker.cohort_snapshot();
        assert_eq!(snapshot.cohorts.len(), 1);
        assert_eq!(snapshot.cohorts[0].cohort_month, "2024-01");
        assert_eq!(snapshot.cohorts[0].used_capacity, 61_00000000);
        assert_eq!(snapshot.cohorts[0].total_balance, 500_00000000);
    }

    #[test]
    fn test_incremental_cohort_existing_address() {
        let mut tracker = CellDistributionTracker::new();
        let dec01 = NaiveDate::from_ymd_opt(2023, 12, 1).unwrap();

        // Existing address first seen in Dec 2023, activity later
        tracker
            .apply_cohort_delta(dec01, 61_00000000, 100_00000000)
            .unwrap();

        let snapshot = tracker.cohort_snapshot();
        assert_eq!(snapshot.cohorts.len(), 1);
        assert_eq!(snapshot.cohorts[0].cohort_month, "2023-12");
    }

    #[test]
    fn test_apply_cohort_delta_uses_first_seen_block_date() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();

        tracker
            .apply_cohort_delta(jan15, 61_00000000, 140_00000000)
            .unwrap();

        let snapshot = tracker.cohort_snapshot();
        assert_eq!(snapshot.cohorts.len(), 1);
        assert_eq!(snapshot.cohorts[0].cohort_month, "2024-01");
        assert_eq!(snapshot.cohorts[0].used_capacity, 61_00000000);
        assert_eq!(snapshot.cohorts[0].total_balance, 140_00000000);
    }

    #[test]
    fn test_cohort_filters_non_positive() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();

        // Add — used=61 CKB, balance=500 CKB
        tracker
            .apply_cohort_delta(jan15, 61_00000000, 500_00000000)
            .unwrap();

        // Now subtract (simulate address going to zero)
        tracker
            .apply_cohort_delta(jan15, -61_00000000, -500_00000000)
            .unwrap();

        let snapshot = tracker.cohort_snapshot();
        // Both used_capacity=0 and balance=0 means entry is filtered out
        assert_eq!(snapshot.cohorts.len(), 0);
        // The tracker retains zero months (a persisted row of (0, 0)), so
        // live sync and bulk build leave the same rows.
        assert_eq!(tracker.cohort("2024-01"), Some((0, 0)));
    }

    #[test]
    fn cohort_delta_rejects_negative_prior_and_result() {
        let mut tracker = CellDistributionTracker::new();
        let error = tracker
            .apply_cohort_delta(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), -1, 0)
            .unwrap_err();
        assert!(error.to_string().contains("underflow"), "{error}");

        let cursor = CellDistTrackerCursor {
            count_by_bucket: [0; NUM_BUCKETS],
            total_capacity_by_bucket: [0; NUM_BUCKETS],
            last_snapshot_date: "20240101".to_string(),
            last_processed_block: 1,
        };
        let error =
            CellDistributionTracker::from_rows(vec![("2024-01".to_string(), -1, 0)], cursor)
                .unwrap_err();
        assert!(error.to_string().contains("negative cohort"), "{error}");
    }

    #[test]
    fn apply_cohort_delta_moves_the_first_seen_days_month() {
        let mut tracker = CellDistributionTracker::new();
        let day = NaiveDate::from_ymd_opt(2023, 12, 31).unwrap();
        tracker.apply_cohort_delta(day, 5, 9).unwrap();
        assert_eq!(cohort_month(day), "2023-12");
        assert_eq!(tracker.cohort("2023-12"), Some((5, 9)));
    }

    #[test]
    fn record_block_requires_ascending_blocks() {
        let mut tracker = CellDistributionTracker::new();
        tracker.record_block(7).unwrap();
        let err = tracker.record_block(6).unwrap_err();
        assert!(err.to_string().contains("must ascend"), "{err}");
    }

    fn assert_rebuild_required(err: &anyhow::Error, what: &str) {
        assert!(
            crate::lifecycle::is_rebuild_required(err),
            "{what}: {err:#}"
        );
        assert!(err.to_string().contains(what), "{what}: {err:#}");
    }

    fn cursor_with(
        count_by_bucket: [i64; NUM_BUCKETS],
        total_capacity_by_bucket: [i128; NUM_BUCKETS],
        last_snapshot_date: &str,
    ) -> CellDistTrackerCursor {
        CellDistTrackerCursor {
            count_by_bucket,
            total_capacity_by_bucket,
            last_snapshot_date: last_snapshot_date.to_string(),
            last_processed_block: 1,
        }
    }

    /// D-4c: stored tracker rows the tracker cannot hold (an undecodable
    /// cursor date, a negative cohort row, a month listed twice) are
    /// corruption the chain cannot repair: rebuild-required.
    #[test]
    fn corrupt_stored_rows_are_rebuild_required() {
        let ok = || cursor_with([0; NUM_BUCKETS], [0; NUM_BUCKETS], "20240101");
        let err = CellDistributionTracker::from_rows(
            Vec::new(),
            cursor_with([0; NUM_BUCKETS], [0; NUM_BUCKETS], "2024-13-01"),
        )
        .unwrap_err();
        assert_rebuild_required(&err, "corrupt cell_dist cursor last_snapshot_date");
        let err = CellDistributionTracker::from_rows(vec![("2024-01".to_string(), -1, 0)], ok())
            .unwrap_err();
        assert_rebuild_required(&err, "negative cohort accumulator in stored");
        let err = CellDistributionTracker::from_rows(
            vec![("2024-01".to_string(), 1, 1), ("2024-01".to_string(), 2, 2)],
            ok(),
        )
        .unwrap_err();
        assert_rebuild_required(&err, "duplicate cohort month");
    }

    /// D-4c: cohort rows without the cursor that describes them are not a
    /// state any sync leaves: rebuild-required.
    #[test]
    fn cohort_rows_without_a_cursor_are_rebuild_required() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        batch.put_cell_dist_cohort("2024-01", 1, 1).unwrap();
        batch.commit().unwrap();
        let err = CellDistributionTracker::load(&store).unwrap_err();
        assert_rebuild_required(&err, "no cell_dist_cursor");
    }

    /// D-4c: a consume the bucket totals cannot explain (no live cell in the
    /// bucket, less capacity than the cell, a total so negative the
    /// subtraction overflows) is rebuild-required.
    #[test]
    fn consume_underflows_are_rebuild_required() {
        let mut tracker = CellDistributionTracker::new();
        let err = tracker.cell_consumed(61_00000000).unwrap_err();
        assert_rebuild_required(&err, "live cell count underflow on consume");

        let mut tracker = CellDistributionTracker::from_rows(
            Vec::new(),
            cursor_with([1, 0, 0, 0, 0, 0], [60_00000000, 0, 0, 0, 0, 0], "20240101"),
        )
        .unwrap();
        let err = tracker.cell_consumed(61_00000000).unwrap_err();
        assert_rebuild_required(&err, "live capacity underflow on consume");

        let mut tracker = CellDistributionTracker::from_rows(
            Vec::new(),
            cursor_with([1, 0, 0, 0, 0, 0], [i128::MIN, 0, 0, 0, 0, 0], "20240101"),
        )
        .unwrap();
        let err = tracker.cell_consumed(61_00000000).unwrap_err();
        assert_rebuild_required(&err, "capacity subtraction overflow");
    }

    /// D-4c: a cohort accumulator going (or found) negative is
    /// rebuild-required.
    #[test]
    fn cohort_underflows_are_rebuild_required() {
        let day = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let mut tracker = CellDistributionTracker::new();
        let err = tracker.apply_cohort_delta(day, -1, 0).unwrap_err();
        assert_rebuild_required(&err, "address cohort accumulator underflow");

        tracker.cohort_accum.insert(cohort_month(day), (-1, 0));
        let err = tracker.apply_cohort_delta(day, 1, 0).unwrap_err();
        assert_rebuild_required(&err, "negative cohort accumulator before delta");
    }

    /// D-4c: the caller's invariants stay plain errors: they are not stored
    /// state (blocks out of order, a non-positive consumed capacity, a cursor
    /// before any block, an overflow).
    #[test]
    fn caller_invariants_stay_plain_errors() {
        let mut tracker = CellDistributionTracker::new();
        tracker.record_block(7).unwrap();
        let err = tracker.record_block(7).unwrap_err();
        assert!(!crate::lifecycle::is_rebuild_required(&err), "{err:#}");
        let err = tracker.cell_consumed(0).unwrap_err();
        assert!(!crate::lifecycle::is_rebuild_required(&err), "{err:#}");
        let err = CellDistributionTracker::new().cursor().unwrap_err();
        assert!(!crate::lifecycle::is_rebuild_required(&err), "{err:#}");
        let day = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        tracker
            .cohort_accum
            .insert(cohort_month(day), (i128::MAX, 0));
        let err = tracker.apply_cohort_delta(day, 1, 0).unwrap_err();
        assert!(err.to_string().contains("overflow"), "{err:#}");
        assert!(!crate::lifecycle::is_rebuild_required(&err), "{err:#}");
    }

    /// Staging every row and loading it back yields the same tracker; a
    /// block stages only the months it moved, plus the cursor.
    #[test]
    fn rows_and_cursor_round_trip_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        assert!(CellDistributionTracker::new().cursor().is_err());

        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let dec01 = NaiveDate::from_ymd_opt(2023, 12, 1).unwrap();
        let mut tracker = CellDistributionTracker::new();
        tracker.record_block(100).unwrap();
        assert!(tracker.maybe_snapshot(jan15).is_none());
        tracker.cell_created(500_00000000);
        tracker.cell_created(61_00000000);
        tracker
            .apply_cohort_delta(jan15, 561_00000000, 600_00000000)
            .unwrap();
        tracker.apply_cohort_delta(dec01, 0, 0).unwrap();

        let mut batch = StoreBatch::new(&store);
        tracker.stage_all_rows(&mut batch).unwrap();
        batch.commit().unwrap();
        let restored = CellDistributionTracker::load(&store).unwrap();
        assert_eq!(restored.count_by_bucket, tracker.count_by_bucket);
        assert_eq!(
            restored.total_capacity_by_bucket,
            tracker.total_capacity_by_bucket
        );
        assert_eq!(restored.last_snapshot_date, Some(jan15));
        assert_eq!(restored.last_processed_block(), Some(100));
        assert_eq!(restored.cohort_accum, tracker.cohort_accum);

        let mut tracker = restored;
        tracker.record_block(101).unwrap();
        tracker
            .apply_cohort_delta(jan15, -61_00000000, -100_00000000)
            .unwrap();
        let mut batch = StoreBatch::new(&store);
        tracker
            .stage_block_rows(&mut batch, &BTreeSet::from([cohort_month(jan15)]))
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(
            store.list_cell_dist_cohorts().unwrap(),
            vec![
                ("2023-12".to_string(), 0, 0),
                ("2024-01".to_string(), 500_00000000, 500_00000000)
            ]
        );
        assert_eq!(
            store
                .get_cell_dist_cursor()
                .unwrap()
                .unwrap()
                .last_processed_block,
            101
        );
    }
}
