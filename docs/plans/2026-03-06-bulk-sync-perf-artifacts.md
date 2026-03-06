# Bulk Sync Perf Artifacts Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make fresh-db bulk sync automatically generate perf artifacts under `workdir/perf/bulk-sync/`, preserve those artifacts across `ckbadger purge`, and delete the obsolete external bulk-sync perf scripts.

**Architecture:** Extend workdir path resolution so `perf/` is a first-class sibling of `data/` and `run/`. Then add an indexer-owned bulk-sync perf writer that starts only for true bulk sync, records batch and heartbeat samples directly from runtime state, finalizes completed/failed runs into deterministic artifacts, and maintains `latest/` as the newest completed baseline.

**Tech Stack:** Rust, Tokio, serde/serde_json, tracing, inline unit tests in `ckbadger`, `ckbadger-config`, and `ckbadger-indexer`

---

### Task 1: Add `workdir/perf/` to WorkDir, init, and purge behavior

**Files:**

- Modify: `crates/config/src/lib.rs`
- Modify: `crates/cli/src/main.rs`
- Test: `crates/config/src/lib.rs`
- Test: `crates/cli/src/main.rs`

**Step 1: Write the failing tests**

Add or extend tests for:

```rust
#[test]
fn test_workdir_resolve_paths_includes_perf_dirs() {
    let root = std::path::Path::new("/tmp/example");
    let wd = WorkDir::resolve(root);
    assert_eq!(wd.perf_dir, root.join("perf"));
    assert_eq!(wd.bulk_sync_perf_dir, root.join("perf/bulk-sync"));
}

#[test]
fn test_init_creates_perf_directory() {
    let dir = tempfile::TempDir::new().unwrap();
    cmd_init(dir.path()).unwrap();
    assert!(dir.path().join("perf").exists());
}

#[test]
fn test_purge_preserves_perf_contents() {
    let dir = tempfile::TempDir::new().unwrap();
    cmd_init(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path().join("perf/bulk-sync/run-1")).unwrap();
    std::fs::write(
        dir.path().join("perf/bulk-sync/run-1/metrics.env"),
        "status=completed\n",
    )
    .unwrap();

    cmd_purge(dir.path(), &PurgeArgs { confirm: true }).unwrap();

    assert!(dir.path().join("perf/bulk-sync/run-1/metrics.env").exists());
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-config test_workdir_resolve_paths_includes_perf_dirs -- --nocapture
cargo test -p ckbadger test_init_creates_perf_directory test_purge_preserves_perf_contents -- --nocapture
```

Expected:

- `WorkDir` test fails because `perf_dir` and `bulk_sync_perf_dir` do not exist yet.
- CLI tests fail because `init` does not create `perf/` and/or `purge` assumptions need updating.

**Step 3: Write the minimal implementation**

- Add `perf_dir` and `bulk_sync_perf_dir` fields to `WorkDir`.
- Make `cmd_init()` create `workdir/perf/`.
- Keep `cmd_purge()` scoped to `data/` and `run/` only.
- Update the user-facing `init`/`purge` output text to mention preserved `perf/` when appropriate.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-config test_workdir_resolve_paths_includes_perf_dirs -- --nocapture
cargo test -p ckbadger test_init_creates_perf_directory test_purge_preserves_perf_contents -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/config/src/lib.rs crates/cli/src/main.rs
git commit -m "feat: add perf paths to workdir layout"
```

### Task 2: Create the bulk-sync perf artifact writer module

**Files:**

- Create: `crates/indexer/src/bulk_sync_perf.rs`
- Modify: `crates/indexer/src/lib.rs`
- Test: `crates/indexer/src/bulk_sync_perf.rs`

**Step 1: Write the failing tests**

Add unit tests in the new module for lifecycle and baseline behavior:

```rust
#[test]
fn test_bulk_sync_perf_run_start_writes_initial_artifacts() {
    let dir = tempfile::TempDir::new().unwrap();
    let run = BulkSyncPerfRun::start_for_test(dir.path(), "run-1").unwrap();

    assert!(dir.path().join("run-1/metadata.env").exists());
    assert!(dir.path().join("run-1/status.env").exists());
    assert!(dir.path().join("run-1/metrics.env").exists());
    assert_eq!(run.status(), "running");
}

#[test]
fn test_bulk_sync_perf_completed_run_updates_latest() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut run = BulkSyncPerfRun::start_for_test(dir.path(), "run-1").unwrap();
    run.finish_completed().unwrap();
    assert!(dir.path().join("latest/metrics.env").exists());
}

#[test]
fn test_bulk_sync_perf_failed_run_does_not_update_latest() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut completed = BulkSyncPerfRun::start_for_test(dir.path(), "run-1").unwrap();
    completed.finish_completed().unwrap();

    let mut failed = BulkSyncPerfRun::start_for_test(dir.path(), "run-2").unwrap();
    failed.finish_failed().unwrap();

    let latest = std::fs::read_to_string(dir.path().join("latest/metrics.env")).unwrap();
    assert!(latest.contains("run_id=run-1"));
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_bulk_sync_perf_run_start_writes_initial_artifacts -- --nocapture
cargo test -p ckbadger-indexer test_bulk_sync_perf_completed_run_updates_latest -- --nocapture
cargo test -p ckbadger-indexer test_bulk_sync_perf_failed_run_does_not_update_latest -- --nocapture
```

Expected: FAIL because `BulkSyncPerfRun` does not exist yet.

**Step 3: Write the minimal implementation**

- Add `BulkSyncPerfRun` and supporting structs for:
  - metadata writing
  - status writing
  - metrics writing
  - report generation
  - `latest/` baseline updates
- Keep file formats simple: env files, markdown report, newline-delimited JSON samples.
- Add a small test-only constructor to avoid needing the full indexer runtime in unit tests.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer test_bulk_sync_perf_run_start_writes_initial_artifacts -- --nocapture
cargo test -p ckbadger-indexer test_bulk_sync_perf_completed_run_updates_latest -- --nocapture
cargo test -p ckbadger-indexer test_bulk_sync_perf_failed_run_does_not_update_latest -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs crates/indexer/src/lib.rs
git commit -m "feat: add bulk sync perf artifact writer"
```

### Task 3: Add summary aggregation and baseline comparison tests

**Files:**

- Modify: `crates/indexer/src/bulk_sync_perf.rs`
- Test: `crates/indexer/src/bulk_sync_perf.rs`

**Step 1: Write the failing tests**

Add tests for summary math and report generation:

```rust
#[test]
fn test_bulk_sync_metrics_use_committed_batch_samples_for_percentiles() {
    let mut run = BulkSyncPerfRun::start_for_test(tempfile::TempDir::new().unwrap().path(), "run-1")
        .unwrap();
    run.record_batch_sample(BatchSample::new(10, 1.0, 40.0, 100, 4, 1));
    run.record_batch_sample(BatchSample::new(20, 2.0, 80.0, 200, 7, 2));
    run.record_heartbeat_sample(HeartbeatSample::new(15, 100, 5.0, 150, 6, 1));

    let metrics = run.build_metrics_for_test("running");

    assert_eq!(metrics.batches, 2);
    assert_eq!(metrics.blocks, 30);
    assert_eq!(metrics.max_l0_files, 7);
    assert_eq!(metrics.max_imm_memtables, 2);
}

#[test]
fn test_bulk_sync_report_includes_baseline_table_when_latest_exists() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut baseline = BulkSyncPerfRun::start_for_test(dir.path(), "run-1").unwrap();
    baseline.record_batch_sample(BatchSample::new(10, 1.0, 40.0, 100, 4, 1));
    baseline.finish_completed().unwrap();

    let mut current = BulkSyncPerfRun::start_for_test(dir.path(), "run-2").unwrap();
    current.record_batch_sample(BatchSample::new(10, 2.0, 80.0, 120, 5, 1));
    current.finish_completed().unwrap();

    let report = std::fs::read_to_string(dir.path().join("run-2/report.md")).unwrap();
    assert!(report.contains("## Baseline Comparison"));
    assert!(report.contains("avg_batch_seconds"));
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_bulk_sync_metrics_use_committed_batch_samples_for_percentiles -- --nocapture
cargo test -p ckbadger-indexer test_bulk_sync_report_includes_baseline_table_when_latest_exists -- --nocapture
```

Expected: FAIL because sample aggregation/report comparison is not implemented yet.

**Step 3: Write the minimal implementation**

- Add batch-sample and heartbeat-sample types.
- Track running aggregates and percentile inputs from committed batch samples only.
- Generate `report.md` with:
  - current metrics table
  - optional baseline comparison table when `latest/metrics.env` exists

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer test_bulk_sync_metrics_use_committed_batch_samples_for_percentiles -- --nocapture
cargo test -p ckbadger-indexer test_bulk_sync_report_includes_baseline_table_when_latest_exists -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs
git commit -m "feat: add bulk sync perf summaries and baseline reports"
```

### Task 4: Wire the artifact writer into the indexer lifecycle and batch paths

**Files:**

- Modify: `crates/indexer/src/entry.rs`
- Modify: `crates/indexer/src/sync/indexer.rs`
- Modify: `crates/indexer/src/sync/pipeline.rs`
- Modify: `crates/indexer/src/sync/batch.rs`
- Test: `crates/indexer/src/sync/indexer.rs`
- Test: `crates/indexer/src/bulk_sync_perf.rs`

**Step 1: Write the failing tests**

Add gating-focused tests around the new runtime hooks:

```rust
#[test]
fn test_bulk_sync_perf_requires_fresh_db_direct_read_and_lag() {
    assert!(should_startup_bulk_sync_mode(1001, 1000, true, 0, &None));
    assert!(!should_startup_bulk_sync_mode(1001, 1000, false, 0, &None));
    assert!(!should_startup_bulk_sync_mode(1001, 1000, true, 10, &Some(vec![1; 32])));
}

#[test]
fn test_non_bulk_sync_run_does_not_create_perf_artifacts() {
    let dir = tempfile::TempDir::new().unwrap();
    let outcome = maybe_start_bulk_sync_perf_run_for_test(false, dir.path(), "run-1");
    assert!(outcome.is_none());
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p ckbadger-indexer test_non_bulk_sync_run_does_not_create_perf_artifacts -- --nocapture
```

Expected: FAIL because the helper/runtime hook does not exist yet.

**Step 3: Write the minimal implementation**

- Extend `IndexerServiceConfig` with `bulk_sync_perf_output_root`.
- Resolve `workdir.bulk_sync_perf_dir` in `crates/cli/src/main.rs` and pass it into the indexer service config.
- Add an optional perf-run field to `Indexer`.
- Start the perf run only after bulk-sync startup checks pass and the runtime is actually entering bulk sync.
- Record heartbeat samples from the existing progress publisher loop in `entry.rs`.
- Record committed batch samples from:
  - pipeline committed writer batches in `crates/indexer/src/sync/pipeline.rs`
  - sequential committed batches in `crates/indexer/src/sync/batch.rs`
- Finalize the perf run as:
  - `completed` on successful run completion
  - `failed` on runtime error after perf run start

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p ckbadger-indexer test_non_bulk_sync_run_does_not_create_perf_artifacts -- --nocapture
cargo test -p ckbadger-indexer test_bulk_sync_perf_run_start_writes_initial_artifacts test_bulk_sync_perf_completed_run_updates_latest test_bulk_sync_perf_failed_run_does_not_update_latest -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/cli/src/main.rs crates/indexer/src/entry.rs crates/indexer/src/sync/indexer.rs crates/indexer/src/sync/pipeline.rs crates/indexer/src/sync/batch.rs crates/indexer/src/bulk_sync_perf.rs
git commit -m "feat: wire bulk sync perf artifacts into indexer runtime"
```

### Task 5: Remove legacy scripts, update docs, and verify end-to-end behavior

**Files:**

- Delete: `scripts/benchmark_sync.sh`
- Delete: `scripts/perf/bulk_sync_monitor.sh`
- Delete: `scripts/perf/bulk_sync_report.sh`
- Delete: `scripts/perf/perf_latest.sh`
- Delete: `scripts/perf/detect_fresh_db_rebuild.sh`
- Delete: `scripts/perf/tests/test_bulk_sync_report.sh`
- Delete: `scripts/perf/tests/test_perf_latest.sh`
- Modify: `README.md`
- Modify: `docs/INDEXER_PIPELINE.md`

**Step 1: Remove stale script references**

Run:

```bash
rg -n "benchmark_sync|bulk_sync_monitor|bulk_sync_report|perf_latest|artifacts/perf/bulk-sync" . -g '!target'
```

Expected: output points only to the obsolete scripts/docs you are about to remove or update.

**Step 2: Delete the obsolete scripts and update docs**

- Remove the external bulk-sync perf scripts and their tests.
- Update `README.md` workdir layout and command behavior to mention `perf/`.
- Update `docs/INDEXER_PIPELINE.md` to say bulk-sync perf artifacts are generated by the indexer under `workdir/perf/bulk-sync/`.
- Do not touch API load-test scripts or `perf-nightly` workflow, since they are unrelated API benchmarks.

**Step 3: Run verification**

Run:

```bash
rg -n "benchmark_sync|bulk_sync_monitor|bulk_sync_report|perf_latest" . -g '!target'
cargo test -p ckbadger -- --nocapture
cargo test -p ckbadger-config -- --nocapture
cargo test -p ckbadger-indexer --lib -- --nocapture
```

Expected:

- `rg` finds no stale references to removed bulk-sync perf scripts.
- All targeted tests pass.

**Step 4: Commit**

```bash
git add -u scripts README.md docs/INDEXER_PIPELINE.md
git commit -m "refactor: remove legacy bulk sync perf scripts"
```

Plan complete and saved to `docs/plans/2026-03-06-bulk-sync-perf-artifacts.md`.

Two execution options:

1. Subagent-Driven (this session) - I dispatch fresh subagent per task, review between tasks, fast iteration
2. Parallel Session (separate) - Open new session with executing-plans, batch execution with checkpoints

If no preference is given, use option 1 in this session.
