# Eliminate Full CF Scans — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove all unnecessary full column-family scans from the API and store layers with zero bulk sync performance penalty.

**Architecture:** Six independent tasks: delete dead code (3 store methods + 1 backfill path), add skip-if-tip-unchanged to warmup loop, cache block-date transitions in warmup, merge two live-cell chart scans into one warmup pass, piggyback address cohort data onto existing warmup addr_balance scan.

**Tech Stack:** Rust, RocksDB, Axum, tokio

---

### Task 1: Delete dead store methods

Three store methods have zero callers outside their own test modules: `top_addresses()`, `block_headers_count()`, `live_cells_count()`.

**Files:**

- Modify: `crates/ckbadger-store/src/address_ops.rs:35-57` — delete `top_addresses()`
- Modify: `crates/ckbadger-store/src/address_ops.rs:127-144` — delete `test_top_addresses_fails_on_invalid_payload`
- Modify: `crates/ckbadger-store/src/block_ops.rs:165-178` — delete `block_headers_count()`
- Modify: `crates/ckbadger-store/src/cell_ops.rs:511-521` — delete `live_cells_count()`
- Modify: `crates/indexer/tests/address_balances.rs:130-174` — delete `test_top_addresses_sorted_and_truncated` test

**Step 1:** Delete `top_addresses()` method from `address_ops.rs:35-57` and its test `test_top_addresses_fails_on_invalid_payload` at `address_ops.rs:127-144`.

**Step 2:** Delete `block_headers_count()` from `block_ops.rs:165-178`.

**Step 3:** Delete `live_cells_count()` from `cell_ops.rs:511-521`.

**Step 4:** Delete `test_top_addresses_sorted_and_truncated` from `crates/indexer/tests/address_balances.rs:130-174`.

**Step 5:** Run: `cargo check -p ckbadger-store -p ckbadger-indexer`
Expected: PASS with no compilation errors.

**Step 6:** Run: `cargo test -p ckbadger-store -p ckbadger-indexer --lib`
Expected: PASS.

**Step 7:** Commit: `refactor(store): delete dead full-CF-scan methods`

---

### Task 2: Delete backfill_code_hash_indexes and code_hash_indexes_populated

Dead migration code. Per BULK_SYNC.md rule 4, all data must be inline. Code hash indexes are written inline during sync. No backward-compat backfill path needed.

**Files:**

- Modify: `crates/ckbadger-store/src/cell_ops.rs:523-593` — delete both methods
- Modify: `crates/indexer/src/entry.rs:103-108` — delete the startup backfill block

**Step 1:** Delete `backfill_code_hash_indexes()` (cell_ops.rs:523-579) and `code_hash_indexes_populated()` (cell_ops.rs:581-593).

**Step 2:** Delete the startup backfill block from `entry.rs:103-108`:

```rust
    // One-time backfill: populate code_hash indexes if they are empty
    if !store.code_hash_indexes_populated() {
        info!("Code hash indexes empty -- running one-time backfill from live_cells...");
        let count = store.backfill_code_hash_indexes(&append_only_store)?;
        info!("Code hash index backfill complete: {} cells indexed", count);
    }
```

**Step 3:** Run: `cargo check -p ckbadger-store -p ckbadger-indexer`
Expected: PASS.

**Step 4:** Run: `cargo test -p ckbadger-store -p ckbadger-indexer --lib`
Expected: PASS.

**Step 5:** Commit: `refactor(store): delete dead backfill_code_hash_indexes migration path`

---

### Task 3: Skip warmup refresh when sync tip unchanged

The warmup loop runs every 30s and scans 7+ CFs unconditionally. When the indexer is idle or paused, these scans are pure waste.

**Files:**

- Modify: `crates/api/src/warmup.rs:186-206` — `refresh_assets_cache_loop`
- Modify: `crates/api/src/warmup.rs:242-337` — `refresh_address_cache_sync`
- Modify: `crates/api/src/warmup.rs:377-680` — `refresh_assets_cache_sync`

**Step 1:** Add a `last_refreshed_tip: std::sync::Mutex<i64>` to track the last tip we refreshed for. Store it as a local variable in the loop (not on AppState), initialized to `-1`.

In `refresh_assets_cache_loop`, before calling `refresh_assets_cache_sync`:

```rust
pub async fn refresh_assets_cache_loop(state: Arc<AppState>) {
    let mut last_refreshed_tip: i64 = -1;
    loop {
        let current_tip = state
            .store
            .get_sync_status()
            .map(|s| s.tip_block_number)
            .unwrap_or(-1);

        if current_tip == last_refreshed_tip {
            tracing::trace!("Warmup: tip unchanged at {}, skipping refresh", current_tip);
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }

        let state_clone = state.clone();
        let result =
            tokio::task::spawn_blocking(move || refresh_assets_cache_sync(&state_clone)).await;

        match result {
            Ok(Ok(())) => {
                last_refreshed_tip = current_tip;
                tracing::debug!("Assets cache refreshed at tip {}", current_tip);
            }
            // ... existing error handling unchanged ...
        }

        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
```

**Step 2:** Run: `cargo check -p ckbadger-api`
Expected: PASS.

**Step 3:** Run: `cargo test -p ckbadger-api --lib`
Expected: PASS.

**Step 4:** Commit: `perf(api): skip warmup refresh when sync tip unchanged`

---

### Task 4: Cache block_date_transitions in warmup

`load_block_date_transitions()` in statistics.rs has a fast path via `get_hodl_tracker_state()`, but falls back to a full `cf_block_headers` scan. Two chart endpoints call it independently. Cache the result in mem_cache so the fallback scan runs at most once.

**Files:**

- Modify: `crates/api/src/routes/statistics.rs:1488-1528` — `load_block_date_transitions`

**Step 1:** Add a mem_cache constant and modify `load_block_date_transitions` to accept `&AppState` (or store + mem_cache) and check/populate cache:

The simplest approach: wrap `load_block_date_transitions` to cache its result using the existing `CacheBackend`. Add a cache check at the top of the function. The function is called from `get_cell_age_vs_occupied_capacity_chart` and `get_address_cohort_retention_chart`.

Modify `load_block_date_transitions` signature to accept `state: &AppState` instead of `store: &CkbadgerStore`:

```rust
const CACHE_KEY_DATE_TRANSITIONS: &str = "internal:block-date-transitions";

async fn load_block_date_transitions_cached(
    state: &AppState,
) -> Result<Vec<(i64, NaiveDate)>, String> {
    if let Some(cached) = state.mem_cache.get::<Vec<(i64, String)>>(CACHE_KEY_DATE_TRANSITIONS) {
        let transitions: Vec<(i64, NaiveDate)> = cached
            .into_iter()
            .filter_map(|(block, date_str)| {
                NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                    .ok()
                    .map(|date| (block, date))
            })
            .collect();
        if !transitions.is_empty() {
            return Ok(transitions);
        }
    }

    let transitions = load_block_date_transitions(state.store.as_ref())?;

    // Cache as serializable (i64, String) pairs
    let cacheable: Vec<(i64, String)> = transitions
        .iter()
        .map(|(block, date)| (*block, date.format("%Y-%m-%d").to_string()))
        .collect();
    state.mem_cache.set(
        CACHE_KEY_DATE_TRANSITIONS,
        &cacheable,
        CacheTtl::CHART,
    );

    Ok(transitions)
}
```

Update callers (`get_cell_age_vs_occupied_capacity_chart` at line 1634 and `get_address_cohort_retention_chart` at line 1868) to call `load_block_date_transitions_cached(&state)` instead.

NOTE: Since `mem_cache.set/get` is sync and `load_block_date_transitions` is sync, this works inside `spawn_blocking` or directly. The chart handlers are async so we can call from there.

**Step 2:** Run: `cargo check -p ckbadger-api`
Expected: PASS.

**Step 3:** Run: `cargo test -p ckbadger-api --lib`
Expected: PASS.

**Step 4:** Commit: `perf(api): cache block_date_transitions to avoid repeated header scans`

---

### Task 5: Single-pass live-cell chart warmup

Two chart endpoints independently do full `cf_live_cells` scans on cache miss: `get_cell_age_vs_occupied_capacity_chart` and `get_cell_size_distribution_chart`. Merge into a single warmup pass that computes both.

**Files:**

- Modify: `crates/api/src/warmup.rs` — add `refresh_live_cell_charts_sync` function
- Modify: `crates/api/src/warmup.rs:689-748` — add live-cell charts to `warmup_chart_caches`
- Modify: `crates/api/src/routes/statistics.rs` — import needed types/helpers, keep chart handlers reading from cache

**Step 1:** Add a new function in `warmup.rs` that does a single `visit_live_cells_in_batches` pass and computes both chart payloads:

```rust
use crate::routes::statistics::{
    load_block_date_transitions, block_number_to_date, current_snapshot_date,
    occupied_capacity_bucket_index, shannon_to_ckb_string,
    StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries,
    ChartResponse, ChartDataPoint,
    visit_live_cells_in_batches,
};
```

The function will:

1. Load block_date_transitions (cached from Task 4)
2. Single `visit_live_cells_in_batches` pass, accumulating:
   - Cell age buckets (lt_1d, d1_7d, d7_30d, d30_180d, gt_180d) for occupied capacity
   - Cell size buckets (6 buckets) for count and occupied capacity
3. Build both chart responses
4. Write both to `state.cache` with `CacheTtl::CHART`

Make helpers in `statistics.rs` that need to be called from warmup `pub(crate)`: `load_block_date_transitions`, `block_number_to_date`, `current_snapshot_date`, `occupied_capacity_bucket_index`, `shannon_to_ckb_string`, `visit_live_cells_in_batches`. Also the response types `StackedAreaChartResponse`, `StackedAreaDataPoint`, `StackedAreaSeries`, `ChartResponse`, `ChartDataPoint`.

**Step 2:** Add live-cell chart warmup to `warmup_chart_caches()`:

```rust
// In warmup_chart_caches, add before the tokio::join!:
{
    let state_clone = state.clone();
    match tokio::task::spawn_blocking(move || refresh_live_cell_charts_sync(&state_clone)).await {
        Ok(Ok(())) => info!("Warmed up live-cell chart caches"),
        Ok(Err(e)) => tracing::warn!("Failed to warmup live-cell charts: {}", e),
        Err(e) => tracing::warn!("Live-cell chart warmup panicked: {}", e),
    }
}
```

**Step 3:** Ensure chart handlers still check cache first (they already do). No changes needed to the handlers themselves — they already do `state.cache.get()` and return early on hit. The warmup just pre-populates them.

**Step 4:** Check which helpers in `statistics.rs` need visibility changes. Functions currently private that need `pub(crate)`:

- `load_block_date_transitions` (line 1488)
- `block_number_to_date` (line 1530)
- `current_snapshot_date` (line 1543)
- `visit_live_cells_in_batches` (line 1554)
- `occupied_capacity_bucket_index` (line 1602)
- `shannon_to_ckb_string` — check current visibility
- Response types: `StackedAreaChartResponse`, `StackedAreaDataPoint`, `StackedAreaSeries`, `ChartResponse`, `ChartDataPoint` — check current visibility

**Step 5:** Run: `cargo check -p ckbadger-api`
Expected: PASS.

**Step 6:** Run: `cargo test -p ckbadger-api`
Expected: PASS.

**Step 7:** Commit: `perf(api): single-pass warmup for cell-age and cell-size charts`

---

### Task 6: Piggyback address cohort retention on warmup addr_balance scan

`get_address_cohort_retention_chart` does a full `cf_addr_balance` scan on cache miss. The warmup already scans `cf_addr_balance` every 30s for address ranking. Piggyback cohort collection during that existing scan.

**Files:**

- Modify: `crates/api/src/warmup.rs:242-337` — `refresh_address_cache_sync`
- Modify: `crates/api/src/routes/statistics.rs:1859-1935` — `get_address_cohort_retention_chart`

**Step 1:** In `refresh_address_cache_sync`, alongside the existing by_balance/by_activity heaps, also collect cohort data:

```rust
// Add to refresh_address_cache_sync, alongside existing heap logic:
let mut cohorts: BTreeMap<String, (i128, i128)> = BTreeMap::new();
// Need transitions — load from mem_cache or compute
let transitions: Vec<(i64, String)> = state
    .mem_cache
    .get::<Vec<(i64, String)>>(CACHE_KEY_DATE_TRANSITIONS)
    .unwrap_or_default();
let parsed_transitions: Vec<(i64, NaiveDate)> = transitions
    .iter()
    .filter_map(|(block, date_str)| {
        NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .ok()
            .map(|date| (*block, date))
    })
    .collect();
```

Then inside the existing `for item in iter` loop, after creating the candidate:

```rust
if !parsed_transitions.is_empty() {
    if let Some(first_seen_date) = block_number_to_date(&parsed_transitions, balance.first_seen_block) {
        let cohort = first_seen_date.format("%Y-%m").to_string();
        let entry = cohorts.entry(cohort).or_insert((0, 0));
        entry.0 += balance.occupied_capacity;
        entry.1 += balance.balance;
    }
}
```

After the loop, cache the cohort data:

```rust
state.mem_cache.set(
    "internal:address-cohort-data",
    &cohorts,
    CacheTtl::ADDRESS_BALANCE,
);
```

**Step 2:** In `get_address_cohort_retention_chart`, check mem_cache first for pre-computed cohort data. If present, build response from cached cohorts instead of scanning `cf_addr_balance` again:

```rust
// At the top of get_address_cohort_retention_chart, after the cache.get check:
if let Some(cohorts) = state.mem_cache.get::<BTreeMap<String, (i128, i128)>>("internal:address-cohort-data") {
    // Build chart response from cached cohorts (same logic as existing, just sourced from cache)
    // ...
    state.cache.set(cache_key, &response, CacheTtl::CHART).await;
    return ok(response);
}
```

Keep the existing full-scan as fallback for when warmup hasn't run yet. This way the chart handler never triggers its own `cf_addr_balance` scan after first warmup cycle.

**Step 3:** Import `block_number_to_date` in warmup.rs (from `statistics.rs`, made `pub(crate)` in Task 5).

**Step 4:** Run: `cargo check -p ckbadger-api`
Expected: PASS.

**Step 5:** Run: `cargo test -p ckbadger-api`
Expected: PASS.

**Step 6:** Commit: `perf(api): piggyback address cohort data on warmup addr_balance scan`

---

## Verification

After all tasks, run full check:

```bash
cargo check && cargo clippy && cargo test --lib
cd frontend && pnpm type-check && pnpm lint
```

## Summary of scans eliminated

| Before                                                         | After                                      |
| -------------------------------------------------------------- | ------------------------------------------ |
| `top_addresses()` full cf_addr_balance scan                    | Deleted (dead code)                        |
| `block_headers_count()` full cf_block_headers scan             | Deleted (dead code)                        |
| `live_cells_count()` full cf_live_cells scan                   | Deleted (dead code)                        |
| `backfill_code_hash_indexes()` full cf_live_cells scan         | Deleted (dead migration)                   |
| `code_hash_indexes_populated()` cf_cell_by_lock_code seek      | Deleted (dead migration)                   |
| Warmup 7-CF scan every 30s even when idle                      | Skipped when tip unchanged                 |
| `load_block_date_transitions()` fallback header scan per chart | Cached in mem_cache, computed at most once |
| Two independent full cf_live_cells scans for charts            | Single warmup pass computes both           |
| `get_address_cohort_retention_chart` full cf_addr_balance scan | Piggybacked on existing warmup scan        |
