# Materialize Chart Data in Indexer — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move expensive chart computations (cell age distribution, cell size distribution, address cohort retention) from inline API CF scans to indexer-materialized daily snapshots. Eliminates DoS-capable full CF scans and multi-minute API request latency.

**Architecture:** Add a `CellDistributionTracker` to the indexer that maintains in-memory cell distribution state (capacity by creation date, capacity by size bucket) incrementally during block processing. At day boundaries, it writes daily snapshots to a new `CF_STATS_CELL_DIST` column family. The API reads snapshots directly instead of scanning. Address cohort data is materialized as a daily snapshot from the existing `AddressBalance` data during the indexer's daily stats pass. The HODL wave tracker's `date_transitions` field already provides block-to-date mapping, so no additional scan is needed.

**Tech Stack:** Rust, RocksDB, existing HodlWaveTracker pattern

**Requires re-sync from genesis after implementation.**

---

### Task 1: Add Store Types for Cell Distribution and Address Cohort Snapshots

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs`
- Modify: `crates/ckbadger-store/src/keys.rs`

**Step 1: Add `DailyCellDistribution` type**

Add after the `DailyHodlWave` struct:

```rust
/// Daily snapshot of live cell distribution by age and size.
/// Materialized by the indexer at each day boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyCellDistribution {
    /// Capacity by age band (shannons): <1d, 1-7d, 7-30d, 30-180d, >180d
    pub age_band_lt1d: i128,
    pub age_band_1d_7d: i128,
    pub age_band_7d_30d: i128,
    pub age_band_30d_180d: i128,
    pub age_band_gt180d: i128,

    /// Cell count and capacity by size bucket: <100, 100-1k, 1k-10k, 10k-100k, 100k-1m, >=1m CKB
    pub size_bucket_counts: [i64; 6],
    pub size_bucket_capacities: [i128; 6],
}
```

**Step 2: Add `DailyAddressCohort` type**

```rust
/// Daily snapshot of address cohort retention data.
/// Each entry: (cohort_month, used_capacity, total_balance)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyAddressCohort {
    pub cohorts: Vec<AddressCohortEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressCohortEntry {
    pub cohort_month: String,       // "YYYY-MM"
    pub used_capacity: i128,        // total occupied capacity
    pub total_balance: i128,        // total balance
}
```

**Step 3: Add `CellDistributionTrackerState` for persistence**

```rust
/// Serializable state for the cell distribution tracker.
/// Persisted to sync_meta for crash recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellDistributionTrackerState {
    /// Capacity per creation-date bucket per size-bucket
    pub capacity_by_date_and_bucket: Vec<(String, [i128; 6])>,
    /// Cell count per size bucket
    pub count_by_bucket: [i64; 6],
    /// Total capacity per size bucket
    pub total_capacity_by_bucket: [i128; 6],
    /// Last snapshot date
    pub last_snapshot_date: Option<String>,
}
```

**Step 4: Add key prefix constant**

In `crates/ckbadger-store/src/keys.rs`, add:

```rust
pub const STATS_PREFIX_CELL_DIST: u8 = 0x0D;
pub const STATS_PREFIX_ADDR_COHORT: u8 = 0x0E;
```

Check what the last used prefix byte is and use the next available. The HODL wave uses `0x0B`, daily stats use `0x01`. Check all existing prefixes before choosing.

**Step 5: Run `cargo check -p ckbadger-store`**

Expected: PASS

**Step 6: Commit**

```
feat(store): add types for materialized cell distribution and address cohort snapshots
```

---

### Task 2: Add Store Operations for Reading/Writing Snapshots

**Files:**

- Modify: `crates/ckbadger-store/src/stats_ops.rs`

**Step 1: Add write operations**

Follow the `put_hodl_wave` / `get_hodl_wave` pattern:

```rust
/// Write a daily cell distribution snapshot.
pub fn put_cell_distribution(&self, date: NaiveDate, snapshot: &DailyCellDistribution, batch: &mut StoreBatch) -> Result<()> {
    let key = self.stats_date_key(STATS_PREFIX_CELL_DIST, date);
    let value = bincode::serialize(snapshot)?;
    batch.put_stats(&key, &value);
    Ok(())
}

/// Read a daily cell distribution snapshot.
pub fn get_cell_distribution(&self, date: NaiveDate) -> Result<Option<DailyCellDistribution>> {
    let key = self.stats_date_key(STATS_PREFIX_CELL_DIST, date);
    match self.get_cf(self.cf_stats_chain(), &key)? {
        Some(value) => Ok(Some(bincode::deserialize(&value)?)),
        None => Ok(None),
    }
}

/// Read the latest cell distribution snapshot (scan backwards from today).
pub fn get_latest_cell_distribution(&self) -> Result<Option<(NaiveDate, DailyCellDistribution)>> {
    // Reverse iterate cf_stats_chain with prefix STATS_PREFIX_CELL_DIST
    // Return the first (most recent) entry
    let prefix = [STATS_PREFIX_CELL_DIST];
    let iter = self.reverse_prefix_iterator(self.cf_stats_chain(), &prefix);
    for item in iter {
        let (key, value) = item?;
        if key.len() != 5 || key[0] != STATS_PREFIX_CELL_DIST { break; }
        let date = self.decode_stats_date_key(&key)?;
        let snapshot: DailyCellDistribution = bincode::deserialize(&value)?;
        return Ok(Some((date, snapshot)));
    }
    Ok(None)
}

/// Write a daily address cohort snapshot.
pub fn put_address_cohort(&self, date: NaiveDate, snapshot: &DailyAddressCohort, batch: &mut StoreBatch) -> Result<()> {
    let key = self.stats_date_key(STATS_PREFIX_ADDR_COHORT, date);
    let value = bincode::serialize(snapshot)?;
    batch.put_stats(&key, &value);
    Ok(())
}

/// Read a daily address cohort snapshot.
pub fn get_address_cohort(&self, date: NaiveDate) -> Result<Option<DailyAddressCohort>> {
    let key = self.stats_date_key(STATS_PREFIX_ADDR_COHORT, date);
    match self.get_cf(self.cf_stats_chain(), &key)? {
        Some(value) => Ok(Some(bincode::deserialize(&value)?)),
        None => Ok(None),
    }
}

/// Read the latest address cohort snapshot.
pub fn get_latest_address_cohort(&self) -> Result<Option<(NaiveDate, DailyAddressCohort)>> {
    let prefix = [STATS_PREFIX_ADDR_COHORT];
    let iter = self.reverse_prefix_iterator(self.cf_stats_chain(), &prefix);
    for item in iter {
        let (key, value) = item?;
        if key.len() != 5 || key[0] != STATS_PREFIX_ADDR_COHORT { break; }
        let date = self.decode_stats_date_key(&key)?;
        let snapshot: DailyAddressCohort = bincode::deserialize(&value)?;
        return Ok(Some((date, snapshot)));
    }
    Ok(None)
}
```

**Step 2: Add tracker state persistence**

```rust
const CELL_DIST_TRACKER_KEY: &[u8] = b"cell_dist_tracker";

pub fn put_cell_dist_tracker_state(&self, state: &CellDistributionTrackerState) -> Result<()> {
    let value = bincode::serialize(state)?;
    self.put_cf(self.cf_sync_meta(), CELL_DIST_TRACKER_KEY, &value);
    Ok(())
}

pub fn get_cell_dist_tracker_state(&self) -> Result<Option<CellDistributionTrackerState>> {
    match self.get_cf(self.cf_sync_meta(), CELL_DIST_TRACKER_KEY)? {
        Some(value) => Ok(Some(bincode::deserialize(&value)?)),
        None => Ok(None),
    }
}
```

Note: Check the exact method signatures and patterns used by the existing `put_hodl_wave` / `get_hodl_wave` / `put_hodl_tracker_state` / `get_hodl_tracker_state` functions and match them exactly. The above is a template — adapt to the actual codebase patterns (e.g., does `put_cf` use `&self` or `&mut self`? does `batch.put_stats` take `&key` or `key`?).

**Step 3: Add unit tests**

```rust
#[test]
fn test_cell_distribution_roundtrip() {
    let store = open_test_unified();
    let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    let snapshot = DailyCellDistribution {
        age_band_lt1d: 100,
        age_band_1d_7d: 200,
        age_band_7d_30d: 300,
        age_band_30d_180d: 400,
        age_band_gt180d: 500,
        size_bucket_counts: [10, 20, 30, 40, 50, 60],
        size_bucket_capacities: [100, 200, 300, 400, 500, 600],
    };
    let mut batch = store.new_batch();
    store.put_cell_distribution(date, &snapshot, &mut batch).unwrap();
    batch.commit().unwrap();
    let read = store.get_cell_distribution(date).unwrap().unwrap();
    assert_eq!(read.age_band_lt1d, 100);
    assert_eq!(read.size_bucket_counts[5], 60);
}
```

**Step 4: Run tests**

Run: `cargo test -p ckbadger-store test_cell_distribution`
Expected: PASS

**Step 5: Commit**

```
feat(store): add read/write ops for cell distribution and address cohort snapshots
```

---

### Task 3: Create Cell Distribution Tracker

**Files:**

- Create: `crates/indexer/src/db/writer/cell_distribution.rs`
- Modify: `crates/indexer/src/db/writer/mod.rs` (add module)

**Step 1: Implement `CellDistributionTracker`**

Follow the `HodlWaveTracker` pattern from `hodl_wave.rs`:

```rust
use chrono::NaiveDate;
use std::collections::HashMap;
use ckbadger_store::types::{CellDistributionTrackerState, DailyCellDistribution};
use anyhow::Result;

/// Tracks live cell distribution by creation date and size bucket.
/// Maintains incremental state during block processing.
/// Writes daily snapshots at day boundaries.
pub struct CellDistributionTracker {
    /// Capacity per creation-date per size-bucket
    capacity_by_date_and_bucket: HashMap<NaiveDate, [i128; 6]>,
    /// Cell count per size bucket
    count_by_bucket: [i64; 6],
    /// Total capacity per size bucket
    total_capacity_by_bucket: [i128; 6],
    /// Last snapshot date
    last_snapshot_date: Option<NaiveDate>,
}

impl CellDistributionTracker {
    pub fn new() -> Self {
        Self {
            capacity_by_date_and_bucket: HashMap::new(),
            count_by_bucket: [0; 6],
            total_capacity_by_bucket: [0; 6],
            last_snapshot_date: None,
        }
    }

    pub fn from_state(state: CellDistributionTrackerState) -> Self {
        let mut capacity_by_date_and_bucket = HashMap::new();
        for (date_str, buckets) in &state.capacity_by_date_and_bucket {
            if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                capacity_by_date_and_bucket.insert(date, *buckets);
            }
        }
        Self {
            capacity_by_date_and_bucket,
            count_by_bucket: state.count_by_bucket,
            total_capacity_by_bucket: state.total_capacity_by_bucket,
            last_snapshot_date: state.last_snapshot_date
                .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        }
    }

    pub fn to_state(&self) -> CellDistributionTrackerState {
        let mut entries: Vec<(String, [i128; 6])> = self.capacity_by_date_and_bucket
            .iter()
            .map(|(date, buckets)| (date.format("%Y-%m-%d").to_string(), *buckets))
            .collect();
        entries.sort_by_key(|(d, _)| d.clone());

        CellDistributionTrackerState {
            capacity_by_date_and_bucket: entries,
            count_by_bucket: self.count_by_bucket,
            total_capacity_by_bucket: self.total_capacity_by_bucket,
            last_snapshot_date: self.last_snapshot_date
                .map(|d| d.format("%Y-%m-%d").to_string()),
        }
    }

    /// Record a cell creation. Called during block processing.
    pub fn cell_created(&mut self, block_date: NaiveDate, occupied_capacity: i64) {
        let bucket = Self::size_bucket(occupied_capacity);
        let entry = self.capacity_by_date_and_bucket.entry(block_date).or_insert([0; 6]);
        entry[bucket] += occupied_capacity as i128;
        self.count_by_bucket[bucket] += 1;
        self.total_capacity_by_bucket[bucket] += occupied_capacity as i128;
    }

    /// Record a cell consumption. Called during block processing.
    pub fn cell_consumed(&mut self, created_at_date: NaiveDate, occupied_capacity: i64) {
        let bucket = Self::size_bucket(occupied_capacity);
        if let Some(entry) = self.capacity_by_date_and_bucket.get_mut(&created_at_date) {
            entry[bucket] -= occupied_capacity as i128;
        }
        self.count_by_bucket[bucket] -= 1;
        self.total_capacity_by_bucket[bucket] -= occupied_capacity as i128;
    }

    /// Check if a day boundary was crossed and return snapshot if so.
    pub fn maybe_snapshot(
        &mut self,
        current_date: NaiveDate,
        date_transitions: &[(i64, NaiveDate)],
    ) -> Option<(NaiveDate, DailyCellDistribution)> {
        if Some(current_date) == self.last_snapshot_date {
            return None;
        }
        if self.last_snapshot_date.is_some() || !self.capacity_by_date_and_bucket.is_empty() {
            let snapshot = self.compute_snapshot(current_date, date_transitions);
            self.last_snapshot_date = Some(current_date);
            return Some((current_date, snapshot));
        }
        None
    }

    fn compute_snapshot(
        &self,
        snapshot_date: NaiveDate,
        _date_transitions: &[(i64, NaiveDate)],
    ) -> DailyCellDistribution {
        let mut age_bands = [0i128; 5]; // lt1d, 1-7d, 7-30d, 30-180d, gt180d

        for (creation_date, bucket_capacities) in &self.capacity_by_date_and_bucket {
            let age_days = (snapshot_date - *creation_date).num_days();
            let age_band = match age_days {
                d if d < 1 => 0,
                d if d < 7 => 1,
                d if d < 30 => 2,
                d if d < 180 => 3,
                _ => 4,
            };
            let total_capacity: i128 = bucket_capacities.iter().sum();
            age_bands[age_band] += total_capacity;
        }

        DailyCellDistribution {
            age_band_lt1d: age_bands[0],
            age_band_1d_7d: age_bands[1],
            age_band_7d_30d: age_bands[2],
            age_band_30d_180d: age_bands[3],
            age_band_gt180d: age_bands[4],
            size_bucket_counts: self.count_by_bucket,
            size_bucket_capacities: self.total_capacity_by_bucket,
        }
    }

    /// Map occupied capacity (shannons) to size bucket index.
    /// Matches the API's `occupied_capacity_bucket_index` in statistics.rs.
    fn size_bucket(occupied_shannons: i64) -> usize {
        let ckb = occupied_shannons as i128 / 100_000_000;
        match ckb {
            c if c < 100 => 0,
            c if c < 1_000 => 1,
            c if c < 10_000 => 2,
            c if c < 100_000 => 3,
            c if c < 1_000_000 => 4,
            _ => 5,
        }
    }
}
```

**Step 2: Add module to writer/mod.rs**

```rust
pub(crate) mod cell_distribution;
```

**Step 3: Add unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_bucket_boundaries() {
        assert_eq!(CellDistributionTracker::size_bucket(99_00000000), 0);
        assert_eq!(CellDistributionTracker::size_bucket(100_00000000), 1);
        assert_eq!(CellDistributionTracker::size_bucket(999_00000000), 1);
        assert_eq!(CellDistributionTracker::size_bucket(1000_00000000), 2);
    }

    #[test]
    fn test_cell_created_consumed() {
        let mut tracker = CellDistributionTracker::new();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        tracker.cell_created(date, 200_00000000); // 200 CKB → bucket 1
        assert_eq!(tracker.count_by_bucket[1], 1);
        assert_eq!(tracker.total_capacity_by_bucket[1], 200_00000000);

        tracker.cell_consumed(date, 200_00000000);
        assert_eq!(tracker.count_by_bucket[1], 0);
        assert_eq!(tracker.total_capacity_by_bucket[1], 0);
    }

    #[test]
    fn test_snapshot_age_bands() {
        let mut tracker = CellDistributionTracker::new();
        let old_date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let recent_date = NaiveDate::from_ymd_opt(2024, 6, 28).unwrap();
        let today = NaiveDate::from_ymd_opt(2024, 7, 1).unwrap();

        tracker.cell_created(old_date, 100_00000000);   // >180 days old
        tracker.cell_created(recent_date, 50_00000000);  // 3 days old → 1-7d band

        let snapshot = tracker.compute_snapshot(today, &[]);
        assert!(snapshot.age_band_gt180d > 0);
        assert!(snapshot.age_band_1d_7d > 0);
        assert_eq!(snapshot.age_band_lt1d, 0);
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p ckbadger-indexer cell_distribution`
Expected: PASS

**Step 5: Commit**

```
feat(indexer): add CellDistributionTracker for incremental cell distribution tracking
```

---

### Task 4: Integrate Tracker into Batch Pipeline

**Files:**

- Modify: `crates/indexer/src/sync/indexer.rs` (tracker initialization)
- Modify: `crates/indexer/src/sync/batch.rs` (batch processing + snapshot writing)
- Modify: `crates/indexer/src/sync/reorg.rs` (tracker update, alongside HODL wave)

**Step 1: Add tracker to the indexer state**

In `indexer.rs`, find where `hodl_tracker` is initialized (likely in the `Indexer` struct or `SyncState`). Add a parallel `cell_dist_tracker` field:

```rust
cell_dist_tracker: Mutex<CellDistributionTracker>,
```

Initialize from persisted state on startup, same pattern as HODL tracker:

```rust
let cell_dist_tracker = match store.get_cell_dist_tracker_state()? {
    Some(state) => CellDistributionTracker::from_state(state),
    None => CellDistributionTracker::new(),
};
```

**Step 2: Update the tracker during batch processing**

In `batch.rs`, find `update_hodl_wave` (called at ~line 4441). Add a parallel `update_cell_distribution` call:

```rust
// After update_hodl_wave:
self.update_cell_distribution(
    all_parsed_blocks,
    &all_tx_data,
    &input_cell_info,
    &batch_cell_infos,
)?;
```

**Step 3: Implement `update_cell_distribution`**

In `reorg.rs` (where `update_hodl_wave` lives), add:

```rust
pub(crate) fn update_cell_distribution(
    &self,
    all_parsed_blocks: &[ParsedBlock],
    all_tx_data: &[TxData],
    input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
) -> Result<()> {
    let mut tracker = self.cell_dist_tracker.lock().unwrap();
    let date_transitions = /* get from hodl_tracker or store */;

    for (block_idx, parsed_block) in all_parsed_blocks.iter().enumerate() {
        let block_date = /* derive from timestamp, same as HODL wave */;

        // Process outputs (cell creations)
        for tx_data in &all_tx_data[/* block's tx range */] {
            for cell in &tx_data.cells {
                tracker.cell_created(block_date, cell.occupied_capacity);
            }
        }

        // Process inputs (cell consumptions)
        for tx_data in &all_tx_data[/* block's tx range */] {
            for input in &tx_data.inputs {
                let key = (input.previous_output_tx_hash.clone(), input.previous_output_index);
                if let Some(cell_info) = input_cell_info.get(&key)
                    .or_else(|| batch_cell_infos.get(&key))
                {
                    let created_at_date = /* find date for cell_info.created_at_block using date_transitions */;
                    tracker.cell_consumed(created_at_date, cell_info.occupied_capacity);
                }
            }
        }

        // Check for day boundary → write snapshot
        if let Some((date, snapshot)) = tracker.maybe_snapshot(block_date, &date_transitions) {
            self.store.put_cell_distribution(date, &snapshot, /* batch or direct write */)?;
        }
    }

    // Persist tracker state
    self.store.put_cell_dist_tracker_state(&tracker.to_state())?;
    Ok(())
}
```

Note: Study `update_hodl_wave` carefully for the exact patterns it uses for:

- Iterating blocks and transactions
- Finding block dates from timestamps
- Looking up `created_at_block` for consumed cells
- Converting block numbers to dates via `date_transitions`
- Writing snapshots and tracker state

Mirror the same patterns exactly.

**Step 4: Run `cargo check`**

Expected: PASS

**Step 5: Commit**

```
feat(indexer): integrate cell distribution tracker into batch pipeline
```

---

### Task 5: Materialize Address Cohort Data in Indexer

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs`
- Modify: `crates/indexer/src/sync/batch.rs`

**Step 1: Add address cohort snapshot to daily stats pass**

The address cohort data needs a full `cf_addr_balance` scan, which is expensive. Two options:

**Option A (simpler):** Compute during the daily stats flush when we already have `address_balance_changes`. The changes include per-address `(balance_delta, cells_created, cells_consumed, used_cap_created, used_cap_consumed, lock_hash, first_seen)`. This is NOT sufficient because we need ALL addresses, not just changed ones.

**Option B (piggyback on HODL wave reconciliation):** The HODL tracker already maintains cumulative state. We can compute address cohort data as a secondary output of the same incremental tracking. However, address cohort needs `first_tx_block` (from `AddressBalance`) which is not tracked incrementally.

**Option C (daily batch job):** At each day boundary, do a full `cf_addr_balance` scan in the indexer (not the API). This is acceptable because:

- It runs in the indexer (which owns writes)
- It runs at most once per day
- The indexer is already I/O-heavy during bulk sync
- It's the same work currently done in the API warmup

Implement Option C: In `write_batch_stats_to_batch` (batch.rs ~line 4538), after writing daily stats for each date, trigger an address cohort snapshot computation:

```rust
// After daily stats are written for a date:
if is_new_day {
    let cohort = compute_address_cohort_snapshot(&self.store, date, &date_transitions)?;
    self.store.put_address_cohort(date, &cohort, batch)?;
}
```

```rust
fn compute_address_cohort_snapshot(
    store: &CkbadgerStore,
    snapshot_date: NaiveDate,
    date_transitions: &[(i64, NaiveDate)],
) -> Result<DailyAddressCohort> {
    let mut cohorts: HashMap<String, (i128, i128)> = HashMap::new();

    // Full scan of cf_addr_balance (same as API warmup does)
    let iter = store.prefix_iterator(store.cf_addr_balance(), &[]);
    for item in iter {
        let (_, value) = item?;
        let balance: AddressBalance = bincode::deserialize(&value)?;
        if balance.balance <= 0 { continue; }

        // Find cohort month from first_tx_block
        let first_date = block_number_to_date(balance.first_tx_block, date_transitions);
        let cohort_month = first_date.format("%Y-%m").to_string();

        let entry = cohorts.entry(cohort_month).or_insert((0, 0));
        entry.0 += balance.used_capacity;
        entry.1 += balance.balance;
    }

    let mut entries: Vec<AddressCohortEntry> = cohorts.into_iter()
        .map(|(month, (used, total))| AddressCohortEntry {
            cohort_month: month,
            used_capacity: used,
            total_balance: total,
        })
        .collect();
    entries.sort_by(|a, b| a.cohort_month.cmp(&b.cohort_month));

    Ok(DailyAddressCohort { cohorts: entries })
}
```

**Step 2: Run `cargo check`**

Expected: PASS

**Step 3: Commit**

```
feat(indexer): materialize address cohort snapshot at day boundaries
```

---

### Task 6: Update API to Read Materialized Data

**Files:**

- Modify: `crates/api/src/routes/statistics.rs`

**Step 1: Replace `get_cell_age_vs_occupied_capacity_chart` inline scan**

Replace the inline `visit_live_cells_in_batches` call with a read from the materialized snapshot:

```rust
async fn get_cell_age_vs_occupied_capacity_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<StackedAreaChartResponse> {
    // Check cache first (existing pattern)
    if let Some(cached) = state.cache.get(&CacheKeys::chart("cell-age-vs-occupied-capacity:v1")).await {
        return Ok(ApiResponse::ok(cached));
    }

    // Read latest materialized snapshot from store
    let snapshot = state.store.get_latest_cell_distribution()
        .map_err(|e| ApiError::internal(format!("failed to read cell distribution: {e}")))?
        .map(|(_, s)| s)
        .ok_or_else(|| ApiError::service_unavailable("cell distribution data not yet available"))?;

    // Build response from snapshot (same format as before)
    let response = build_cell_age_response(&snapshot);

    // Cache it
    state.cache.set(&CacheKeys::chart("cell-age-vs-occupied-capacity:v1"), &response, CacheTtl::CHART).await;

    Ok(ApiResponse::ok(response))
}
```

Check if `ApiError::service_unavailable` exists. If not, add it to `response.rs` or use `ApiError::internal` with a 503-appropriate message. The key point is NOT falling back to inline scanning.

**Step 2: Replace `get_cell_size_distribution_chart` inline scan**

Same pattern — read from `get_latest_cell_distribution`:

```rust
async fn get_cell_size_distribution_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    if let Some(cached) = state.cache.get(&CacheKeys::chart("cell-size-distribution:v1")).await {
        return Ok(ApiResponse::ok(cached));
    }

    let snapshot = state.store.get_latest_cell_distribution()
        .map_err(|e| ApiError::internal(format!("failed to read cell distribution: {e}")))?
        .map(|(_, s)| s)
        .ok_or_else(|| ApiError::service_unavailable("cell distribution data not yet available"))?;

    let response = build_cell_size_response(&snapshot);
    state.cache.set(&CacheKeys::chart("cell-size-distribution:v1"), &response, CacheTtl::CHART).await;
    Ok(ApiResponse::ok(response))
}
```

**Step 3: Replace `get_address_cohort_retention_chart` inline scan**

```rust
async fn get_address_cohort_retention_chart(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ChartResponse> {
    if let Some(cached) = state.cache.get(&CacheKeys::chart("address-cohort-retention:v1")).await {
        return Ok(ApiResponse::ok(cached));
    }

    let snapshot = state.store.get_latest_address_cohort()
        .map_err(|e| ApiError::internal(format!("failed to read address cohort: {e}")))?
        .map(|(_, s)| s)
        .ok_or_else(|| ApiError::service_unavailable("address cohort data not yet available"))?;

    let response = build_address_cohort_response(&snapshot);
    state.cache.set(&CacheKeys::chart("address-cohort-retention:v1"), &response, CacheTtl::CHART).await;
    Ok(ApiResponse::ok(response))
}
```

**Step 4: Add response builder helper functions**

Extract the response construction logic from the old handlers into helper functions:

```rust
fn build_cell_age_response(snapshot: &DailyCellDistribution) -> StackedAreaChartResponse {
    // Convert age_band_* fields to the StackedAreaChartResponse format
    // Match the exact format the frontend expects
}

fn build_cell_size_response(snapshot: &DailyCellDistribution) -> ChartResponse {
    // Convert size_bucket_* fields to ChartResponse format
}

fn build_address_cohort_response(cohort: &DailyAddressCohort) -> ChartResponse {
    // Convert cohort entries to ChartResponse format
}
```

Study the existing handler code to extract the exact response construction logic.

**Step 5: Run `cargo check`**

Expected: PASS

**Step 6: Commit**

```
feat(api): read chart data from materialized store snapshots instead of inline CF scans
```

---

### Task 7: Remove Inline CF Scan Functions and Update Warmup

**Files:**

- Modify: `crates/api/src/routes/statistics.rs` (remove unused scan functions)
- Modify: `crates/api/src/warmup.rs` (simplify warmup)
- Modify: `crates/api/src/lib.rs` (remove background chart warmup)

**Step 1: Remove `visit_live_cells_in_batches`**

This function should now be unused. Delete it. Also delete `load_block_date_transitions` and `load_block_date_transitions_cached` if they're no longer called from any handler.

Check for any remaining callers before deleting:

```bash
cargo grep 'visit_live_cells_in_batches\|load_block_date_transitions'
```

**Step 2: Simplify `warmup_chart_caches`**

Remove the `compute_live_cell_charts` call since the data is now materialized by the indexer. The warmup can either:

- Pre-populate the API cache from the store snapshots (fast read, no scan)
- Be removed entirely if the handlers already do cache-on-first-read

Recommended: Keep warmup but make it a fast snapshot read:

```rust
pub async fn warmup_chart_caches(state: Arc<AppState>) {
    // Read materialized snapshots and populate cache
    if let Ok(Some((_, snapshot))) = state.store.get_latest_cell_distribution() {
        let age_response = build_cell_age_response(&snapshot);
        state.cache.set(&CacheKeys::chart("cell-age-vs-occupied-capacity:v1"), &age_response, CacheTtl::CHART).await;
        let size_response = build_cell_size_response(&snapshot);
        state.cache.set(&CacheKeys::chart("cell-size-distribution:v1"), &size_response, CacheTtl::CHART).await;
    }
    if let Ok(Some((_, cohort))) = state.store.get_latest_address_cohort() {
        let response = build_address_cohort_response(&cohort);
        state.cache.set(&CacheKeys::chart("address-cohort-retention:v1"), &response, CacheTtl::CHART).await;
    }
}
```

**Step 3: Remove `refresh_address_cache_sync` cohort piggybacking**

In `warmup.rs`, `refresh_address_cache_sync` currently piggybacks address cohort computation on its full `cf_addr_balance` scan. Remove the cohort-related code since it's now materialized by the indexer. Keep the rest of the function (top addresses, active addresses).

**Step 4: Run `cargo check && cargo clippy`**

Expected: PASS, with possible warnings about unused imports/functions to clean up.

**Step 5: Commit**

```
refactor(api): remove inline CF scan functions, simplify warmup to read materialized data
```

---

### Task 8: Add Integration Tests

**Files:**

- Add tests in store and/or indexer crates

**Step 1: Test cell distribution snapshot read/write roundtrip**

Already covered in Task 2.

**Step 2: Test address cohort snapshot read/write roundtrip**

```rust
#[test]
fn test_address_cohort_roundtrip() {
    let store = open_test_unified();
    let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
    let cohort = DailyAddressCohort {
        cohorts: vec![
            AddressCohortEntry { cohort_month: "2024-01".into(), used_capacity: 1000, total_balance: 5000 },
            AddressCohortEntry { cohort_month: "2024-06".into(), used_capacity: 500, total_balance: 2000 },
        ],
    };
    let mut batch = store.new_batch();
    store.put_address_cohort(date, &cohort, &mut batch).unwrap();
    batch.commit().unwrap();
    let (read_date, read) = store.get_latest_address_cohort().unwrap().unwrap();
    assert_eq!(read_date, date);
    assert_eq!(read.cohorts.len(), 2);
    assert_eq!(read.cohorts[0].cohort_month, "2024-01");
}
```

**Step 3: Test tracker state persistence roundtrip**

```rust
#[test]
fn test_cell_dist_tracker_state_roundtrip() {
    let mut tracker = CellDistributionTracker::new();
    let date = NaiveDate::from_ymd_opt(2024, 3, 15).unwrap();
    tracker.cell_created(date, 150_00000000); // 150 CKB → bucket 1

    let state = tracker.to_state();
    let restored = CellDistributionTracker::from_state(state);
    assert_eq!(restored.count_by_bucket[1], 1);
    assert_eq!(restored.total_capacity_by_bucket[1], 150_00000000);
}
```

**Step 4: Run all tests**

```bash
cargo test --lib
```

Expected: PASS

**Step 5: Commit**

```
test: add integration tests for materialized chart data
```

---

### Task 9: Final Validation and Documentation

**Step 1: Run full workspace checks**

```bash
cargo check && cargo clippy && cargo test --lib
```

**Step 2: Update STORE_SCHEMA.md**

Add documentation for the new key prefixes in `CF_STATS_CHAIN`:

- `0x0D` — `STATS_PREFIX_CELL_DIST` — Daily cell distribution snapshots
- `0x0E` — `STATS_PREFIX_ADDR_COHORT` — Daily address cohort retention snapshots

**Step 3: Update CLAUDE.md if needed**

Remove any mention of "full CF scan fallback" as a known issue.

**Step 4: Final commit**

```
docs: update store schema and docs for materialized chart data
```

---

## Summary

| Task | Description                           | Risk                                              |
| ---- | ------------------------------------- | ------------------------------------------------- |
| 1    | Store types for snapshots             | Low — additive                                    |
| 2    | Store read/write ops                  | Low — follows existing pattern                    |
| 3    | CellDistributionTracker               | Medium — new incremental tracker                  |
| 4    | Integrate into batch pipeline         | Medium — mirrors HODL wave pattern                |
| 5    | Address cohort materialization        | Medium — full scan but in indexer at day boundary |
| 6    | API reads from materialized data      | Medium — replaces inline scan with store read     |
| 7    | Remove inline scans + simplify warmup | Low — deletion + simplification                   |
| 8    | Integration tests                     | Low — test-only                                   |
| 9    | Final validation + docs               | Low — verification                                |

**After implementation:** Delete RocksDB and re-sync from genesis.
