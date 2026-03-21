# Facts Phase Parallel Metrics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decompose the opaque `facts_ms` into 6 sub-metrics that reveal parallel efficiency, interner contention, and serial merge overhead during bulk sync.

**Architecture:** Instrument `IdentityInterner` with atomic counters, add timing breakdown to `build_bulk_facts_arena_from_raw_blocks`, and propagate through the existing metrics pipeline (BatchBuildTimings → BulkBuildPerfStats → BulkBuildProgressData → TUI).

**Tech Stack:** Rust, `std::sync::atomic`, `std::time::Instant`, ratatui

**Spec:** `docs/superpowers/specs/2026-03-21-facts-parallel-metrics-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/indexer/src/sync/bulk_build/interner.rs` | Modify | Add 2 AtomicU64 counters + `drain_counters()` |
| `crates/indexer/src/sync/bulk_build/binary_facts.rs` | Modify | Add `FactsTimingBreakdown` struct, instrument `build_bulk_facts_arena_from_raw_blocks` |
| `crates/indexer/src/sync/pipeline.rs` | Modify | Instrument `build_bulk_facts_arena_from_blocks` (hex/test path) |
| `crates/indexer/src/sync/bulk_build/mod.rs` | Modify | Add `facts_breakdown` to `BatchBuildTimings`, propagate in `apply_blocks`/`apply_blocks_hex` |
| `crates/common/src/sync.rs` | Modify | Add 6 fields to `BulkBuildProgressData` |
| `crates/indexer/src/sync/diagnostics.rs` | Modify | Add 6 AtomicU64 fields, extend `record_batch()`/`snapshot()` |
| `crates/indexer/src/bulk_sync_perf.rs` | Modify | Add 6 fields to `BatchSample` |
| `crates/tui/src/ui.rs` | Modify | Add facts breakdown detail line in `build_batch_left_column` |

---

### Task 1: IdentityInterner counters

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/interner.rs`

- [ ] **Step 1: Write test for `drain_counters`**

Add to the existing `#[cfg(test)] mod tests` at the bottom of `interner.rs`:

```rust
#[test]
fn drain_counters_returns_counts_and_resets() {
    let interner = IdentityInterner::default();
    // First intern: new value → slow path (Mutex)
    interner.intern_bytes(vec![1, 2, 3]);
    // Second intern: same value → fast path (DashMap hit)
    interner.intern_bytes(vec![1, 2, 3]);
    // Third intern: new value → slow path
    interner.intern_bytes(vec![4, 5, 6]);

    let (total, slow) = interner.drain_counters();
    assert_eq!(total, 3, "3 total intern_bytes calls");
    assert_eq!(slow, 2, "2 slow-path Mutex acquisitions (new identities)");

    // After drain, counters reset to zero
    let (total2, slow2) = interner.drain_counters();
    assert_eq!(total2, 0);
    assert_eq!(slow2, 0);
}
```

- [ ] **Step 2: Run test, verify it fails**

Run: `cargo test -p ckbadger-indexer drain_counters_returns_counts_and_resets`
Expected: compilation error — `drain_counters` method does not exist.

- [ ] **Step 3: Add counter fields and `drain_counters` to `IdentityInterner`**

In `interner.rs`, add `use std::sync::atomic::{AtomicU64, Ordering};` to imports.

Add two fields to the struct (after `values`):

```rust
pub(crate) struct IdentityInterner {
    by_value: DashMap<Vec<u8>, InternId>,
    values: Mutex<Arc<Vec<Arc<[u8]>>>>,
    intern_call_count: AtomicU64,
    intern_slow_path_count: AtomicU64,
}
```

Update `Default` impl to initialize both to `AtomicU64::new(0)`.

Update `with_capacity` to initialize both to `AtomicU64::new(0)`.

Instrument `intern_bytes` — add `self.intern_call_count.fetch_add(1, Ordering::Relaxed);` as the first line. Add `self.intern_slow_path_count.fetch_add(1, Ordering::Relaxed);` right after the fast-path `if` block returns (before `let mut values = self.values.lock()...`), with the comment: `// Counts Mutex acquisitions, not new-identity insertions. The double-check inside the Mutex may find the value already inserted by another thread, but the contention (Mutex wait) still occurred.`

Add `drain_counters` method:

```rust
/// Read and reset per-batch intern counters. Called once per batch.
pub(crate) fn drain_counters(&self) -> (u64, u64) {
    let total = self.intern_call_count.swap(0, Ordering::Relaxed);
    let slow = self.intern_slow_path_count.swap(0, Ordering::Relaxed);
    (total, slow)
}
```

- [ ] **Step 4: Run test, verify it passes**

Run: `cargo test -p ckbadger-indexer drain_counters_returns_counts_and_resets`
Expected: PASS

- [ ] **Step 5: Run all interner tests**

Run: `cargo test -p ckbadger-indexer interner`
Expected: All existing tests pass (no regression).

- [ ] **Step 6: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/interner.rs
git commit -m "feat(indexer): add intern call/slow-path counters to IdentityInterner"
```

---

### Task 2: FactsTimingBreakdown struct and instrumented binary_facts builder

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/binary_facts.rs`

- [ ] **Step 1: Add `FactsTimingBreakdown` struct**

Add at the top of `binary_facts.rs` (after imports, before `parse_block_to_facts`):

```rust
/// Per-batch timing breakdown for the Facts phase.
/// Returned alongside `FactsArena` to decompose `facts_ms` into parallel vs serial components.
#[derive(Debug, Default, Clone)]
pub(crate) struct FactsTimingBreakdown {
    /// Wall-clock time of the rayon par_iter phase (ms).
    pub par_iter_ms: f64,
    /// Wall-clock time of the serial arena merge phase (ms).
    pub merge_ms: f64,
    /// Sum of per-block parse times across all rayon threads (ms).
    /// `serial_equivalent_ms / par_iter_ms` = actual speedup ratio.
    pub serial_equivalent_ms: f64,
    /// Number of `intern_bytes` calls that took the Mutex slow path.
    pub intern_slow_path_count: u64,
    /// Total number of `intern_bytes` calls.
    pub intern_total_count: u64,
    /// Total cells parsed in this batch.
    pub cell_count: u64,
}
```

- [ ] **Step 2: Instrument `build_bulk_facts_arena_from_raw_blocks`**

Change the return type from `Result<super::facts::FactsArena>` to `Result<(super::facts::FactsArena, FactsTimingBreakdown)>`.

Add `use std::sync::atomic::{AtomicU64, Ordering};` and `use std::time::Instant;` to the function-level imports (alongside existing `use rayon::prelude::*;`).

Replace the function body with:

```rust
pub(crate) fn build_bulk_facts_arena_from_raw_blocks(
    blocks: &[RawCkbBlock],
    interner: &IdentityInterner,
) -> Result<(super::facts::FactsArena, FactsTimingBreakdown)> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    let serial_equivalent_us = AtomicU64::new(0);
    let total_cells = AtomicU64::new(0);

    let par_start = Instant::now();
    #[allow(clippy::type_complexity)]
    let per_block_results: Vec<Result<(BlockFacts, Vec<TxFacts>, Vec<CellFacts>)>> = blocks
        .par_iter()
        .map(|raw| {
            let block_start = Instant::now();
            let result = parse_block_to_facts(raw, interner);
            serial_equivalent_us.fetch_add(
                block_start.elapsed().as_micros() as u64,
                Ordering::Relaxed,
            );
            if let Ok((_, _, ref cells)) = result {
                total_cells.fetch_add(cells.len() as u64, Ordering::Relaxed);
            }
            result
        })
        .collect();
    let par_elapsed = par_start.elapsed();

    let merge_start = Instant::now();
    let mut arena = super::facts::FactsArena::default();
    for result in per_block_results {
        let (block_facts, txs, cells) = result?;
        let tx_start = arena.txs.len();
        let cell_start = arena.cells.len();

        for mut tx in txs {
            tx.output_range =
                (cell_start + tx.output_range.start)..(cell_start + tx.output_range.end);
            arena.txs.push(tx);
        }
        arena.cells.extend(cells);

        let tx_end = arena.txs.len();
        let mut block = block_facts;
        block.tx_range = tx_start..tx_end;
        arena.blocks.push(block);
    }
    let merge_elapsed = merge_start.elapsed();

    let (intern_total, intern_slow) = interner.drain_counters();

    let breakdown = FactsTimingBreakdown {
        par_iter_ms: par_elapsed.as_secs_f64() * 1000.0,
        merge_ms: merge_elapsed.as_secs_f64() * 1000.0,
        serial_equivalent_ms: serial_equivalent_us.load(Ordering::Relaxed) as f64 / 1000.0,
        intern_slow_path_count: intern_slow,
        intern_total_count: intern_total,
        cell_count: total_cells.load(Ordering::Relaxed),
    };

    Ok((arena, breakdown))
}
```

- [ ] **Step 3: Write test for `FactsTimingBreakdown` population**

The spec requires a unit test verifying breakdown fields are populated. Add to the existing `#[cfg(test)] mod tests` at the bottom of `binary_facts.rs`. Use an existing test helper that constructs `RawCkbBlock` fixtures, or if none exists, write a minimal test that constructs a single-block batch with at least one cell and asserts that the returned `FactsTimingBreakdown` has:
- `par_iter_ms > 0.0`
- `merge_ms >= 0.0`
- `serial_equivalent_ms > 0.0`
- `cell_count > 0`
- `intern_total_count > 0` (at least lock_script_hash + lock_code_hash + lock_args per cell = 3)

If constructing `RawCkbBlock` in a unit test is impractical (it requires binary molecule data), defer this test to Task 9 and verify through the existing integration tests that exercise `apply_blocks` end-to-end, which will exercise this path.

- [ ] **Step 4: Run `cargo check -p ckbadger-indexer` to see compilation errors**

Expected: compilation errors in `mod.rs` where `build_bulk_facts_arena_from_raw_blocks` is called — now returns a tuple. Fix in Task 4.

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/binary_facts.rs
git commit -m "feat(indexer): add FactsTimingBreakdown and instrument binary facts builder"
```

---

### Task 3: Instrument pipeline.rs hex/test facts builder

**Files:**
- Modify: `crates/indexer/src/sync/pipeline.rs`

- [ ] **Step 1: Instrument `build_bulk_facts_arena_from_blocks`**

This is the hex-based path used by `apply_blocks_hex` (tests). Apply the same instrumentation pattern as binary_facts.

Change return type from `Result<FactsArena>` to `Result<(FactsArena, binary_facts::FactsTimingBreakdown)>`.

Add the import: `use super::bulk_build::binary_facts::FactsTimingBreakdown;`

Instrument identically: wrap `par_iter` in `Instant::now`, add `AtomicU64` for `serial_equivalent_us` and `total_cells`, wrap merge in `Instant::now`, call `interner.drain_counters()`. Return `Ok((arena, breakdown))`.

- [ ] **Step 2: Run `cargo check -p ckbadger-indexer`**

Expected: compilation errors in `mod.rs` `apply_blocks_hex` which calls this function. Fix in Task 4.

- [ ] **Step 3: Commit**

```bash
git add crates/indexer/src/sync/pipeline.rs
git commit -m "feat(indexer): instrument hex-based facts builder with FactsTimingBreakdown"
```

---

### Task 4: Propagate through BatchBuildTimings and apply_blocks

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs`

- [ ] **Step 1: Add `facts_breakdown` to `BatchBuildTimings`**

At line ~643, add the field:

```rust
#[derive(Debug, Default, Clone)]
struct BatchBuildTimings {
    facts_ms: f64,
    facts_breakdown: binary_facts::FactsTimingBreakdown,
    resolve_ms: f64,
    reduce_ms: f64,
    history_ms: f64,
    address_reduce_ms: f64,
    activity_stats_ms: f64,
}
```

- [ ] **Step 2: Update `apply_blocks` (binary path, ~line 1499)**

At line ~1517, change:
```rust
let arena = binary_facts::build_bulk_facts_arena_from_raw_blocks(blocks, &self.interner)?;
```
to:
```rust
let (arena, facts_breakdown) = binary_facts::build_bulk_facts_arena_from_raw_blocks(blocks, &self.interner)?;
```

At line ~1773 where `BatchBuildTimings` is constructed, add the `facts_breakdown` field:
```rust
let timings = BatchBuildTimings {
    facts_ms: facts_elapsed.as_secs_f64() * 1000.0,
    facts_breakdown,
    resolve_ms: resolve_elapsed.as_secs_f64() * 1000.0,
    // ... rest unchanged ...
};
```

- [ ] **Step 3: Update `apply_blocks_hex` (hex path, ~line 1798)**

At line ~1815, change:
```rust
let arena =
    crate::sync::pipeline::build_bulk_facts_arena_from_blocks(blocks, &self.interner)?;
```
to:
```rust
let (arena, facts_breakdown) =
    crate::sync::pipeline::build_bulk_facts_arena_from_blocks(blocks, &self.interner)?;
```

At line ~1911 where `BatchBuildTimings` is constructed, add:
```rust
BatchBuildTimings {
    facts_ms: facts_elapsed.as_secs_f64() * 1000.0,
    facts_breakdown,
    // ... rest unchanged ...
},
```

- [ ] **Step 4: Run `cargo check -p ckbadger-indexer`**

Expected: PASS (or minor fixes needed in batch loop where `build_timings.facts_breakdown` is accessed — those will compile fine since `BatchBuildTimings` just has a new field).

- [ ] **Step 5: Run all existing tests**

Run: `cargo test -p ckbadger-indexer --lib`
Expected: All pass. No behavior change yet — breakdown is computed but not consumed.

- [ ] **Step 6: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(indexer): propagate FactsTimingBreakdown through BatchBuildTimings"
```

---

### Task 5: Add fields to BulkBuildProgressData

**Files:**
- Modify: `crates/common/src/sync.rs`

- [ ] **Step 1: Add 6 new fields to `BulkBuildProgressData`**

After the existing `target_iteration_ms` field (line ~285), add:

```rust
    /// Facts phase: rayon par_iter wall-clock time in ms.
    #[serde(default)]
    pub facts_par_iter_ms: Option<f64>,
    /// Facts phase: serial arena merge wall-clock time in ms.
    #[serde(default)]
    pub facts_merge_ms: Option<f64>,
    /// Facts phase: sum of per-block parse times (serial equivalent) in ms.
    #[serde(default)]
    pub facts_serial_equivalent_ms: Option<f64>,
    /// Facts phase: number of intern_bytes calls that took the Mutex slow path.
    #[serde(default)]
    pub facts_intern_slow_path_count: Option<u64>,
    /// Facts phase: total number of intern_bytes calls.
    #[serde(default)]
    pub facts_intern_total_count: Option<u64>,
    /// Facts phase: total cells parsed in the batch.
    #[serde(default)]
    pub facts_cell_count: Option<u64>,
```

- [ ] **Step 2: Run `cargo check -p ckbadger-common`**

Expected: PASS. All fields have `#[serde(default)]` and are `Option`, so existing deserialization is unaffected.

- [ ] **Step 3: Commit**

```bash
git add crates/common/src/sync.rs
git commit -m "feat(common): add facts breakdown fields to BulkBuildProgressData"
```

---

### Task 6: Extend BulkBuildPerfStats atomics and snapshot

**Files:**
- Modify: `crates/indexer/src/sync/diagnostics.rs`

- [ ] **Step 1: Write test for facts breakdown in snapshot**

Add to existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn test_snapshot_includes_facts_breakdown() {
    let perf = BulkBuildPerfStats::default();
    perf.record_batch(
        45.2, 35.8, 28.1, 18.5, 8.3, 5.1, 52.0, 120.5, 141.0,
        1_800_000_000, 12_345_678, 5_000, 3_000, 45_230, 12_890,
        8_500, 1, 4.7, 0.042, 1380.0, 1500.0,
        // facts breakdown:
        40.0,    // facts_par_iter_ms
        5.2,     // facts_merge_ms
        280.0,   // facts_serial_equivalent_ms
        1_200,   // facts_intern_slow_path_count
        42_000,  // facts_intern_total_count
        28_000,  // facts_cell_count
    );
    let snap = perf.snapshot().unwrap();
    assert!((snap.facts_par_iter_ms.unwrap() - 40.0).abs() < 0.01);
    assert!((snap.facts_merge_ms.unwrap() - 5.2).abs() < 0.01);
    assert!((snap.facts_serial_equivalent_ms.unwrap() - 280.0).abs() < 0.01);
    assert_eq!(snap.facts_intern_slow_path_count, Some(1_200));
    assert_eq!(snap.facts_intern_total_count, Some(42_000));
    assert_eq!(snap.facts_cell_count, Some(28_000));
}
```

- [ ] **Step 2: Run test, verify it fails**

Run: `cargo test -p ckbadger-indexer test_snapshot_includes_facts_breakdown`
Expected: compilation error — `record_batch` doesn't accept the extra parameters yet.

- [ ] **Step 3: Add 6 AtomicU64 fields to `BulkBuildPerfStats`**

After `target_iteration_ms_us` (line ~316), add:

```rust
    // Facts phase breakdown
    last_facts_par_iter_us: AtomicU64,
    last_facts_merge_us: AtomicU64,
    last_facts_serial_equivalent_us: AtomicU64,
    last_facts_intern_slow_path_count: AtomicU64,
    last_facts_intern_total_count: AtomicU64,
    last_facts_cell_count: AtomicU64,
```

- [ ] **Step 4: Extend `record_batch` signature and body**

Add 6 new parameters after `target_iteration_ms`:

```rust
    facts_par_iter_ms: f64,
    facts_merge_ms: f64,
    facts_serial_equivalent_ms: f64,
    facts_intern_slow_path_count: u64,
    facts_intern_total_count: u64,
    facts_cell_count: u64,
```

Add stores at end of method body:

```rust
    self.last_facts_par_iter_us
        .store(ms_to_us(facts_par_iter_ms), Ordering::Relaxed);
    self.last_facts_merge_us
        .store(ms_to_us(facts_merge_ms), Ordering::Relaxed);
    self.last_facts_serial_equivalent_us
        .store(ms_to_us(facts_serial_equivalent_ms), Ordering::Relaxed);
    self.last_facts_intern_slow_path_count
        .store(facts_intern_slow_path_count, Ordering::Relaxed);
    self.last_facts_intern_total_count
        .store(facts_intern_total_count, Ordering::Relaxed);
    self.last_facts_cell_count
        .store(facts_cell_count, Ordering::Relaxed);
```

- [ ] **Step 5: Extend `snapshot()` to populate new fields**

In the `Some(BulkBuildProgressData { ... })` block, add after existing fields:

```rust
    facts_par_iter_ms: Some(us_to_ms(self.last_facts_par_iter_us.load(Ordering::Relaxed))),
    facts_merge_ms: Some(us_to_ms(self.last_facts_merge_us.load(Ordering::Relaxed))),
    facts_serial_equivalent_ms: Some(us_to_ms(self.last_facts_serial_equivalent_us.load(Ordering::Relaxed))),
    facts_intern_slow_path_count: Some(self.last_facts_intern_slow_path_count.load(Ordering::Relaxed)),
    facts_intern_total_count: Some(self.last_facts_intern_total_count.load(Ordering::Relaxed)),
    facts_cell_count: Some(self.last_facts_cell_count.load(Ordering::Relaxed)),
```

- [ ] **Step 6: Update existing `test_bulk_build_perf_snapshot_returns_data_after_record` test**

The `record_batch` call in this test now needs 6 extra arguments. Append them:

```rust
    // facts breakdown:
    0.0, 0.0, 0.0, 0, 0, 0,
```

Do the same for any other tests that call `record_batch` (check compilation errors).

- [ ] **Step 7: Run all diagnostics tests**

Run: `cargo test -p ckbadger-indexer diagnostics`
Expected: All pass including the new `test_snapshot_includes_facts_breakdown`.

- [ ] **Step 8: Commit**

```bash
git add crates/indexer/src/sync/diagnostics.rs
git commit -m "feat(indexer): extend BulkBuildPerfStats with facts breakdown atomics"
```

---

### Task 7: Add fields to BatchSample and wire the bulk-build batch loop

**Files:**
- Modify: `crates/indexer/src/bulk_sync_perf.rs`
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs`

- [ ] **Step 1: Add 6 fields to `BatchSample`**

After `flush_ms` (line ~60), add:

```rust
    pub facts_par_iter_ms: f64,
    pub facts_merge_ms: f64,
    pub facts_serial_equivalent_ms: f64,
    pub facts_intern_slow_path_count: u64,
    pub facts_intern_total_count: u64,
    pub facts_cell_count: u64,
```

Update `BatchSample::new()` to initialize all 6 to `0.0` / `0`.

- [ ] **Step 2: Wire breakdown into batch loop perf recording**

In `mod.rs`, in the batch loop (~line 324-368), after `sample.activity_stats_ms = build_timings.activity_stats_ms;` add:

```rust
    sample.facts_par_iter_ms = build_timings.facts_breakdown.par_iter_ms;
    sample.facts_merge_ms = build_timings.facts_breakdown.merge_ms;
    sample.facts_serial_equivalent_ms = build_timings.facts_breakdown.serial_equivalent_ms;
    sample.facts_intern_slow_path_count = build_timings.facts_breakdown.intern_slow_path_count;
    sample.facts_intern_total_count = build_timings.facts_breakdown.intern_total_count;
    sample.facts_cell_count = build_timings.facts_breakdown.cell_count;
```

In the `indexer.bulk_build_perf.record_batch(...)` call (~line 349-370), append the 6 new arguments:

```rust
    build_timings.facts_breakdown.par_iter_ms,
    build_timings.facts_breakdown.merge_ms,
    build_timings.facts_breakdown.serial_equivalent_ms,
    build_timings.facts_breakdown.intern_slow_path_count,
    build_timings.facts_breakdown.intern_total_count,
    build_timings.facts_breakdown.cell_count,
```

- [ ] **Step 3: Run `cargo check -p ckbadger-indexer`**

Expected: PASS

- [ ] **Step 4: Run full test suite**

Run: `cargo test -p ckbadger-indexer --lib`
Expected: All pass. This wires the data flow end-to-end within the indexer crate.

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(indexer): wire facts breakdown into BatchSample and batch loop"
```

---

### Task 8: TUI display

**Files:**
- Modify: `crates/tui/src/ui.rs`

- [ ] **Step 1: Add facts breakdown detail line to `build_batch_left_column`**

In `build_batch_left_column` (~line 2232), after the `for (name, ms_opt, color) in &stages` loop that pushes stage bars (ending ~line 2249), insert a conditional facts breakdown line.

After the Facts bar is pushed (first iteration of the loop where `name == "Facts"`), insert a detail line. The cleanest approach: after the entire stages loop, check if facts breakdown data is available and insert after the first stage line.

Alternative (simpler): after the loop ends (~line 2250), insert a conditional block:

```rust
    // Facts parallel breakdown detail line (after stage bars)
    if let (Some(par_ms), Some(merge_ms), Some(serial_ms)) = (
        bb.facts_par_iter_ms,
        bb.facts_merge_ms,
        bb.facts_serial_equivalent_ms,
    ) {
        let speedup = if par_ms > 0.0 {
            serial_ms / par_ms
        } else {
            0.0
        };
        let miss_rate_text = match (bb.facts_intern_slow_path_count, bb.facts_intern_total_count) {
            (Some(slow), Some(total)) if total > 0 => {
                format!("{:.1}%", slow as f64 / total as f64 * 100.0)
            }
            _ => "-".to_string(),
        };
        if dense_panel {
            // Compact: one line
            left.push(Line::from(vec![
                Span::styled("  par ", Style::default().fg(SLATE_500)),
                Span::styled(format!("{par_ms:.0}ms"), Style::default().fg(FOREGROUND)),
                Span::styled("  merge ", Style::default().fg(SLATE_500)),
                Span::styled(format!("{merge_ms:.0}ms"), Style::default().fg(FOREGROUND)),
                Span::styled(format!("  {speedup:.1}"), Style::default().fg(TERMINAL_GREEN)),
                Span::styled("\u{00d7} speedup", Style::default().fg(SLATE_500)),
                Span::styled("  miss ", Style::default().fg(SLATE_500)),
                Span::styled(miss_rate_text.clone(), Style::default().fg(FOREGROUND)),
            ]));
        } else {
            // Detail: multi-line
            left.push(Line::from(vec![
                Span::styled("  par_iter ", Style::default().fg(SLATE_500)),
                Span::styled(format!("{par_ms:>7.1}ms", ), Style::default().fg(FOREGROUND)),
                Span::styled(
                    format!("  (serial equiv {serial_ms:.0}ms \u{2192} {speedup:.1}\u{00d7})"),
                    Style::default().fg(SLATE_500),
                ),
            ]));
            left.push(Line::from(vec![
                Span::styled("  merge    ", Style::default().fg(SLATE_500)),
                Span::styled(format!("{merge_ms:>7.1}ms", ), Style::default().fg(FOREGROUND)),
                Span::styled(
                    format!("  ({:.1}%)", merge_ms / (par_ms + merge_ms) * 100.0),
                    Style::default().fg(SLATE_500),
                ),
            ]));
            let intern_text = match (bb.facts_intern_total_count, bb.facts_intern_slow_path_count) {
                (Some(total), Some(slow)) => {
                    format!("  intern   {}k calls  {} miss ({})", total / 1000, format_num_u64(slow), miss_rate_text)
                }
                _ => "  intern   -".to_string(),
            };
            left.push(Line::from(Span::styled(intern_text, Style::default().fg(SLATE_500))));
            // Volume line (cells + blocks from batch_block_span)
            let cells_text = bb.facts_cell_count.map(|c| format!("{}k", c / 1000)).unwrap_or_else(|| "-".to_string());
            let blocks_text = bb.batch_block_span.map(|b| format!("{}k", b / 1000)).unwrap_or_else(|| "-".to_string());
            left.push(Line::from(vec![
                Span::styled("  volume   ", Style::default().fg(SLATE_500)),
                Span::styled(format!("{cells_text} cells"), Style::default().fg(FOREGROUND)),
                Span::styled("  ", Style::default().fg(SLATE_500)),
                Span::styled(format!("{blocks_text} blocks"), Style::default().fg(FOREGROUND)),
            ]));
        }
    }
```

**`dense_panel` parameter threading:** `dense_panel` is not available inside `build_batch_left_column`. Thread it as follows:
1. Change `build_batch_left_column` signature from `fn build_batch_left_column(bb: &BulkBuildProgressData, cols: &[Rect])` to `fn build_batch_left_column(bb: &BulkBuildProgressData, cols: &[Rect], dense_panel: bool)`.
2. `build_bulk_build_diagnostics` (line ~2101) calls `build_batch_left_column`. Add `dense_panel: bool` parameter to `build_bulk_build_diagnostics` as well, and pass it through.
3. The caller of `build_bulk_build_diagnostics` is in the sync diagnostics rendering function (~line 2000) where `dense_panel` is already computed as a local variable. Pass it as an additional argument.

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p ckbadger-tui`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/tui/src/ui.rs
git commit -m "feat(tui): display facts parallel breakdown in bulk-build diagnostics"
```

---

### Task 9: Full build and test verification

- [ ] **Step 1: Full cargo check**

Run: `cargo check`
Expected: PASS across all crates.

- [ ] **Step 2: Clippy**

Run: `cargo clippy`
Expected: No new warnings.

- [ ] **Step 3: Full test suite**

Run: `cargo test --lib`
Expected: All pass.

- [ ] **Step 4: Frontend type-check (if BulkBuildProgressData is consumed)**

Run: `cd frontend && pnpm type-check`
Expected: PASS. The new fields are `Option` with `#[serde(default)]` and use `camelCase` via the struct's `#[serde(rename_all = "camelCase")]`. The frontend won't use them yet, so no TS changes needed. If the frontend has a TypeScript type for `BulkBuildProgressData`, add the optional fields there.

- [ ] **Step 5: Commit if any fixes needed**

```bash
git add -A && git commit -m "fix: address clippy/type-check feedback for facts metrics"
```
