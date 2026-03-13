//! Cell distribution tracker — tracks live cell capacity by age and size bucket.
//!
//! Maintains an in-memory `HashMap<NaiveDate, [i128; 6]>` mapping cell creation dates
//! to their total live capacity per size bucket. At each day boundary, computes age-band
//! and size-bucket snapshots for the cell distribution chart.

use std::collections::HashMap;

use anyhow::{bail, Result};
use chrono::NaiveDate;
use ckbadger_store::{CellDistributionTrackerState, DailyCellDistribution};

/// Number of size buckets for cell distribution.
#[allow(dead_code)]
const NUM_BUCKETS: usize = 6;

/// CKB per shannon.
#[allow(dead_code)]
const CKB: i64 = 100_000_000;

/// Determine the size bucket index for a given occupied capacity (in shannons).
///
/// Bucket 0: <100 CKB
/// Bucket 1: 100-999 CKB
/// Bucket 2: 1,000-9,999 CKB
/// Bucket 3: 10,000-99,999 CKB
/// Bucket 4: 100,000-999,999 CKB
/// Bucket 5: >=1,000,000 CKB
#[allow(dead_code)]
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

/// Tracks live cell capacity by creation date and size bucket for cell distribution chart computation.
#[derive(Debug)]
#[allow(dead_code)]
pub struct CellDistributionTracker {
    /// Total live capacity (shannons) per cell creation date per size bucket.
    capacity_by_date_and_bucket: HashMap<NaiveDate, [i128; NUM_BUCKETS]>,
    /// Cell count per size bucket (global totals).
    count_by_bucket: [i64; NUM_BUCKETS],
    /// Total capacity per size bucket (global totals, shannons).
    total_capacity_by_bucket: [i128; NUM_BUCKETS],
    /// Sorted list of (block_number, date) transitions for block→date lookup.
    block_date_transitions: Vec<(i64, NaiveDate)>,
    /// The date of the last snapshot written (to detect day boundaries).
    last_snapshot_date: Option<NaiveDate>,
}

#[allow(dead_code)]
impl CellDistributionTracker {
    pub fn new() -> Self {
        Self {
            capacity_by_date_and_bucket: HashMap::new(),
            count_by_bucket: [0; NUM_BUCKETS],
            total_capacity_by_bucket: [0; NUM_BUCKETS],
            block_date_transitions: Vec::new(),
            last_snapshot_date: None,
        }
    }

    /// Restore tracker from persisted state.
    pub fn from_state(state: CellDistributionTrackerState) -> Result<Self> {
        let mut capacity_by_date_and_bucket = HashMap::new();
        for (date_str, buckets) in state.capacity_by_date_and_bucket {
            let date = NaiveDate::parse_from_str(&date_str, "%Y%m%d").map_err(|e| {
                anyhow::anyhow!(
                    "corrupt cell_dist capacity_by_date_and_bucket entry: date='{}': {}",
                    date_str,
                    e
                )
            })?;
            capacity_by_date_and_bucket.insert(date, buckets);
        }

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

        Ok(Self {
            capacity_by_date_and_bucket,
            count_by_bucket: state.count_by_bucket,
            total_capacity_by_bucket: state.total_capacity_by_bucket,
            block_date_transitions,
            last_snapshot_date,
        })
    }

    /// Serialize tracker state for persistence.
    pub fn to_state(&self) -> CellDistributionTrackerState {
        let capacity_by_date_and_bucket = self
            .capacity_by_date_and_bucket
            .iter()
            .map(|(d, buckets)| (d.format("%Y%m%d").to_string(), *buckets))
            .collect();
        let date_transitions = self
            .block_date_transitions
            .iter()
            .map(|(b, d)| (*b, d.format("%Y%m%d").to_string()))
            .collect();
        let last_snapshot_date = self
            .last_snapshot_date
            .map(|d| d.format("%Y%m%d").to_string());
        CellDistributionTrackerState {
            capacity_by_date_and_bucket,
            count_by_bucket: self.count_by_bucket,
            total_capacity_by_bucket: self.total_capacity_by_bucket,
            date_transitions,
            last_snapshot_date,
        }
    }

    /// Record a block→date mapping. Only records transitions (when date changes).
    pub fn record_block_date(&mut self, block_number: i64, date: NaiveDate) {
        if let Some((_, last_date)) = self.block_date_transitions.last() {
            if *last_date == date {
                return; // Same date, no transition needed
            }
        }
        self.block_date_transitions.push((block_number, date));
    }

    /// A cell was created at the given date with the given occupied capacity.
    pub fn cell_created(&mut self, block_date: NaiveDate, occupied_capacity: i64) {
        let bucket = size_bucket(occupied_capacity);
        let entry = self
            .capacity_by_date_and_bucket
            .entry(block_date)
            .or_insert([0; NUM_BUCKETS]);
        entry[bucket] += occupied_capacity as i128;
        self.count_by_bucket[bucket] += 1;
        self.total_capacity_by_bucket[bucket] += occupied_capacity as i128;
    }

    /// A cell was consumed. Look up its creation date from block number and subtract capacity.
    pub fn cell_consumed(&mut self, created_at_block: i64, occupied_capacity: i64) -> Result<()> {
        if occupied_capacity <= 0 {
            bail!(
                "invalid consumed cell occupied_capacity: created_at_block={}, occupied_capacity={}",
                created_at_block,
                occupied_capacity
            );
        }

        let creation_date = self.block_number_to_date(created_at_block).ok_or_else(|| {
            anyhow::anyhow!(
                "missing block-date transition for consumed cell: created_at_block={}, transitions={}",
                created_at_block,
                self.block_date_transitions.len()
            )
        })?;

        let bucket = size_bucket(occupied_capacity);

        let buckets = self
            .capacity_by_date_and_bucket
            .get_mut(&creation_date)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "missing live capacity bucket for consumed cell: created_at_block={}, creation_date={}",
                    created_at_block,
                    creation_date
                )
            })?;

        let current = buckets[bucket];
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
                "live capacity underflow on consume: created_at_block={}, creation_date={}, bucket={}, current={}, occupied_capacity={}",
                created_at_block,
                creation_date,
                bucket,
                current,
                occupied_capacity
            );
        }

        buckets[bucket] = next;

        // Update global totals
        self.count_by_bucket[bucket] -= 1;
        self.total_capacity_by_bucket[bucket] -= occupied_capacity as i128;

        // Clean up zero entries: remove the date entry if all buckets are zero
        if buckets.iter().all(|&c| c == 0) {
            self.capacity_by_date_and_bucket.remove(&creation_date);
        }

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

    /// Compute the cell distribution snapshot for the given date.
    pub fn compute_snapshot(&self, snapshot_date: NaiveDate) -> DailyCellDistribution {
        let mut dist = DailyCellDistribution {
            size_bucket_counts: self.count_by_bucket,
            size_bucket_capacities: self.total_capacity_by_bucket,
            ..Default::default()
        };

        for (&creation_date, buckets) in &self.capacity_by_date_and_bucket {
            let total_capacity: i128 = buckets.iter().sum();
            if total_capacity <= 0 {
                continue;
            }
            let age_days = (snapshot_date - creation_date).num_days();
            match age_days {
                d if d < 1 => dist.age_band_lt1d += total_capacity,
                1..=6 => dist.age_band_1d_7d += total_capacity,
                7..=29 => dist.age_band_7d_30d += total_capacity,
                30..=179 => dist.age_band_30d_180d += total_capacity,
                _ => dist.age_band_gt180d += total_capacity,
            }
        }

        dist
    }

    /// Look up the date for a given block number using binary search on transitions.
    fn block_number_to_date(&self, block_number: i64) -> Option<NaiveDate> {
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
        tracker.cell_created(date, 500_00000000);
        assert_eq!(tracker.count_by_bucket[1], 1);
        assert_eq!(tracker.total_capacity_by_bucket[1], 500_00000000_i128);
        assert_eq!(
            tracker.capacity_by_date_and_bucket[&date][1],
            500_00000000_i128
        );

        // Create another cell: 50 CKB → bucket 0
        tracker.cell_created(date, 50_00000000);
        assert_eq!(tracker.count_by_bucket[0], 1);
        assert_eq!(tracker.total_capacity_by_bucket[0], 50_00000000_i128);

        // Consume the 500 CKB cell
        tracker.cell_consumed(100, 500_00000000).unwrap();
        assert_eq!(tracker.count_by_bucket[1], 0);
        assert_eq!(tracker.total_capacity_by_bucket[1], 0);

        // Consume the 50 CKB cell
        tracker.cell_consumed(100, 50_00000000).unwrap();
        assert_eq!(tracker.count_by_bucket[0], 0);
        assert_eq!(tracker.total_capacity_by_bucket[0], 0);

        // Date entry should be cleaned up (all buckets zero)
        assert!(!tracker.capacity_by_date_and_bucket.contains_key(&date));
    }

    #[test]
    fn test_snapshot_age_bands() {
        let mut tracker = CellDistributionTracker::new();
        let snapshot_date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();

        // age_band_lt1d: same day (age_days=0)
        tracker.cell_created(snapshot_date, 61_00000000);
        // age_band_1d_7d: 3 days ago (age_days=3)
        tracker.cell_created(
            NaiveDate::from_ymd_opt(2024, 6, 12).unwrap(),
            200_00000000, // bucket 1
        );
        // age_band_7d_30d: 14 days ago (age_days=14)
        tracker.cell_created(
            NaiveDate::from_ymd_opt(2024, 6, 1).unwrap(),
            5_000_00000000, // bucket 2
        );
        // age_band_30d_180d: 60 days ago (age_days=60)
        tracker.cell_created(
            NaiveDate::from_ymd_opt(2024, 4, 16).unwrap(),
            50_000_00000000, // bucket 3
        );
        // age_band_gt180d: 200 days ago (age_days=200)
        tracker.cell_created(
            NaiveDate::from_ymd_opt(2023, 11, 28).unwrap(),
            500_000_00000000, // bucket 4
        );

        let dist = tracker.compute_snapshot(snapshot_date);
        assert_eq!(dist.age_band_lt1d, 61_00000000);
        assert_eq!(dist.age_band_1d_7d, 200_00000000);
        assert_eq!(dist.age_band_7d_30d, 5_000_00000000);
        assert_eq!(dist.age_band_30d_180d, 50_000_00000000);
        assert_eq!(dist.age_band_gt180d, 500_000_00000000);
        // Size bucket counts and capacities
        assert_eq!(dist.size_bucket_counts, [1, 1, 1, 1, 1, 0]);
    }

    #[test]
    fn test_state_roundtrip() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let jan16 = NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
        tracker.record_block_date(100, jan15);
        tracker.record_block_date(200, jan16);

        // Create cells on two dates
        tracker.cell_created(jan15, 500_00000000); // bucket 1
        tracker.cell_created(jan15, 61_00000000); // bucket 0
        tracker.cell_created(jan16, 1_000_00000000); // bucket 2
        tracker.last_snapshot_date = Some(jan16);

        let state = tracker.to_state();
        let restored = CellDistributionTracker::from_state(state).unwrap();

        assert_eq!(restored.block_date_transitions.len(), 2);
        assert_eq!(restored.last_snapshot_date, Some(jan16));
        assert_eq!(restored.count_by_bucket[0], 1);
        assert_eq!(restored.count_by_bucket[1], 1);
        assert_eq!(restored.count_by_bucket[2], 1);
        assert_eq!(restored.total_capacity_by_bucket[1], 500_00000000_i128);
        assert_eq!(
            restored.capacity_by_date_and_bucket[&jan15][1],
            500_00000000_i128
        );
        assert_eq!(
            restored.capacity_by_date_and_bucket[&jan15][0],
            61_00000000_i128
        );
        assert_eq!(
            restored.capacity_by_date_and_bucket[&jan16][2],
            1_000_00000000_i128
        );
    }

    #[test]
    fn test_maybe_snapshot_day_boundary() {
        let mut tracker = CellDistributionTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let jan16 = NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
        tracker.cell_created(jan15, 100_00000000);

        // First call: sets last_snapshot_date, no snapshot produced
        assert!(tracker.maybe_snapshot(jan15).is_none());
        // Same day: no snapshot
        assert!(tracker.maybe_snapshot(jan15).is_none());
        // Day boundary crossed: produces snapshot for jan15
        let result = tracker.maybe_snapshot(jan16);
        assert!(result.is_some());
        let (date, dist) = result.unwrap();
        assert_eq!(date, jan15);
        // Cell created on jan15, snapshot on jan15 → age_days=0 → age_band_lt1d
        assert_eq!(dist.age_band_lt1d, 100_00000000);
        // Same day again: no snapshot
        assert!(tracker.maybe_snapshot(jan16).is_none());
    }

    #[test]
    fn test_consumed_removes_zero_entries() {
        let mut tracker = CellDistributionTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, date);

        // Create two cells on the same date, same bucket
        tracker.cell_created(date, 61_00000000); // bucket 0
        tracker.cell_created(date, 80_00000000); // bucket 0
        assert!(tracker.capacity_by_date_and_bucket.contains_key(&date));

        // Consume both
        tracker.cell_consumed(100, 61_00000000).unwrap();
        assert!(tracker.capacity_by_date_and_bucket.contains_key(&date)); // still has 80 CKB
        tracker.cell_consumed(100, 80_00000000).unwrap();

        // Entry should be cleaned up since all buckets are zero
        assert!(!tracker.capacity_by_date_and_bucket.contains_key(&date));
        assert_eq!(tracker.count_by_bucket[0], 0);
        assert_eq!(tracker.total_capacity_by_bucket[0], 0);
    }

    #[test]
    fn test_cell_consumed_errors_on_underflow() {
        let mut tracker = CellDistributionTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, date);
        tracker.cell_created(date, 61_00000000); // bucket 0, 61 CKB

        let err = tracker.cell_consumed(100, 80_00000000).unwrap_err();
        assert!(err.to_string().contains("underflow"));
    }

    #[test]
    fn test_cell_consumed_errors_when_no_matching_bucket() {
        let mut tracker = CellDistributionTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, date);

        let err = tracker.cell_consumed(100, 61_00000000).unwrap_err();
        assert!(err.to_string().contains("missing live capacity bucket"));
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
    fn test_from_state_rejects_corrupt_date() {
        let state = CellDistributionTrackerState {
            capacity_by_date_and_bucket: vec![("not-a-date".to_string(), [0; 6])],
            count_by_bucket: [0; 6],
            total_capacity_by_bucket: [0; 6],
            date_transitions: vec![],
            last_snapshot_date: None,
        };
        let err = CellDistributionTracker::from_state(state).unwrap_err();
        assert!(err
            .to_string()
            .contains("corrupt cell_dist capacity_by_date_and_bucket"));
    }

    #[test]
    fn test_from_state_rejects_corrupt_transition_date() {
        let state = CellDistributionTrackerState {
            capacity_by_date_and_bucket: vec![],
            count_by_bucket: [0; 6],
            total_capacity_by_bucket: [0; 6],
            date_transitions: vec![(100, "bad-date".to_string())],
            last_snapshot_date: None,
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
        tracker.cell_created(date, 61_00000000); // bucket 0 (61 CKB)
        tracker.cell_created(date, 500_00000000); // bucket 1 (500 CKB)
        tracker.cell_created(date, 5_000_00000000); // bucket 2 (5000 CKB)

        assert_eq!(tracker.count_by_bucket[0], 1);
        assert_eq!(tracker.count_by_bucket[1], 1);
        assert_eq!(tracker.count_by_bucket[2], 1);
        assert_eq!(
            tracker.capacity_by_date_and_bucket[&date][0],
            61_00000000_i128
        );
        assert_eq!(
            tracker.capacity_by_date_and_bucket[&date][1],
            500_00000000_i128
        );
        assert_eq!(
            tracker.capacity_by_date_and_bucket[&date][2],
            5_000_00000000_i128
        );

        // Consume only bucket 1 cell — date entry should remain (other buckets still non-zero)
        tracker.cell_consumed(100, 500_00000000).unwrap();
        assert!(tracker.capacity_by_date_and_bucket.contains_key(&date));
        assert_eq!(tracker.capacity_by_date_and_bucket[&date][1], 0);
        assert_eq!(tracker.count_by_bucket[1], 0);
    }
}
