# Bulk-Build Adaptive Batch Sizing — Write-Budget Feedback

**Goal:** Reduce bulk sync wall clock by ~15-25% by (A) unlocking the existing adaptive batch sizing that is currently clamped to a no-op by config defaults, and (B) replacing the fixed 40K-txs-per-batch target with an iteration-wall-clock feedback loop that adapts batch size to actual system throughput.

**Architecture:** Two incremental changes to the bulk-build main loop in `crates/indexer/src/sync/bulk_build/mod.rs`. Part A widens the block-span clamp bounds. Part B replaces the tx-count target with a ms-per-block EMA controller that targets 1500ms per iteration.

**Key references:**
- `docs/prompts/BULK_SYNC.md` — bulk sync rules
- `docs/superpowers/specs/2026-03-17-bulk-sync-build-engine-design.md` — build engine design
- `docs/superpowers/plans/2026-03-19-bulk-build-performance-optimizations.md` — Phase 2 plan (Task 5: original adaptive sizing)

---

## Problem

The bulk-build engine processes ~1890 batches of 10K blocks each to sync 18.9M blocks. Adaptive batch sizing (Task 5 from Phase 2) was implemented but is effectively dead code: the clamp uses `configured_batch_size` (default 10K) as both min and max, producing 10K blocks/batch regardless of tx density.

This wastes per-batch overhead in sparse phases. Phase 1 (blocks 0-4M, 1.1 tx/block) runs 400 batches of ~11K txs when ~107 batches of ~40K txs would suffice. The fixed 40K-txs target also ignores that cost-per-tx varies 2× across phases (20-37 µs/tx) and that fetch cost scales with block count, not tx count.

**Perf baseline (best run, build 2e7422, 2096s wall clock):**

| Phase | Blocks | Density | Batches | Avg batch(s) | tx/s (wall) |
|-------|--------|---------|---------|-------------|-------------|
| 1 (0-4M) | 4M | 1.1 tx/blk | 400 | 0.396 | 28,535 |
| 2 (4-8M) | 4M | 2.5 tx/blk | 400 | 1.083 | 22,931 |
| 3 (8-12M) | 4M | 3.2 tx/blk | 400 | 1.366 | 23,632 |
| 4 (12-16M) | 4M | 3.5 tx/blk | 400 | 1.880 | 18,770 |
| 5 (16-19M) | 3M | 2.6 tx/blk | 289 | 1.350 | 19,304 |

---

## Part A: Unlock Adaptive Batch Sizing

### Current code (mod.rs:397-416)

```rust
if tx_density > 0.0 {
    let target_txs: f64 = 40_000.0;
    let desired_f64 = target_txs / tx_density;
    // ...
    let adaptive_min = std::cmp::min(10_000, configured_batch_size);
    batch_block_span = (desired_f64 as u64).clamp(adaptive_min, configured_batch_size);
}
```

With `configured_batch_size = 10_000` (default), clamp range is `[10_000, 10_000]` — a no-op.

### Change

Replace config-dependent clamp with independent constants:

```rust
const BULK_BUILD_MIN_BLOCK_SPAN: u64 = 10_000;
const BULK_BUILD_MAX_BLOCK_SPAN: u64 = 100_000;

if tx_density > 0.0 {
    let target_txs: f64 = 40_000.0;
    let desired_f64 = target_txs / tx_density;
    // ... (existing validation) ...
    batch_block_span = (desired_f64 as u64).clamp(
        BULK_BUILD_MIN_BLOCK_SPAN,
        BULK_BUILD_MAX_BLOCK_SPAN,
    );
}
```

### Rationale

- `batch_size` config is a pipeline-sync parameter. Bulk-build batch memory is bounded by tx count (target 40K), not block count. 36K sparse blocks at 1.1 tx/blk use the same memory as 10K dense blocks at 4 tx/blk.
- First batch always uses `configured_batch_size` (line 110). Adaptive kicks in from batch 2.
- Operators who set `batch_size` low for pipeline sync are not affected; bulk-build uses its own constants.

### Expected behavior after Part A

| Phase | Density | Desired span | Clamped | Batches (before → after) |
|-------|---------|-------------|---------|-------------------------|
| 1 | 1.1 tx/blk | 36,364 | 36,364 | 400 → ~110 |
| 2 | 2.5 tx/blk | 16,000 | 16,000 | 400 → ~250 |
| 3 | 3.2 tx/blk | 12,500 | 12,500 | 400 → ~320 |
| 4 | 3.5 tx/blk | 11,429 | 11,429 | 400 → ~350 |
| 5 | 2.6 tx/blk | 15,385 | 15,385 | 289 → ~195 |

Total: 1889 → ~1225 batches. Estimated savings: ~150-200s.

---

## Part B: Write-Budget Adaptive Sizing

### Feedback signal: controllable_ms

The loop iteration wall clock includes three pipeline stages:

```
[spawn prefetch(N+1)] → [build(N)] → [collect prefetch(N+1)] → [await flush(N-1)] → [spawn flush(N)]
                         |____________controllable_ms___________|
```

`controllable_ms = build_elapsed + prefetch_collect_wait` measures the cost that batch size can influence. Flush wait is excluded because it depends on RocksDB compaction, not batch size.

**Why exclude flush_wait:** If flush(N-1) is slow and we include it in feedback, the controller would shrink batches. Smaller batches finish build faster, causing longer flush waits, creating a positive feedback loop that drives batches to the minimum floor. Excluding flush_wait breaks this loop: when flush-bound, batch size stays stable and the flush pipeline catches up naturally.

### Timing change

Current code collects the prefetch AFTER build but BEFORE flush_wait. Add timing around the collect:

```rust
let build_started = Instant::now();
let (batch_stats, build_timings, pending_flush) =
    runtime.apply_blocks(&blocks, is_mainnet, &token_info_cache)?;
let build_elapsed = build_started.elapsed();

let collect_started = Instant::now();
if let Some(handle) = prefetch_handle {
    prefetched_blocks = Some(handle.await??);
}
let prefetch_collect_elapsed = collect_started.elapsed();

let controllable_ms = (build_elapsed + prefetch_collect_elapsed).as_secs_f64() * 1000.0;
```

### Constants

```rust
const BULK_BUILD_TARGET_ITERATION_MS: f64 = 1500.0;
const BULK_BUILD_MS_PER_BLOCK_ALPHA: f64 = 0.5;
const BULK_BUILD_INITIAL_MS_PER_BLOCK: f64 = 0.05;
const BULK_BUILD_MAX_STEP_RATIO: f64 = 2.0;
const BULK_BUILD_MIN_BLOCK_SPAN: u64 = 10_000;
const BULK_BUILD_MAX_BLOCK_SPAN: u64 = 100_000;
```

**Parameter rationale:**

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| target 1500ms | Phase 2-5 avg batch time is 1.1-1.9s; 1500ms is the comfortable midpoint |
| alpha 0.5 | See note below on alpha choice |
| initial 0.05 ms/blk | Conservative (Phase 1 actual is 0.04). Avoids first-batch overshoot. |
| step ratio 2× | Prevents wild swings from stall spikes (single 15s stall can't shrink batch more than 2×). |
| min 10K blocks | Prevents batch fragmentation and per-batch overhead domination. |
| max 100K blocks | Limits memory for block-level structures (100K × ~100B BlockFacts = 10MB, negligible). |

**Why alpha 0.5 (not 0.20 like the pipeline controller):** The pipeline adaptive controller (`adaptive.rs`) uses alpha 0.20 because it operates in a steady-state environment where tx density and write cost change gradually. Bulk-build faces abrupt phase transitions (density jumps from 1.1 to 2.5 at block ~4M) and needs to converge within 2-3 batches to avoid wasting many batches at the wrong size. Alpha 0.5 means each new sample carries 50% weight, reaching 75% convergence in 2 batches. The 2× per-step clamp provides sufficient noise resistance against single-batch outliers (stalls). Additionally, the pipeline controller shares state across threads via atomics and uses integer-scaled arithmetic to avoid floating-point atomics. The bulk-build controller is a local `mut f64` variable in a single-threaded loop, so bare f64 EMA arithmetic is appropriate.

### Algorithm (replaces mod.rs:397-417)

```rust
let actual_blocks = batch_stats.block_count as f64;

// Skip EMA update when sample would be unrepresentative:
// - batch_count == 0: first batch has cold-cache inflated read times
// - actual_blocks < batch_block_span/2: runt batch (truncated by handoff_target)
let is_representative = batch_count > 1
    && actual_blocks >= (batch_block_span as f64 * 0.5);

if is_representative && actual_blocks > 0.0 && controllable_ms > 0.0 {
    let sample = controllable_ms / actual_blocks;
    ms_per_block_ema = ms_per_block_ema * (1.0 - BULK_BUILD_MS_PER_BLOCK_ALPHA)
        + sample * BULK_BUILD_MS_PER_BLOCK_ALPHA;

    // Fail-fast: EMA must remain finite and positive
    if !ms_per_block_ema.is_finite() || ms_per_block_ema <= 0.0 {
        bail!(
            "bulk build adaptive sizing: ms_per_block_ema became invalid: \
             ms_per_block_ema={} sample={} controllable_ms={} actual_blocks={}",
            ms_per_block_ema, sample, controllable_ms, actual_blocks
        );
    }

    let desired_f64 = BULK_BUILD_TARGET_ITERATION_MS / ms_per_block_ema;
    if !desired_f64.is_finite() || desired_f64 < 0.0 {
        bail!(
            "bulk build adaptive sizing: desired blocks is invalid: \
             desired_f64={} ms_per_block_ema={} target_ms={}",
            desired_f64, ms_per_block_ema, BULK_BUILD_TARGET_ITERATION_MS
        );
    }
    let desired = desired_f64 as u64;

    // Per-step change limit
    let step_max = (batch_block_span as f64 * BULK_BUILD_MAX_STEP_RATIO) as u64;
    let step_min = (batch_block_span as f64 / BULK_BUILD_MAX_STEP_RATIO) as u64;

    batch_block_span = desired
        .clamp(step_min, step_max)
        .clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
}
```

### Convergence analysis

**Steady state (Phase 3, ms_per_block=0.137):**
- desired = 1500/0.137 = 10,949 blocks
- EMA stable, batch size stable at ~11K

**Phase 1→2 transition (density jumps from 1.1 to 2.5):**
1. Last Phase 1 batch: ms_per_block_ema ≈ 0.040, batch = 37.5K blocks
2. First Phase 2 batch at 37.5K blocks: ms_per_block rises to ~0.108
3. EMA: 0.040×0.5 + 0.108×0.5 = 0.074. desired=20,270. step_max=75K. → 20.3K blocks
4. Next batch: ms_per_block=0.108. EMA: 0.074×0.5 + 0.108×0.5 = 0.091. desired=16,484. → ~16.5K
5. Converged within 2-3 batches. First over-target batch runs ~4000ms (not catastrophic).

**Stall batch (Phase 4, 15s spike):**
1. EMA before: 0.188. batch=10K (at min floor)
2. Stall: ms_per_block sample = 1.5. EMA: 0.188×0.5 + 1.5×0.5 = 0.844
3. desired = 1500/0.844 = 1,777. step_min = 5K. hard min = 10K. → stays at 10K
4. Already at floor, so stall doesn't cause further shrinkage.

**Flush-bound scenario (flush 2s, build 500ms):**
1. controllable_ms = 500ms + 0ms (prefetch done) = 500ms
2. ms_per_block = 0.05. desired = 30K blocks.
3. Batch grows appropriately — flush slowness doesn't suppress batch size.

### Observability

- Add `controllable_ms` to the info log line (alongside existing `build_ms`, `fetch_ms`)
- `batch_block_span` is already published to TUI via `bulk_build_perf.record_batch()`
- `tx_density` computation (mod.rs:322-327) preserved for TUI display; removed from adaptive sizing

### Removed code

- The `target_txs = 40_000.0` / `tx_density` division logic (lines 397-417) is fully replaced
- The `configured_batch_size` dependency for adaptive bounds is removed

### Edge cases

- **Cold-cache first batch:** The first batch has inflated read times from a cold CKB RocksDB page cache. Since `batch_count` is incremented (line 369) before the adaptive block runs (line 397), the first batch has `batch_count=1`. The check `batch_count > 1` skips it. The initial ms_per_block value (0.05) is used for batch 2's sizing, and real adaptation begins from batch 3's sample.
- **Runt batch near handoff:** When `blocks_remaining < batch_block_span`, the batch is truncated to fewer blocks than requested. This produces a distorted ms_per_block sample (low total time, normal per-block cost). EMA update is skipped when `actual_blocks < batch_block_span * 0.5` to avoid corrupting the EMA near the end of sync.
- **EMA invariant:** `ms_per_block_ema` and `desired_f64` are explicitly checked for finite/positive values. If either becomes invalid (denormal, NaN, Inf), the loop fails fast with actionable context per project coding principles.

---

## Expected Impact

| Phase | Current batches | After A+B | batch time (target) |
|-------|----------------|-----------|-------------------|
| 1 (0-4M) | 400 | ~107 | ~1500ms |
| 2 (4-8M) | 400 | ~288 | ~1500ms |
| 3 (8-12M) | 400 | ~365 | ~1500ms |
| 4 (12-16M) | 400 | 400 (at min) | ~1880ms (unchanged) |
| 5 (16-19M) | 289 | ~222 | ~1500ms |
| **Total** | **1889** | **~1382** | |

Estimated wall clock: ~1382 batches × 1.5s + 54s finalize ≈ **2127s** (pessimistic, Phase 4 unchanged) to **~1700s** (optimistic, reduced per-batch overhead amortized).

Conservative estimate: **~200-400s saved** (10-20% of 2096s baseline).

---

## Invariants

- First batch always uses `configured_batch_size` (10K). Adaptive kicks in from batch 2.
- `BULK_BUILD_MIN_BLOCK_SPAN = 10_000` hard floor prevents batch fragmentation.
- Per-step 2× limit ensures no single anomalous batch causes a >2× jump.
- Flush wait is excluded from the feedback signal, preventing flush-driven batch shrinkage.
- No config changes required. Pipeline sync is unaffected.

---

## Testing Strategy

- Update existing 4 adaptive sizing tests for Part A (new clamp bounds)
- Add unit tests for ms_per_block EMA update logic
- Add test for per-step change limit (2× clamp)
- Add test for stall behavior (verify batch stays at min floor)
- Add test for phase transition convergence
- Add test for runt-batch skip (actual_blocks < batch_block_span * 0.5 → no EMA update)
- Add test for cold-cache skip (batch_count == 0 → no EMA update)
- Add test for fail-fast on invalid EMA (verify bail on non-finite values)
- Full indexer test suite: `cargo test -p ckbadger-indexer`

## Principle Alignment

- **CKB Native**: No domain logic changes, pure performance optimization
- **Local First**: Faster bulk rebuild = cheaper experiments, aligns with "if DB is broken, rebuild it"
- **Agent Friendly**: New timing field (`controllable_ms`) improves observability
