# Remove Sequential Sync Mode — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove the sequential sync code path, making pipeline the only sync mode. Eliminates ~2000 lines of duplicated logic.

**Architecture:** Delete `sequential.rs`, remove `sync_batch()`/`sync_blocks_batch()`/`cleanup_failed_batch_range()` from `batch.rs`, remove `pipeline_enabled` config field and all guards. Pipeline becomes unconditional.

**Tech Stack:** Rust (indexer crate, config crate, CLI crate)

**Key constraint:** `PerfStats` in diagnostics.rs is shared (pipeline uses it too) — do NOT delete. `parse_udt_cells_with_store_fallback_inner` is shared — do NOT delete. `maybe_start_label_import`, `check_bulk_sync_completion`, `maybe_invalidate_chart_caches`, `write_parsed_batch` are shared — do NOT delete.

---

### Task 1: Delete sequential.rs and mod declaration

**Files:**

- Delete: `crates/indexer/src/sync/sequential.rs`
- Modify: `crates/indexer/src/sync/mod.rs:12` — remove `mod sequential;`

**Step 1: Delete the file**

```bash
rm crates/indexer/src/sync/sequential.rs
```

**Step 2: Remove mod declaration**

In `crates/indexer/src/sync/mod.rs`, delete line 12:

```rust
mod sequential;
```

**Step 3: Verify compilation**

Run: `cargo check -p ckbadger-indexer 2>&1 | head -30`
Expected: Errors about `run_sequential` not found (will fix in Task 2)

**Step 4: Commit**

```bash
git add -A crates/indexer/src/sync/sequential.rs crates/indexer/src/sync/mod.rs
git commit -m "refactor(indexer): delete sequential.rs module"
```

---

### Task 2: Remove pipeline_enabled branching in indexer.rs

**Files:**

- Modify: `crates/indexer/src/sync/indexer.rs`

**Step 1: Simplify `run()` method**

At lines 987–991, replace:

```rust
        if self.config.pipeline_enabled {
            self.run_pipeline().await
        } else {
            self.run_sequential().await
        }
```

with:

```rust
        self.run_pipeline().await
```

**Step 2: Remove pipeline_enabled from log/flight messages**

At lines 791–801, update the info! and record_flight_event to remove `pipeline=` references:

```rust
        info!(
            run_id = %self.run_id,
            "Starting indexer ({} blocks behind, threshold={})",
            blocks_behind, self.config.bulk_sync_threshold
        );
        self.record_flight_event(
            "run_start",
            format!(
                "blocks_behind={} bulk_threshold={}",
                blocks_behind, self.config.bulk_sync_threshold
            ),
        );
```

**Step 3: Remove pipeline_enabled guards on snapshot methods**

At lines 690–718, remove the early-return guards:

`pipeline_progress_snapshot` (line 690): remove lines 691–693
`adaptive_batch_snapshot` (line 697): remove lines 698–700
`pipeline_reset_snapshot` (line 715): remove lines 716–718

**Step 4: Remove comment**

At line 776, update comment from `// === run / run_sequential / run_pipeline ===` to `// === run ===`

**Step 5: Verify compilation**

Run: `cargo check -p ckbadger-indexer 2>&1 | head -30`
Expected: Errors about `pipeline_enabled` field not existing (will fix in Tasks 3–4). May also see warnings about unused `sync_batch`/`sync_blocks_batch`/`cleanup_failed_batch_range`.

**Step 6: Commit**

```bash
git add crates/indexer/src/sync/indexer.rs
git commit -m "refactor(indexer): remove pipeline_enabled branching, pipeline is now unconditional"
```

---

### Task 3: Remove pipeline_enabled from config structs

**Files:**

- Modify: `crates/config/src/lib.rs`
- Modify: `crates/indexer/src/config.rs`
- Modify: `crates/indexer/src/entry.rs`
- Modify: `crates/cli/src/main.rs`

**Step 1: crates/config/src/lib.rs**

- Line 63: Delete `pub pipeline_enabled: bool,` from `IndexerConfig` struct
- Line 141: Delete `pipeline_enabled: true,` from `Default` impl
- Line 345: Delete `pipeline_enabled = true` from TOML example
- Line 564: Delete `assert!(cfg.indexer.pipeline_enabled);` from test
- Line 624: Delete `pipeline_enabled = false` from custom TOML test
- Line 650: Delete `assert!(!cfg.indexer.pipeline_enabled);` from custom test

**Step 2: crates/indexer/src/config.rs**

- Lines 29–30: Delete `pipeline_enabled` field from `Config` struct
- Lines 62–64: Delete `default_pipeline_enabled()` function
- Line 167: Delete `pipeline_enabled: true,` from test's `make_valid_config()`

**Step 3: crates/indexer/src/entry.rs**

- Line 31: Delete `pub pipeline_enabled: bool,` from `IndexerServiceConfig`
- Line 49: Delete `pipeline_enabled: svc.pipeline_enabled,` from `From` impl
- Line 1218: Delete `pipeline_enabled: false,` from test
- Line 1238: Delete `assert!(!config.pipeline_enabled);` from test

**Step 4: crates/cli/src/main.rs**

- Line 227: Delete `pipeline_enabled: config.indexer.pipeline_enabled,`

**Step 5: Verify compilation**

Run: `cargo check -p ckbadger-indexer -p ckbadger-config -p ckbadger 2>&1 | head -30`
Expected: Warnings about unused `sync_batch`/`sync_blocks_batch`/`cleanup_failed_batch_range` (will fix in Task 4). Should compile.

**Step 6: Commit**

```bash
git add crates/config/src/lib.rs crates/indexer/src/config.rs crates/indexer/src/entry.rs crates/cli/src/main.rs
git commit -m "refactor(config): remove pipeline_enabled field from all config structs"
```

---

### Task 4: Delete sequential-only methods from batch.rs

This is the biggest deletion — ~2000 lines of duplicated sequential sync logic.

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs`

**Step 1: Delete `sync_batch()` method**

Delete lines 844–1011 (the `pub(crate) async fn sync_batch` method).

**Step 2: Delete `cleanup_failed_batch_range()` method**

Delete lines 1068–1115 (the `pub(crate) fn cleanup_failed_batch_range` method). Only called from `sync_batch`.

**Step 3: Delete `sync_blocks_batch()` method**

Delete lines 1186–3038 (the `async fn sync_blocks_batch` method). This is the large sequential-only code path.

**Step 4: Delete `parse_udt_cells_with_store_fallback` method (the `&self` wrapper)**

Delete lines 815–826 (the `fn parse_udt_cells_with_store_fallback(&self, ...)` method). Only called from `sync_blocks_batch` (lines 2440, 5426 — both within the deleted range). Keep `parse_udt_cells_with_store_fallback_inner` (line 304) — it's called from `write_parsed_batch` at line 3551.

**Step 5: Check for dead imports**

After deletion, check if any imports at the top of batch.rs became unused. Remove any that produce warnings.

**Step 6: Verify compilation**

Run: `cargo check -p ckbadger-indexer 2>&1 | head -30`
Expected: Clean compilation (possibly with unrelated warnings).

**Step 7: Run tests**

Run: `cargo test -p ckbadger-indexer -- parse_udt_cells_with_store_fallback 2>&1 | tail -20`
Expected: The `_inner` tests still pass (lines 7079, 7123, 7160).

**Step 8: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "refactor(indexer): delete ~2000 lines of sequential-only sync code from batch.rs"
```

---

### Task 5: Update CLAUDE.md

**Files:**

- Modify: `CLAUDE.md`

**Step 1: Remove pipeline_enabled row from config table**

At line 179, delete:

```
| `pipeline_enabled`    | `true`  | Enable pipeline mode (vs sequential)    |
```

**Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: remove pipeline_enabled from config table"
```

---

### Task 6: Final verification

**Step 1: Full check + clippy**

Run: `cargo check && cargo clippy 2>&1 | tail -20`
Expected: Clean

**Step 2: Run all Rust tests**

Run: `cargo test 2>&1 | tail -30`
Expected: All pass

**Step 3: Frontend checks (unchanged, sanity)**

Run: `cd frontend && pnpm type-check && pnpm lint 2>&1 | tail -10`
Expected: Clean (no frontend changes)
