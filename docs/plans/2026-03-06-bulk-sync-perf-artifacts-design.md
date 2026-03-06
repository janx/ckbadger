# Bulk Sync Perf Artifacts Design

## Goal

- Make fresh-db bulk sync automatically emit perf artifacts from the indexer runtime itself for both local runs and CI runs.
- Remove the legacy external bulk-sync perf scripts under `scripts/`.
- Preserve `workdir/perf/` across `ckbadger purge`.

## Principle Alignment

- CKB Native: perf data is derived from the canonical bulk-sync write path, not reconstructed later from side channels.
- Local First: artifacts live in the work directory beside `data/` and `run/`, so local rebuild history stays cheap and inspectable.
- Agent Friendly: outputs are deterministic files (`*.env`, `*.md`, optional `*.jsonl`) with no Docker/log-scraping dependency.

## Problem Summary

- Current bulk sync runtime does not generate its own perf artifacts.
- Existing perf scripts under `scripts/perf/` and `scripts/benchmark_sync.sh` are external monitors that infer state from logs, Docker, or current working directory assumptions.
- That creates drift from the true runtime state and splits local vs CI behavior into different paths.
- Current `ckbadger purge` removes `data/` and `run/`, but perf history needs to survive purges so rebuild benchmarks remain comparable over time.

## Constraints

- Bulk sync rules in `docs/prompts/BULK_SYNC.md` remain authoritative.
- Only fresh-db direct-read bulk sync should generate bulk-sync perf artifacts.
- Perf artifact generation must not write to RocksDB or create new mutable DB responsibilities.
- Failed bulk sync runs must fail fast and record `failed` artifacts instead of trying to recover in place.
- `latest/` baseline must be updated only by completed runs.

## Approaches Considered

### Approach 1: Indexer-owned artifact writer

- Add a runtime-owned bulk-sync perf recorder/writer inside `ckbadger-indexer`.
- Start it only when startup resolves to fresh-db bulk sync.
- Feed it committed batch timings, queue/throughput snapshots, and RocksDB pressure metrics directly from the runtime.

Trade-offs:

- Best fit for single-calculation-path rules.
- Local and CI use the same producer.
- Requires a new runtime file-writing component and explicit workdir perf path wiring.

### Approach 2: Supervisor-owned or external monitor

- Keep artifact generation outside the indexer and watch logs, IPC, or process output.

Trade-offs:

- Lower indexer intrusion.
- Still fundamentally a log-parsing design.
- Keeps a second source of truth for bulk-sync state and preserves the same class of drift as the current scripts.

### Approach 3: Persist perf samples in RocksDB and export later

- Write perf samples into store metadata and generate files from a follow-up command.

Trade-offs:

- Easy to query later.
- Mixes benchmark artifacts into canonical/query state.
- Adds store surface area for data that should remain local runtime output.

## Recommendation

- Use Approach 1.
- The indexer already knows when bulk sync is truly active, when a batch commits, when compaction pressure spikes, and when the run completes or fails.
- The artifact writer should sit on that exact runtime path and write files under `workdir/perf/bulk-sync/`.

## Proposed Design

### 1. Workdir layout

- Extend `WorkDir` with:
  - `perf_dir = root/perf`
  - `bulk_sync_perf_dir = root/perf/bulk-sync`
- `ckbadger init` creates `perf/` alongside `data/` and `run/`.
- `ckbadger purge` continues to delete only `data/` and `run/` contents; `perf/` is explicitly preserved.

Resulting workdir layout:

```text
<workdir>/
├── ckbadger.toml
├── data/
├── run/
└── perf/
    └── bulk-sync/
```

### 2. Runtime ownership and activation

- Add a new internal module in `ckbadger-indexer`, e.g. `crates/indexer/src/bulk_sync_perf.rs`.
- CLI resolves `workdir/perf/bulk-sync` and passes it into `IndexerServiceConfig`.
- `Indexer::run()` decides whether startup is true bulk sync exactly once using the existing fresh-db + direct-read + lag checks.
- The perf run directory is created only after bulk-sync startup checks pass and the indexer is actually entering bulk sync.
- Non-bulk sync paths do not create any bulk-sync perf artifacts.

### 3. Artifact lifecycle

- On run start, create `workdir/perf/bulk-sync/<run_id>/`.
- Immediately write initial files:
  - `metadata.env`
  - `status.env`
  - `metrics.env`
- During the run, update:
  - `status.env`
  - `metrics.env`
  - `samples.jsonl` (append-only sample stream)
- On completion or failure, finalize:
  - `status.env`
  - `metrics.env`
  - `report.md`

Statuses are:

- `running`
- `completed`
- `failed`

### 4. Artifact format

Each run directory contains:

- `metadata.env`
  - run metadata such as `run_id`, `started_at_utc`, `pipeline_enabled`, `batch_size`, `parallel_fetch_size`, `bulk_sync_threshold`, `domain_data_path`, `append_only_data_path`
- `status.env`
  - current run state such as `status`, `current_block`, `target_block`, `finished_at_utc`
- `metrics.env`
  - summarized metrics for current run
- `report.md`
  - current metrics table plus baseline comparison
- `samples.jsonl`
  - append-only time-series samples for future charting and debugging

`metrics.env` keeps the current useful shape from the old scripts, but the values now come from the runtime directly:

- `run_id`
- `status`
- `started_at_utc`
- `finished_at_utc`
- `batches`
- `blocks`
- `avg_batch_seconds`
- `p95_batch_seconds`
- `p99_batch_seconds`
- `avg_commit_ms`
- `p95_commit_ms`
- `p99_commit_ms`
- `max_compaction_pending_mb`
- `max_l0_files`
- `max_imm_memtables`

### 5. Sampling model

- Batch-level metrics come from committed writer batches in both pipeline and sequential paths.
- Heartbeat-level samples come from the existing progress loop in `entry.rs`.
- Heartbeats update current progress and RocksDB pressure snapshots.
- Batch samples drive the batch/commit percentiles and counts.
- Heartbeat samples do not distort batch percentiles; they only update progress and max-pressure style metrics.

This keeps one exact path:

- committed batch timing is sourced from the code that already knows a batch committed
- current progress is sourced from the runtime progress publisher that already emits sync state

### 6. Baseline management

- Keep historical run directories under `workdir/perf/bulk-sync/<run_id>/`.
- Maintain `workdir/perf/bulk-sync/latest/` as the most recent completed run.
- On a completed run:
  - generate `report.md`
  - copy `metadata.env`, `metrics.env`, and `report.md` into `latest/`
- On a failed run:
  - finalize that run directory as `failed`
  - do not modify `latest/`

`report.md` behavior:

- if no completed baseline exists, report current metrics only
- if `latest/metrics.env` exists, include a baseline comparison table with deltas

### 7. Failure handling

- Startup fail-fast before actual bulk-sync entry does not create a perf run directory.
- Once a perf run directory is created, any later failure must finalize the run as `failed`.
- No in-place continuation, no resume logic, no repair flow.
- A new rebuild is always a new `run_id` and a new run directory.

This matches bulk sync’s single-shot rebuild rule.

### 8. Legacy cleanup

Delete the obsolete external bulk-sync perf tooling:

- `scripts/benchmark_sync.sh`
- `scripts/perf/bulk_sync_monitor.sh`
- `scripts/perf/bulk_sync_report.sh`
- `scripts/perf/perf_latest.sh`
- `scripts/perf/detect_fresh_db_rebuild.sh`
- related tests under `scripts/perf/tests/`

Keep API load-test scripts intact:

- `scripts/run-load-tests.sh`
- `scripts/load-test.js`
- `scripts/load-test-quick.js`
- `scripts/wrk-test.lua`

These are a separate concern and should not be conflated with bulk-sync perf artifacts.

## Affected Files

- `crates/config/src/lib.rs`
- `crates/cli/src/main.rs`
- `crates/indexer/src/entry.rs`
- `crates/indexer/src/lib.rs`
- `crates/indexer/src/sync/indexer.rs`
- `crates/indexer/src/sync/pipeline.rs`
- `crates/indexer/src/sync/batch.rs`
- `README.md`
- `docs/INDEXER_PIPELINE.md`
- legacy deletions under `scripts/`

## Testing Strategy

- Add WorkDir tests for `perf/` path resolution.
- Add CLI tests for:
  - `ckbadger init` creates `perf/`
  - `ckbadger purge` preserves `perf/`
- Add unit tests in the new bulk-sync perf module for:
  - run start writes initial artifacts
  - `completed` updates `latest/`
  - `failed` does not update `latest/`
  - summary metrics and percentile calculations
- Add indexer gating tests proving non-bulk sync does not emit bulk-sync artifacts.
- Run targeted `cargo test -p ckbadger-indexer` and CLI/config test coverage before broader verification.

## Success Criteria

- A fresh-db bulk sync automatically creates `workdir/perf/bulk-sync/<run_id>/`.
- The run directory contains live-updating metrics during bulk sync and a finalized report at the end.
- A completed run updates `workdir/perf/bulk-sync/latest/`; a failed run does not.
- `ckbadger purge --confirm` leaves `workdir/perf/` untouched.
- All legacy external bulk-sync perf scripts are removed.
