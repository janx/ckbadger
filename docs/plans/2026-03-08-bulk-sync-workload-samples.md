# Bulk Sync Workload Samples Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extend bulk-sync batch samples so future fresh-db comparisons can normalize parser and writer cost by tx/cell/input workload.

**Architecture:** Keep one batch sample record in `samples.jsonl`. Reuse parser-stage metrics from `pipeline.rs` and writer-stage metrics from `write_parsed_batch()`; do not add a second calculation path or change aggregate perf reports in this step.

**Tech Stack:** Rust, serde JSON, inline unit tests in `ckbadger-indexer`

---

### Task 1: Extend batch sample schema with workload and hot-path timing fields

**Files:**

- Modify: `crates/indexer/src/bulk_sync_perf.rs`
- Modify: `crates/indexer/src/sync/types.rs`
- Modify: `crates/indexer/src/sync/batch.rs`
- Modify: `crates/indexer/src/sync/pipeline.rs`
- Test: `crates/indexer/src/bulk_sync_perf.rs`
- Test: `crates/indexer/src/sync/pipeline.rs`

**Step 1: Write the failing tests**

Add tests that prove a batch sample written to `samples.jsonl` now includes:

- `txs`
- `cells`
- `inputs`
- `parse_ms`
- `precompute_ms`
- `nft_precompute_ms`
- `write_ms`
- `t1_ms`
- `t_act_ms`

Also add a small pipeline/helper test if needed for packaging parser metrics into the writer payload.

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_batch_samples_include_workload_and_hotpath_fields -- --nocapture
```

Expected: FAIL because the sample schema and record path do not include the new fields yet.

**Step 3: Write minimal implementation**

- Extend `BatchSample` in `crates/indexer/src/bulk_sync_perf.rs`
- Introduce a tiny parser-side perf payload for the `ParsedBatch` channel tuple in `crates/indexer/src/sync/pipeline.rs`
- Extend `BatchWriteMetrics` in `crates/indexer/src/sync/types.rs` to include:
  - `write_ms`
  - `t1_ms`
  - `t_act_ms`
- Populate those fields from the existing writer measurements in `crates/indexer/src/sync/batch.rs`
- Record the combined parser/workload/writer fields into `samples.jsonl`

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer bulk_sync_perf::tests:: -- --nocapture
cargo test -p ckbadger-indexer sync::pipeline::tests:: -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs crates/indexer/src/sync/types.rs crates/indexer/src/sync/batch.rs crates/indexer/src/sync/pipeline.rs
git commit -m "feat: capture bulk sync workload perf samples"
```

### Task 2: Verify raw sample payload while leaving aggregate report behavior unchanged

**Files:**

- Modify: `crates/indexer/src/bulk_sync_perf.rs`
- Test: `crates/indexer/src/bulk_sync_perf.rs`

**Step 1: Write the failing test**

Add a regression test that confirms `metrics.env` / `report.md` still render the current aggregate fields and do not require the new batch-sample-only fields.

**Step 2: Run test to verify it fails only if aggregate behavior regresses**

Run:

```bash
cargo test -p ckbadger-indexer test_bulk_sync_metrics_report_remains_aggregate_only -- --nocapture
```

Expected: PASS if behavior is preserved; otherwise fix before continuing.

**Step 3: Run focused verification**

Run:

```bash
cargo test -p ckbadger-indexer bulk_sync_perf::tests:: -- --nocapture
cargo test -p ckbadger-indexer sync::pipeline::tests:: -- --nocapture
cargo fmt --all
```

Expected: PASS

**Step 4: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs
git commit -m "test: preserve aggregate bulk sync perf reporting"
```
