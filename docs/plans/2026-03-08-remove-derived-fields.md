# Remove `derived_*` Fields Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove dead `derived_tip_block_number`, `derived_sync_in_progress`, and `derived_last_synced_at` fields from the entire codebase — they track a scenario (append-only store lagging domain store) that cannot occur.

**Architecture:** Bottom-up removal: store types first, then common types, indexer writers, API gates, TUI display, CLI output, and finally tests. Each task is independently compilable after completion.

**Tech Stack:** Rust (all crates), integration tests

---

### Task 1: Remove `derived_*` from `SyncStatus` (store types)

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs:657-704`

**Step 1: Remove fields from `SyncStatus` struct**

Remove these three fields (lines 662, 668, 670):

```rust
// DELETE these lines:
    #[serde(default)]
    pub derived_tip_block_number: i64,
    // ...
    #[serde(default)]
    pub derived_last_synced_at: i64,
    #[serde(default)]
    pub derived_sync_in_progress: bool,
```

**Step 2: Clean `init_sync_start()`**

Remove `derived_sync_in_progress` assignments (lines 697, 704). The method becomes:

```rust
pub fn init_sync_start(&mut self, start_block: i64, is_bulk_sync: bool) {
    if is_bulk_sync {
        let should_start_new_bulk_session = self.sync_started_at.is_none()
            || self.bulk_sync_completed_at.is_some()
            || start_block < self.sync_started_block;

        if should_start_new_bulk_session {
            self.sync_started_at = Some(chrono::Utc::now().timestamp());
            self.sync_started_block = start_block;
            self.bulk_sync_completed_at = None;
            self.bulk_sync_completed_block = None;
        }
    } else {
        if self.sync_started_at.is_none() || start_block < self.sync_started_block {
            self.sync_started_at = Some(chrono::Utc::now().timestamp());
        }
        self.sync_started_block = start_block;
    }
}
```

**Step 3: Verify compilation**

Run: `cargo check -p ckbadger-store`
Expected: Compilation errors in downstream crates (expected — we'll fix them in subsequent tasks).

**Step 4: Commit**

```
refactor: remove derived_* fields from SyncStatus struct
```

---

### Task 2: Remove `derived_*` from `SyncStatusData` (common types)

**Files:**

- Modify: `crates/common/src/sync.rs:10-91`

**Step 1: Remove fields from `SyncStatusData` struct**

Remove these fields (lines 15, 22, 24):

```rust
// DELETE:
    #[serde(default)]
    pub derived_tip_block_number: Option<i64>,
    // ...
    #[serde(default)]
    pub derived_last_synced_at: Option<i64>,
    #[serde(default)]
    pub derived_sync_in_progress: bool,
```

**Step 2: Clean `update_batch()`**

Remove lines 54, 60, 61 from `update_batch()`:

```rust
// DELETE:
        self.derived_tip_block_number = Some(block_number);
        self.derived_last_synced_at = Some(self.last_synced_at);
        self.derived_sync_in_progress = false;
```

**Step 3: Clean `init_sync_start()`**

Remove `derived_sync_in_progress` assignments (lines 82, 89). Same pattern as Task 1.

**Step 4: Verify compilation**

Run: `cargo check -p ckbadger-common`
Expected: PASS (no downstream deps checked yet)

**Step 5: Commit**

```
refactor: remove derived_* fields from SyncStatusData
```

---

### Task 3: Clean indexer writers and cache

**Files:**

- Modify: `crates/indexer/src/db/writer/sync.rs:49-64`
- Modify: `crates/indexer/src/cache.rs:57-64`
- Modify: `crates/indexer/src/db/repository.rs:61-68`
- Modify: `crates/indexer/src/sync/batch.rs:950`

**Step 1: Clean `update_sync_status()` in writer**

In `crates/indexer/src/db/writer/sync.rs`, remove lines 53, 59, 60 from the closure:

```rust
// DELETE:
            status.derived_tip_block_number = block_number;
            status.derived_last_synced_at = now;
            status.derived_sync_in_progress = false;
```

**Step 2: Clean cache `get_sync_status()` builder**

In `crates/indexer/src/cache.rs`, remove lines 63-64 from the `SyncStatusData` construction:

```rust
// DELETE:
            derived_last_synced_at: Some(sync.derived_last_synced_at),
            derived_sync_in_progress: sync.derived_sync_in_progress,
```

Also remove the `derived_tip_block_number` line (around line 57):

```rust
// DELETE:
            derived_tip_block_number: Some(sync.derived_tip_block_number),
```

**Step 3: Clean repository `update_sync_tip()` cache update**

In `crates/indexer/src/db/repository.rs`, remove lines 64-65, 67-68 from the cache closure:

```rust
// DELETE:
                    status.derived_tip_block_number = Some(block_number);
                    status.derived_last_synced_at = Some(status.last_synced_at);
                    status.derived_sync_in_progress = false;
```

**Step 4: Clean `check_bulk_sync_completion()`**

In `crates/indexer/src/sync/batch.rs`, remove line 950:

```rust
// DELETE:
                status.derived_sync_in_progress = false;
```

**Step 5: Verify compilation**

Run: `cargo check -p ckbadger-indexer`
Expected: PASS

**Step 6: Commit**

```
refactor: remove derived_* writes from indexer
```

---

### Task 4: Remove `ensure_derived_ready()` from API

**Files:**

- Delete: `crates/api/src/utils/derived.rs`
- Modify: `crates/api/src/utils/mod.rs:3,12`
- Modify: `crates/api/src/cache/mod.rs:56,62-63`
- Modify: 12 route files (remove import + call)

**Step 1: Delete `derived.rs` and remove from `mod.rs`**

Delete `crates/api/src/utils/derived.rs`.

In `crates/api/src/utils/mod.rs`, remove:

```rust
// DELETE line 3:
pub mod derived;
// DELETE line 12:
pub use derived::ensure_derived_ready;
```

**Step 2: Clean cache initialization**

In `crates/api/src/cache/mod.rs`, remove lines 56, 62-63 from the `SyncStatusData` construction:

```rust
// DELETE:
            derived_tip_block_number: Some(sync.derived_tip_block_number),
            derived_last_synced_at: Some(sync.derived_last_synced_at),
            derived_sync_in_progress: sync.derived_sync_in_progress,
```

**Step 3: Remove `ensure_derived_ready` from all route files**

For each file below, remove the `ensure_derived_ready` import and all `ensure_derived_ready(&state)?` / `ensure_derived_ready(state.as_ref())?` calls:

- `crates/api/src/routes/activities.rs` — import line 15, call line 248
- `crates/api/src/routes/transactions.rs` — import line 20, call line 606
- `crates/api/src/routes/statistics.rs` — import line 14, calls at lines 151, 171, 709, 863, 1076, 1105, 1165, 1308, 1733, 2025, 2063, 2103, 2493, 2541, 2587, 2653, 2760, 2893
- `crates/api/src/routes/search.rs` — import line 11, call line 250
- `crates/api/src/routes/scripts.rs` — import line 20, calls at lines 453, 574, 616, 727, 776, 1034, 1079
- `crates/api/src/routes/cells.rs` — import line 23, call line 1847
- `crates/api/src/routes/assets.rs` — find and remove import + calls
- `crates/api/src/routes/spore.rs` — find and remove import + calls
- `crates/api/src/routes/blocks.rs` — find and remove import + calls
- `crates/api/src/routes/tokens.rs` — find and remove import + calls
- `crates/api/src/routes/hardforks.rs` — find and remove import + calls
- `crates/api/src/routes/dao.rs` — find and remove import + calls

**Step 4: Verify compilation**

Run: `cargo check -p ckbadger-api`
Expected: PASS (possibly with unused import warnings — fix those)

**Step 5: Commit**

```
refactor: remove ensure_derived_ready API gate
```

---

### Task 5: Simplify TUI sync display

**Files:**

- Modify: `crates/tui/src/db.rs:42-48, 118-128, 150-178, 181-196, 390-445, 448-513, 601-631`
- Modify: `crates/tui/src/ui.rs:1291-1294, 2146-2172, 2811-2825`

**Step 1: Remove `derived_*` fields from `SyncStatusRow`**

In `crates/tui/src/db.rs`, remove lines 46-48:

```rust
// DELETE from SyncStatusRow:
    pub derived_tip_block: Option<i64>,
    pub derived_lag_blocks: Option<i64>,
    pub derived_sync_in_progress: bool,
```

**Step 2: Remove `derived_syncing` from `ApiServiceInfo`**

In `crates/tui/src/db.rs`, remove line 123:

```rust
// DELETE from ApiServiceInfo:
    pub derived_syncing: bool,
```

**Step 3: Delete helper functions**

Delete these functions entirely:

- `derive_sync_status_fields()` (lines 150-164)
- `response_indicates_derived_syncing()` (lines 181-196)

**Step 4: Simplify `sync_modes_from_progress()`**

Replace `sync_modes_from_progress()` (lines 166-178). Remove `derived_sync_in_progress` reference:

```rust
fn sync_modes_from_progress(
    _progress: &SyncProgressData,
    _status_data: Option<&SyncStatusData>,
    blocks_behind: i64,
) -> (bool, bool) {
    let is_syncing = blocks_behind > 0;
    let is_bulk_sync = blocks_behind > LEGACY_BULK_SYNC_THRESHOLD_BLOCKS;
    (is_syncing, is_bulk_sync)
}
```

**Step 5: Update `build_from_progress()`**

In `build_from_progress()`, use `max(status.tip_block_number, progress.current_block)` for `tip_block` to fix 10s staleness. Remove derived field assignments:

```rust
fn build_from_progress(
    &self,
    progress: &SyncProgressData,
    status_data: &Option<SyncStatusData>,
) -> SyncStatusRow {
    let progress_tip = progress.current_block as i64;
    let status_tip = status_data.as_ref().map(|s| s.tip_block_number).unwrap_or(0);
    let tip_block = progress_tip.max(status_tip);
    let chain_tip = progress.target_block as i64;
    let blocks_behind = chain_tip - tip_block;
    let (is_syncing, is_bulk_sync) =
        sync_modes_from_progress(progress, status_data.as_ref(), blocks_behind);

    // ... rest unchanged, but remove derived_tip_block, derived_lag_blocks,
    // derived_sync_in_progress from the SyncStatusRow construction
```

**Step 6: Update `build_from_status()`**

Remove derived field assignments and fix `is_bulk_sync`:

```rust
// Change is_bulk_sync from:
    is_bulk_sync: status.derived_sync_in_progress,
// To:
    is_bulk_sync: false, // not in bulk sync if progress loop is stale
```

Remove `derived_tip_block`, `derived_lag_blocks`, `derived_sync_in_progress` from the returned struct.

**Step 7: Clean API health check**

In `get_chain_info_and_api_service_info()` (lines 621-631), remove `derived_syncing` handling:

```rust
// Replace lines 625-630:
        if !response.status().is_success() {
            let status_text = response.status().to_string();
            api_info.error = Some(format!("http {}", status_text));
            return (None, api_info);
        }
```

**Step 8: Clean `api_health_state()`**

In `crates/tui/src/ui.rs` (lines 2815-2816), remove the `derived_syncing` check:

```rust
// DELETE:
    if info.derived_syncing {
        return ("DEGRADED", CYAN);
    }
```

**Step 9: Remove `derived_status_line()` call and function**

In `crates/tui/src/ui.rs`:

- Remove the call (lines 1291-1295):

```rust
// DELETE:
        derived_status_line(
            sync.derived_tip_block,
            sync.derived_lag_blocks,
            sync.derived_sync_in_progress,
        ),
```

- Delete the `derived_status_line()` function definition (lines 2146-2172)

**Step 10: Verify compilation**

Run: `cargo check -p ckbadger-tui`
Expected: PASS

**Step 11: Commit**

```
refactor: remove Derived line from TUI, fix Current staleness
```

---

### Task 6: Clean CLI output

**Files:**

- Modify: `crates/cli/src/main.rs:384`

**Step 1: Remove derived tip block line from CLI status**

Remove line 384:

```rust
// DELETE:
                println!("  Derived tip block:   {}", status.derived_tip_block_number);
```

**Step 2: Verify compilation**

Run: `cargo check -p ckbadger`
Expected: PASS

**Step 3: Commit**

```
refactor: remove derived tip block from CLI status output
```

---

### Task 7: Update all tests

**Files:**

- Modify: `crates/common/src/sync.rs` (test module)
- Modify: `crates/indexer/tests/dao_deferred.rs:20-26, 54-60`
- Modify: `crates/tui/src/db.rs` (test module, lines 720-843)
- Modify: `crates/tui/src/ui.rs` (test module, lines 3979, 4384-4396)
- Delete: 22 `*_returns_503_when_derived_store_lags` test functions from `crates/api/tests/api_integration.rs`
- Modify: remaining test data in `api_integration.rs` that sets `derived_tip_block_number`

**Step 1: Fix `common/sync.rs` tests**

Remove `derived_*` fields from test data (lines 344, 350-351) and remove `derived_sync_in_progress` assertion (line 429).

**Step 2: Fix `dao_deferred.rs` tests**

Remove `derived_tip_block_number`, `derived_last_synced_at`, `derived_sync_in_progress` from both `SyncStatus` literals (lines 20-26, 54-60).

**Step 3: Delete 22 `*_derived_store_lags` integration tests**

Delete all test functions matching `*_returns_503_when_derived_store_lags` from `crates/api/tests/api_integration.rs`. These are at lines: 186, 326, 350, 717, 741, 981, 1005, 1129, 1365, 1599, 1623, 1647, 1671, 2281, 2321, 2821, 3560, 4005, 4124, 4286, 4884, 4908, 6758, 7283.

**Step 4: Fix remaining `derived_tip_block_number` in test data**

In `api_integration.rs`, find remaining test SyncStatus constructions that set `derived_tip_block_number` (lines 1412, 1475, 6832, 6876, 7334) and remove the field.

**Step 5: Fix TUI tests**

In `crates/tui/src/db.rs`:

- Delete test `derive_sync_status_fields_maps_lag_and_progress` (around line 783)
- Delete test `derive_sync_status_fields_handles_missing_status` (around line 797)
- Delete test `response_indicates_derived_syncing_detects_marker` (around line 831)
- Remove `derived_*` fields from test data in remaining TUI tests
- Remove imports of deleted functions from test module (line 720)

In `crates/tui/src/ui.rs`:

- Delete test `test_derived_status_line_ready` (around line 4384)
- Delete test `test_derived_status_line_syncing` (around line 4393)
- Remove `derived_status_line` from test import (line 3979)
- Remove `derived_syncing: true` from test data (line 4600)

**Step 6: Run all tests**

Run: `cargo test`
Expected: All pass

Run: `cargo clippy`
Expected: No warnings

**Step 7: Commit**

```
test: update tests for derived_* field removal
```
