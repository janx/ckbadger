# Logging & Perf Artifacts Optimization

## Goal

Improve the observability stack so that performance regressions and bottlenecks are easier to diagnose from perf artifacts alone, without cross-referencing source code.

## Principle Alignment

- **Local First** — perf artifacts are files on disk; improvements make them more self-describing for local analysis.
- **Agent Friendly** — structured, unambiguous field names and machine-readable diagnosis reduce manual interpretation.

## Changes

### 1. Remove dead `t1_ms..t7_ms` timing fields

**Problem:** `BatchWriteMetrics` contains 10 opaque timing fields (`t1_ms`, `t1b_ms`, `t2_ms`, `t4_ms`, `t5_ms`, `t6a_ms`, `t6b_ms`, `t7_ms`, `t_act_ms`, `t_track_ms`). These are always `[0.0; 10]` in the pipeline writer (`batch.rs:3252`) and never populated by the bulk-build path (which uses semantic `facts_ms`/`resolve_ms`/`reduce_ms`/`history_ms` names). They appear in `BatchSample` and are serialized to JSONL, adding noise to every sample.

**Change:** Remove all 10 fields from:
- `BatchWriteMetrics` in `crates/indexer/src/sync/types.rs`
- `BatchSample` in `crates/indexer/src/bulk_sync_perf.rs`
- `BatchSample::new()` default initialization
- The pipeline writer construction in `batch.rs` and `pipeline.rs`

**Verification:** `cargo check` passes. JSONL samples no longer contain the 10 zero-value fields.

### 2. Add `start_block`, `end_block`, `batch_index`, `bottleneck` to `BatchSample`

**Problem:** Each batch sample records `blocks` (count) but not *which* blocks. There is no way to correlate a slow batch with a specific chain region. Controller bottleneck classification (`Fetch`/`Build`/`Flush`) is recorded to `BulkBuildPerfStats` atomics but never persisted in the per-batch JSONL.

**Change:** Add four fields to `BatchSample`:

| Field | Type | Source |
|---|---|---|
| `start_block` | `u64` | `current_block` at batch start (from `indexer.progress.current()`) |
| `end_block` | `u64` | `last_block_u64` after batch completes |
| `batch_index` | `u64` | 0-based counter incremented per batch |
| `bottleneck` | `Option<String>` | Latest controller classification: `"fetch"`, `"build"`, `"flush"`, or `None` before first controller observation |

Sources in `bulk_build/mod.rs`:
- `current_block` is available at line 183 (start of loop iteration)
- `last_block_u64` is computed at line 252
- `batch_count` serves as batch_index (0-based, incremented after sample recording at line 389)
- `output.bottleneck` from `controller.observe()` at line 416; track the latest value in a local variable

For pipeline-engine samples, `start_block`/`end_block` come from the parsed block range, `batch_index` from the pipeline's internal counter, and `bottleneck` is always `None`.

Also add `start_block`, `end_block`, `batch_index` to `TrendEntry` for cross-run block-range comparison.

**Verification:** JSONL samples contain block ranges. `jq 'select(.sample.start_block > 5000000 and .sample.start_block < 6000000)' samples.jsonl` works to filter by chain region.

### 3. Add auto-diagnosis section to `report.md`

**Problem:** The report contains raw metrics tables but no interpretation. Users must mentally apply threshold rules to diagnose bottlenecks.

**Change:** Add a `## Diagnosis` section after the existing `## Stall Events` section in `write_report()`. The section applies deterministic rules to `BulkSyncPerfMetrics` and emits plain-English findings.

Rules (using existing threshold constants):

| Condition | Finding |
|---|---|
| `p95_disk_await_ms > 8.0` AND `saturated_window_ratio > 0.3` | "Disk I/O latency is the primary bottleneck. p95 disk await {X}ms with {Y}% saturated windows." |
| `total_commit_seconds / total_batch_seconds > 0.5` | "RocksDB commit dominates batch time ({X}% of wall clock). Consider tuning write_buffer_size or max_background_jobs." |
| `max_l0_files > 32` OR `max_imm_memtables > 8` | "RocksDB compaction backlog detected (L0={X}, immutable memtables={Y}). Write stalls likely." |
| `max_compaction_pending_mb > 256` | "Large compaction backlog ({X}MB pending). Disk throughput may be insufficient." |
| `stall_count > 0` AND `stall_count as f64 / batches as f64 > 0.1` | "Stall rate is {X}% ({Y} stalls / {Z} batches). Check per-batch samples for outliers." |
| `avg_load_avg_1m > cpu_cores * 1.5` (if environment captured) | "System under CPU pressure (avg load {X} vs {Y} cores)." |
| None of the above triggered | "No bottleneck detected from aggregate metrics. Check per-batch samples for localized anomalies." |

Implementation: a standalone function `fn build_diagnosis(metrics: &BulkSyncPerfMetrics, environment: Option<&EnvironmentSnapshot>) -> Vec<String>` that returns finding strings. `write_report` joins them into the Diagnosis section.

**Verification:** Generate a report from existing test fixtures; diagnosis section is non-empty.

### 4. Add tracing spans for batch and finalize phases

**Problem:** All log lines are flat — no span context links related events. When debugging a specific batch or finalize step, you must manually correlate by timestamps.

**Change:** Add targeted tracing spans (not `#[instrument]` — manual spans for control over field names).

#### 4a. Bulk-build batch span

Wrap the batch processing loop body (lines ~183-470 in `bulk_build/mod.rs`) in a `tracing::info_span!`:

```rust
let batch_span = tracing::info_span!(
    "bulk_batch",
    batch_index,
    start_block = current_block,
    end_block = tracing::field::Empty,  // filled after build
);
let _guard = batch_span.enter();
// ... after build completes:
batch_span.record("end_block", last_block_u64);
```

All existing `info!`/`debug!` calls within the loop body automatically inherit the span context.

#### 4b. Finalize phase spans

Each of the 13 finalize sub-phases gets a span:

```rust
let _guard = tracing::info_span!("bulk_finalize", phase = 1, label = "drain_flush").entered();
```

Phase labels (matching existing comments):
1. `drain_flush` — close channel, drain queued flushes
2. `activity_stats` — daily + hourly aggregates
3. `chain_stats` — hash rate, difficulty, uncle rate, epoch time
4. `final_snapshot` — live cell markers + index CFs
5. `owner_address` — address owner flush + materialize
6. `owner_script` — script owner flush + materialize
7. `owner_token` — token owner flush + materialize
8. `owner_dao` — DAO owner flush + materialize
9. `owner_fiber` — fiber owner flush + materialize
10. `owner_object` — object owner flush + materialize
11. `metadata` — HODL + cell distribution tracker state
12. `memtable_flush` — RocksDB memtable flush
13. `sync_cleanup` — sync status + session marker cleanup

#### 4c. Pipeline batch span (matching)

Add an equivalent `info_span!("pipeline_batch", batch_index, start_block, end_block)` in the pipeline writer path (`pipeline.rs`) for consistency.

**Verification:** Run with `RUST_LOG=ckbadger_indexer[bulk_batch]=debug` and confirm span context appears in output. Existing tests still pass.

## Files Changed

| File | Changes |
|---|---|
| `crates/indexer/src/sync/types.rs` | Remove 10 `t*_ms` fields from `BatchWriteMetrics` |
| `crates/indexer/src/bulk_sync_perf.rs` | Remove 10 `t*_ms` fields from `BatchSample`; add `start_block`, `end_block`, `batch_index`, `bottleneck`; add `build_diagnosis()` fn; add diagnosis section to `write_report()` |
| `crates/indexer/src/sync/batch.rs` | Remove `thread_ms` array and `t*_ms` field assignments |
| `crates/indexer/src/sync/pipeline.rs` | Remove `t*_ms` field assignments from sample construction; add pipeline_batch span |
| `crates/indexer/src/sync/bulk_build/mod.rs` | Pass `start_block`/`end_block`/`batch_index`/`bottleneck` to sample; add batch span + finalize spans |

## Not in Scope

- FlightRecorder restructuring (low priority, separate concern)
- `debug!/trace!` enrichment across the codebase (ongoing, not discrete)
- trend.jsonl environment hash (deferred)
- Per-CF RocksDB write amplification metrics
- jemalloc allocator integration

## Re-sync Required

No. These changes only affect logging, tracing spans, and perf artifact output format. No DB schema or indexer logic changes.
