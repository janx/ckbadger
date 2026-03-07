# Bulk Sync Phase 2b Retune Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Retune the adaptive bulk-sync controller so a fresh-db end-to-end sync finishes faster than `run-20260307T100056.237Z-pid1153755` (`1:19:34`) while keeping the phase-1 parser refactor intact.

**Architecture:** Treat the current phase-2 result as a failed policy experiment, not a failed signal integration. Keep the new observability (`writer_queue_fill_pct`, `l0_files_total`) but change the controller so it optimizes for fresh-db wall-clock instead of per-batch latency. Judge every change against the post-phase-1 baseline run, not against pre-phase-1 history and not against local micro-metrics in isolation.

**Tech Stack:** Rust, Tokio, RocksDB, inline unit tests in `ckbadger-indexer`, perf artifacts under `temp/perf/bulk-sync`

---

### Task 1: Make bulk-sync perf artifacts report the real optimization target

**Files:**

- Modify: `crates/indexer/src/bulk_sync_perf.rs`
- Test: `crates/indexer/src/bulk_sync_perf.rs`

**Step 1: Write the failing tests**

Add focused tests in `crates/indexer/src/bulk_sync_perf.rs` for new report fields that expose wall-clock-oriented metrics directly.

```rust
#[test]
fn test_bulk_sync_perf_report_includes_wall_clock_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let writer = BulkSyncPerfWriter::new(dir.path().to_path_buf(), "run-1").unwrap();

    writer.write_metadata().unwrap();
    writer.write_status_completed_for_test(1_000, 4_774).unwrap();
    writer
        .write_metrics_for_test(BulkSyncPerfMetrics {
            batches: 3_807,
            blocks: 18_785_192,
            avg_batch_seconds: 1.102,
            p95_batch_seconds: 2.281,
            p99_batch_seconds: 4.871,
            avg_commit_ms: 719.457,
            p95_commit_ms: 1456.662,
            p99_commit_ms: 2593.383,
            max_compaction_pending_mb: 3972,
            max_l0_files: 119,
            max_imm_memtables: 36,
            wall_clock_seconds: 4_774,
            blocks_per_sec_wall: 3934.9,
            blocks_per_batch: 4934.4,
            total_commit_seconds: 2739.0,
        })
        .unwrap();
    writer.write_report().unwrap();

    let report = std::fs::read_to_string(dir.path().join("run-1/report.md")).unwrap();
    assert!(report.contains("wall_clock_seconds"));
    assert!(report.contains("blocks_per_sec_wall"));
    assert!(report.contains("blocks_per_batch"));
    assert!(report.contains("total_commit_seconds"));
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p ckbadger-indexer test_bulk_sync_perf_report_includes_wall_clock_metrics -- --nocapture
```

Expected: FAIL because the metrics struct and report rendering do not include wall-clock fields yet.

**Step 3: Write minimal implementation**

- Extend `BulkSyncPerfMetrics` in `crates/indexer/src/bulk_sync_perf.rs` with:
  - `wall_clock_seconds`
  - `blocks_per_sec_wall`
  - `blocks_per_batch`
  - `total_commit_seconds`
- Compute these fields from `metadata.env`, `status.env`, and current aggregate metrics.
- Persist the new fields into `metrics.env`.
- Render them into `report.md` and baseline comparison.
- Keep existing batch/commit metrics; do not remove them.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer bulk_sync_perf::tests:: -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs
git commit -m "feat: add wall-clock bulk sync perf metrics"
```

### Task 2: Retune adaptive backoff for fresh-db wall-clock instead of batch latency

**Files:**

- Modify: `crates/indexer/src/sync/adaptive.rs`
- Modify: `crates/indexer/src/sync/pipeline.rs`
- Test: `crates/indexer/src/sync/adaptive.rs`

**Step 1: Write the failing tests**

Add tests in `crates/indexer/src/sync/adaptive.rs` that encode the new wall-clock-oriented controller behavior.

```rust
#[test]
fn test_update_after_write_writer_queue_full_but_healthy_cost_keeps_far_bulk_target() {
    let controller = AdaptiveBatchController::new(8);
    seed_far_bulk_snapshot(&controller, 160_000, 8, 40_000);
    seed_txps_ema_for_mild_drop(&controller);

    let adjustment = controller.update_after_write(AdaptiveBatchInput {
        write_ms: 336.0,
        commit_ms: 135.6,
        batch_tx_count: 5_488,
        blocks_remaining: 18_700_000,
        parse_queue_fill_pct: Some(3.4),
        writer_queue_fill_pct: Some(100.0),
        memory_ratio_pct: Some(55.0),
        l0_files_total: Some(0),
        l0_files_max: Some(0),
        compaction_pending_bytes: Some(0),
        immutable_memtables: Some(0),
        severe_pending_threshold: 1_000_000,
        moderate_pending_threshold: 500_000,
        severe_imm_threshold: 64,
        moderate_imm_threshold: 32,
    });

    assert!(adjustment.is_none(), "writer backlog alone should not shard healthy far-bulk batches");
}

#[test]
fn test_update_after_write_l0_total_without_write_slowdown_does_not_backoff_in_far_bulk() {
    let controller = AdaptiveBatchController::new(8);
    seed_far_bulk_snapshot(&controller, 160_000, 8, 40_000);

    let adjustment = controller.update_after_write(test_input_with_l0_total_only());

    assert!(adjustment.is_none(), "l0_total alone should stay diagnostic until write cost actually degrades");
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_update_after_write_writer_queue_full_but_healthy_cost_keeps_far_bulk_target -- --nocapture
cargo test -p ckbadger-indexer test_update_after_write_l0_total_without_write_slowdown_does_not_backoff_in_far_bulk -- --nocapture
```

Expected: FAIL because the current controller still backs off aggressively on writer-only pressure and standalone `l0_total`.

**Step 3: Write minimal implementation**

- Keep the new phase-2 telemetry fields and logging in place.
- Change `update_after_write()` in `crates/indexer/src/sync/adaptive.rs` so:
  - writer-queue-only pressure does not reduce `target_batch_txs` unless there is also real throughput degradation or unhealthy absolute write cost
  - standalone `l0_total` does not trigger far-bulk moderate backoff by itself when `write_ms`, `commit_ms`, and `write_us_per_tx` are still healthy
  - far-bulk floor relaxation is harder to trigger than it is now; do not let the controller dwell at `40_000` unless sustained pressure is backed by actual write slowdown
- Keep `crates/indexer/src/sync/pipeline.rs` logging fields unchanged unless the tests need a small helper refactor.
- Do not touch parser code and do not start `T1_cells` instrumentation here.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer sync::adaptive::tests:: -- --nocapture
cargo test -p ckbadger-indexer sync::pipeline::tests:: -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/adaptive.rs crates/indexer/src/sync/pipeline.rs
git commit -m "refactor: retune adaptive bulk sync backoff for wall-clock"
```

### Task 3: Verify fresh-db wall-clock against the phase-1 winner

**Files:**

- Modify: `crates/indexer/src/bulk_sync_perf.rs`
- Modify: `crates/indexer/src/sync/adaptive.rs`
- Modify: `crates/indexer/src/sync/pipeline.rs`

**Step 1: Run targeted verification**

Run:

```bash
cargo test -p ckbadger-indexer bulk_sync_perf::tests:: -- --nocapture
cargo test -p ckbadger-indexer sync::adaptive::tests:: -- --nocapture
cargo test -p ckbadger-indexer sync::pipeline::tests:: -- --nocapture
```

Expected: PASS

**Step 2: Run the fresh-db wall-clock verification**

Run:

```bash
ckbadger purge
ckbadger run
```

Expected:

- A new completed artifact appears under `temp/perf/bulk-sync/latest/`.
- `wall_clock_seconds < 4774` (beats `run-20260307T100056.237Z-pid1153755`)
- `blocks_per_sec_wall > 3934.9`
- `batches` should no longer explode toward the `6350` level from `run-20260307T115905.242Z-pid3173595`
- controller logs should show materially fewer `new_target_batch_txs=40000` adjustments than the current `3209` count

**Step 3: Stop rule**

If the fresh-db run does **not** beat `1:19:34`, stop here.

- Do **not** start `T1_cells`.
- Compare the new wall-clock report directly against:
  - `run-20260307T100056.237Z-pid1153755` as the current winner
  - `run-20260307T115905.242Z-pid3173595` as the over-backoff failure case
- Only after a controller variant wins on wall-clock should the plan return to write-path micro-optimization.

**Step 4: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs crates/indexer/src/sync/adaptive.rs crates/indexer/src/sync/pipeline.rs
git commit -m "test: verify adaptive retune against fresh-db wall clock"
```
