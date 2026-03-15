# Bulk Sync Performance Regression Fix — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Recover ~15 min wall clock regression in bulk sync by eliminating three bottlenecks: cohort full-table scan, detector per-TX overhead, and serial tracker traversal.

**Architecture:** Three independent optimizations applied in order of risk (C → B → A). C merges two serial post-scope traversals into one parallel thread. B pre-computes per-TX detector flags during the existing precompute pass. A replaces the day-boundary full CF scan with incremental cohort accumulation inside the tracker.

**Tech Stack:** Rust, RocksDB, std::thread::scope, HashMap/HashSet

**Design doc:** `docs/plans/2026-03-15-bulk-sync-perf-regression-design.md`

---

## Task 1: Merge Trackers into Single Parallel Thread (Optimization C)

**Files:**
- Modify: `crates/indexer/src/sync/batch.rs` (lines 1731-3067 thread::scope, lines 4614-4629 post-scope calls)
- Modify: `crates/indexer/src/sync/reorg.rs` (lines 36-197 update_hodl_wave, update_cell_distribution)
- Modify: `crates/indexer/src/sync/types.rs` (BatchWriteMetrics, lines 140-155)

### Step 1: Add `t_track_ms` to BatchWriteMetrics

In `crates/indexer/src/sync/types.rs`, add the new timing field:

```rust
// In struct BatchWriteMetrics (line 140):
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BatchWriteMetrics {
    pub(crate) commit_ms: f64,
    pub(crate) write_ms: f64,
    pub(crate) txs: u64,
    pub(crate) cells: u64,
    pub(crate) inputs: u64,
    pub(crate) t1_ms: f64,
    pub(crate) t1b_ms: f64,
    pub(crate) t2_ms: f64,
    pub(crate) t4_ms: f64,
    pub(crate) t5_ms: f64,
    pub(crate) t6a_ms: f64,
    pub(crate) t6b_ms: f64,
    pub(crate) t7_ms: f64,
    pub(crate) t_act_ms: f64,
    pub(crate) t_track_ms: f64,  // NEW
}
```

Update the thread_times array from `[f64; 9]` to `[f64; 10]` everywhere it appears in `batch.rs`:
- Line 1734: `[f64; 9]` → `[f64; 10]`
- Line 3067: add `t_track_ms` to the array
- Line 4666: destructure includes `t_track`
- Line 4681: add `t_track_ms` log field
- Line 4687: `[0.0; 9]` → `[0.0; 10]`
- Line 4715: assign `t_track_ms: thread_ms[9]`

### Step 2: Create merged `update_trackers` method

In `crates/indexer/src/sync/reorg.rs`, add a new method that combines `update_hodl_wave` and `update_cell_distribution` into a single cell traversal:

```rust
/// Merged tracker update: single traversal feeds both HODL wave and cell distribution trackers.
/// Designed to run inside thread::scope as T_TRACK, parallel with T1-T_ACT.
pub(crate) fn update_trackers(
    &self,
    all_parsed_blocks: &[crate::parser::block::ParsedBlock],
    all_tx_data: &[TxData],
    input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    address_balance_changes: &HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>,
) -> Result<()> {
    let mut hodl = self.hodl_tracker.lock().unwrap();
    let mut cell_dist = self.cell_dist_tracker.lock().unwrap();
    let store = self.writer.store();

    // Single traversal over all blocks → txs → cells
    let mut block_tx_idx = 0usize;
    for parsed in all_parsed_blocks {
        let block_date = ckbadger_common::block_date(parsed.timestamp);
        hodl.record_block_date(parsed.number, block_date);
        cell_dist.record_block_date(parsed.number, block_date);

        let tx_count = checked_tx_count(parsed.transactions_count, parsed.number)?;
        let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count];
        block_tx_idx += tx_count;

        for tx_data in tx_slice {
            // Cell creates
            for cell in &tx_data.cells {
                hodl.cell_created(block_date, cell.capacity);
                let occ = super::dao_helpers::occupied_capacity_shannons_i64(
                    cell.lock_args.len(),
                    cell.type_args.as_ref().map(|args| args.len()),
                    cell.data_size,
                );
                cell_dist.cell_created(block_date, occ);
            }
            // Cell consumes
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        super::helpers::parsed_input_outpoint_index_i16(
                            input.previous_output_index,
                            "tracker_update",
                        ),
                    );
                    let info = input_cell_info
                        .get(&key)
                        .or_else(|| batch_cell_infos.get(&key));
                    if let Some(info) = info {
                        hodl.cell_consumed(info.created_at_block, info.capacity);
                        cell_dist.cell_consumed(info.created_at_block, info.occupied_capacity)?;
                    }
                }
            }
        }

        // Cell dist day boundary snapshot
        if let Some((snapshot_date, snapshot)) = cell_dist.maybe_snapshot(block_date) {
            let date_str = snapshot_date.format("%Y%m%d").to_string();
            store.put_cell_distribution(&date_str, &snapshot)?;
            let cohort = Self::compute_address_cohort_snapshot(store, &cell_dist, &date_str)?;
            store.put_address_cohort(&date_str, &cohort)?;
        }
    }

    // HODL holder count update (from address_balance_changes)
    let balance_map: HashMap<&Vec<u8>, Option<AddressBalance>> = HashMap::new();
    // NOTE: In the existing code, update_hodl_wave reads from a pre-fetched balance_map.
    // We need to replicate this by passing prefetched_addr_balances to update_trackers.
    // See the actual update_hodl_wave code at reorg.rs:96-115 for the holder count loop.
    // For now, we integrate the same loop here using address_balance_changes + prefetched balances.

    // Persist tracker states
    store.put_hodl_tracker_state(&hodl.to_state())?;
    store.put_cell_dist_tracker_state(&cell_dist.to_state())?;

    Ok(())
}
```

**Important**: The actual implementation must also pass `prefetched_addr_balances` (or a reference to a balance map) for the HODL holder count update. Check `update_hodl_wave` lines 96-115 for the exact loop that calls `tracker.update_holder_count(old_live, post_live)`.

### Step 3: Add T_TRACK thread inside thread::scope

In `batch.rs`, inside the `thread::scope` block (after T_ACT spawn at line 2858), add:

```rust
// T_TRACK: Merged HODL wave + cell distribution tracker update
// CFs: CF_SYNC_META (tracker states), CF_STATS (cell_distribution, address_cohort)
// No CF overlap with T1-T_ACT.
let h_track = s.spawn(|| -> Result<f64> {
    let t = Instant::now();
    self.update_trackers(
        all_parsed_blocks,
        &all_tx_data,
        &input_cell_info,
        &batch_cell_infos,
        &address_balance_changes,
    )?;
    Ok(t.elapsed().as_secs_f64() * 1000.0)
});
```

Join it alongside other threads (after line 2997):

```rust
let t_track_ms = h_track.join().expect("T_TRACK panicked")?;
```

### Step 4: Remove post-scope tracker calls

Delete lines 4614-4629 in `batch.rs`:

```rust
// DELETE these lines:
// HODL wave tracker update
self.update_hodl_wave(...)?;
// Cell distribution tracker update
self.update_cell_distribution(...)?;
```

### Step 5: Update timing log

Update the `info!` log at line 4667 to include `t_track_ms`:

```rust
t_track_ms = format!("{:.1}", t_track),
```

### Step 6: Verify

Run: `cargo check -p ckbadger-indexer`
Run: `cargo test -p ckbadger-indexer`
Run: `cargo clippy -p ckbadger-indexer`

### Step 7: Commit

```bash
git add crates/indexer/src/sync/batch.rs crates/indexer/src/sync/reorg.rs crates/indexer/src/sync/types.rs
git commit -m "perf: merge hodl+cell_dist trackers into parallel T_TRACK thread

Eliminates serial double cell traversal after thread::scope join.
Both trackers now run as T_TRACK inside thread::scope, parallel with
T1-T_ACT. Single traversal feeds both trackers. No CF conflicts."
```

---

## Task 2: Batch-Level Detector Pre-filter + Per-TX Bit Flags (Optimization B)

**Files:**
- Modify: `crates/indexer/src/db/writer/activities.rs` (ProtocolDetector trait, build_tx_activity_bundle)
- Modify: `crates/indexer/src/db/writer/rgbpp_detector.rs`
- Modify: `crates/indexer/src/db/writer/fiber_detector.rs`
- Modify: `crates/indexer/src/db/writer/stablepp_detector.rs`
- Modify: `crates/indexer/src/db/writer/utxoswap_detector.rs`
- Modify: `crates/indexer/src/sync/batch.rs` (precompute pass)

### Step 1: Add `might_apply_batch` to ProtocolDetector trait

In `activities.rs` line 178, add to the trait:

```rust
/// Batch-level pre-filter: returns false if no code_hash in the entire batch
/// matches this detector. Called once per batch (not per TX).
fn might_apply_batch(
    &self,
    lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
    type_code_hashes: &std::collections::HashSet<[u8; 32]>,
) -> bool;
```

### Step 2: Implement `might_apply_batch` for each detector

Each detector checks if its known code_hashes appear in the batch sets.

**RgbppDetector** (`rgbpp_detector.rs`):
```rust
fn might_apply_batch(
    &self,
    lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
    _type_code_hashes: &std::collections::HashSet<[u8; 32]>,
) -> bool {
    // RGB++ only checks lock code_hashes
    lock_code_hashes.iter().any(|h| {
        RgbppParser::detect_lock_type(h, self.is_mainnet) != RgbppLockType::Other
    })
}
```

**FiberDetector** (`fiber_detector.rs`):
```rust
fn might_apply_batch(
    &self,
    lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
    _type_code_hashes: &std::collections::HashSet<[u8; 32]>,
) -> bool {
    lock_code_hashes.iter().any(|h| classify_lock(h) != FiberLockType::Other)
}
```

**StableppDetector** (`stablepp_detector.rs`):
```rust
fn might_apply_batch(
    &self,
    lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
    type_code_hashes: &std::collections::HashSet<[u8; 32]>,
) -> bool {
    lock_code_hashes.iter().any(|h| is_stablepp_script(h))
        || type_code_hashes.iter().any(|h| is_stablepp_script(h))
}
```

**UtxoSwapDetector** (`utxoswap_detector.rs`):
```rust
fn might_apply_batch(
    &self,
    lock_code_hashes: &std::collections::HashSet<[u8; 32]>,
    _type_code_hashes: &std::collections::HashSet<[u8; 32]>,
) -> bool {
    lock_code_hashes.iter().any(|h| is_intent_lock(h))
}
```

### Step 3: Collect unique code_hashes during precompute

In `batch.rs`, during the existing precompute pass (around line 1128-1161 where `cell_index_puts` is built), collect unique code_hashes:

```rust
// Collect unique code_hashes for batch-level detector pre-filtering
let mut batch_lock_code_hashes: HashSet<[u8; 32]> = HashSet::new();
let mut batch_type_code_hashes: HashSet<[u8; 32]> = HashSet::new();

// Populate during the existing cell iteration
for tx_data in &all_tx_data {
    for cell in &tx_data.cells {
        if cell.lock_code_hash.len() == 32 {
            let mut h = [0u8; 32];
            h.copy_from_slice(&cell.lock_code_hash);
            batch_lock_code_hashes.insert(h);
        }
        if let Some(ref tc) = cell.type_code_hash {
            if tc.len() == 32 {
                let mut h = [0u8; 32];
                h.copy_from_slice(tc);
                batch_type_code_hashes.insert(h);
            }
        }
    }
    // Also collect from inputs (consumed cells)
    if !tx_data.is_cellbase {
        for input in &tx_data.inputs {
            let key = (
                input.previous_tx_hash.to_vec(),
                parsed_input_outpoint_index_i16(input.previous_output_index, "detector_prefilter"),
            );
            if let Some(info) = input_cell_info.get(&key).or_else(|| batch_cell_infos.get(&key)) {
                if info.lock_code_hash.len() == 32 {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(&info.lock_code_hash);
                    batch_lock_code_hashes.insert(h);
                }
                if let Some(ref tc) = info.type_code_hash {
                    if tc.len() == 32 {
                        let mut h = [0u8; 32];
                        h.copy_from_slice(tc);
                        batch_type_code_hashes.insert(h);
                    }
                }
            }
        }
    }
}
```

### Step 4: Build per-TX detector flags

After batch-level filtering:

```rust
// Batch-level detector filtering
let batch_active_detectors: Vec<bool> = protocol_detectors
    .iter()
    .map(|d| d.might_apply_batch(&batch_lock_code_hashes, &batch_type_code_hashes))
    .collect();

// Per-TX flags (only for active detectors — skip might_apply for inactive ones)
let tx_detector_flags: Vec<u8> = all_tx_data
    .iter()
    .map(|tx_data| {
        let mut flags = 0u8;
        for (i, detector) in protocol_detectors.iter().enumerate() {
            if batch_active_detectors[i] {
                // Build a minimal TxView just for might_apply — only needs inputs/outputs
                // OR: inline the code_hash check directly here
                // For simplicity, check if any cell in this TX has matching code_hashes
                // This avoids building TxView in precompute
                let has_match = tx_data.cells.iter().any(|cell| {
                    // Check if this cell's code_hashes match this detector
                    // Reuse the same logic as might_apply but on ParsedCell
                    check_detector_match(detector.as_ref(), cell, i)
                }) || (!tx_data.is_cellbase && tx_data.inputs.iter().any(|input| {
                    let key = (input.previous_tx_hash.to_vec(),
                        parsed_input_outpoint_index_i16(input.previous_output_index, "det_flag"));
                    input_cell_info.get(&key)
                        .or_else(|| batch_cell_infos.get(&key))
                        .is_some_and(|info| check_detector_match_info(detector.as_ref(), info, i))
                }));
                if has_match {
                    flags |= 1 << i;
                }
            }
        }
        flags
    })
    .collect();
```

**Note**: The exact implementation of `check_detector_match` depends on how each detector's `might_apply` works. Since all 4 detectors just check `lock_code_hash` and/or `type_code_hash` against known constants, the simplest approach is to:

1. Extract all known code_hashes from each detector into a static method returning `&'static [[u8; 32]]`
2. Check intersection with the TX's cell code_hashes

Alternatively, keep using `might_apply` but pass a pre-built `TxView` — the key optimization is the batch-level skip, not the per-TX flags. If batch-level filtering eliminates the detector entirely for 0-8M blocks, the per-TX flags are moot for those ranges.

**Simpler alternative**: Just use batch-level filtering + existing `might_apply()`. This gets 80% of the benefit with 20% of the code:

```rust
// In batch.rs where protocol_detectors is created (line 1707):
let protocol_detectors: Vec<Box<dyn ProtocolDetector>> = vec![...];

// Filter to only batch-active detectors
let protocol_detectors: Vec<Box<dyn ProtocolDetector>> = protocol_detectors
    .into_iter()
    .enumerate()
    .filter(|(i, d)| d.might_apply_batch(&batch_lock_code_hashes, &batch_type_code_hashes))
    .map(|(_, d)| d)
    .collect();
```

This is the recommended approach — batch-level filtering eliminates all 4 detectors for blocks 0-8M, and for 8M+ the per-TX `might_apply()` cost is acceptable since it's only ~10% of total TX count.

### Step 5: Verify

Run: `cargo check -p ckbadger-indexer`
Run: `cargo test -p ckbadger-indexer`
Run: `cargo clippy -p ckbadger-indexer`

Verify test `test_protocol_detector_might_apply_filters_irrelevant_tx` still passes.

### Step 6: Commit

```bash
git add crates/indexer/src/db/writer/activities.rs crates/indexer/src/db/writer/rgbpp_detector.rs \
    crates/indexer/src/db/writer/fiber_detector.rs crates/indexer/src/db/writer/stablepp_detector.rs \
    crates/indexer/src/db/writer/utxoswap_detector.rs crates/indexer/src/sync/batch.rs
git commit -m "perf: batch-level detector pre-filter skips all detectors for blocks 0-8M

Adds might_apply_batch() to ProtocolDetector trait. Each detector checks
if any of its known code_hashes appear in the batch's unique code_hash
sets. For blocks before protocol deployment (~0-8M), all 4 detectors
are skipped entirely — zero per-TX overhead."
```

---

## Task 3: Incremental Cohort Accumulation (Optimization A)

**Files:**
- Modify: `crates/indexer/src/db/writer/cell_distribution.rs` (add cohort_accum field and methods)
- Modify: `crates/ckbadger-store/src/types.rs` (CellDistributionTrackerState)
- Modify: `crates/indexer/src/sync/reorg.rs` (update_trackers to call cohort update, delete compute_address_cohort_snapshot)
- Modify: `crates/indexer/src/sync/batch.rs` (pass prefetched_addr_balances to T_TRACK)

### Step 1: Add `cohort_accum` to CellDistributionTrackerState

In `crates/ckbadger-store/src/types.rs` line 785:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CellDistributionTrackerState {
    pub capacity_by_date_and_bucket: Vec<(String, [i128; 6])>,
    pub count_by_bucket: [i64; 6],
    pub total_capacity_by_bucket: [i128; 6],
    pub date_transitions: Vec<(i64, String)>,
    pub last_snapshot_date: Option<String>,
    #[serde(default)]  // backward compat with existing persisted state
    pub cohort_accum: Vec<(String, i128, i128)>,  // (YYYY-MM, used_capacity, balance)
}
```

### Step 2: Add cohort_accum field and methods to CellDistributionTracker

In `crates/indexer/src/db/writer/cell_distribution.rs`:

```rust
use std::collections::BTreeMap;

pub struct CellDistributionTracker {
    // ... existing fields ...
    /// Incremental address cohort accumulator: cohort_month → (used_capacity, balance).
    /// Updated per-batch from address_balance_changes, snapshot at day boundaries.
    cohort_accum: BTreeMap<String, (i128, i128)>,
}
```

Add to `new()`: `cohort_accum: BTreeMap::new()`

Add to `from_state()`:
```rust
let cohort_accum: BTreeMap<String, (i128, i128)> = state
    .cohort_accum
    .into_iter()
    .map(|(month, used, bal)| (month, (used, bal)))
    .collect();
```

Add to `to_state()`:
```rust
let cohort_accum = self.cohort_accum
    .iter()
    .map(|(month, (used, bal))| (month.clone(), *used, *bal))
    .collect();
```

Add new method:
```rust
/// Update cohort accumulator from address balance changes in this batch.
///
/// For each changed address:
/// - Determine cohort month from first_seen_block via block_number_to_date()
/// - Apply balance delta and used_capacity delta
///
/// `prefetched_balances` provides existing AddressBalance for addresses that
/// existed before this batch (contains first_seen_block).
/// `changes` provides the deltas: (balance_delta, live_delta, cells_created, tx_delta, last_block, last_tx, used_delta).
/// For new addresses (not in prefetched_balances), first_seen_block = changes.4 (first block in this batch).
pub fn update_cohort_deltas(
    &mut self,
    changes: &HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>,
    prefetched_balances: &HashMap<Vec<u8>, Option<AddressBalance>>,
) {
    for (lock_hash, (balance_delta, _live, _created, _tx, last_block, _tx_hash, used_delta)) in changes {
        // Determine first_seen_block
        let first_seen_block = prefetched_balances
            .get(lock_hash)
            .and_then(|opt| opt.as_ref())
            .map(|bal| bal.first_seen_block)
            .unwrap_or(*last_block);  // new address: first seen in this batch

        let cohort_date = match self.block_number_to_date(first_seen_block) {
            Some(d) => d,
            None => continue,  // before first recorded transition — skip
        };
        let cohort_month = cohort_date.format("%Y-%m").to_string();

        let entry = self.cohort_accum.entry(cohort_month).or_insert((0, 0));
        entry.0 += *used_delta;
        entry.1 += *balance_delta;
    }
}

/// Produce address cohort snapshot from incremental accumulator.
pub fn cohort_snapshot(&self) -> DailyAddressCohort {
    let entries: Vec<AddressCohortEntry> = self.cohort_accum
        .iter()
        .filter(|(_, (used, bal))| *used > 0 || *bal > 0)
        .map(|(month, (used, bal))| AddressCohortEntry {
            cohort_month: month.clone(),
            used_capacity: *used,
            total_balance: *bal,
        })
        .collect();
    DailyAddressCohort { cohorts: entries }
}
```

### Step 3: Update `update_trackers` to use incremental cohort

In `reorg.rs`, modify `update_trackers`:

1. Accept `prefetched_addr_balances` parameter
2. Call `cell_dist.update_cohort_deltas(address_balance_changes, prefetched_addr_balances)` once per batch
3. At day boundary, use `cell_dist.cohort_snapshot()` instead of `compute_address_cohort_snapshot()`

```rust
// In the day boundary section:
if let Some((snapshot_date, snapshot)) = cell_dist.maybe_snapshot(block_date) {
    let date_str = snapshot_date.format("%Y%m%d").to_string();
    store.put_cell_distribution(&date_str, &snapshot)?;
    // Use incremental cohort instead of full CF scan
    let cohort = cell_dist.cohort_snapshot();
    store.put_address_cohort(&date_str, &cohort)?;
}
```

### Step 4: Delete `compute_address_cohort_snapshot`

Remove the method at `reorg.rs:199-258`.

### Step 5: Pass `prefetched_addr_balances` to T_TRACK

In `batch.rs`, update the T_TRACK spawn to pass `&prefetched_addr_balances`:

```rust
let h_track = s.spawn(|| -> Result<f64> {
    let t = Instant::now();
    self.update_trackers(
        all_parsed_blocks,
        &all_tx_data,
        &input_cell_info,
        &batch_cell_infos,
        &address_balance_changes,
        &prefetched_addr_balances,  // NEW
    )?;
    Ok(t.elapsed().as_secs_f64() * 1000.0)
});
```

### Step 6: Add unit tests for incremental cohort

In `cell_distribution.rs` tests module:

```rust
#[test]
fn test_incremental_cohort_new_address() {
    let mut tracker = CellDistributionTracker::new();
    let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    tracker.record_block_date(100, jan15);

    let mut changes = HashMap::new();
    // New address: balance +500 CKB, used +61 CKB, first seen at block 100
    changes.insert(
        vec![0xAA; 32],
        (500_00000000i128, 1, 1, 1i64, 100i64, vec![0x01; 32], 61_00000000i128),
    );

    let prefetched: HashMap<Vec<u8>, Option<AddressBalance>> = HashMap::new();
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

    // Simulate existing address first seen at block 50 (December 2023)
    let mut changes = HashMap::new();
    changes.insert(
        vec![0xBB; 32],
        (100_00000000i128, 1, 1, 1i64, 100i64, vec![0x02; 32], 61_00000000i128),
    );

    let mut prefetched: HashMap<Vec<u8>, Option<AddressBalance>> = HashMap::new();
    prefetched.insert(vec![0xBB; 32], Some(AddressBalance {
        first_seen_block: 50,
        ..Default::default()
    }));

    tracker.update_cohort_deltas(&changes, &prefetched);

    let snapshot = tracker.cohort_snapshot();
    assert_eq!(snapshot.cohorts.len(), 1);
    assert_eq!(snapshot.cohorts[0].cohort_month, "2023-12");  // Uses first_seen from Dec
}

#[test]
fn test_cohort_state_roundtrip() {
    let mut tracker = CellDistributionTracker::new();
    let jan15 = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
    tracker.record_block_date(100, jan15);

    let mut changes = HashMap::new();
    changes.insert(
        vec![0xCC; 32],
        (1000i128, 1, 1, 1i64, 100i64, vec![0x03; 32], 500i128),
    );
    tracker.update_cohort_deltas(&changes, &HashMap::new());

    let state = tracker.to_state();
    assert_eq!(state.cohort_accum.len(), 1);

    let restored = CellDistributionTracker::from_state(state).unwrap();
    let snapshot = restored.cohort_snapshot();
    assert_eq!(snapshot.cohorts.len(), 1);
    assert_eq!(snapshot.cohorts[0].total_balance, 1000);
}
```

### Step 7: Verify

Run: `cargo check`
Run: `cargo test -p ckbadger-indexer`
Run: `cargo test -p ckbadger-store`
Run: `cargo clippy`

### Step 8: Commit

```bash
git add crates/indexer/src/db/writer/cell_distribution.rs crates/ckbadger-store/src/types.rs \
    crates/indexer/src/sync/reorg.rs crates/indexer/src/sync/batch.rs
git commit -m "perf: incremental cohort accumulation eliminates full CF scan

Replaces compute_address_cohort_snapshot() (full CF_ADDR_BALANCE scan at
each day boundary) with incremental delta accumulation from
address_balance_changes. Cohort accum is maintained in
CellDistributionTracker and persisted in tracker state."
```

---

## Task 4: Final Verification

### Step 1: Run full test suite

```bash
cargo check && cargo clippy && cargo test
cd frontend && pnpm type-check && pnpm lint && pnpm test
```

### Step 2: Verify data integrity

After a re-sync:

```bash
ckbadger verify --depth fast
```

### Step 3: Commit plan doc

```bash
git add -f docs/plans/2026-03-15-bulk-sync-perf-regression.md
git commit -m "docs: add bulk sync perf regression implementation plan"
```
