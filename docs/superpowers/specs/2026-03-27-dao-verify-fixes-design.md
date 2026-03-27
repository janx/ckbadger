# Fix DAO Verify Failures: Status=1 Inclusion, Phase-2 Subtraction, Timestamp Conversion

**Goal:** Fix four sampling verify failures by correcting how DAO statistics handle withdraw-request deposits, secondary issuance split tracking, and deposit-time conversion.

**Context:** Sampling verify shows:
- `explorer_block_time_distribution` — error (empty chart data)
- `nervos_dao_unclaimed_compensation` — 10.27% low
- `nervos_dao_average_deposit_time` — 18.87% low
- `nervos_dao_deposit_compensation` — 4.41% low

**Re-sync required:** Yes (Fix B changes daily snapshot accumulation).

---

## Root Cause Analysis

### Root Cause A: Status=1 deposits excluded from statistics

`refresh_latest_dao_statistics()` scans only `status=0` deposits. `accumulate_dao_statistics_entry()` ignores `status=1` entries entirely. But status=1 deposits (withdraw request pending) are still locked in the DAO — the CKB hasn't left until phase-2 completes. The explorer includes them.

This causes `unclaimed_compensation` and `average_deposit_time` to be systematically low.

### Root Cause B: `running_total_deposited` subtracts at phase-1 instead of phase-2

In `accumulate_dao_snapshot_deltas_for_txs()`, capacity is subtracted from `dao_daily_active_delta` at phase-1 (withdraw request). No subtraction occurs at phase-2 (withdrawal completion). Per the CKB protocol (RFC-0023), the secondary issuance split uses ALL DAO-locked capacity — including withdraw-request cells. Subtracting at phase-1 makes `running_total_deposited` too low, causing `split_secondary_issuance()` to underallocate to depositors.

This causes `deposit_compensation` (cum_dao_compensation) to drift low cumulatively.

### Root Cause C: `epochs_to_days` uses fixed 1800 blocks/epoch

The conversion `blocks / 1800 / 6` assumes exactly 1800 blocks per epoch. CKB epoch lengths vary dynamically. The ratio `1186 / 962 ≈ 1.233` matches `1800 / actual_avg_blocks_per_epoch`, confirming a systematic conversion bias. Using actual timestamps eliminates this.

### Root Cause D: Block time distribution returns None on empty data

`weighted_avg_block_time_ms_from_distribution()` returns `None` when ratio_sum is zero (no block pairs found). This happens when the secondary store hasn't replicated block headers at chart generation time.

---

## Fix A: Include status=1 deposits in DAO statistics

### Changes

**File: `crates/indexer/src/db/writer/statistics.rs`** — `refresh_latest_dao_statistics()`

Replace `scan_dao_deposits_by_status(0, ...)` with a scan that processes both status=0 and status=1 deposits. For the unclaimed compensation calculation:

- **Status=0**: Use `tip_ar` as today (compensation still accruing)
- **Status=1**: Use `withdraw_request_ar` (compensation locked in at request time)

Both statuses contribute to: `total_deposited`, `unique_depositors`, `active_deposits`, `total_blocks_held`, `active_filtered_count`, `unclaimed_compensation`, and `depositor_map`.

```rust
// Before: only status=0
self.store.scan_dao_deposits_by_status(0, |_, entry| { ... })?;

// After: status=0 and status=1
for status in [0, 1] {
    self.store.scan_dao_deposits_by_status(status, |_, entry| {
        // ... same accumulation logic ...
        // AR selection:
        let effective_ar = if entry.status == 1 {
            entry.withdraw_request_ar
                .and_then(|ar| u64::try_from(ar).ok())
                .unwrap_or(tip_ar)
        } else {
            tip_ar
        };
        // ... use effective_ar instead of tip_ar in compensation calc ...
    })?;
}
```

**File: `crates/api/src/routes/dao.rs`** — `accumulate_dao_statistics_entry()`

Add a `status == 1` arm that mirrors the `status == 0` logic but uses `withdraw_request_ar`. The fallback path (API recomputing when tip doesn't match cached) must also include status=1.

```rust
match entry.status {
    0 => { /* existing: use latest_ar */ }
    1 => {
        acc.total_deposited += entry.capacity as i128;
        acc.unique_depositors.insert(entry.lock_script_hash.clone());
        acc.active_count += 1;
        // blocks held + unclaimed using withdraw_request_ar
        // ...
    }
    2 => { /* existing: paid compensation */ }
    _ => {}
}
```

### Naming

The existing field names `active_deposits` / `active_count` will now include status=1 deposits. This is semantically correct — they are "active" in the sense that CKB is still locked. No rename needed.

---

## Fix B: Move active delta subtraction from phase-1 to phase-2

### Changes

**File: `crates/indexer/src/sync/dao_helpers.rs`** — `accumulate_dao_snapshot_deltas_for_txs()`

1. **Remove phase-1 subtraction** (current lines ~484-534): The block guarded by `if !has_withdraw_request_output { continue; }` that subtracts from `daily_active_delta` and decrements unique depositors — remove the `daily_active_delta` subtraction and the `bump_unique_active_depositors(..., -1, ...)` call from this block. Keep the `same_batch_dao_map` insertions and other tracking.

2. **Add phase-2 subtraction** (current lines ~472-481): When `consumed_dao_map` shows `*status == 1`, subtract capacity from `daily_active_delta` and decrement unique depositors:

```rust
for input in &tx_data.inputs {
    let outpoint = (...);
    if let Some((_, _, capacity_str, _, status)) = consumed_dao_map.get(&outpoint) {
        if *status == 1 {
            *daily_withdrawals_delta.entry(block_date).or_default() += 1;
            // NEW: subtract from active delta at phase-2
            let capacity: i64 = capacity_str.parse()?;
            *daily_active_delta.entry(block_date).or_default() -= capacity as i128;
            // NEW: decrement unique depositors
            if let Some(lock_hash) = /* resolve lock_hash from input_cell_info */ {
                bump_unique_active_depositors(
                    active_deposit_counts_by_lock,
                    daily_unique_depositors_delta,
                    block_date,
                    lock_hash,
                    -1,
                    &tx_data.hash,
                    outpoint.1,
                )?;
            }
        }
    }
}
```

Note: resolving the lock_hash for the phase-2 consumed cell requires looking it up from `input_cell_info` or `batch_cell_infos` using the withdraw-request outpoint. The consumed_dao_map entry is keyed by the withdraw-request cell's outpoint, so we need the lock_script_hash from the original deposit. We can get this from the consumed_dao_map entry or by looking up the deposit entry in the store.

**Approach for lock_hash at phase-2:** Extend the `consumed_dao_map` type to include `lock_script_hash`. Currently the map value is `(Vec<u8>, i16, String, i64, i16)` — `(tx_hash, output_index, capacity_str, deposit_block, status)`. Add lock_script_hash as a 6th field: `(Vec<u8>, i16, String, i64, i16, Vec<u8>)`.

This requires updating `find_consumed_dao_deposits_batch()` in `dao.rs` and the bulk-build equivalent to include `lock_script_hash` in the returned tuple.

**File: `crates/indexer/src/sync/bulk_build/owners/dao.rs`**

Apply the same phase-1→phase-2 subtraction change in the bulk-build materializer. The bulk-build path has its own DAO delta accumulation that mirrors the batch path.

---

## Fix C: Use actual timestamps for average deposit time

### Changes

**File: `crates/ckbadger-store/src/types.rs`** — `DaoDepositCacheEntry`

Add `deposit_timestamp: i64` field (milliseconds). This avoids a per-deposit block header lookup during statistics refresh.

```rust
pub struct DaoDepositCacheEntry {
    pub capacity: i64,
    pub deposit_block_number: i64,
    pub deposit_timestamp: i64,  // NEW: ms since epoch
    pub lock_script_hash: Vec<u8>,
    // ... rest unchanged
}
```

**Deposit insertion paths** (`dao.rs:build_dao_cache_entry`, bulk-build DAO owner): Populate `deposit_timestamp` from the block timestamp.

**Statistics computation** (`statistics.rs`, `dao.rs`): Replace blocks-to-days conversion:

```rust
// Before:
total_blocks_held += (tip_block_number - entry.deposit_block_number) as f64;
// ... later:
let avg_epochs = total_blocks_held / active_filtered_count as f64 / 1800.0;
let avg_days = avg_epochs * 4.0 / 24.0;

// After:
let held_ms = tip_timestamp - entry.deposit_timestamp;
total_ms_held += held_ms as f64;
// ... later:
let avg_days = total_ms_held / active_filtered_count as f64 / 86_400_000.0;
```

The `tip_timestamp` comes from the sync tip block header (already available in `refresh_latest_dao_statistics`).

Remove the `epochs_to_days()` function (no longer needed for this purpose).

**Format change:** The `average_deposit_days` field in `DaoLatestStatistics` remains a formatted string. The formatting logic stays the same, only the input changes from epoch-derived days to timestamp-derived days.

---

## Fix D: Block time distribution empty-data guard

### Changes

**File: `crates/indexer/src/verify/explorer.rs`** — `ExplorerBlockTimeDistribution::run()`

Instead of failing with an opaque "failed to derive" error, check for empty data explicitly:

```rust
let distribution: ChartResponse = api_get(ctx, "charts/block-time-distribution")?;
if distribution.data.is_empty() {
    return Ok(CheckResult::fail(1, vec![CheckFinding::new(
        "empty_data",
        "block-time-distribution chart returned no data (secondary store may not have replicated block headers yet)",
    )]));
}
let our_ms = weighted_avg_block_time_ms_from_distribution(&distribution.data)
    .ok_or_else(|| anyhow::anyhow!(
        "failed to derive avg block time from distribution: {} points, all ratios zero",
        distribution.data.len()
    ))?;
```

This gives a clear diagnostic instead of a confusing error.

---

## Testing

### Unit Tests

1. **Status=1 compensation** (`statistics.rs`): Test that `refresh_latest_dao_statistics` includes status=1 deposits with `withdraw_request_ar` for compensation.

2. **Phase-2 active delta** (`dao_helpers.rs`): Test that `accumulate_dao_snapshot_deltas_for_txs` does NOT subtract at phase-1 and DOES subtract at phase-2.

3. **Timestamp-based average** (`statistics.rs`): Test that average deposit days uses timestamps, not block-count conversion.

4. **Empty distribution guard** (`explorer.rs`): Test that empty chart data produces a clear finding, not an opaque error.

### Integration Validation

After re-sync, run `ckbadger verify --depth sampling` — the four failing checks should pass within tolerance.

---

## Files Changed

| File | Change |
|---|---|
| `crates/indexer/src/db/writer/statistics.rs` | Scan status=0+1, timestamp-based avg |
| `crates/api/src/routes/dao.rs` | Include status=1 in accumulator |
| `crates/indexer/src/sync/dao_helpers.rs` | Move subtraction to phase-2 |
| `crates/indexer/src/sync/bulk_build/owners/dao.rs` | Same phase-2 change for bulk path |
| `crates/indexer/src/db/writer/dao.rs` | Add lock_hash to consumed_dao_map; populate deposit_timestamp |
| `crates/ckbadger-store/src/types.rs` | Add `deposit_timestamp` to `DaoDepositCacheEntry` |
| `crates/indexer/src/verify/explorer.rs` | Empty-data guard for block time distribution |

## Principle Alignment

- **Single Calculation Path**: Each metric has one computation path, made correct
- **No Fallback Chains**: Not adding fallbacks — fixing the upstream logic
- **Fix Root Cause, Not Symptoms**: Not adjusting tolerances — correcting the data
- **Fail Fast**: Block time distribution check reports clear context on empty data
