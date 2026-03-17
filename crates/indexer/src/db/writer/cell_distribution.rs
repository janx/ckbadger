//! Cell distribution tracker — tracks live cell capacity by size bucket.
//!
//! Maintains global live cell counts/capacities per size bucket and materializes a daily
//! snapshot for the size distribution chart. The same tracker also owns block→date
//! transitions plus cohort accumulation for address retention snapshots.

use std::collections::{BTreeMap, HashMap};

use anyhow::{bail, Result};
use chrono::NaiveDate;
use ckbadger_store::{CellDistributionTrackerState, DailyCellDistribution};

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

/// Tracks live cell capacity by size bucket for cell distribution chart computation.
#[derive(Debug)]
pub struct CellDistributionTracker {
    /// Cell count per size bucket (global totals).
    count_by_bucket: [i64; NUM_BUCKETS],
    /// Total capacity per size bucket (global totals, shannons).
    total_capacity_by_bucket: [i128; NUM_BUCKETS],
    /// Sorted list of (block_number, date) transitions for block→date lookup.
    block_date_transitions: Vec<(i64, NaiveDate)>,
    /// The date of the last snapshot written (to detect day boundaries).
    last_snapshot_date: Option<NaiveDate>,
    /// Incremental address cohort accumulator: cohort_month → (used_capacity, balance).
    cohort_accum: BTreeMap<String, (i128, i128)>,
    /// The last block number processed (updated for every block, not just date transitions).
    last_processed_block: Option<i64>,
}

impl CellDistributionTracker {
    pub fn new() -> Self {
        Self {
            count_by_bucket: [0; NUM_BUCKETS],
            total_capacity_by_bucket: [0; NUM_BUCKETS],
            block_date_transitions: Vec::new(),
            last_snapshot_date: None,
            cohort_accum: BTreeMap::new(),
            last_processed_block: None,
        }
    }

    /// Restore tracker from persisted state.
    pub fn from_state(state: CellDistributionTrackerState) -> Result<Self> {
        let mut block_date_transitions = Vec::new();
        for (block, date_str) in state.date_transitions {
            let date = NaiveDate::parse_from_str(&date_str, "%Y%m%d").map_err(|e| {
                anyhow::anyhow!(
                    "corrupt cell_dist date_transitions entry: block={}, date='{}': {}",
                    block,
                    date_str,
                    e
                )
            })?;
            block_date_transitions.push((block, date));
        }

        let last_snapshot_date = state
            .last_snapshot_date
            .map(|s| {
                NaiveDate::parse_from_str(&s, "%Y%m%d").map_err(|e| {
                    anyhow::anyhow!("corrupt cell_dist last_snapshot_date: date='{}': {}", s, e)
                })
            })
            .transpose()?;

        let cohort_accum: BTreeMap<String, (i128, i128)> = state
            .cohort_accum
            .into_iter()
            .map(|(month, used, bal)| (month, (used, bal)))
            .collect();

        Ok(Self {
            count_by_bucket: state.count_by_bucket,
            total_capacity_by_bucket: state.total_capacity_by_bucket,
            block_date_transitions,
            last_snapshot_date,
            cohort_accum,
            last_processed_block: state.last_processed_block,
        })
    }

    /// Serialize tracker state for persistence.
    pub fn to_state(&self) -> CellDistributionTrackerState {
        let date_transitions = self
            .block_date_transitions
            .iter()
            .map(|(b, d)| (*b, d.format("%Y%m%d").to_string()))
            .collect();
        let last_snapshot_date = self
            .last_snapshot_date
            .map(|d| d.format("%Y%m%d").to_string());
        let cohort_accum = self
            .cohort_accum
            .iter()
            .map(|(month, (used, bal))| (month.clone(), *used, *bal))
            .collect();
        CellDistributionTrackerState {
            count_by_bucket: self.count_by_bucket,
            total_capacity_by_bucket: self.total_capacity_by_bucket,
            date_transitions,
            last_snapshot_date,
            cohort_accum,
            last_processed_block: self.last_processed_block,
        }
    }

    /// Record a block→date mapping. Only records transitions (when date changes),
    /// but always updates `last_processed_block` for consistency tracking.
    pub fn record_block_date(&mut self, block_number: i64, date: NaiveDate) {
        self.last_processed_block = Some(block_number);
        if let Some((_, last_date)) = self.block_date_transitions.last() {
            if *last_date == date {
                return; // Same date, no transition needed
            }
        }
        self.block_date_transitions.push((block_number, date));
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
        if self.count_by_bucket[bucket] <= 0 {
            bail!(
                "live cell count underflow on consume: bucket={}, count={}, occupied_capacity={}",
                bucket,
                self.count_by_bucket[bucket],
                occupied_capacity
            );
        }

        let current = self.total_capacity_by_bucket[bucket];
        let next = current
            .checked_sub(occupied_capacity as i128)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "capacity subtraction overflow: bucket={}, current={}, occupied_capacity={}",
                    bucket,
                    current,
                    occupied_capacity
                )
            })?;

        if next < 0 {
            bail!(
                "live capacity underflow on consume: bucket={}, current={}, occupied_capacity={}",
                bucket,
                current,
                occupied_capacity
            );
        }

        // Update global totals
        self.count_by_bucket[bucket] -= 1;
        self.total_capacity_by_bucket[bucket] = next;

        Ok(())
    }

    /// Check if a day boundary was crossed. If so, compute and return a snapshot
    /// for the previous day along with its date.
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

    /// Update cohort accumulator from address balance changes in this batch.
    ///
    /// For each changed address, determine cohort month from first_seen_block
    /// (existing addresses: from prefetched AddressBalance, new addresses: last_block from changes).
    /// Apply balance and used_capacity deltas.
    pub fn update_cohort_deltas(
        &mut self,
        changes: &HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>,
        prefetched_balances: &HashMap<Vec<u8>, Option<ckbadger_store::types::AddressBalance>>,
    ) {
        for (
            lock_hash,
            &(balance_delta, _live, _created, _tx, last_block, ref _tx_hash, used_delta),
        ) in changes
        {
            let first_seen_block = prefetched_balances
                .get(lock_hash)
                .and_then(|opt| opt.as_ref())
                .map(|bal| bal.first_seen_block)
                .unwrap_or(last_block); // new address

            let cohort_date = match self.block_number_to_date(first_seen_block) {
                Some(d) => d,
                None => continue,
            };
            let cohort_month = cohort_date.format("%Y-%m").to_string();

            let entry = self.cohort_accum.entry(cohort_month).or_insert((0, 0));
            entry.0 += used_delta;
            entry.1 += balance_delta;
        }
    }

    /// Produce address cohort snapshot from incremental accumulator.
    pub fn cohort_snapshot(&self) -> ckbadger_store::DailyAddressCohort {
        use ckbadger_store::types::AddressCohortEntry;
        let entries: Vec<AddressCohortEntry> = self
            .cohort_accum
            .iter()
            .filter(|(_, (used, bal))| *used > 0 || *bal > 0)
            .map(|(month, (used, bal))| AddressCohortEntry {
                cohort_month: month.clone(),
                used_capacity: *used,
                total_balance: *bal,
            })
            .collect();
        ckbadger_store::DailyAddressCohort { cohorts: entries }
    }

    /// Look up the date for a given block number using binary search on transitions.
    pub fn block_number_to_date(&self, block_number: i64) -> Option<NaiveDate> {
        if self.block_date_transitions.is_empty() {
            return None;
        }
        // Binary search for the last transition where block_number >= transition.block_number
        let idx = self
            .block_date_transitions
            .partition_point(|(b, _)| *b <= block_number);
        if idx == 0 {
            // Block is before our first recorded transition — use first date
            Some(self.block_date_transitions[0].1)
        } else {
            Some(self.block_date_transitions[idx - 1].1)
        }
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
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, date);

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
    fn test_state_roundtrip() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let jan16 = NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
        tracker.record_block_date(100, jan15);
        tracker.record_block_date(200, jan16);

        tracker.cell_created(500_00000000); // bucket 1
        tracker.cell_created(61_00000000); // bucket 0
        tracker.cell_created(1_000_00000000); // bucket 2
        tracker.last_snapshot_date = Some(jan16);

        let state = tracker.to_state();
        let restored = CellDistributionTracker::from_state(state).unwrap();

        assert_eq!(restored.block_date_transitions.len(), 2);
        assert_eq!(restored.last_snapshot_date, Some(jan16));
        assert_eq!(restored.count_by_bucket[0], 1);
        assert_eq!(restored.count_by_bucket[1], 1);
        assert_eq!(restored.count_by_bucket[2], 1);
        assert_eq!(restored.total_capacity_by_bucket[0], 61_00000000_i128);
        assert_eq!(restored.total_capacity_by_bucket[1], 500_00000000_i128);
        assert_eq!(restored.total_capacity_by_bucket[2], 1_000_00000000_i128);
    }

    #[test]
    fn test_maybe_snapshot_day_boundary() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let jan16 = NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
        tracker.cell_created(100_00000000);

        // First call: sets last_snapshot_date, no snapshot produced
        assert!(tracker.maybe_snapshot(jan15).is_none());
        // Same day: no snapshot
        assert!(tracker.maybe_snapshot(jan15).is_none());
        // Day boundary crossed: produces snapshot for jan15
        let result = tracker.maybe_snapshot(jan16);
        assert!(result.is_some());
        let (date, dist) = result.unwrap();
        assert_eq!(date, jan15);
        assert_eq!(dist.size_bucket_counts, [1, 0, 0, 0, 0, 0]);
        assert_eq!(dist.size_bucket_capacities, [100_00000000, 0, 0, 0, 0, 0]);
        // Same day again: no snapshot
        assert!(tracker.maybe_snapshot(jan16).is_none());
    }

    #[test]
    fn test_consumed_updates_totals() {
        let mut tracker = CellDistributionTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, date);

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
    fn test_record_block_date_dedup() {
        let mut tracker = CellDistributionTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, date);
        tracker.record_block_date(101, date);
        tracker.record_block_date(102, date);
        // Only one transition recorded (dedup)
        assert_eq!(tracker.block_date_transitions.len(), 1);
    }

    #[test]
    fn test_block_date_lookup() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let jan16 = NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
        let jan17 = NaiveDate::from_ymd_opt(2024, 1, 17).unwrap();
        tracker.record_block_date(100, jan15);
        tracker.record_block_date(200, jan16);
        tracker.record_block_date(300, jan17);
        // Blocks within jan15 range
        assert_eq!(tracker.block_number_to_date(100), Some(jan15));
        assert_eq!(tracker.block_number_to_date(150), Some(jan15));
        assert_eq!(tracker.block_number_to_date(199), Some(jan15));
        // Blocks within jan16 range
        assert_eq!(tracker.block_number_to_date(200), Some(jan16));
        // Block in jan17
        assert_eq!(tracker.block_number_to_date(300), Some(jan17));
        assert_eq!(tracker.block_number_to_date(999), Some(jan17));
        // Block before first transition
        assert_eq!(tracker.block_number_to_date(50), Some(jan15));
    }

    #[test]
    fn test_from_state_rejects_corrupt_transition_date() {
        let state = CellDistributionTrackerState {
            count_by_bucket: [0; 6],
            total_capacity_by_bucket: [0; 6],
            date_transitions: vec![(100, "bad-date".to_string())],
            last_snapshot_date: None,
            cohort_accum: vec![],
            last_processed_block: None,
        };
        let err = CellDistributionTracker::from_state(state).unwrap_err();
        assert!(err
            .to_string()
            .contains("corrupt cell_dist date_transitions"));
    }

    #[test]
    fn test_multiple_buckets_same_date() {
        let mut tracker = CellDistributionTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, date);

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
        tracker.record_block_date(100, jan15);

        let mut changes = HashMap::new();
        changes.insert(
            vec![0xAA; 32],
            (
                500_00000000i128,
                1i32,
                1i32,
                1i64,
                100i64,
                vec![0x01; 32],
                61_00000000i128,
            ),
        );

        let prefetched: HashMap<Vec<u8>, Option<ckbadger_store::types::AddressBalance>> =
            HashMap::new();
        tracker.update_cohort_deltas(&changes, &prefetched);

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
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(50, dec01);
        tracker.record_block_date(100, jan15);

        let mut changes = HashMap::new();
        changes.insert(
            vec![0xBB; 32],
            (
                100_00000000i128,
                1i32,
                1i32,
                1i64,
                100i64,
                vec![0x02; 32],
                61_00000000i128,
            ),
        );

        let mut prefetched = HashMap::new();
        prefetched.insert(
            vec![0xBB; 32],
            Some(ckbadger_store::types::AddressBalance {
                first_seen_block: 50,
                ..Default::default()
            }),
        );

        tracker.update_cohort_deltas(&changes, &prefetched);

        let snapshot = tracker.cohort_snapshot();
        assert_eq!(snapshot.cohorts.len(), 1);
        assert_eq!(snapshot.cohorts[0].cohort_month, "2023-12");
    }

    #[test]
    fn test_cohort_state_roundtrip() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, jan15);

        let mut changes = HashMap::new();
        changes.insert(
            vec![0xCC; 32],
            (1000i128, 1i32, 1i32, 1i64, 100i64, vec![0x03; 32], 500i128),
        );
        tracker.update_cohort_deltas(&changes, &HashMap::new());

        let state = tracker.to_state();
        assert_eq!(state.cohort_accum.len(), 1);

        let restored = CellDistributionTracker::from_state(state).unwrap();
        let snapshot = restored.cohort_snapshot();
        assert_eq!(snapshot.cohorts.len(), 1);
        assert_eq!(snapshot.cohorts[0].total_balance, 1000);
    }

    #[test]
    fn test_cohort_filters_non_positive() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, jan15);

        // Add then subtract — both used_capacity and balance become zero
        let mut changes = HashMap::new();
        changes.insert(
            vec![0xDD; 32],
            (
                500_00000000i128,
                1i32,
                1i32,
                1i64,
                100i64,
                vec![0x04; 32],
                61_00000000i128,
            ),
        );
        tracker.update_cohort_deltas(&changes, &HashMap::new());

        // Now subtract (simulate address going to zero)
        let mut changes2 = HashMap::new();
        changes2.insert(
            vec![0xDD; 32],
            (
                -500_00000000i128,
                -1i32,
                0i32,
                0i64,
                100i64,
                vec![0x05; 32],
                -61_00000000i128,
            ),
        );
        tracker.update_cohort_deltas(&changes2, &HashMap::new());

        let snapshot = tracker.cohort_snapshot();
        // Both used_capacity=0 and balance=0 means entry is filtered out
        assert_eq!(snapshot.cohorts.len(), 0);
    }
}
