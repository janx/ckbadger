# Remove Derived Fields Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove dead `derived_*` fields from sync status types and all downstream consumers, then clean up the TUI sync display.

**Architecture:** Bottom-up removal across 6 layers: store types → common types → indexer writers → API gates → TUI display → CLI output. Each layer compiles independently after its changes. The `derived_*` fields were designed for a store-lag scenario that never occurs (both stores write in the same batch).

**Tech Stack:** Rust (serde, rocksdb, axum), ratatui TUI

---

### Task 1: Remove derived fields from store SyncStatus

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs:728-776`

**Step 1: Remove 3 fields from SyncStatus struct**

Remove these fields from the struct definition:

```rust
// DELETE these 3 fields (and their #[serde(default)] annotations):
pub derived_tip_block_number: i64,        // line 732
pub derived_last_synced_at: i64,          // line 738
pub derived_sync_in_progress: bool,       // line 740
```

**Step 2: Remove derived lines from SyncStatus::init_sync_start()**

In `init_sync_start()` (line 756-776), remove:

- `self.derived_sync_in_progress = true;` (line 767, in bulk sync branch)
- `self.derived_sync_in_progress = false;` (line 774, in non-bulk branch)

**Step 3: Verify compile**

Run: `cargo check -p ckbadger-store`
Expected: PASS (store crate compiles standalone). Downstream crates will fail — that's expected.

**Step 4: Commit**

```
refactor: remove derived_* fields from store SyncStatus
```

---

### Task 2: Remove derived fields from common SyncStatusData

**Files:**

- Modify: `crates/common/src/sync.rs:10-113`

**Step 1: Remove 3 fields from SyncStatusData struct**

Remove from struct definition (lines 15, 22, 24):

```rust
// DELETE (with their #[serde(default)] annotations):
pub derived_tip_block_number: Option<i64>,
pub derived_last_synced_at: Option<i64>,
pub derived_sync_in_progress: bool,
```

**Step 2: Clean update_batch()**

Remove from `update_batch()` (lines 54, 60, 61):

```rust
// DELETE:
self.derived_tip_block_number = Some(block_number);
self.derived_last_synced_at = Some(self.last_synced_at);
self.derived_sync_in_progress = false;
```

**Step 3: Clean init_sync_start()**

Remove from `init_sync_start()` (lines 82, 89):

```rust
// DELETE:
self.derived_sync_in_progress = true;   // in bulk sync branch
self.derived_sync_in_progress = false;  // in non-bulk branch
```

**Step 4: Update tests in same file**

- `test_sync_status_serialization` (line 342): Remove `derived_tip_block_number`, `derived_last_synced_at`, `derived_sync_in_progress` from test fixture
- `test_init_sync_start_non_bulk_initializes_started_at_when_missing` (line 431): Remove `assert!(!status.derived_sync_in_progress);`

**Step 5: Verify compile and test**

Run: `cargo check -p ckbadger-common && cargo test -p ckbadger-common`
Expected: PASS

**Step 6: Commit**

```
refactor: remove derived_* fields from common SyncStatusData
```

---

### Task 3: Remove derived writes from indexer

**Files:**

- Modify: `crates/indexer/src/db/writer/sync.rs:38-87`
- Modify: `crates/indexer/src/db/repository.rs:55-73`
- Modify: `crates/indexer/src/cache.rs:54-70`
- Modify: `crates/indexer/src/sync/batch.rs:957-960`
- Modify: `crates/indexer/tests/dao_deferred.rs:17-68`

**Step 1: Clean writer/sync.rs update_sync_status()**

In `update_sync_status()`, remove from the store closure (lines 53, 59, 60):

```rust
// DELETE:
status.derived_tip_block_number = block_number;
status.derived_last_synced_at = now;
status.derived_sync_in_progress = false;
```

**Step 2: Clean db/repository.rs update_sync_tip()**

Remove from cache update closure (lines 64, 67, 68):

```rust
// DELETE:
status.derived_tip_block_number = Some(block_number);
status.derived_last_synced_at = Some(status.last_synced_at);
status.derived_sync_in_progress = false;
```

**Step 3: Clean cache.rs get_sync_status() builder**

Remove from SyncStatusData construction (lines 57, 63, 64):

```rust
// DELETE:
derived_tip_block_number: Some(sync.derived_tip_block_number),
derived_last_synced_at: Some(sync.derived_last_synced_at),
derived_sync_in_progress: sync.derived_sync_in_progress,
```

**Step 4: Clean sync/batch.rs check_bulk_sync_completion()**

Remove line 959:

```rust
// DELETE:
status.derived_sync_in_progress = false;
```

**Step 5: Update dao_deferred.rs test fixtures**

Remove `derived_tip_block_number`, `derived_last_synced_at`, `derived_sync_in_progress` from both SyncStatus fixtures (lines 20-26 and 54-60).

**Step 6: Update writer/sync.rs test**

In `test_init_sync_start_persists_bulk_sync_start_metadata` (line 656):

```rust
// DELETE:
assert!(status.derived_sync_in_progress);
```

**Step 7: Verify compile and test**

Run: `cargo check -p ckbadger-indexer && cargo test -p ckbadger-indexer --lib`
Expected: PASS

**Step 8: Commit**

```
refactor: remove derived_* writes from indexer
```

---

### Task 4: Remove ensure_derived_ready from API

**Files:**

- Delete: `crates/api/src/utils/derived.rs`
- Modify: `crates/api/src/utils/mod.rs`
- Modify: `crates/api/src/cache/mod.rs:53-69`
- Modify: 12 route files in `crates/api/src/routes/`
- Modify: `crates/api/tests/api_integration.rs`

**Step 1: Delete derived.rs**

Delete the file `crates/api/src/utils/derived.rs`.

**Step 2: Clean utils/mod.rs**

Remove:

```rust
pub mod derived;                          // line 3
pub use derived::ensure_derived_ready;    // line 12
```

**Step 3: Clean cache/mod.rs**

Remove from SyncStatusData construction (lines 56, 62, 63):

```rust
// DELETE:
derived_tip_block_number: Some(sync.derived_tip_block_number),
derived_last_synced_at: Some(sync.derived_last_synced_at),
derived_sync_in_progress: sync.derived_sync_in_progress,
```

**Step 4: Remove ensure_derived_ready from all route files**

For each route file, remove `ensure_derived_ready` from imports and all call sites.

Route files and call counts:

- `activities.rs`: 1 call
- `assets.rs`: 6 calls
- `blocks.rs`: 2 calls
- `cells.rs`: 4 calls
- `dao.rs`: 5 calls
- `hardforks.rs`: 1 call
- `scripts.rs`: 7 calls
- `search.rs`: 1 call
- `spore.rs`: 6 calls
- `statistics.rs`: 16 calls
- `tokens.rs`: 1 call
- `transactions.rs`: 1 call

**Step 5: Delete 24 derived_store_lags integration tests**

Delete all test functions in `api_integration.rs` whose names contain `derived_store_lags`. There are 24 of them. Each is a self-contained `#[tokio::test]` function.

**Step 6: Verify compile and test**

Run: `cargo check -p ckbadger-api && cargo test -p ckbadger-api`
Expected: PASS

**Step 7: Commit**

```
refactor: remove ensure_derived_ready gate from API routes
```

---

### Task 5: Clean TUI derived display

**Files:**

- Modify: `crates/tui/src/db.rs`
- Modify: `crates/tui/src/ui.rs`

**Step 1: Remove derived fields from SyncStatusRow (db.rs)**

Remove from struct (lines 46-48):

```rust
// DELETE:
pub derived_tip_block: Option<i64>,
pub derived_lag_blocks: Option<i64>,
pub derived_sync_in_progress: bool,
```

**Step 2: Remove derived_syncing from ApiServiceInfo (db.rs)**

Remove field (line 123):

```rust
// DELETE:
pub derived_syncing: bool,
```

**Step 3: Delete derive_sync_status_fields() (db.rs)**

Delete the entire function (lines 150-164).

**Step 4: Delete response_indicates_derived_syncing() (db.rs)**

Delete the entire function (lines 181-197).

**Step 5: Refactor sync_modes_from_progress() (db.rs)**

Replace the function body (lines 166-179) with pure blocks_behind logic:

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

**Step 6: Clean build_from_progress() (db.rs)**

- Remove the 3-tuple destructure and derived field assignments (lines 400-401, 415-417)
- Use `max(progress.current_block, status.tip_block_number)` for `tip_block` to fix 10s staleness

**Step 7: Clean build_from_status() (db.rs)**

- Remove the 3-tuple destructure (lines 451-452)
- Change `is_syncing` (line 455): `let is_syncing = blocks_behind > 0;`
- Change `is_bulk_sync` (line 489): `is_bulk_sync: false,` (not in bulk sync if progress loop is stale)
- Remove derived field assignments from SyncStatusRow constructor (lines 485-487)

**Step 8: Clean get_sync_status_from_store() (db.rs)**

Remove derived fields from SyncStatusData construction (lines 374, 380, 381).

**Step 9: Clean get_chain_info_and_api_service_info() (db.rs)**

Remove response_indicates_derived_syncing call and derived_syncing handling (lines 628-631). Replace the if/else with just:

```rust
api_info.error = Some(format!("http {}", status_text));
```

**Step 10: Delete derived_status_line() from ui.rs**

Delete the entire function (lines 2146-2177).

**Step 11: Remove derived_status_line() call from ui.rs**

Remove the call at lines 1291-1295:

```rust
// DELETE:
derived_status_line(
    sync.derived_tip_block,
    sync.derived_lag_blocks,
    sync.derived_sync_in_progress,
),
```

**Step 12: Clean api_health_state() in ui.rs**

Remove the derived_syncing check (lines 2750-2752):

```rust
// DELETE:
if info.derived_syncing {
    return ("DEGRADED", CYAN);
}
```

**Step 13: Update db.rs tests**

- Remove `derive_sync_status_fields` and `response_indicates_derived_syncing` from test imports (line 723)
- Delete test `derive_sync_status_fields_maps_lag_and_progress` (lines 786-797)
- Delete test `derive_sync_status_fields_handles_missing_status` (lines 800-805)
- Update `sync_modes_from_progress_falls_back_to_status_or_legacy_lag` (lines 816-831): remove the `derived_sync_in_progress: true` status_hint branch, test only the legacy lag branch
- Delete test `response_indicates_derived_syncing_detects_marker` (lines 834-848)
- Remove `derived_tip_block_number`, `derived_last_synced_at`, `derived_sync_in_progress` from SyncStatus test fixtures (lines 912, 917-918, 973, 978-979)

**Step 14: Update ui.rs tests**

- Remove `derived_status_line` from test imports (line 3913)
- Delete test `test_derived_status_line_ready` (lines 4319-4325)
- Delete test `test_derived_status_line_syncing` (lines 4328-4335)
- Update api_health_state DEGRADED test (line 4532-4541): remove `derived_syncing: true` from fixture. With derived_syncing gone, a 503 is just WARN. Delete the DEGRADED test case since it's now redundant with the existing `warn_http` test case.

**Step 15: Verify compile and test**

Run: `cargo check -p ckbadger-tui && cargo test -p ckbadger-tui`
Expected: PASS

**Step 16: Commit**

```
refactor: remove derived display from TUI, simplify sync mode detection
```

---

### Task 6: Remove derived from CLI output and final verification

**Files:**

- Modify: `crates/cli/src/main.rs:384`

**Step 1: Remove the derived tip block print line**

Delete line 384:

```rust
// DELETE:
println!("  Derived tip block:   {}", status.derived_tip_block_number);
```

**Step 2: Verify full build and tests**

Run: `cargo test`
Expected: All tests pass

**Step 3: Grep for remaining references**

Run: `rg 'derived_tip_block|derived_last_synced|derived_sync_in_progress|ensure_derived_ready|derived_syncing|derived_status_line|derive_sync_status_fields|response_indicates_derived' --type rust`
Expected: No matches

**Step 4: Delete outdated design doc**

Delete `docs/plans/2026-03-08-remove-derived-fields-design.md`.

**Step 5: Commit**

```
refactor: remove derived tip from CLI, delete outdated design doc
```
