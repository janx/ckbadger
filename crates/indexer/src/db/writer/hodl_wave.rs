//! HODL Wave tracker — tracks live cell capacity distribution by age.
//!
//! Maintains an in-memory `HashMap<NaiveDate, i128>` mapping cell creation dates
//! to their total live capacity. At each day boundary, computes age-band snapshots
//! for the HODL wave chart.

use std::collections::HashMap;

use chrono::NaiveDate;
use ckbadger_store::{DailyHodlWave, HodlTrackerState};

/// Tracks live cell capacity by creation date for HODL wave chart computation.
pub struct HodlWaveTracker {
    /// Total live capacity (shannons) per cell creation date.
    capacity_by_creation_date: HashMap<NaiveDate, i128>,
    /// Sorted list of (block_number, date) transitions for block→date lookup.
    block_date_transitions: Vec<(i64, NaiveDate)>,
    /// Number of addresses with at least one live cell.
    pub holder_count: i64,
    /// The date of the last snapshot written (to detect day boundaries).
    pub last_snapshot_date: Option<NaiveDate>,
}

impl HodlWaveTracker {
    pub fn new() -> Self {
        Self {
            capacity_by_creation_date: HashMap::new(),
            block_date_transitions: Vec::new(),
            holder_count: 0,
            last_snapshot_date: None,
        }
    }

    /// Restore tracker from persisted state.
    pub fn from_state(state: HodlTrackerState) -> Self {
        let capacity_by_creation_date = state
            .capacity_by_date
            .into_iter()
            .filter_map(|(date_str, cap)| {
                NaiveDate::parse_from_str(&date_str, "%Y%m%d")
                    .ok()
                    .map(|d| (d, cap))
            })
            .collect();
        let block_date_transitions = state
            .date_transitions
            .into_iter()
            .filter_map(|(block, date_str)| {
                NaiveDate::parse_from_str(&date_str, "%Y%m%d")
                    .ok()
                    .map(|d| (block, d))
            })
            .collect();
        let last_snapshot_date = state
            .last_snapshot_date
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y%m%d").ok());
        Self {
            capacity_by_creation_date,
            block_date_transitions,
            holder_count: state.holder_count,
            last_snapshot_date,
        }
    }

    /// Serialize tracker state for persistence.
    pub fn to_state(&self) -> HodlTrackerState {
        let capacity_by_date = self
            .capacity_by_creation_date
            .iter()
            .map(|(d, c)| (d.format("%Y%m%d").to_string(), *c))
            .collect();
        let date_transitions = self
            .block_date_transitions
            .iter()
            .map(|(b, d)| (*b, d.format("%Y%m%d").to_string()))
            .collect();
        let last_snapshot_date = self
            .last_snapshot_date
            .map(|d| d.format("%Y%m%d").to_string());
        HodlTrackerState {
            capacity_by_date,
            date_transitions,
            holder_count: self.holder_count,
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

    /// A cell was created at the given date with the given capacity.
    pub fn cell_created(&mut self, block_date: NaiveDate, capacity: i64) {
        *self
            .capacity_by_creation_date
            .entry(block_date)
            .or_insert(0) += capacity as i128;
    }

    /// A cell was consumed. Look up its creation date from block number and subtract capacity.
    pub fn cell_consumed(&mut self, created_at_block: i64, capacity: i64) {
        if let Some(creation_date) = self.block_number_to_date(created_at_block) {
            let entry = self
                .capacity_by_creation_date
                .entry(creation_date)
                .or_insert(0);
            *entry -= capacity as i128;
            if *entry <= 0 {
                self.capacity_by_creation_date.remove(&creation_date);
            }
        }
    }

    /// Track holder count transitions based on live cell count changes.
    /// old_live=0, new_live>0 → new holder (+1)
    /// old_live>0, new_live=0 → lost holder (-1)
    pub fn update_holder_count(&mut self, old_live_cells: i32, new_live_cells: i32) {
        if old_live_cells == 0 && new_live_cells > 0 {
            self.holder_count += 1;
        } else if old_live_cells > 0 && new_live_cells == 0 {
            self.holder_count -= 1;
        }
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
        tracker.record_block_date(100, date);
        tracker.cell_created(date, 200_00000000);
        tracker.cell_consumed(100, 50_00000000);
        assert_eq!(tracker.capacity_by_creation_date[&date], 150_00000000_i128);
    }

    #[test]
    fn test_cell_consumed_removes_zero_entries() {
        let mut tracker = HodlWaveTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, date);
        tracker.cell_created(date, 100_00000000);
        tracker.cell_consumed(100, 100_00000000);
        assert!(!tracker.capacity_by_creation_date.contains_key(&date));
    }

    #[test]
    fn test_block_date_lookup() {
        let mut tracker = HodlWaveTracker::new();
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
        assert_eq!(tracker.block_number_to_date(250), Some(jan16));
        // Block in jan17
        assert_eq!(tracker.block_number_to_date(300), Some(jan17));
        assert_eq!(tracker.block_number_to_date(999), Some(jan17));
        // Block before first transition
        assert_eq!(tracker.block_number_to_date(50), Some(jan15));
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
        tracker.update_holder_count(0, 1);
        assert_eq!(tracker.holder_count, 1);
        // Existing holder gets more cells: 1 → 5 (no change)
        tracker.update_holder_count(1, 5);
        assert_eq!(tracker.holder_count, 1);
        // Another new holder
        tracker.update_holder_count(0, 3);
        assert_eq!(tracker.holder_count, 2);
        // First holder loses all cells: 5 → 0
        tracker.update_holder_count(5, 0);
        assert_eq!(tracker.holder_count, 1);
        // 0 → 0 (no change)
        tracker.update_holder_count(0, 0);
        assert_eq!(tracker.holder_count, 1);
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
    fn test_record_block_date_dedup() {
        let mut tracker = HodlWaveTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.record_block_date(100, date);
        tracker.record_block_date(101, date);
        tracker.record_block_date(102, date);
        // Only one transition recorded (dedup)
        assert_eq!(tracker.block_date_transitions.len(), 1);
    }

    #[test]
    fn test_state_roundtrip() {
        let mut tracker = HodlWaveTracker::new();
        let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let jan16 = NaiveDate::from_ymd_opt(2024, 1, 16).unwrap();
        tracker.record_block_date(100, jan15);
        tracker.record_block_date(200, jan16);
        tracker.cell_created(jan15, 500_00000000);
        tracker.cell_created(jan16, 300_00000000);
        tracker.holder_count = 42;
        tracker.last_snapshot_date = Some(jan16);

        let state = tracker.to_state();
        let restored = HodlWaveTracker::from_state(state);

        assert_eq!(restored.block_date_transitions.len(), 2);
        assert_eq!(restored.holder_count, 42);
        assert_eq!(restored.last_snapshot_date, Some(jan16));
        assert_eq!(
            restored.capacity_by_creation_date[&jan15],
            500_00000000_i128
        );
        assert_eq!(
            restored.capacity_by_creation_date[&jan16],
            300_00000000_i128
        );
    }
}
