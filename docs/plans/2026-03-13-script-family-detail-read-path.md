# Script Family Detail Read Path Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make script detail reads family-consistent by guaranteeing code-cell resolution for unresolved bytecode families and expanding compressed capacity-history charts into continuous deployment-family time series.

**Architecture:** Keep all persistent stores unchanged. Fix the API read path in `crates/api/src/routes/scripts.rs` so both code-cell lookup and capacity-history aggregation operate on one deployment-family resolution flow. Use the direct CKB RocksDB reader only as a fallback for unresolved `data`/`data1`/`data2` families, and expand missing chart days as zero-delta carry-forward days up to the latest complete indexed UTC day.

**Tech Stack:** Rust, Axum, ckbadger-store domain/append-only stores, ckb-store-reader, inline Rust unit tests, API integration tests.

---

### Task 1: Add regression tests for script code-cell fallback

**Files:**

- Modify: `crates/api/src/routes/scripts.rs`
- Test: `crates/api/src/routes/scripts.rs`

**Step 1: Write the failing test**

Add inline tests around an extracted helper so the fallback behavior is testable without a live HTTP server.

Target behaviors:

```rust
#[test]
fn resolve_code_cell_prefers_type_lookup_before_data_hash_fallback() {
    // family has type ref; direct data-hash fallback must not be consulted
}

#[test]
fn resolve_code_cell_falls_back_to_ckb_data_hash_lookup_for_unresolved_data_family() {
    // family has hash_type=data, no dep_type_hash, no cached outpoint,
    // direct reader returns tx/index, helper resolves that outpoint
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-api resolve_code_cell_falls_back_to_ckb_data_hash_lookup_for_unresolved_data_family -- --nocapture`

Expected: FAIL because the helper or fallback path does not exist yet.

**Step 3: Write minimal implementation**

In `crates/api/src/routes/scripts.rs`, extract code-cell lookup into a helper that accepts:

- `&ScriptInfo`
- domain store
- append-only store
- optional `&CkbChainReader`

Implement fallback order:

```rust
fn resolve_code_cell_with_fallback(
    info: &ScriptInfo,
    store: &CkbadgerStore,
    cells_store: &CkbadgerStore,
    ckb_store: Option<&ckb_store_reader::CkbChainReader>,
) -> anyhow::Result<(Option<String>, Option<i32>)> {
    if let Some(type_hash) = usable_type_hash(info) {
        if let Some((tx_hash, idx, _)) = first_type_cell(store, cells_store, type_hash)? {
            return Ok((Some(format!("0x{}", hex::encode(tx_hash))), Some(idx as i32)));
        }
    }

    if let Some((tx_hash, idx)) = cached_code_cell(info) {
        return Ok((Some(format!("0x{}", hex::encode(tx_hash))), Some(idx as i32)));
    }

    if is_bytecode_hash_family(info.hash_type) {
        if let Some(reader) = ckb_store {
            let data_hash = info.code_hash.as_slice();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(data_hash);
            if let Some((tx_hash, idx)) = reader.find_cell_by_data_hash(&hash) {
                return Ok((Some(format!("0x{}", hex::encode(tx_hash))), Some(idx as i32)));
            }
        }
    }

    Ok((None, None))
}
```

Update route call sites to use the new helper and pass `state.ckb_store.as_deref()`.

**Step 4: Run test to verify it passes**

Run: `cargo test -p ckbadger-api resolve_code_cell_ -- --nocapture`

Expected: PASS for the new resolver tests.

**Step 5: Commit**

```bash
git add crates/api/src/routes/scripts.rs
git commit -m "fix: resolve script code cells by family"
```

### Task 2: Add regression tests for capacity-history expansion

**Files:**

- Modify: `crates/api/src/routes/scripts.rs`
- Test: `crates/api/src/routes/scripts.rs`

**Step 1: Write the failing test**

Add inline tests for an extracted chart-bounds helper and chart builder behavior.

Target behaviors:

```rust
#[test]
fn script_capacity_history_extends_single_delta_day_to_latest_complete_indexed_day() {
    // one stored delta day, known latest complete day several days later
    // chart data includes all intermediate days with carried-forward values
}

#[test]
fn script_capacity_history_respects_explicit_to_date_without_auto_extension() {
    // explicit range should not be extended beyond requested bound
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-api script_capacity_history_extends_single_delta_day_to_latest_complete_indexed_day -- --nocapture`

Expected: FAIL because chart end currently stops at the last stored delta day.

**Step 3: Write minimal implementation**

In `crates/api/src/routes/scripts.rs`:

- extract a helper to compute the latest complete indexed UTC day from `state.store.get_sync_tip_block()`,
- thread that value into `build_script_capacity_history_chart`,
- when `from/to` are absent, compute chart bounds as:

```rust
let first = daily_deltas.keys().next().copied();
let last = latest_complete_indexed_day.or_else(|| daily_deltas.keys().next_back().copied());
let chart_bounds = first.zip(last);
```

- keep explicit `from` / `to` requests authoritative,
- keep zero-delta carry-forward semantics by continuing to use `date_keys_inclusive(...)` with `unwrap_or((0, 0))` daily values.

**Step 4: Run test to verify it passes**

Run: `cargo test -p ckbadger-api script_capacity_history_ -- --nocapture`

Expected: PASS for the new chart tests.

**Step 5: Commit**

```bash
git add crates/api/src/routes/scripts.rs
git commit -m "fix: expand script family capacity history charts"
```

### Task 3: Add route-level regression coverage

**Files:**

- Modify: `crates/api/tests/api_integration.rs`
- Test: `crates/api/tests/api_integration.rs`

**Step 1: Write the failing test**

Add integration coverage for:

- `GET /api/v1/scripts/code-cell?code_hash=<unknown-data-hash>&hash_type=data`
- `GET /api/v1/scripts/charts/capacity-history?code_hash=<family-hash>`

The test fixture should seed:

- one unresolved bytecode-family `ScriptInfo`,
- one live code cell discoverable by data hash through the direct reader seam or helper seam used in tests,
- one single stored daily delta plus a later synced tip day.

Prefer testing extracted seams if the full integration harness cannot host a direct CKB reader deterministically.

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-api api_integration -- --nocapture`

Expected: the new assertions fail before implementation is wired through the route.

**Step 3: Write minimal implementation**

Finish any route wiring still missing after Tasks 1 and 2:

- `get_code_cell` uses the family-aware resolver with direct-reader fallback,
- capacity-history by code hash uses the extended chart logic for family-related hashes.

**Step 4: Run test to verify it passes**

Run: `cargo test -p ckbadger-api api_integration -- --nocapture`

Expected: PASS for the new regression coverage.

**Step 5: Commit**

```bash
git add crates/api/tests/api_integration.rs crates/api/src/routes/scripts.rs
git commit -m "test: cover script family detail regressions"
```

### Task 4: Run focused verification

**Files:**

- Modify: none
- Test: existing test targets

**Step 1: Run targeted Rust tests**

Run:

```bash
cargo test -p ckbadger-api resolve_code_cell_ -- --nocapture
cargo test -p ckbadger-api script_capacity_history_ -- --nocapture
cargo test -p ckbadger-api api_integration -- --nocapture
```

Expected: all targeted tests PASS.

**Step 2: Run broader crate verification**

Run:

```bash
cargo test -p ckbadger-api
```

Expected: PASS with no failing route regressions.

**Step 3: Commit verification-only changes if needed**

```bash
git status
```

Expected: no unexpected files beyond the intended implementation and test changes.

### Task 5: Update the close-out summary

**Files:**

- Modify: none

**Step 1: Prepare the final summary in the required project format**

Include:

- Goal
- Principle Alignment
- Scope
- Validation
- Result

Explicitly state:

- Store boundary checks passed
- Domain vs append-only target confirmed: yes
- Append-only update/delete path check: pass
- Re-sync required: no

**Step 2: Confirm no Transactions tab work is included**

State in the close-out that the Transactions tab was explicitly removed from scope and not implemented.
