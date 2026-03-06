# Parser Bottleneck Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reduce parser-stage stalls by instrumenting precompute hotspots, tightening parser-facing sub-batch sizing, and moving heavy precompute work off the async parser loop.

**Architecture:** Keep the existing parser/write derivation path, but make it observable and easier to control. First add phase timing and tests, then tighten sub-batch boundaries so adaptive backoff actually limits parser work, then extract parser precompute into a blocking helper with identical semantics.

**Tech Stack:** Rust, Tokio, Rayon, tracing, inline unit tests in `ckbadger-indexer`

---

### Task 1: Add parser precompute phase timing

**Files:**

- Create: `docs/plans/2026-03-06-parser-bottleneck-design.md`
- Modify: `crates/indexer/src/sync/pipeline.rs`
- Test: `crates/indexer/src/sync/pipeline.rs`

**Step 1: Write the failing test**

Add a unit test near `crates/indexer/src/sync/pipeline.rs` test module for a new parser phase metrics formatter/helper. The test should assert that the helper exposes the expected phase names and preserves deterministic ordering or field values.

```rust
#[test]
fn test_parser_precompute_phase_metrics_capture_all_expected_fields() {
    let metrics = ParserPrecomputePhaseMetrics {
        build_batch_cell_infos_ms: 10.0,
        compute_fee_ms: 20.0,
        cache_and_balance_ms: 30.0,
        spore_precompute_ms: 40.0,
        nft_precompute_ms: 50.0,
    };

    assert_eq!(metrics.total_ms(), 150.0);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-indexer test_parser_precompute_phase_metrics_capture_all_expected_fields -- --nocapture`

Expected: FAIL because `ParserPrecomputePhaseMetrics` does not exist yet.

**Step 3: Write minimal implementation**

- Add `ParserPrecomputePhaseMetrics` plus `total_ms()` in `crates/indexer/src/sync/pipeline.rs`.
- Wrap the major precompute sections with timers.
- Extend the parser batch log line to include the new phase metrics.

**Step 4: Run test to verify it passes**

Run: `cargo test -p ckbadger-indexer test_parser_precompute_phase_metrics_capture_all_expected_fields -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/pipeline.rs docs/plans/2026-03-06-parser-bottleneck-design.md docs/plans/2026-03-06-parser-bottleneck.md
git commit -m "feat: instrument parser precompute phases"
```

### Task 2: Tighten parser-facing sub-batch planning

**Files:**

- Modify: `crates/indexer/src/sync/adaptive.rs`
- Modify: `crates/indexer/src/sync/pipeline.rs`
- Test: `crates/indexer/src/sync/adaptive.rs`

**Step 1: Write the failing test**

Add tests for cell-aware sub-batch planning and for a stricter parser-facing tx cap.

```rust
#[test]
fn test_plan_fetch_sub_batches_splits_when_cell_cap_is_exceeded() {
    let plan = plan_fetch_sub_batches(&[5, 5, 5], &[5, 5, 5], &[40_000, 50_000, 10_000], 100, 100, 80_000);
    assert_eq!(plan, vec![(2, 10, 10, 90_000), (1, 5, 5, 10_000)]);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-indexer test_plan_fetch_sub_batches_splits_when_cell_cap_is_exceeded -- --nocapture`

Expected: FAIL because planner does not accept cell counts/caps yet.

**Step 3: Write minimal implementation**

- Extend `plan_fetch_sub_batches()` to consider per-block cell counts.
- Introduce a parser-facing cell cap and a less permissive tx cap.
- Update fetch planning in `crates/indexer/src/sync/pipeline.rs` to pass cell counts.

**Step 4: Run test to verify it passes**

Run: `cargo test -p ckbadger-indexer test_plan_fetch_sub_batches_splits_when_cell_cap_is_exceeded -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/adaptive.rs crates/indexer/src/sync/pipeline.rs
git commit -m "feat: tighten parser sub-batch planning"
```

### Task 3: Move parser precompute into blocking execution

**Files:**

- Modify: `crates/indexer/src/sync/pipeline.rs`
- Test: `crates/indexer/src/sync/pipeline.rs`

**Step 1: Write the failing test**

Add a focused unit test around a new extracted helper that runs parser precompute and returns the expected aggregate artifacts.

```rust
#[test]
fn test_run_parser_precompute_produces_expected_artifacts_for_simple_tx() {
    let result = run_parser_precompute(test_parser_precompute_input());
    assert_eq!(result.address_balance_changes.len(), 1);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-indexer test_run_parser_precompute_produces_expected_artifacts_for_simple_tx -- --nocapture`

Expected: FAIL because the helper does not exist yet.

**Step 3: Write minimal implementation**

- Extract the serial precompute section into a helper input/output struct.
- Call that helper through `tokio::task::spawn_blocking`.
- Keep all semantics identical and preserve fail-fast errors.

**Step 4: Run test to verify it passes**

Run: `cargo test -p ckbadger-indexer test_run_parser_precompute_produces_expected_artifacts_for_simple_tx -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/pipeline.rs
git commit -m "refactor: offload parser precompute to blocking task"
```

### Task 4: Verify the targeted parser regression window

**Files:**

- Modify: `temp/run/logs/indexer.log` (read-only for verification, no code change)
- Test: `crates/indexer/src/sync/pipeline.rs`
- Test: `crates/indexer/src/sync/adaptive.rs`

**Step 1: Run targeted tests**

Run: `cargo test -p ckbadger-indexer test_parser_precompute_phase_metrics_capture_all_expected_fields test_plan_fetch_sub_batches_splits_when_cell_cap_is_exceeded test_run_parser_precompute_produces_expected_artifacts_for_simple_tx -- --nocapture`

Expected: PASS

**Step 2: Run broader crate verification**

Run: `cargo test -p ckbadger-indexer --lib -- --nocapture`

Expected: PASS

**Step 3: Re-check log output with the new metrics**

Run:

```bash
rg "Parser batch 1427|Parser batch 1428|Pipeline idle timeout while waiting for parsed batches" temp/run/logs/indexer.log
```

Expected:

- Parser logs now show phase-level timing fields.
- The hotspot can be attributed to a specific sub-phase.
- If replayed on the same workload, parser batch sizing should no longer overshoot adaptive intent as badly.

**Step 4: Commit**

```bash
git add crates/indexer/src/sync/pipeline.rs crates/indexer/src/sync/adaptive.rs
git commit -m "test: verify parser bottleneck mitigation"
```

Plan complete and saved to `docs/plans/2026-03-06-parser-bottleneck.md`.

Two execution options:

1. Subagent-Driven (this session) - I dispatch fresh subagent per task, review between tasks, fast iteration
2. Parallel Session (separate) - Open new session with executing-plans, batch execution with checkpoints

If no preference is given, use option 1 in this session.
