# Design: Bulk Sync Performance Regression Fix

**Date**: 2026-03-15
**Problem**: 15-minute wall clock regression in bulk sync between commits c69474df and c3fc2e4d

## Root Cause Analysis

Three bottlenecks introduced across the commit range:

| # | Bottleneck | Location | Estimated Cost |
|---|-----------|----------|---------------|
| A | `compute_address_cohort_snapshot()` full CF scan at day boundaries | `reorg.rs:204-258` | ~5-7 min |
| B | Protocol detector `might_apply()` per-TX overhead × 4 detectors | `activities.rs:420-424` | ~3-5 min |
| C | `update_hodl_wave` + `update_cell_distribution` serial double traversal | `batch.rs:4614-4629` | ~2-3 min |

### A: Address Cohort Full Table Scan

At each day boundary (~2000 times during full sync), `compute_address_cohort_snapshot()` iterates **all** addresses in `CF_ADDR_BALANCE` via `IteratorMode::Start`, deserializes each `AddressBalance`, and does a binary search for cohort month. Address count grows from 0 to millions as sync progresses, making late-sync snapshots increasingly expensive. This cost is hidden inside `finalize_ms` with no separate timing.

### B: Protocol Detector might_apply() Overhead

Four detectors (RGB++, Fiber, Stable++, UTXOSwap) each call `might_apply()` on every TX, iterating all inputs + outputs to check code_hashes. The individual comparisons are cheap (`==` on 32-byte slices), but multiplied by ~20M TXs × 4 detectors × (avg inputs + outputs), the aggregate is significant. At 8M+ block heights where protocol TXs are denser, `detect()` calls add further cost per matching TX per owner.

### C: Serial Double Cell Traversal

`update_hodl_wave` and `update_cell_distribution` both run **after** `thread::scope` joins (serial, on critical path). Each independently traverses all blocks → TXs → cells in the batch, doing create/consume tracking. This is effectively two full passes over the same data, and neither can overlap with the parallel write threads.

## Optimization Design

### A: Incremental Cohort Accumulation (Eliminate Full Scan)

**Current flow**:
```
day boundary → iterator_cf(cf_addr_balance) → deserialize all → binary search each → write snapshot
```

**New flow**:
```
each batch → iterate address_balance_changes (already available) → delta-update cohort_accum → day boundary → snapshot from accum
```

**Implementation**:

1. Add `cohort_accum: HashMap<String, (i128, i128)>` to `CellDistributionTracker` (keyed by `YYYY-MM` cohort month, values are `(used_capacity, balance)` totals)

2. Add method `update_cohort_deltas(&mut self, changes: &HashMap<Vec<u8>, BalanceChangeTuple>, existing_balances: &HashMap<...>)`:
   - For each changed address, compute its cohort month from `first_seen_block` via existing `block_number_to_date()`
   - Apply balance delta and used_capacity delta to `cohort_accum`

3. Call `update_cohort_deltas()` during the merged tracker update (see optimization C), passing `address_balance_changes` which is already computed for T2

4. At day boundary, `maybe_snapshot()` returns cohort data directly from `cohort_accum` instead of scanning CF

5. Persist `cohort_accum` as part of `CellDistributionTrackerState` (add field to the state struct)

6. Delete `compute_address_cohort_snapshot()` entirely

**Key invariant**: `cohort_accum` must be consistent with `CF_ADDR_BALANCE`. Since both are updated from the same `address_balance_changes` in the same batch, and bulk sync has no concurrent writers, consistency is guaranteed.

**Edge case — first_seen_block for existing addresses**: For addresses that already existed before this batch, `first_seen_block` comes from the prefetched `AddressBalance` record (already available in the T2 write path as `prefetched_addr_balances`). For new addresses in this batch, `first_seen_block` is the current block number (set during T2 address balance creation).

### B: Batch-Level Detector Pre-filter + Per-TX Bit Flags

**Current flow**:
```
per TX: 4 × might_apply() → each iterates inputs + outputs → filter to applicable_detectors
```

**New flow**:
```
precompute: scan all cells once → build unique code_hash sets → batch-level filter + per-TX 4-bit flags
per TX: read 4-bit flag (O(1)) → filter to applicable_detectors
```

**Implementation**:

1. During the existing precompute pass (which already iterates all cells for cell index key generation), collect:
   - `batch_lock_code_hashes: HashSet<[u8; 32]>` — all unique lock code_hashes in the batch
   - `batch_type_code_hashes: HashSet<[u8; 32]>` — all unique type code_hashes in the batch

2. Add `might_apply_batch(lock_hashes: &HashSet, type_hashes: &HashSet) -> bool` to `ProtocolDetector` trait:
   - Each detector checks if any of its known code_hashes appear in the batch sets
   - Called once per batch per detector (4 calls total)
   - If false, detector is excluded from the entire batch

3. For detectors that pass batch-level filter, compute per-TX flags during precompute:
   - `tx_detector_flags: Vec<u8>` — one byte per TX, bits 0-3 correspond to 4 detectors
   - During the same cell iteration that builds code_hash sets, set bits for each TX that has matching code_hashes
   - This replaces the per-TX `might_apply()` call in `build_tx_activity_bundle()`

4. In T_ACT, replace:
   ```rust
   let applicable_detectors: Vec<&dyn ProtocolDetector> = detectors
       .iter()
       .filter(|d| d.might_apply(tx))
       .collect();
   ```
   with:
   ```rust
   let flags = tx_detector_flags[tx_global_idx];
   let applicable_detectors: Vec<&dyn ProtocolDetector> = detectors
       .iter()
       .enumerate()
       .filter(|(i, _)| flags & (1 << i) != 0)
       .map(|(_, d)| d.as_ref())
       .collect();
   ```

**Cost**: One additional byte per TX during precompute (~200KB for 200K TXs), negligible. The code_hash set checks during precompute add ~1 branch per cell per detector, but this merges into the existing cell iteration loop.

### C: Merge Trackers + Move Into thread::scope

**Current flow**:
```
thread::scope { T1|T1b|T2|T4|T5|T6a|T6b|T7|T_ACT } → join all
→ finalize (serial)
→ update_hodl_wave (serial, full cell traversal)
→ update_cell_distribution (serial, full cell traversal + cohort scan)
```

**New flow**:
```
thread::scope { T1|T1b|T2|T4|T5|T6a|T6b|T7|T_ACT|T_TRACK } → join all
→ finalize (serial, lighter)
```

**Implementation**:

1. Add `T_TRACK` thread inside `thread::scope`:
   ```rust
   let h_track = s.spawn(|| -> Result<f64> {
       let t = Instant::now();
       let mut hodl = self.hodl_tracker.lock().unwrap();
       let mut cell_dist = self.cell_dist_tracker.lock().unwrap();
       // Single traversal over all blocks → txs → cells
       for parsed in all_parsed_blocks {
           let block_date = block_date(parsed.timestamp);
           hodl.record_block_date(parsed.number, block_date);
           cell_dist.record_block_date(parsed.number, block_date);
           for tx_data in tx_slice {
               for cell in &tx_data.cells {
                   hodl.cell_created(block_date, cell.capacity);
                   cell_dist.cell_created(block_date, occupied_capacity(cell));
               }
               if !tx_data.is_cellbase {
                   for input in &tx_data.inputs {
                       // resolve from input_cell_info / batch_cell_infos (shared refs)
                       hodl.cell_consumed(...);
                       cell_dist.cell_consumed(...)?;
                   }
               }
           }
           // HODL holder count update
           // ... (from address_balance_changes)
           // Cell dist day boundary snapshot
           if let Some((date, snapshot)) = cell_dist.maybe_snapshot(block_date) {
               // ... write snapshot + cohort from accum
           }
       }
       // Persist tracker states
       store.put_hodl_tracker_state(&hodl.to_state())?;
       store.put_cell_dist_tracker_state(&cell_dist.to_state())?;
       Ok(t.elapsed().as_secs_f64() * 1000.0)
   });
   ```

2. Shared data requirements (all read-only in scope, no conflicts):
   - `all_parsed_blocks` — shared `&[ParsedBlock]`
   - `all_tx_data` — shared `&[TxData]`
   - `input_cell_info` — shared `&HashMap`
   - `batch_cell_infos` — shared `&HashMap`
   - `address_balance_changes` — shared `&HashMap` (for hodl holder count)

3. CF conflict check:
   - T_TRACK writes to: `CF_HODL_TRACKER_STATE`, `CF_CELL_DIST_STATE`, `CF_CELL_DISTRIBUTION`, `CF_ADDRESS_COHORT`
   - These CFs are NOT written by any other thread (T1-T7, T_ACT)
   - Safe to run in parallel with independent StoreBatch

4. Remove the post-scope `update_hodl_wave()` and `update_cell_distribution()` calls from the serial path

5. Add `t_track_ms` to the batch timing log alongside other thread timings

**Effect**: Double traversal → single traversal (halved loop overhead). Serial → parallel (hidden behind T_ACT or longest-running thread). Net cost on critical path: zero when T_ACT > T_TRACK.

## Execution Order

1. **C first** (lowest risk, mechanical refactor) — merge trackers, move into scope
2. **B second** (low risk, precompute extension) — batch-level pre-filter + bit flags
3. **A last** (medium risk, new accumulation logic) — incremental cohort, delete full scan

## Validation

- Run full sync before and after, compare wall clock and per-batch timing logs
- Verify `CF_CELL_DISTRIBUTION` and `CF_ADDRESS_COHORT` produce identical snapshots vs old code (sample 10 day boundaries)
- Existing tests: `cargo test` + `pnpm test`
- Add unit tests for incremental cohort accumulation
- `ckbadger verify --depth fast` after sync completes
