# DAO Verify Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix four sampling verify failures by including status=1 deposits, moving subtraction to phase-2, using timestamps for deposit time, and guarding empty chart data.

**Architecture:** Four independent fixes applied in dependency order: schema changes first (types), then indexer logic (dao_helpers, bulk_build, statistics), then API consumers (dao.rs routes), then verify checks (explorer.rs).

**Tech Stack:** Rust, RocksDB (ckbadger-store), Axum API

**Spec:** `docs/superpowers/specs/2026-03-27-dao-verify-fixes-design.md`

---

### Task 1: Add `deposit_timestamp` to `DaoDepositCacheEntry` and rename `DaoTopDepositorEntry.average_deposit_blocks`

**Files:**
- Modify: `crates/ckbadger-store/src/types.rs:190-206` (DaoDepositCacheEntry)
- Modify: `crates/ckbadger-store/src/types.rs:264-269` (DaoTopDepositorEntry)

- [ ] **Step 1: Add `deposit_timestamp` field to `DaoDepositCacheEntry`**

In `crates/ckbadger-store/src/types.rs`, add after `deposit_block_number` (line 192):

```rust
pub struct DaoDepositCacheEntry {
    pub capacity: i64,
    pub deposit_block_number: i64,
    #[serde(default)]
    pub deposit_timestamp: i64,  // milliseconds since epoch
    pub lock_script_hash: Vec<u8>,
    // ... rest unchanged
}
```

Use `#[serde(default)]` for backward compat during deserialization of old data (will default to 0 until re-sync).

- [ ] **Step 2: Rename `average_deposit_blocks` to `average_deposit_ms` in `DaoTopDepositorEntry`**

```rust
pub struct DaoTopDepositorEntry {
    pub lock_script_hash: Vec<u8>,
    pub total_capacity: i128,
    pub deposit_count: i32,
    #[serde(alias = "average_deposit_blocks")]
    pub average_deposit_ms: f64,
}
```

Use `serde(alias)` so existing serialized data can still deserialize.

- [ ] **Step 3: Run `cargo check` to find all compilation errors from the rename**

Run: `cargo check 2>&1 | head -60`
Expected: Errors in statistics.rs and dao.rs where `average_deposit_blocks` is referenced.

- [ ] **Step 4: Fix all `average_deposit_blocks` references**

In `crates/indexer/src/db/writer/statistics.rs` (~line 1068), change field name:
```rust
DaoTopDepositorEntry {
    lock_script_hash: lock_hash,
    total_capacity,
    deposit_count,
    average_deposit_ms: avg_ms, // was average_deposit_blocks: avg_blocks
}
```

In `crates/api/src/routes/dao.rs` (~line 917), change:
```rust
let avg_days = d.average_deposit_ms / 86_400_000.0;
```
(Remove the `/ 1800.0` and `* 4.0 / 24.0` conversion.)

- [ ] **Step 5: Run `cargo check` to confirm clean**

Run: `cargo check`
Expected: Clean build (warnings OK).

- [ ] **Step 6: Commit**

```
git add crates/ckbadger-store/src/types.rs crates/indexer/src/db/writer/statistics.rs crates/api/src/routes/dao.rs
git commit -m "refactor: add deposit_timestamp to DaoDepositCacheEntry, rename average_deposit_blocks to average_deposit_ms"
```

---

### Task 2: Replace `DaoConsumedRow` tuple with named struct

**Files:**
- Modify: `crates/indexer/src/sync/dao_helpers.rs:71-72` (type aliases)
- Modify: `crates/indexer/src/db/writer/dao.rs:36-48,50-51,260-330` (dao_cache_entry_to_row, trait, find_consumed)
- Modify: `crates/indexer/src/sync/batch.rs:1619-1650` (same_batch_dao_deposits map + pending_dao_entries)
- Modify: `crates/indexer/src/db/writer/dao.rs:614-616,999-1001` (BatchCtx and TestCtx test structs)

- [ ] **Step 1: Define `DaoConsumedRow` as a named struct**

In `crates/indexer/src/sync/dao_helpers.rs`, replace lines 71-72:

```rust
// Before:
pub(crate) type DaoConsumedRow = (Vec<u8>, i16, String, i64, i16);
pub(crate) type DaoConsumedMap = HashMap<(Vec<u8>, i16), DaoConsumedRow>;

// After:
#[derive(Debug, Clone)]
pub(crate) struct DaoConsumedRow {
    pub tx_hash: Vec<u8>,
    pub output_index: i16,
    pub capacity_str: String,
    pub deposit_block: i64,
    pub status: i16,
    pub lock_script_hash: Vec<u8>,
}
pub(crate) type DaoConsumedMap = HashMap<(Vec<u8>, i16), DaoConsumedRow>;
```

- [ ] **Step 2: Update `dao_cache_entry_to_row` to return `DaoConsumedRow`**

In `crates/indexer/src/db/writer/dao.rs`, replace the function (lines 36-48):

```rust
fn dao_cache_entry_to_row(
    tx_hash: Vec<u8>,
    output_index: i16,
    entry: DaoDepositCacheEntry,
) -> DaoConsumedRow {
    DaoConsumedRow {
        tx_hash,
        output_index,
        capacity_str: entry.capacity.to_string(),
        deposit_block: entry.deposit_block_number,
        status: entry.status,
        lock_script_hash: entry.lock_script_hash,
    }
}
```

Add the import at the top of the file:
```rust
use crate::sync::dao_helpers::DaoConsumedRow;
```

- [ ] **Step 3: Update `DaoWithdrawalContextTrait` and its implementors**

In `crates/indexer/src/db/writer/dao.rs`, change the trait (line 51):
```rust
pub trait DaoWithdrawalContextTrait {
    fn consumed_deposits(&self) -> &[DaoConsumedRow];
    // ... rest unchanged
}
```

Update `DaoWithdrawalContext.consumed_deposits` field (line 66):
```rust
pub struct DaoWithdrawalContext {
    pub consumed_deposits: Vec<DaoConsumedRow>,
    // ... rest unchanged
}
```

- [ ] **Step 4: Update `find_consumed_dao_deposits_batch` return type and construction**

Change the return type (line 263) and all `result_map` insertions to construct `DaoConsumedRow` structs instead of tuples. Update the map type:
```rust
pub fn find_consumed_dao_deposits_batch(
    &self,
    inputs: &[(&[u8], i16)],
) -> Result<HashMap<(Vec<u8>, i16), DaoConsumedRow>> {
```

- [ ] **Step 5: Update `process_dao_withdrawals_batch` destructuring**

All tuple destructuring patterns like `(original_tx_hash, original_output_index, capacity_str, deposit_block, status)` must use struct field access instead: `row.tx_hash`, `row.output_index`, `row.capacity_str`, `row.deposit_block`, `row.status`.

- [ ] **Step 6: Update `accumulate_dao_snapshot_deltas_for_txs` tuple destructuring**

In `crates/indexer/src/sync/dao_helpers.rs`, update lines 477 and 497 to use struct field access:
```rust
// Line 477: was (_, _, _, _, status)
if let Some(row) = consumed_dao_map.get(&outpoint) {
    if row.status == 1 {
        *daily_withdrawals_delta.entry(block_date).or_default() += 1;
    }
}

// Line 497: was (_, _, capacity_str, _, status)
if let Some(row) = consumed_dao_map.get(&outpoint) {
    if row.status == 0 {
        maybe_cap = Some(row.capacity_str.parse::<i64>().map_err(|e| { ... })?);
    }
}
```

- [ ] **Step 7: Update `same_batch_dao_deposits` in `batch.rs`**

In `crates/indexer/src/sync/batch.rs:1619-1650`, the `same_batch_dao_deposits` map uses a standalone 5-tuple type (NOT the `DaoConsumedMap` alias). Change it to use `DaoConsumedRow`:

```rust
let mut same_batch_dao_deposits: DaoConsumedMap = HashMap::new();
// ...
same_batch_dao_deposits.insert(
    (deposit.tx_hash.clone(), deposit_output_index),
    DaoConsumedRow {
        tx_hash: deposit.tx_hash.clone(),
        output_index: deposit_output_index,
        capacity_str: deposit.capacity.to_string(),
        deposit_block: *block_number,
        status: 0,
        lock_script_hash: deposit.lock_script_hash.clone(),
    },
);
```

Import `DaoConsumedMap` and `DaoConsumedRow` at the top of `batch.rs`.

- [ ] **Step 8: Update test structs `BatchCtx` and `TestCtx` in `dao.rs`**

In `crates/indexer/src/db/writer/dao.rs`, update `BatchCtx` (~line 614) and `TestCtx` (~line 999) to use `Vec<DaoConsumedRow>` instead of `Vec<(Vec<u8>, i16, String, i64, i16)>`. Update their test constructions with the struct form including `lock_script_hash`.

- [ ] **Step 9: Run `cargo check` iteratively to fix all remaining compilation errors**

Run: `cargo check`

The compiler will catch every remaining call site. Known blast radius:
- `crates/indexer/src/sync/batch.rs` — `consumed_dao_map` usage, `DaoWithdrawalContext` construction
- `crates/indexer/src/db/writer/dao.rs` — `process_dao_withdrawals_batch` destructuring, test structs
- `crates/indexer/tests/dao_batch_operations.rs` — test data construction

- [ ] **Step 10: Run tests**

Run: `cargo test --lib -p ckbadger-indexer -- dao`
Expected: All existing DAO tests pass.

- [ ] **Step 11: Commit**

```
git add crates/indexer/src/sync/dao_helpers.rs crates/indexer/src/db/writer/dao.rs crates/indexer/src/sync/batch.rs crates/indexer/tests/dao_batch_operations.rs
git commit -m "refactor: replace DaoConsumedRow tuple with named struct, add lock_script_hash field"
```

---

### Task 3: Move active delta subtraction from phase-1 to phase-2 (batch sync)

**Files:**
- Modify: `crates/indexer/src/sync/dao_helpers.rs:472-536` (accumulate_dao_snapshot_deltas_for_txs)
- Test: `crates/indexer/src/sync/dao_helpers.rs` (inline tests)

- [ ] **Step 1: Write failing test for phase-2 subtraction**

Add test in the `#[cfg(test)]` module of `crates/indexer/src/sync/dao_helpers.rs`:

```rust
#[test]
fn test_phase1_does_not_subtract_active_delta() {
    // Setup: a phase-1 tx (has withdraw_request_output, consumes status=0 deposit)
    // Assert: daily_active_delta is NOT decremented at phase-1
    // (deposit capacity stays in active total because CKB is still locked in DAO)
}

#[test]
fn test_phase2_subtracts_active_delta() {
    // Setup: a phase-2 tx (consumes status=1 deposit from consumed_dao_map)
    // Assert: daily_active_delta IS decremented at phase-2
    // Assert: daily_withdrawals_delta IS incremented
    // Assert: unique depositors decremented
}
```

Build test structure using existing test helpers in the file (e.g., `dummy_parsed_block`, `dummy_tx_data`). Look at the existing test `test_accumulate_dao_snapshot_deltas_subtracts_phase1_even_when_capacity_differs` for the pattern.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib -p ckbadger-indexer -- test_phase1_does_not_subtract test_phase2_subtracts`
Expected: FAIL (phase-1 still subtracts, phase-2 doesn't).

- [ ] **Step 3: Modify `accumulate_dao_snapshot_deltas_for_txs` — add phase-2 subtraction**

In the phase-2 block (lines 472-481), add capacity subtraction and depositor decrement:

```rust
for input in &tx_data.inputs {
    let outpoint = (
        input.previous_tx_hash.to_vec(),
        parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer")?,
    );
    if let Some(row) = consumed_dao_map.get(&outpoint) {
        if row.status == 1 {
            *daily_withdrawals_delta.entry(block_date).or_default() += 1;
            // Phase-2: CKB leaves the DAO — subtract from active delta
            let capacity: i64 = row.capacity_str.parse().map_err(|e| {
                anyhow!(
                    "invalid DAO capacity string at phase-2 withdrawal: value='{}', tx_hash=0x{}, error={}",
                    row.capacity_str,
                    hex::encode(tx_data.hash),
                    e
                )
            })?;
            *daily_active_delta.entry(block_date).or_default() -= capacity as i128;
            bump_unique_active_depositors(
                active_deposit_counts_by_lock,
                daily_unique_depositors_delta,
                block_date,
                &row.lock_script_hash,
                -1,
                &tx_data.hash,
                outpoint.1,
            )?;
        }
    }
}
```

- [ ] **Step 4: Remove phase-1 subtraction**

Remove the `daily_active_delta -= capacity` line and the `bump_unique_active_depositors(..., -1, ...)` call from the phase-1 block (lines 511-534). Keep the `same_batch_dao_map` insertion and the status=0 capacity parsing (still needed for `same_batch_dao_map`).

The phase-1 block should now only track same-batch deposits for cross-tx references within the batch, not modify `daily_active_delta` or unique depositors.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib -p ckbadger-indexer -- dao`
Expected: New tests pass, existing tests may need adjustment.

- [ ] **Step 6: Commit**

```
git add crates/indexer/src/sync/dao_helpers.rs
git commit -m "fix: move active delta subtraction from phase-1 to phase-2 in batch sync"
```

---

### Task 4: Move active delta subtraction from phase-1 to phase-2 (bulk-build)

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/owners/dao.rs:131-318`

- [ ] **Step 1: Remove phase-1 subtraction in bulk-build**

In `crates/indexer/src/sync/bulk_build/owners/dao.rs`, in the `0 =>` arm (lines 163-176), remove:

```rust
// REMOVE these lines:
Self::bump_daily_i128(
    &mut self.daily_active_delta,
    tx_date,
    -(entry.capacity as i128),
    "dao daily active delta",
)?;
Self::bump_active_depositor_count(
    &mut self.active_deposit_counts_by_lock,
    &mut self.daily_unique_depositors_delta,
    tx_date,
    &entry.lock_script_hash,
    -1,
    "dao unique active depositor count",
)?;
```

- [ ] **Step 2: Add phase-2 subtraction in bulk-build**

In the `1 =>` arm (around line 317, after the existing withdrawal counting), add:

```rust
// Phase-2: CKB leaves the DAO — subtract from active delta
Self::bump_daily_i128(
    &mut self.daily_active_delta,
    tx_date,
    -(entry.capacity as i128),
    "dao daily active delta (phase-2 withdrawal)",
)?;
Self::bump_active_depositor_count(
    &mut self.active_deposit_counts_by_lock,
    &mut self.daily_unique_depositors_delta,
    tx_date,
    &entry.lock_script_hash,
    -1,
    "dao unique active depositor count (phase-2 withdrawal)",
)?;
```

Place this after `entry.compensation = Some(compensation);` and the existing `bump_daily_i64` for withdrawals (line 317).

- [ ] **Step 3: Run `cargo check`**

Run: `cargo check`
Expected: Clean build.

- [ ] **Step 4: Run existing bulk-build tests**

Run: `cargo test --lib -p ckbadger-indexer -- bulk_build`
Expected: All pass. The bulk-build path has existing tests that cover DAO processing. If any tests assert on the old phase-1 subtraction behavior, update them to expect phase-2 subtraction instead.

- [ ] **Step 5: Commit**

```
git add crates/indexer/src/sync/bulk_build/owners/dao.rs
git commit -m "fix: move active delta subtraction from phase-1 to phase-2 in bulk-build"
```

---

### Task 5: Populate `deposit_timestamp` in both insertion paths

**Files:**
- Modify: `crates/indexer/src/db/writer/dao.rs:14-34` (build_dao_cache_entry)
- Modify: `crates/indexer/src/db/writer/dao.rs:244` (insert_dao_deposits_batch)
- Modify: `crates/indexer/src/sync/bulk_build/owners/dao.rs:339-352` (bulk deposit insertion)

- [ ] **Step 1: Add `deposit_timestamp` parameter to `build_dao_cache_entry`**

```rust
fn build_dao_cache_entry(
    deposit: &ParsedDaoDeposit,
    block_number: i64,
    deposit_ar: i64,
    deposit_timestamp: i64,
) -> DaoDepositCacheEntry {
    DaoDepositCacheEntry {
        capacity: deposit.capacity,
        deposit_block_number: block_number,
        deposit_timestamp,
        lock_script_hash: deposit.lock_script_hash.clone(),
        deposit_ar,
        // ... rest unchanged
    }
}
```

- [ ] **Step 2: Pass timestamp from `insert_dao_deposits_batch`**

In `crates/indexer/src/db/writer/dao.rs` line 244, the `_timestamp` is a `DateTime<Utc>`. Use it:

```rust
for (deposit, block_number, timestamp, ar) in deposits {
    let entry = build_dao_cache_entry(deposit, *block_number, *ar, timestamp.timestamp_millis());
```

- [ ] **Step 3: Populate `deposit_timestamp` in bulk-build path**

In `crates/indexer/src/sync/bulk_build/owners/dao.rs` (~line 339), add the field:

```rust
DaoDepositCacheEntry {
    capacity: output.capacity,
    deposit_block_number: tx.block_number,
    deposit_timestamp: tx.timestamp_ms,
    lock_script_hash: output.lock_hash.clone(),
    // ... rest unchanged
}
```

- [ ] **Step 4: Fix all test code that constructs `DaoDepositCacheEntry` directly**

There are 30+ struct literals across these files that need `deposit_timestamp` added:
- `crates/ckbadger-store/src/dao_ops.rs` (~12 occurrences in tests)
- `crates/indexer/tests/dao_batch_operations.rs` (~10 occurrences)
- `crates/api/tests/api_integration.rs` (~8 occurrences)
- `crates/indexer/src/db/writer/statistics.rs` (test module)
- `crates/indexer/src/sync/batch.rs:1655` (pending_dao_entries construction)

Add `deposit_timestamp: 0` for test data (or a meaningful timestamp like `1_700_000_000_000` where tests need realistic values). Run `cargo check` to find them all — the compiler will flag every missing field.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib -- dao`
Expected: All pass.

- [ ] **Step 6: Commit**

```
git add crates/indexer/src/db/writer/dao.rs crates/indexer/src/sync/bulk_build/owners/dao.rs
git commit -m "feat: populate deposit_timestamp in DaoDepositCacheEntry from both insertion paths"
```

---

### Task 6: Include status=1 deposits in `refresh_latest_dao_statistics`

**Files:**
- Modify: `crates/indexer/src/db/writer/statistics.rs:895-1099` (refresh_latest_dao_statistics)
- Test: `crates/indexer/src/db/writer/statistics.rs` (inline tests)

- [ ] **Step 1: Write failing test**

Add test in the `#[cfg(test)]` module of `statistics.rs`:

```rust
#[test]
fn test_refresh_dao_statistics_includes_status1_deposits() {
    // Setup: store with one status=0 deposit and one status=1 deposit
    // (status=1 has withdraw_request_ar set)
    // Call refresh_latest_dao_statistics()
    // Assert: total_deposited includes BOTH deposits
    // Assert: unclaimed_compensation includes BOTH
    // Assert: active_deposits count = 2
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib -p ckbadger-indexer -- test_refresh_dao_statistics_includes_status1`
Expected: FAIL (status=1 deposits not included).

- [ ] **Step 3: Change scan to include both status=0 and status=1**

Replace the single `scan_dao_deposits_by_status(0, ...)` call with two scans. Extract the closure body into a shared helper or inline both scans. The key change for compensation:

```rust
for scan_status in [0i16, 1] {
    self.store.scan_dao_deposits_by_status(scan_status, |_, entry| {
        total_deposited += entry.capacity as i128;
        unique_depositors.insert(entry.lock_script_hash.clone());
        active_deposits += 1;

        // ... depositor_map accumulation ...

        if entry.deposit_block_number <= tip_block_number {
            // Use timestamps for blocks held
            let held_ms = tip_timestamp - entry.deposit_timestamp;
            total_ms_held += held_ms as f64;
            active_filtered_count += 1;

            // ... capacity validation (unchanged) ...

            // AR selection: fail fast for status=1 with missing AR
            let effective_ar = if entry.status == 1 {
                let ar_i64 = entry.withdraw_request_ar.ok_or_else(|| {
                    anyhow!(
                        "status=1 deposit missing withdraw_request_ar: deposit_block={}, lock_hash=0x{}",
                        entry.deposit_block_number,
                        hex::encode(&entry.lock_script_hash)
                    )
                })?;
                u64::try_from(ar_i64).map_err(|_| {
                    anyhow!(
                        "status=1 deposit withdraw_request_ar negative: deposit_block={}, ar={}",
                        entry.deposit_block_number,
                        ar_i64
                    )
                })?
            } else {
                tip_ar
            };

            if ar_deposit > 0 && effective_ar > ar_deposit {
                // ... compensation calc using effective_ar instead of tip_ar ...
            }
        }
        Ok(())
    })?;
}
```

- [ ] **Step 4: Switch average deposit time to timestamp-based**

Replace the block-count-based average with timestamp-based:

```rust
// Before:
let avg_epochs = if active_filtered_count > 0 {
    (total_blocks_held / active_filtered_count as f64) / 1800.0
} else {
    0.0
};
// ... epochs_to_days(avg_epochs)

// After:
let avg_days = if active_filtered_count > 0 {
    total_ms_held / active_filtered_count as f64 / 86_400_000.0
} else {
    0.0
};
// ... format_days(avg_days) — use same formatting logic as old epochs_to_days
```

Get `tip_timestamp` from the header: `header.timestamp` (already available from `get_sync_tip_block`).

- [ ] **Step 5: Update depositor_map to accumulate ms instead of blocks**

```rust
// Before (line 929):
dm.2 += (tip_block_number - entry.deposit_block_number) as f64;

// After:
dm.2 += (tip_timestamp - entry.deposit_timestamp) as f64;
```

And when building `DaoTopDepositorEntry` (~line 1068):
```rust
DaoTopDepositorEntry {
    lock_script_hash: lock_hash,
    total_capacity,
    deposit_count,
    average_deposit_ms: avg_ms, // was average_deposit_blocks
}
```

where `avg_ms = total_ms / deposit_count as f64`.

- [ ] **Step 6: Remove or rename `epochs_to_days` in statistics.rs**

Replace `epochs_to_days` with a `format_days` function that takes days directly:

```rust
fn format_days(days: f64) -> String {
    if days >= 1000.0 {
        format!("{:.1}K days+", days / 1000.0)
    } else if days < 1.0 && days > 0.0 {
        format!("{:.1} days", days)
    } else {
        format!("{:.0} days", days)
    }
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test --lib -p ckbadger-indexer -- dao`
Expected: New and existing tests pass.

- [ ] **Step 8: Commit**

```
git add crates/indexer/src/db/writer/statistics.rs
git commit -m "fix: include status=1 deposits in refresh_latest_dao_statistics, use timestamp-based avg"
```

---

### Task 7: Include status=1 deposits in API `accumulate_dao_statistics_entry`

**Files:**
- Modify: `crates/api/src/routes/dao.rs:195-254` (accumulate_dao_statistics_entry)
- Modify: `crates/api/src/routes/dao.rs:800-810` (fallback computation)
- Modify: `crates/api/src/routes/dao.rs:882-891` (epochs_to_days → format_days)

- [ ] **Step 1: Add status=1 arm to `accumulate_dao_statistics_entry`**

```rust
fn accumulate_dao_statistics_entry(
    acc: &mut DaoStatisticsAccumulator,
    entry: &DaoDepositCacheEntry,
    latest_block_number: i64,
    latest_ar: u64,
    tip_timestamp: i64,  // NEW parameter
) -> anyhow::Result<()> {
    match entry.status {
        0 | 1 => {
            acc.total_deposited += entry.capacity as i128;
            acc.unique_depositors.insert(entry.lock_script_hash.clone());
            acc.active_count += 1;

            if entry.deposit_block_number <= latest_block_number {
                let held_ms = tip_timestamp - entry.deposit_timestamp;
                acc.total_ms_held += held_ms as f64;
                acc.active_filtered_count += 1;

                // ... capacity validation unchanged ...

                let effective_ar = if entry.status == 1 {
                    let ar_i64 = entry.withdraw_request_ar.ok_or_else(|| {
                        anyhow::anyhow!(
                            "status=1 deposit missing withdraw_request_ar: deposit_block={}, lock_hash=0x{}",
                            entry.deposit_block_number,
                            hex::encode(&entry.lock_script_hash)
                        )
                    })?;
                    u64::try_from(ar_i64).map_err(|_| {
                        anyhow::anyhow!(
                            "status=1 deposit withdraw_request_ar negative: ar={}",
                            ar_i64
                        )
                    })?
                } else {
                    latest_ar
                };

                // ... compensation calc using effective_ar ...
            }
        }
        2 => { /* existing: paid compensation */ }
        _ => {}
    }
    Ok(())
}
```

- [ ] **Step 2: Update `DaoStatisticsAccumulator` to use `total_ms_held`**

```rust
#[derive(Default)]
struct DaoStatisticsAccumulator {
    total_deposited: i128,
    unique_depositors: HashSet<Vec<u8>>,
    active_count: i32,
    total_compensation_paid: i128,
    total_ms_held: f64,           // was total_blocks_held
    active_filtered_count: usize,
    total_unclaimed: u128,
}
```

- [ ] **Step 3: Extend `resolve_latest_block_and_ar` to also return timestamp**

`resolve_latest_block_and_ar` at `dao.rs:99-108` currently returns `(i64, u64)` (block_number, ar), discarding the header. Change it to return `(i64, u64, i64)` — adding the header timestamp:

```rust
fn resolve_latest_block_and_ar(
    state: &AppState,
    context: &str,
) -> Result<(i64, u64, i64), ApiRouteError> {
    // ... same tip fetch ...
    let timestamp = header.timestamp;
    // ...
    Ok((block_number, ar, timestamp))
}
```

Update both callers (`get_statistics` at line 777 and `get_address_summary` at line 664) to destructure the 3-tuple: `let (latest_block_number, latest_ar, tip_timestamp) = ...`. The `get_address_summary` caller can ignore `tip_timestamp` with `_` if not needed there.

- [ ] **Step 4: Pass `tip_timestamp` to `accumulate_dao_statistics_entry`**

Update the fallback scan call in `get_statistics`:
```rust
state.store.scan_dao_deposits(|_, entry| {
    accumulate_dao_statistics_entry(&mut acc, entry, latest_block_number, latest_ar, tip_timestamp)
})
```

Update average days computation:
```rust
let avg_days = if acc.active_filtered_count > 0 {
    acc.total_ms_held / acc.active_filtered_count as f64 / 86_400_000.0
} else {
    0.0
};
let avg_days_str = format_days(avg_days);
```

Also update existing test calls to `accumulate_dao_statistics_entry` (at ~line 1455) to pass the new `tip_timestamp` parameter.

- [ ] **Step 5: Replace `epochs_to_days` with `format_days` in dao.rs**

Replace `epochs_to_days` (line 882-891) with `format_days` — same body as statistics.rs, takes days directly. Note: `format_deposit_days` (line 938-946) is a DIFFERENT function with different formatting (no "days" suffix, used for top depositors). Keep it separate — it already receives days from `d.average_deposit_ms / 86_400_000.0` after the Task 1 rename.

- [ ] **Step 6: Run `cargo check`**

Run: `cargo check`
Expected: Clean build.

- [ ] **Step 7: Commit**

```
git add crates/api/src/routes/dao.rs
git commit -m "fix: include status=1 deposits in API DAO statistics accumulator, use timestamp-based avg"
```

---

### Task 8: Block time distribution empty-data guard

**Files:**
- Modify: `crates/indexer/src/verify/explorer.rs:1559-1586` (ExplorerBlockTimeDistribution::run)

- [ ] **Step 1: Write failing test**

Add test in the `#[cfg(test)]` module of `crates/indexer/src/verify/explorer.rs`:

```rust
#[test]
fn test_weighted_avg_block_time_ms_from_distribution_empty_data() {
    let points: Vec<ChartDataPoint> = vec![];
    assert_eq!(weighted_avg_block_time_ms_from_distribution(&points), None);
}
```

- [ ] **Step 2: Add empty-data guard to `ExplorerBlockTimeDistribution::run`**

Before the weighted average call, add:

```rust
fn run(&self, ctx: &CheckContext, _progress: &ProgressReporter) -> anyhow::Result<CheckResult> {
    let distribution: ChartResponse = api_get(ctx, "charts/block-time-distribution")?;

    // Guard: empty or all-zero distribution
    let has_nonzero = distribution.data.iter().any(|p| {
        p.value.parse::<f64>().unwrap_or(0.0) > 0.0
    });
    if distribution.data.is_empty() || !has_nonzero {
        return Ok(CheckResult::fail(1, vec![Finding {
            entity: "distribution".to_string(),
            details: vec![format!(
                "block-time-distribution chart has no data ({} points, all ratios zero); secondary store may not have replicated block headers yet",
                distribution.data.len()
            )],
        }]));
    }

    let our_ms = weighted_avg_block_time_ms_from_distribution(&distribution.data)
        .ok_or_else(|| anyhow::anyhow!(
            "failed to derive avg block time from distribution: {} points, unexpected parse failure",
            distribution.data.len()
        ))?;
    // ... rest unchanged
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib -p ckbadger-indexer -- block_time`
Expected: Pass.

- [ ] **Step 4: Commit**

```
git add crates/indexer/src/verify/explorer.rs
git commit -m "fix: guard against empty block-time-distribution data in verify check"
```

---

### Task 9: Run full test suite and fix any remaining issues

- [ ] **Step 1: Run all Rust tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No new warnings.

- [ ] **Step 3: Run frontend checks (in case API types changed)**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: Pass (API response types use camelCase via serde, frontend types should match).

- [ ] **Step 4: Final commit if any fixes needed**

```
git commit -m "fix: address test and lint issues from DAO verify fixes"
```
