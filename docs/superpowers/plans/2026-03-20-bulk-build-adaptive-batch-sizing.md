# Bulk-Build Adaptive Batch Sizing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce bulk sync wall clock by ~15-25% by unlocking adaptive batch sizing and replacing the fixed 40K-txs target with an iteration-wall-clock feedback loop.

**Architecture:** Two incremental changes to the bulk-build main loop. Part A widens the block-span clamp from [10K, 10K] (dead code) to [10K, 100K]. Part B replaces the tx-count target with a ms-per-block EMA controller targeting 1500ms per iteration, using `controllable_ms` (build + prefetch_collect, excluding flush_wait) as the feedback signal.

**Tech Stack:** Rust, tokio (existing async loop)

**Spec:** `docs/superpowers/specs/2026-03-20-bulk-build-adaptive-batch-sizing-design.md`

---

## Task 1: Part A — Unlock Adaptive Batch Sizing Constants

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs:397-417` (adaptive sizing block)

- [ ] **Step 1: Add block span constants**

Add constants near the top of the file, after the existing `BULK_PHASE_COMMIT_SLOW_WARN_MS` and other bulk-build constants (find the constants section near `preload_token_info_cache` or above the `run_bulk_stage_until_pipeline_handoff` function). If no suitable location exists among module-level constants, add them just before the function:

```rust
const BULK_BUILD_MIN_BLOCK_SPAN: u64 = 10_000;
const BULK_BUILD_MAX_BLOCK_SPAN: u64 = 100_000;
```

- [ ] **Step 2: Replace the clamp bounds and stale comment in the adaptive sizing block**

At `mod.rs:397-417`, replace:

```rust
    // Respect configured batch_size as upper bound so operators can
    // limit memory by lowering batch_size. When batch_size < 10K the
    // minimum equals batch_size (no silent override).
    let adaptive_min = std::cmp::min(10_000, configured_batch_size);
    batch_block_span = (desired_f64 as u64).clamp(adaptive_min, configured_batch_size);
```

With:

```rust
    batch_block_span =
        (desired_f64 as u64).clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
```

Keep everything else in the block unchanged (the `tx_density > 0.0` guard, `target_txs`, `desired_f64` computation, and the `bail!` validation).

- [ ] **Step 3: Run tests to verify existing tests still pass**

Run: `cargo test -p ckbadger-indexer --lib -- test_adaptive_batch_sizing`

Expected: all 4 tests pass (they already use `clamp(10_000, 100_000)` internally, matching the new constants).

- [ ] **Step 4: Run clippy**

Run: `cargo clippy -p ckbadger-indexer`

Expected: no new warnings. The `configured_batch_size` variable is still used for `batch_block_span` initialization at line 110, so it remains in use.

- [ ] **Step 5: Commit**

```
perf(bulk-build): unlock adaptive batch sizing with independent block span bounds

The adaptive sizing code (40K txs target) was effectively dead: it
clamped to [configured_batch_size, configured_batch_size] which is
[10K, 10K] by default. Replace with independent constants [10K, 100K]
so sparse Phase 1 blocks can use 36K blocks/batch instead of 10K,
reducing ~400 batches to ~110.
```

---

## Task 2: Part B — Add Controllable-ms Timing

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs:225-237` (build + prefetch collection)

- [ ] **Step 1: Add timing around prefetch collection**

At `mod.rs:230-237`, the current code is:

```rust
        // Collect prefetched next batch (typically already done since build >> fetch).
        if let Some(handle) = prefetch_handle {
            prefetched_blocks = Some(
                handle
                    .await
                    .map_err(|e| anyhow!("bulk build prefetch task panicked: {}", e))??,
            );
        }
```

Wrap it with timing:

```rust
        // Collect prefetched next batch (typically already done since build >> fetch).
        let collect_started = Instant::now();
        if let Some(handle) = prefetch_handle {
            prefetched_blocks = Some(
                handle
                    .await
                    .map_err(|e| anyhow!("bulk build prefetch task panicked: {}", e))??,
            );
        }
        let prefetch_collect_elapsed = collect_started.elapsed();

        // controllable_ms: build + prefetch_collect. Excludes flush_wait because
        // flush depends on RocksDB compaction, not batch size. Including flush_wait
        // would create a positive feedback loop (slow flush → shrink batch → faster
        // build → longer flush wait → shrink more → drives to minimum floor).
        let controllable_ms =
            (build_elapsed + prefetch_collect_elapsed).as_secs_f64() * 1000.0;
```

- [ ] **Step 2: Add controllable_ms to the info log**

At `mod.rs:375-395`, add `controllable_ms` after `build_ms`:

```rust
            fetch_ms = format!("{:.1}", fetch_elapsed.as_secs_f64() * 1000.0),
            build_ms = format!("{:.1}", build_elapsed.as_secs_f64() * 1000.0),
            controllable_ms = format!("{:.1}", controllable_ms),
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p ckbadger-indexer`

- [ ] **Step 4: Commit**

```
feat(bulk-build): add controllable_ms timing for adaptive feedback

Measures build_elapsed + prefetch_collect_wait, excluding flush_wait.
This is the feedback signal for the write-budget adaptive controller:
it captures the cost that batch size can influence without being
distorted by RocksDB compaction delays.
```

---

## Task 3: Part B — Add EMA Constants and State

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs` (constants + loop state)

- [ ] **Step 1: Add Part B constants**

Add alongside the `BULK_BUILD_MIN_BLOCK_SPAN` / `BULK_BUILD_MAX_BLOCK_SPAN` constants from Task 1:

```rust
const BULK_BUILD_TARGET_ITERATION_MS: f64 = 1500.0;
const BULK_BUILD_MS_PER_BLOCK_ALPHA: f64 = 0.5;
const BULK_BUILD_INITIAL_MS_PER_BLOCK: f64 = 0.05;
const BULK_BUILD_MAX_STEP_RATIO: f64 = 2.0;
```

- [ ] **Step 2: Add EMA state variable**

After `let mut batch_block_span = configured_batch_size;` (line 110), add:

```rust
        let mut ms_per_block_ema: f64 = BULK_BUILD_INITIAL_MS_PER_BLOCK;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p ckbadger-indexer`

Expected: compiles with a warning about `ms_per_block_ema` being unused (will be used in Task 4).

- [ ] **Step 4: Commit**

```
refactor(bulk-build): add write-budget adaptive constants and EMA state

Constants for the ms-per-block feedback controller: 1500ms target,
alpha 0.5, initial 0.05 ms/blk, 2x step ratio. The EMA state variable
will replace the fixed 40K-txs target in the next commit.
```

---

## Task 4: Part B — Replace Adaptive Sizing with EMA Controller

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs:397-417` (adaptive sizing block)

- [ ] **Step 1: Replace the adaptive sizing block**

Replace the entire block at lines 397-417:

```rust
            // Adjust batch span for next iteration based on observed tx density.
            // Target: ~40K txs per batch to balance per-batch overhead vs memory.
            // Early blocks are sparse (~1 tx/block) so larger batches reduce overhead;
            // late blocks are dense (~4+ tx/block) so 10K blocks is appropriate.
            if tx_density > 0.0 {
                let target_txs: f64 = 40_000.0;
                let desired_f64 = target_txs / tx_density;
                if !desired_f64.is_finite() || desired_f64 < 0.0 {
                    bail!(
                        "bulk build adaptive sizing produced invalid desired blocks: tx_density={} target_txs={} desired_f64={}",
                        tx_density,
                        target_txs,
                        desired_f64
                    );
                }
                batch_block_span =
                    (desired_f64 as u64).clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
            }
```

With:

```rust
            // Adaptive batch sizing: target a fixed wall-clock budget per iteration
            // using EMA of ms-per-block as the cost model. Excludes flush_wait
            // (captured in controllable_ms above) to avoid flush-driven shrinkage.
            //
            // Skip EMA update when the sample would be unrepresentative:
            // - batch_count <= 1: first batch has cold-cache inflated read times
            //   (batch_count is incremented before this block runs, so first batch = 1)
            // - actual_blocks < batch_block_span/2: runt batch truncated by handoff_target
            let actual_blocks = batch_stats.block_count as f64;
            let is_representative =
                batch_count > 1 && actual_blocks >= (batch_block_span as f64 * 0.5);

            if is_representative && actual_blocks > 0.0 && controllable_ms > 0.0 {
                let sample = controllable_ms / actual_blocks;
                ms_per_block_ema = ms_per_block_ema
                    * (1.0 - BULK_BUILD_MS_PER_BLOCK_ALPHA)
                    + sample * BULK_BUILD_MS_PER_BLOCK_ALPHA;

                if !ms_per_block_ema.is_finite() || ms_per_block_ema <= 0.0 {
                    bail!(
                        "bulk build adaptive sizing: ms_per_block_ema became invalid: \
                         ms_per_block_ema={} sample={} controllable_ms={} actual_blocks={}",
                        ms_per_block_ema,
                        sample,
                        controllable_ms,
                        actual_blocks
                    );
                }

                let desired_f64 = BULK_BUILD_TARGET_ITERATION_MS / ms_per_block_ema;
                if !desired_f64.is_finite() || desired_f64 < 0.0 {
                    bail!(
                        "bulk build adaptive sizing: desired blocks is invalid: \
                         desired_f64={} ms_per_block_ema={} target_ms={}",
                        desired_f64,
                        ms_per_block_ema,
                        BULK_BUILD_TARGET_ITERATION_MS
                    );
                }
                let desired = desired_f64 as u64;

                let step_max = (batch_block_span as f64 * BULK_BUILD_MAX_STEP_RATIO) as u64;
                let step_min = (batch_block_span as f64 / BULK_BUILD_MAX_STEP_RATIO) as u64;

                batch_block_span = desired
                    .clamp(step_min, step_max)
                    .clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
            }
```

- [ ] **Step 2: Update the 4 existing adaptive sizing tests**

The old tests verified the tx-density → 40K target → clamp logic which is now removed. Replace all 4 tests with tests that verify the new EMA-based controller.

Replace `test_adaptive_batch_sizing_sparse_blocks` (lines 5477-5487) with:

```rust
    #[test]
    fn test_adaptive_batch_sizing_sparse_blocks() {
        // Sparse blocks: ms_per_block is low, controller should expand batch.
        // Phase 1 reality: 10K blocks, 466ms controllable → 0.047 ms/blk
        let ms_per_block_ema: f64 = 0.047;
        let desired = (BULK_BUILD_TARGET_ITERATION_MS / ms_per_block_ema) as u64;
        // desired = 1500/0.047 ≈ 31914
        let clamped = desired.clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
        assert!(
            clamped > 30_000 && clamped < 35_000,
            "sparse blocks should expand to ~32K, got {clamped}"
        );
    }
```

Replace `test_adaptive_batch_sizing_dense_blocks` (lines 5489-5499) with:

```rust
    #[test]
    fn test_adaptive_batch_sizing_dense_blocks() {
        // Dense blocks: ms_per_block is high, controller should shrink to floor.
        // Phase 4 reality: 10K blocks, 1880ms controllable → 0.188 ms/blk
        let ms_per_block_ema: f64 = 0.188;
        let desired = (BULK_BUILD_TARGET_ITERATION_MS / ms_per_block_ema) as u64;
        // desired = 1500/0.188 = 7978
        let clamped = desired.clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
        assert_eq!(clamped, BULK_BUILD_MIN_BLOCK_SPAN); // clamped at floor
    }
```

Replace `test_adaptive_batch_sizing_very_sparse` (lines 5501-5511) with:

```rust
    #[test]
    fn test_adaptive_batch_sizing_very_sparse() {
        // Very sparse: ms_per_block is very low, should clamp at ceiling.
        // Hypothetical: 0.01 ms/blk → desired = 150K → clamp to 100K
        let ms_per_block_ema: f64 = 0.01;
        let desired = (BULK_BUILD_TARGET_ITERATION_MS / ms_per_block_ema) as u64;
        let clamped = desired.clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
        assert_eq!(clamped, BULK_BUILD_MAX_BLOCK_SPAN); // clamped at ceiling
    }
```

Replace `test_adaptive_batch_sizing_at_clamp_boundaries` (lines 5513-5527) with:

```rust
    #[test]
    fn test_adaptive_batch_sizing_at_clamp_boundaries() {
        // Exactly at lower boundary: 0.15 ms/blk → 10000 blocks
        let ms_per_block_ema: f64 = BULK_BUILD_TARGET_ITERATION_MS / 10_000.0;
        let desired = (BULK_BUILD_TARGET_ITERATION_MS / ms_per_block_ema) as u64;
        assert_eq!(
            desired.clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN),
            BULK_BUILD_MIN_BLOCK_SPAN,
        );

        // Exactly at upper boundary: 0.015 ms/blk → 100000 blocks
        let ms_per_block_ema: f64 = BULK_BUILD_TARGET_ITERATION_MS / 100_000.0;
        let desired = (BULK_BUILD_TARGET_ITERATION_MS / ms_per_block_ema) as u64;
        assert_eq!(
            desired.clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN),
            BULK_BUILD_MAX_BLOCK_SPAN,
        );
    }
```

- [ ] **Step 3: Verify compilation and tests**

Run: `cargo test -p ckbadger-indexer --lib -- test_adaptive_batch_sizing`

Expected: all 4 tests PASS.

- [ ] **Step 4: Commit**

```
perf(bulk-build): replace 40K-txs target with write-budget EMA controller

The adaptive sizing now targets 1500ms controllable iteration time
(build + prefetch_collect, excluding flush_wait) using a ms-per-block
EMA with alpha=0.5. Skips EMA update for cold-cache first batch and
runt batches near handoff. Fails fast on invalid EMA/desired values.
```

---

## Task 5: Add EMA Behavior Tests

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs` (test module, after existing adaptive tests)

- [ ] **Step 1: Add EMA update test**

```rust
    #[test]
    fn test_adaptive_ema_update() {
        // Verify EMA blending with alpha=0.5
        let mut ema: f64 = BULK_BUILD_INITIAL_MS_PER_BLOCK; // 0.05
        let sample: f64 = 0.10;
        ema = ema * (1.0 - BULK_BUILD_MS_PER_BLOCK_ALPHA)
            + sample * BULK_BUILD_MS_PER_BLOCK_ALPHA;
        // 0.05 * 0.5 + 0.10 * 0.5 = 0.075
        assert!((ema - 0.075).abs() < 1e-10);

        // Second sample converges further
        ema = ema * (1.0 - BULK_BUILD_MS_PER_BLOCK_ALPHA)
            + sample * BULK_BUILD_MS_PER_BLOCK_ALPHA;
        // 0.075 * 0.5 + 0.10 * 0.5 = 0.0875
        assert!((ema - 0.0875).abs() < 1e-10);
    }
```

- [ ] **Step 2: Add step-ratio clamp test**

```rust
    #[test]
    fn test_adaptive_step_ratio_clamp() {
        // Verify per-step 2x limit prevents wild jumps
        let batch_block_span: u64 = 20_000;
        let step_max = (batch_block_span as f64 * BULK_BUILD_MAX_STEP_RATIO) as u64; // 40K
        let step_min = (batch_block_span as f64 / BULK_BUILD_MAX_STEP_RATIO) as u64; // 10K

        // Large desired value clamped to 2x current
        let desired: u64 = 100_000;
        let result = desired
            .clamp(step_min, step_max)
            .clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
        assert_eq!(result, 40_000, "should clamp to 2x current span");

        // Small desired value clamped to 0.5x current
        let desired: u64 = 5_000;
        let result = desired
            .clamp(step_min, step_max)
            .clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
        assert_eq!(result, 10_000, "should clamp to max(0.5x, hard min)");
    }
```

- [ ] **Step 3: Add stall behavior test**

```rust
    #[test]
    fn test_adaptive_stall_stays_at_floor() {
        // When already at min floor and a stall occurs, batch stays at floor.
        let batch_block_span: u64 = BULK_BUILD_MIN_BLOCK_SPAN; // 10K
        let mut ema: f64 = 0.188; // Phase 4 steady state

        // Simulate 15s stall on 10K blocks
        let stall_sample: f64 = 15_000.0 / 10_000.0; // 1.5 ms/blk
        ema = ema * (1.0 - BULK_BUILD_MS_PER_BLOCK_ALPHA)
            + stall_sample * BULK_BUILD_MS_PER_BLOCK_ALPHA;
        // 0.188 * 0.5 + 1.5 * 0.5 = 0.844

        let desired = (BULK_BUILD_TARGET_ITERATION_MS / ema) as u64;
        // 1500/0.844 = 1777
        let step_max = (batch_block_span as f64 * BULK_BUILD_MAX_STEP_RATIO) as u64;
        let step_min = (batch_block_span as f64 / BULK_BUILD_MAX_STEP_RATIO) as u64;
        let result = desired
            .clamp(step_min, step_max)
            .clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
        assert_eq!(result, BULK_BUILD_MIN_BLOCK_SPAN, "stall: stays at floor");
    }
```

- [ ] **Step 4: Add runt-batch and cold-cache skip tests**

```rust
    #[test]
    fn test_adaptive_runt_batch_skip() {
        // Runt batch (actual < span/2) should not update EMA
        let batch_block_span: u64 = 30_000;
        let actual_blocks: f64 = 5_000.0; // < 30000 * 0.5 = 15000
        let is_representative =
            /* batch_count > 0 */ true && actual_blocks >= (batch_block_span as f64 * 0.5);
        assert!(!is_representative, "runt batch should be skipped");
    }

    #[test]
    fn test_adaptive_cold_cache_skip() {
        // First batch: batch_count is already incremented to 1 before the
        // adaptive block runs (line 369), so first batch has batch_count=1.
        // The check `batch_count > 1` skips it.
        let batch_count: u64 = 1; // after increment
        let actual_blocks: f64 = 10_000.0;
        let batch_block_span: u64 = 10_000;
        let is_representative =
            batch_count > 1 && actual_blocks >= (batch_block_span as f64 * 0.5);
        assert!(!is_representative, "cold-cache first batch should be skipped");
    }

    #[test]
    fn test_adaptive_normal_batch_representative() {
        // Normal batch (batch_count > 1 after increment) should update EMA
        let batch_count: u64 = 5;
        let actual_blocks: f64 = 10_000.0;
        let batch_block_span: u64 = 10_000;
        let is_representative =
            batch_count > 1 && actual_blocks >= (batch_block_span as f64 * 0.5);
        assert!(is_representative, "normal batch should be representative");
    }
```

- [ ] **Step 5: Add phase transition convergence test**

```rust
    #[test]
    fn test_adaptive_phase_transition_convergence() {
        // Simulate Phase 1 → Phase 2 transition.
        // Phase 1 steady state: 0.04 ms/blk, batch = 37.5K blocks
        let mut ema: f64 = 0.040;
        let mut batch_block_span: u64 = 37_500;

        // First Phase 2 batch: 37.5K blocks at 0.108 ms/blk → 4050ms
        let sample = 0.108;
        ema = ema * (1.0 - BULK_BUILD_MS_PER_BLOCK_ALPHA)
            + sample * BULK_BUILD_MS_PER_BLOCK_ALPHA;
        // ema = 0.074
        let desired = (BULK_BUILD_TARGET_ITERATION_MS / ema) as u64;
        let step_max = (batch_block_span as f64 * BULK_BUILD_MAX_STEP_RATIO) as u64;
        let step_min = (batch_block_span as f64 / BULK_BUILD_MAX_STEP_RATIO) as u64;
        batch_block_span = desired
            .clamp(step_min, step_max)
            .clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
        // desired ≈ 20270, within step bounds
        assert!(
            batch_block_span < 25_000 && batch_block_span > 15_000,
            "first transition batch should be ~20K, got {}",
            batch_block_span
        );

        // Second Phase 2 batch: converges further toward ~14K
        ema = ema * (1.0 - BULK_BUILD_MS_PER_BLOCK_ALPHA)
            + sample * BULK_BUILD_MS_PER_BLOCK_ALPHA;
        let desired = (BULK_BUILD_TARGET_ITERATION_MS / ema) as u64;
        let step_max = (batch_block_span as f64 * BULK_BUILD_MAX_STEP_RATIO) as u64;
        let step_min = (batch_block_span as f64 / BULK_BUILD_MAX_STEP_RATIO) as u64;
        batch_block_span = desired
            .clamp(step_min, step_max)
            .clamp(BULK_BUILD_MIN_BLOCK_SPAN, BULK_BUILD_MAX_BLOCK_SPAN);
        assert!(
            batch_block_span < 20_000 && batch_block_span > 12_000,
            "second transition batch should converge to ~16K, got {}",
            batch_block_span
        );
    }
```

- [ ] **Step 6: Add fail-fast validation test**

```rust
    #[test]
    fn test_adaptive_ema_fail_fast_invariants() {
        // Verify that the EMA must remain finite and positive
        let ms_per_block_ema: f64 = 0.05;
        assert!(ms_per_block_ema.is_finite() && ms_per_block_ema > 0.0);

        // Verify desired_f64 is finite for valid EMA
        let desired_f64 = BULK_BUILD_TARGET_ITERATION_MS / ms_per_block_ema;
        assert!(desired_f64.is_finite() && desired_f64 > 0.0);

        // Verify that zero EMA would be caught (division produces Inf)
        let bad_ema: f64 = 0.0;
        let bad_desired = BULK_BUILD_TARGET_ITERATION_MS / bad_ema;
        assert!(
            !bad_desired.is_finite() || bad_desired < 0.0,
            "zero EMA should produce non-finite desired"
        );
    }
```

- [ ] **Step 7: Run all new tests**

Run: `cargo test -p ckbadger-indexer --lib -- test_adaptive`

Expected: all tests PASS.

- [ ] **Step 8: Commit**

```
test(bulk-build): add EMA behavior tests for write-budget controller

Tests cover: EMA blending arithmetic, per-step 2x clamp, stall-at-floor
behavior, runt-batch skip, cold-cache skip, representative batch check,
fail-fast invariants, and Phase 1→2 transition convergence.
```

---

## Task 6: Integration Verification

- [ ] **Step 1: Run full indexer test suite**

Run: `cargo test -p ckbadger-indexer`

Expected: all tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -p ckbadger-indexer`

Expected: no new warnings.

- [ ] **Step 3: Build release**

Run: `cargo build -p ckbadger --release`

Expected: successful build.

---

## Expected Impact

| Phase | Current batches | After A+B | Batch time (target) |
|-------|----------------|-----------|-------------------|
| 1 (0-4M) | 400 | ~107 | ~1500ms |
| 2 (4-8M) | 400 | ~288 | ~1500ms |
| 3 (8-12M) | 400 | ~365 | ~1500ms |
| 4 (12-16M) | 400 | 400 (at min) | ~1880ms (unchanged) |
| 5 (16-19M) | 289 | ~222 | ~1500ms |
| **Total** | **1889** | **~1382** | |

Conservative estimate: **~200-400s saved** (10-20% of 2096s baseline).

## Principle Alignment

- **CKB Native**: No domain logic changes, pure performance optimization
- **Local First**: Faster bulk rebuild = cheaper experiments
- **Agent Friendly**: New `controllable_ms` timing field improves observability
