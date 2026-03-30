# Logging & Perf Artifacts Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make perf artifacts self-describing so bottlenecks can be diagnosed without cross-referencing source code.

**Architecture:** Four independent changes: (1) remove dead fields, (2) add block-range + controller state to samples, (3) add auto-diagnosis to report, (4) add tracing spans. Changes 1-3 touch `bulk_sync_perf.rs` and related structs; change 4 touches bulk_build and pipeline modules.

**Tech Stack:** Rust, tracing 0.1, serde, RocksDB perf artifacts (JSONL/env/md)

**Spec:** `docs/superpowers/specs/2026-03-30-logging-perf-artifacts-optimization-design.md`

---

### Task 1: Remove dead `t1_ms..t7_ms` fields from BatchWriteMetrics

**Files:**
- Modify: `crates/indexer/src/sync/types.rs:138-156`
- Modify: `crates/indexer/src/sync/batch.rs:3252-3270`

- [ ] **Step 1: Remove fields from BatchWriteMetrics**

In `crates/indexer/src/sync/types.rs`, replace the struct:

```rust
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BatchWriteMetrics {
    pub(crate) commit_ms: f64,
    pub(crate) write_ms: f64,
    pub(crate) prefetch_ms: f64,
    pub(crate) finalize_ms: f64,
    pub(crate) txs: u64,
    pub(crate) cells: u64,
    pub(crate) inputs: u64,
}
```

- [ ] **Step 2: Remove thread_ms array and field assignments in batch.rs**

In `crates/indexer/src/sync/batch.rs`, replace lines 3252-3270 with:

```rust
        Ok(BatchWriteMetrics {
            commit_ms: write_commit_ms,
            write_ms,
            prefetch_ms: 0.0,
            finalize_ms,
            txs: u64::try_from(batch_tx_count).expect("parsed batch tx count exceeds u64"),
            cells: u64::try_from(batch_cell_count).expect("parsed batch cell count exceeds u64"),
            inputs: u64::try_from(batch_input_count).expect("parsed batch input count exceeds u64"),
        })
```

- [ ] **Step 3: Run cargo check**

Run: `cargo check -p ckbadger-indexer 2>&1 | head -50`
Expected: Compiler errors in `pipeline.rs` referencing removed fields — those are fixed in Task 2.

- [ ] **Step 4: Commit**

```bash
git add crates/indexer/src/sync/types.rs crates/indexer/src/sync/batch.rs
git commit -m "refactor: remove dead t1_ms..t7_ms fields from BatchWriteMetrics"
```

---

### Task 2: Remove dead `t1_ms..t7_ms` fields from BatchSample and pipeline

**Files:**
- Modify: `crates/indexer/src/bulk_sync_perf.rs` (BatchSample struct + BatchSample::new)
- Modify: `crates/indexer/src/sync/pipeline.rs:2605-2614`

- [ ] **Step 1: Remove fields from BatchSample struct**

In `crates/indexer/src/bulk_sync_perf.rs`, remove these 10 fields from the `BatchSample` struct (lines ~51-60):

```rust
    pub t1_ms: f64,
    pub t1b_ms: f64,
    pub t2_ms: f64,
    pub t4_ms: f64,
    pub t5_ms: f64,
    pub t6a_ms: f64,
    pub t6b_ms: f64,
    pub t7_ms: f64,
    pub t_act_ms: f64,
    pub t_track_ms: f64,
```

- [ ] **Step 2: Remove field defaults from BatchSample::new()**

In `BatchSample::new()` (around lines 130-143), remove the 10 corresponding zero-init lines:

```rust
            t1_ms: 0.0,
            t1b_ms: 0.0,
            t2_ms: 0.0,
            t4_ms: 0.0,
            t5_ms: 0.0,
            t6a_ms: 0.0,
            t6b_ms: 0.0,
            t7_ms: 0.0,
            t_act_ms: 0.0,
            t_track_ms: 0.0,
```

- [ ] **Step 3: Remove field assignments in pipeline.rs**

In `crates/indexer/src/sync/pipeline.rs`, remove lines 2605-2614:

```rust
                            t1_ms: write_metrics.t1_ms,
                            t1b_ms: write_metrics.t1b_ms,
                            t2_ms: write_metrics.t2_ms,
                            t4_ms: write_metrics.t4_ms,
                            t5_ms: write_metrics.t5_ms,
                            t6a_ms: write_metrics.t6a_ms,
                            t6b_ms: write_metrics.t6b_ms,
                            t7_ms: write_metrics.t7_ms,
                            t_act_ms: write_metrics.t_act_ms,
                            t_track_ms: write_metrics.t_track_ms,
```

- [ ] **Step 4: Run cargo check to verify clean compile**

Run: `cargo check -p ckbadger-indexer 2>&1 | tail -5`
Expected: Compiles successfully with no errors.

- [ ] **Step 5: Run existing tests**

Run: `cargo test -p ckbadger-indexer --lib -- bulk_sync_perf 2>&1 | tail -20`
Expected: All existing `bulk_sync_perf` tests pass. JSONL assertions that previously matched `t1_ms` etc. may need updating in the next step.

- [ ] **Step 6: Fix any test assertions that reference removed fields**

If any test checks for the presence of `t1_ms` etc. in JSONL output, remove those assertions. The test `test_batch_samples_include_engine_and_bulk_build_sub_step_fields` and `test_pipeline_batch_sample_defaults_to_pipeline_engine` are the most likely candidates — check their assertions and remove any lines that assert on `t1_ms..t_track_ms`.

- [ ] **Step 7: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs crates/indexer/src/sync/pipeline.rs
git commit -m "refactor: remove dead t1_ms..t7_ms fields from BatchSample and pipeline"
```

---

### Task 3: Add `start_block`, `end_block`, `batch_index`, `bottleneck` to BatchSample

**Files:**
- Modify: `crates/indexer/src/bulk_sync_perf.rs` (BatchSample struct, new(), TrendEntry)
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs` (~lines 183-389)
- Modify: `crates/indexer/src/sync/pipeline.rs` (~lines 2596-2631)

- [ ] **Step 1: Add fields to BatchSample struct**

In `crates/indexer/src/bulk_sync_perf.rs`, add these fields to the `BatchSample` struct after the `engine` field:

```rust
    pub start_block: u64,
    pub end_block: u64,
    pub batch_index: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottleneck: Option<String>,
```

- [ ] **Step 2: Add field defaults to BatchSample::new()**

In `BatchSample::new()`, add after `engine: "pipeline".to_string(),`:

```rust
            start_block: 0,
            end_block: 0,
            batch_index: 0,
            bottleneck: None,
```

- [ ] **Step 3: Populate fields in bulk_build/mod.rs**

In `crates/indexer/src/sync/bulk_build/mod.rs`, add a `last_bottleneck` tracker before the loop (around line 175):

```rust
        let mut last_bottleneck: Option<String> = None;
```

After the controller observation block (around line 416-457), update the tracker:

```rust
            if let Some(output) = controller.observe(&BatchSignals { ... }) {
                // ... existing code ...
                last_bottleneck = Some(output.bottleneck.to_string());
                // ... existing indexer.bulk_build_perf.record_controller(...) ...
            }
```

When constructing the sample (around line 266-330), set the new fields after `sample.engine = "bulk_build".to_string();`:

```rust
            sample.start_block = current_block;
            sample.end_block = last_block_u64;
            sample.batch_index = batch_count;
            sample.bottleneck = last_bottleneck.clone();
```

Note: `current_block` is captured at line 183 (start of iteration). `last_block_u64` is computed at line 252. `batch_count` is 0-indexed (incremented at line 389 after sample recording).

- [ ] **Step 4: Populate fields in pipeline.rs**

In `crates/indexer/src/sync/pipeline.rs`, add a batch counter before the writer loop. Find the writer loop start (around line 2088). Add before it:

```rust
                    let mut pipeline_batch_index: u64 = 0;
```

In the `BatchSample` construction (around line 2596), set the new fields:

```rust
                        self.record_bulk_sync_perf_batch_sample(BatchSample {
                            start_block,
                            end_block,
                            batch_index: pipeline_batch_index,
                            bottleneck: None,
                            txs: write_metrics.txs,
                            // ... rest unchanged ...
                        });
```

After the batch is fully processed (after `self.perf.report_and_reset();`), increment:

```rust
                    pipeline_batch_index += 1;
```

Note: `start_block` and `end_block` are already in scope from the destructured `ParsedBatch` (lines 2097-2098). The same `pipeline_batch_index` variable will be reused in Task 7 for the span.

- [ ] **Step 5: Run cargo check**

Run: `cargo check -p ckbadger-indexer 2>&1 | tail -5`
Expected: Compiles cleanly.

- [ ] **Step 6: Update test helper and add assertion**

In `crates/indexer/src/bulk_sync_perf.rs`, update the `test_batch_sample` helper to verify defaults:

Add a new test:

```rust
    #[test]
    fn test_batch_sample_includes_block_range_and_batch_index() {
        let mut sample = test_batch_sample(100, 2.0, 50.0, 0, 0, 0);
        assert_eq!(sample.start_block, 0);
        assert_eq!(sample.end_block, 0);
        assert_eq!(sample.batch_index, 0);
        assert_eq!(sample.bottleneck, None);

        sample.start_block = 1000;
        sample.end_block = 1099;
        sample.batch_index = 5;
        sample.bottleneck = Some("build".to_string());

        let json = serde_json::to_string(&sample).unwrap();
        assert!(json.contains("\"start_block\":1000"));
        assert!(json.contains("\"end_block\":1099"));
        assert!(json.contains("\"batch_index\":5"));
        assert!(json.contains("\"bottleneck\":\"build\""));
    }

    #[test]
    fn test_batch_sample_omits_bottleneck_when_none() {
        let sample = test_batch_sample(10, 1.0, 20.0, 0, 0, 0);
        let json = serde_json::to_string(&sample).unwrap();
        assert!(!json.contains("bottleneck"));
    }
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p ckbadger-indexer --lib -- bulk_sync_perf 2>&1 | tail -20`
Expected: All tests pass including the new ones.

- [ ] **Step 8: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs crates/indexer/src/sync/bulk_build/mod.rs crates/indexer/src/sync/pipeline.rs
git commit -m "feat(perf): add start_block, end_block, batch_index, bottleneck to BatchSample"
```

---

### Task 4: Add auto-diagnosis section to report.md

**Files:**
- Modify: `crates/indexer/src/bulk_sync_perf.rs` (add `build_diagnosis` fn, call from `write_report`)

- [ ] **Step 1: Write the test first**

Add to the test module in `crates/indexer/src/bulk_sync_perf.rs`:

```rust
    #[test]
    fn test_diagnosis_identifies_disk_io_bottleneck() {
        let mut metrics = build_test_metrics(10, 100);
        metrics.p95_disk_await_ms = Some(12.0);
        metrics.saturated_window_ratio = Some(0.5);
        let findings = build_diagnosis(&metrics, None);
        assert!(findings.iter().any(|f| f.contains("Disk I/O latency")));
    }

    #[test]
    fn test_diagnosis_identifies_commit_dominance() {
        let mut metrics = build_test_metrics(10, 100);
        metrics.total_commit_seconds = 60.0;
        metrics.total_batch_seconds = 100.0;
        let findings = build_diagnosis(&metrics, None);
        assert!(findings.iter().any(|f| f.contains("commit dominates")));
    }

    #[test]
    fn test_diagnosis_identifies_compaction_backlog() {
        let mut metrics = build_test_metrics(10, 100);
        metrics.max_l0_files = 40;
        metrics.max_imm_memtables = 10;
        let findings = build_diagnosis(&metrics, None);
        assert!(findings.iter().any(|f| f.contains("compaction backlog")));
    }

    #[test]
    fn test_diagnosis_identifies_large_compaction_pending() {
        let mut metrics = build_test_metrics(10, 100);
        metrics.max_compaction_pending_mb = 400;
        let findings = build_diagnosis(&metrics, None);
        assert!(findings.iter().any(|f| f.contains("pending")));
    }

    #[test]
    fn test_diagnosis_identifies_high_stall_rate() {
        let mut metrics = build_test_metrics(100, 1000);
        metrics.stall_count = 15;
        let findings = build_diagnosis(&metrics, None);
        assert!(findings.iter().any(|f| f.contains("Stall rate")));
    }

    #[test]
    fn test_diagnosis_identifies_cpu_pressure() {
        let mut metrics = build_test_metrics(10, 100);
        metrics.avg_load_avg_1m = 60.0;
        let env = EnvironmentSnapshot {
            cpu_cores: 16,
            ..Default::default()
        };
        let findings = build_diagnosis(&metrics, Some(&env));
        assert!(findings.iter().any(|f| f.contains("CPU pressure")));
    }

    #[test]
    fn test_diagnosis_returns_no_bottleneck_when_clean() {
        let metrics = build_test_metrics(10, 100);
        let findings = build_diagnosis(&metrics, None);
        assert!(findings.iter().any(|f| f.contains("No bottleneck")));
    }
```

Also add a helper:

```rust
    fn build_test_metrics(batches: u64, blocks: u64) -> BulkSyncPerfMetrics {
        BulkSyncPerfMetrics {
            run_id: "test-run".to_string(),
            status: "completed".to_string(),
            started_at_utc: "2026-01-01T00:00:00.000Z".to_string(),
            finished_at_utc: Some("2026-01-01T01:00:00.000Z".to_string()),
            wall_clock_seconds: 3600.0,
            batches,
            blocks,
            total_txs: blocks * 2,
            blocks_per_sec_wall: blocks as f64 / 3600.0,
            txs_per_sec_wall: (blocks * 2) as f64 / 3600.0,
            blocks_per_batch: blocks as f64 / batches as f64,
            avg_batch_seconds: 3.0,
            p95_batch_seconds: 5.0,
            p99_batch_seconds: 8.0,
            total_commit_seconds: 10.0,
            avg_commit_ms: 100.0,
            p95_commit_ms: 200.0,
            p99_commit_ms: 300.0,
            finalize_seconds: 60.0,
            max_compaction_pending_mb: 100,
            max_l0_files: 10,
            max_imm_memtables: 3,
            avg_load_avg_1m: 4.0,
            max_load_avg_1m: 8.0,
            min_mem_available_mb: 8000,
            avg_disk_write_mb_per_batch: 50.0,
            avg_disk_util_pct: Some(40.0),
            p95_disk_util_pct: Some(60.0),
            avg_disk_await_ms: Some(2.0),
            p95_disk_await_ms: Some(4.0),
            max_disk_avg_queue_depth: Some(0.5),
            peak_disk_write_mb_s: Some(200.0),
            peak_disk_write_iops: Some(5000.0),
            saturated_window_count: 0,
            saturated_window_ratio: Some(0.0),
            disk_telemetry_status: "available".to_string(),
            peak_owner_memory_bytes: HashMap::new(),
            peak_live_cell_count: 0,
            streamed_history_rows: 0,
            sealed_aggregate_rows: 0,
            final_snapshot_rows: 0,
            history_flushes: 0,
            sealed_aggregate_flushes: 0,
            final_snapshot_flushes: 0,
            total_batch_seconds: 30.0,
            stall_count: 0,
        }
    }
```

- [ ] **Step 2: Run tests to confirm they fail**

Run: `cargo test -p ckbadger-indexer --lib -- test_diagnosis 2>&1 | tail -10`
Expected: Compilation error — `build_diagnosis` not defined yet.

- [ ] **Step 3: Implement build_diagnosis function**

Add this function in `crates/indexer/src/bulk_sync_perf.rs` (before the `#[cfg(test)]` module, after the helper functions):

```rust
fn build_diagnosis(
    metrics: &BulkSyncPerfMetrics,
    environment: Option<&crate::sys_info::EnvironmentSnapshot>,
) -> Vec<String> {
    let mut findings = Vec::new();

    // Disk I/O latency
    if let (Some(p95_await), Some(sat_ratio)) =
        (metrics.p95_disk_await_ms, metrics.saturated_window_ratio)
    {
        if p95_await > DISK_AWAIT_SATURATION_THRESHOLD_MS && sat_ratio > 0.3 {
            findings.push(format!(
                "Disk I/O latency is the primary bottleneck. p95 disk await {:.1}ms with {:.0}% saturated windows.",
                p95_await,
                sat_ratio * 100.0,
            ));
        }
    }

    // Commit dominance
    if metrics.total_batch_seconds > 0.0 {
        let commit_ratio = metrics.total_commit_seconds / metrics.total_batch_seconds;
        if commit_ratio > 0.5 {
            findings.push(format!(
                "RocksDB commit dominates batch time ({:.0}% of wall clock). Consider tuning write_buffer_size or max_background_jobs.",
                commit_ratio * 100.0,
            ));
        }
    }

    // RocksDB compaction backlog
    if metrics.max_l0_files > L0_BACKLOG_THRESHOLD as u64
        || metrics.max_imm_memtables > IMM_MEMTABLE_BACKLOG_THRESHOLD as u64
    {
        findings.push(format!(
            "RocksDB compaction backlog detected (L0={}, immutable memtables={}). Write stalls likely.",
            metrics.max_l0_files, metrics.max_imm_memtables,
        ));
    }

    // Large compaction pending
    if metrics.max_compaction_pending_mb > COMPACTION_BACKLOG_THRESHOLD_MB as u64 {
        findings.push(format!(
            "Large compaction backlog ({}MB pending). Disk throughput may be insufficient.",
            metrics.max_compaction_pending_mb,
        ));
    }

    // High stall rate
    if metrics.stall_count > 0 && metrics.batches > 0 {
        let stall_ratio = metrics.stall_count as f64 / metrics.batches as f64;
        if stall_ratio > 0.1 {
            findings.push(format!(
                "Stall rate is {:.0}% ({} stalls / {} batches). Check per-batch samples for outliers.",
                stall_ratio * 100.0,
                metrics.stall_count,
                metrics.batches,
            ));
        }
    }

    // CPU pressure
    if let Some(env) = environment {
        if env.cpu_cores > 0 {
            let threshold = env.cpu_cores as f64 * 1.5;
            if metrics.avg_load_avg_1m > threshold {
                findings.push(format!(
                    "System under CPU pressure (avg load {:.1} vs {} cores).",
                    metrics.avg_load_avg_1m, env.cpu_cores,
                ));
            }
        }
    }

    // Default when no issues found
    if findings.is_empty() {
        findings.push(
            "No bottleneck detected from aggregate metrics. Check per-batch samples for localized anomalies.".to_string(),
        );
    }

    findings
}
```

- [ ] **Step 4: Run diagnosis tests**

Run: `cargo test -p ckbadger-indexer --lib -- test_diagnosis 2>&1 | tail -20`
Expected: All 7 diagnosis tests pass.

- [ ] **Step 5: Wire diagnosis into write_report**

In `write_report()`, add the diagnosis section after the stall events section call (`self.write_report_stall_events(...)`) and before `content.push_str("## System Pressure\n\n")`:

```rust
        // Diagnosis section
        let findings = build_diagnosis(metrics, self.environment.as_ref());
        content.push_str("## Diagnosis\n\n");
        for finding in &findings {
            content.push_str(&format!("- {}\n", finding));
        }
        content.push('\n');
```

- [ ] **Step 6: Add integration test for diagnosis in report**

```rust
    #[test]
    fn test_report_includes_diagnosis_section() {
        let dir = TempDir::new().unwrap();
        let mut run =
            BulkSyncPerfRun::start_for_test(dir.path(), "run-diag", TEST_BUILD_VERSION).unwrap();
        let mut sample = test_batch_sample(100, 2.0, 50.0, 0, 0, 0);
        sample.disk_state = Some("saturated".to_string());
        sample.disk_await_ms = Some(15.0);
        sample.disk_util_pct = Some(95.0);
        run.record_batch_sample(sample).unwrap();
        run.finish_completed().unwrap();

        let report = fs::read_to_string(dir.path().join("run-diag/report.md")).unwrap();
        assert!(report.contains("## Diagnosis"));
    }
```

- [ ] **Step 7: Run all perf tests**

Run: `cargo test -p ckbadger-indexer --lib -- bulk_sync_perf 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs
git commit -m "feat(perf): add auto-diagnosis section to bulk sync perf report"
```

---

### Task 5: Add tracing spans for bulk-build batch loop

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs` (batch loop body)

- [ ] **Step 1: Add batch span wrapping the loop body**

In `crates/indexer/src/sync/bulk_build/mod.rs`, at the top of the loop body (after `let current_block = indexer.progress.current();` at line 183), add:

```rust
            let batch_span = tracing::info_span!(
                "bulk_batch",
                batch_index = batch_count,
                start_block = current_block,
                end_block = tracing::field::Empty,
            );
            let _batch_guard = batch_span.enter();
```

After `last_block_u64` is computed (line 252), record the end block:

```rust
            batch_span.record("end_block", last_block_u64);
```

- [ ] **Step 2: Run cargo check**

Run: `cargo check -p ckbadger-indexer 2>&1 | tail -5`
Expected: Compiles cleanly. The `tracing` crate is already a dependency.

- [ ] **Step 3: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(tracing): add info_span for bulk-build batch loop"
```

---

### Task 6: Add tracing spans for finalize phases

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs` (finalize section, lines ~472-614)

- [ ] **Step 1: Add spans to each finalize phase**

Replace each finalize phase block with a span-wrapped version. The pattern for each phase is:

```rust
        // Phase 0: close channel and drain all queued flushes.
        {
            let _guard = tracing::info_span!("bulk_finalize", phase = 1, label = "drain_flush").entered();
            indexer
                .bulk_build_perf
                .record_finalize_step(1, finalize_started.elapsed());
            // ... existing phase code ...
        }
```

Apply this pattern to all 13 phases. Phase labels:

| Step | Label |
|---|---|
| 1 | `drain_flush` |
| 2 | `activity_stats` |
| 3 | `chain_stats` |
| 4 | `final_snapshot` |
| 5 | `owner_address` |
| 6 | `owner_script` |
| 7 | `owner_token` |
| 8 | `owner_dao` |
| 9 | `owner_fiber` |
| 10 | `owner_object` |
| 11 | `metadata` |
| 12 | `memtable_flush` |
| 13 | `sync_cleanup` |

Note: Phase 0 (drain_flush) contains an early-return error path (`return Err(err)`). The span guard will be dropped on early return, which is correct behavior. However, the `flush_drain.wait().await` inside the error path must remain within the span scope. Structure the block carefully:

```rust
        {
            let _guard = tracing::info_span!("bulk_finalize", phase = 1, label = "drain_flush").entered();
            indexer
                .bulk_build_perf
                .record_finalize_step(1, finalize_started.elapsed());
            let flush_drain = flush_channel.begin_shutdown();
            let prepared_finalize = match runtime.prepare_finalize_artifacts() {
                Ok(prepared) => prepared,
                Err(err) => {
                    let _ = flush_drain.wait().await;
                    return Err(err);
                }
            };
            let flush_stats = flush_drain.wait().await?;
            materializer.add_external_counts(
                flush_stats.total_history_rows,
                flush_stats.total_sealed_rows,
                flush_stats.flush_count,
            );
            info!(
                "flush pipeline: prepare={:.1}s commit={:.1}s flushes={} rows={}",
                flush_stats.total_prepare_ms / 1000.0,
                flush_stats.total_commit_ms / 1000.0,
                flush_stats.flush_count,
                flush_stats.total_history_rows + flush_stats.total_sealed_rows,
            );
        }
```

For the remaining phases (2-13), the pattern is simpler since they don't have early returns. Example for phase 2:

```rust
        // Phase 1: activity stats
        {
            let _guard = tracing::info_span!("bulk_finalize", phase = 2, label = "activity_stats").entered();
            indexer
                .bulk_build_perf
                .record_finalize_step(2, finalize_started.elapsed());
            materializer.stream_sealed_aggregate_rows(&prepared_finalize.activity_sealed_rows)?;
        }
```

For phases 4-9 (owners), the destructure of `BulkBuildRuntimeState` must happen before the phase blocks, and `owners` must be `mut` accessible across phases. This is already the case — `let mut owners = owners;` at line 540 precedes all owner phases.

- [ ] **Step 2: Run cargo check**

Run: `cargo check -p ckbadger-indexer 2>&1 | tail -5`
Expected: Compiles cleanly. Note: `prepared_finalize` is used across phases 1-3. It must be declared before the phase 1 block and assigned inside it, or the phases must share the variable. Since the existing code uses `let prepared_finalize = ...` inside phase 0, you need to hoist it:

```rust
        let prepared_finalize;
        {
            let _guard = tracing::info_span!("bulk_finalize", phase = 1, label = "drain_flush").entered();
            // ...
            prepared_finalize = match runtime.prepare_finalize_artifacts() { ... };
            // ...
        }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p ckbadger-indexer --lib 2>&1 | tail -10`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(tracing): add info_span for each bulk-build finalize phase"
```

---

### Task 7: Add tracing span for pipeline batch

**Files:**
- Modify: `crates/indexer/src/sync/pipeline.rs` (writer section)

- [ ] **Step 1: Add batch span using existing counter**

In `crates/indexer/src/sync/pipeline.rs`, `pipeline_batch_index` was already added in Task 3. Inside the writer loop, after the `ParsedBatch` is destructured (around line 2095-2100), wrap the processing in a span:

```rust
                Ok(Some(ParsedBatch {
                    batch_epoch,
                    start_block,
                    end_block,
                    chain_tip,
                    batch_tx_count: parsed_batch_tx_count_u64,
                    blocks: all_parsed_blocks,
                })) => {
                    let batch_span = tracing::info_span!(
                        "pipeline_batch",
                        batch_index = pipeline_batch_index,
                        start_block,
                        end_block,
                    );
                    let _batch_guard = batch_span.enter();
```

The counter increment (`pipeline_batch_index += 1`) was already added in Task 3.

- [ ] **Step 2: Run cargo check**

Run: `cargo check -p ckbadger-indexer 2>&1 | tail -5`
Expected: Compiles cleanly.

- [ ] **Step 3: Commit**

```bash
git add crates/indexer/src/sync/pipeline.rs
git commit -m "feat(tracing): add info_span for pipeline batch processing"
```

---

### Task 8: Final verification

- [ ] **Step 1: Full cargo check and clippy**

Run: `cargo check && cargo clippy -p ckbadger-indexer 2>&1 | tail -20`
Expected: No errors, no new warnings.

- [ ] **Step 2: Full test suite**

Run: `cargo test -p ckbadger-indexer --lib 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 3: Verify JSONL sample no longer has dead fields**

Run: `cargo test -p ckbadger-indexer --lib -- test_batch_sample_includes_block_range 2>&1`
Expected: Test passes, confirming new fields are present and old fields are gone.

- [ ] **Step 4: Final commit if any fixups needed**

If clippy or tests required fixes, commit them:

```bash
git add -A && git commit -m "fix: address clippy/test issues from perf artifacts optimization"
```
