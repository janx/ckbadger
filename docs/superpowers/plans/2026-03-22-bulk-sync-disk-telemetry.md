# Bulk Sync Disk Telemetry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Linux-first bulk-sync disk telemetry so perf artifacts, runtime logs, and TUI can show real device saturation and phase-2 RocksDB attribution without masking unavailable data as zeros.

**Architecture:** Keep the existing bulk-build sampling pipeline, but upgrade it from raw read/write MB deltas into explicit windowed disk telemetry derived from `/proc/diskstats`. Feed the live window metrics into two paths: `BatchSample` for perf artifacts and `BulkBuildProgressData` for runtime heartbeats/TUI. Keep prefetch throttling behavior unchanged in phase 1 by preserving the legacy `disk_read_mb` / `disk_write_mb` fields alongside the new optional saturation metrics. Phase 2 stays in `bulk_sync_perf.rs`: aggregate disk windows and align them with RocksDB backlog signals to explain whether runs are hardware-bound or internally write-path-bound.

**Tech Stack:** Rust, serde, tokio watch channels, `/proc/diskstats`, RocksDB, ratatui

---

## File Map

- `crates/indexer/src/sys_info.rs`
  - Linux diskstats parsing, device resolution, windowed metric computation, environment snapshot.
- `crates/indexer/src/sync/bulk_build/sampler.rs`
  - Background sampler that turns `sys_info` output into live sampler snapshots.
- `crates/indexer/src/sync/diagnostics.rs`
  - Atomic bulk-build diagnostics that are exported to `BulkBuildProgressData`.
- `crates/common/src/sync.rs`
  - Shared `BulkBuildProgressData` wire format used by indexer, API/WebSocket, and TUI.
- `crates/indexer/src/sync/bulk_build/mod.rs`
  - Bulk-build loop that reads sampler snapshots and records `BatchSample`.
- `crates/indexer/src/bulk_sync_perf.rs`
  - Persisted perf artifacts, run-level aggregation, `report.md`, `metrics.env`.
- `crates/indexer/src/entry.rs`
  - Periodic runtime heartbeat log emission during sync.
- `crates/tui/src/ui.rs`
  - TUI rendering of disk pressure state.

## Guardrails

- Do not silently coerce missing telemetry to numeric zero.
- Do not clamp `util_pct` with `min(100.0, ...)`; preserve the raw computed value and let classification logic interpret it.
- Do not use fallback denominators such as `max(1, io_count)`; use `Option` / unavailable state instead.
- Do not change prefetch throttling thresholds or controller behavior in this plan. Telemetry first, behavior tuning later.
- Read before editing:
  - `docs/prompts/WORLD_VIEW.md`
  - `docs/prompts/BULK_SYNC.md`
  - `docs/superpowers/specs/2026-03-22-bulk-sync-disk-telemetry-design.md`

### Task 1: Build windowed disk telemetry in `sys_info.rs`

**Files:**
- Modify: `crates/indexer/src/sys_info.rs`
- Test: `crates/indexer/src/sys_info.rs` (`#[cfg(test)]` module)

- [ ] **Step 1: Write failing parser and window-computation tests**

Add tests for:

```rust
#[test]
fn test_parse_diskstats_snapshot_reads_required_fields() {}

#[test]
fn test_disk_tracker_reports_warmup_then_sample() {}

#[test]
fn test_disk_tracker_zero_io_window_marks_await_unavailable() {}

#[test]
fn test_disk_tracker_missing_device_reports_unavailable() {}

#[test]
fn test_resolve_diskstats_device_supports_dm_and_btrfs_cases() {}
```

The tests must assert:

- read/write IOPS are derived from completed I/O counters
- MB/s uses sectors delta and window duration
- `await_ms` is `None` when `read_ios_delta + write_ios_delta == 0`
- unresolved devices do not produce fake zeroed metrics

- [ ] **Step 2: Add explicit disk window types**

In `crates/indexer/src/sys_info.rs`, add small focused types instead of stuffing more floats into ad hoc tuples:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskTelemetryState {
    Idle,
    Active,
    Saturated,
    Unavailable,
}

#[derive(Debug, Clone, Default)]
pub struct DiskWindowMetrics {
    pub read_mb: f64,
    pub write_mb: f64,
    pub read_mb_s: f64,
    pub write_mb_s: f64,
    pub read_iops: f64,
    pub write_iops: f64,
    pub util_pct: Option<f64>,
    pub await_ms: Option<f64>,
    pub avg_queue_depth: Option<f64>,
    pub in_flight: Option<u64>,
    pub state: DiskTelemetryState,
}
```

Add an internal snapshot struct for the parsed `/proc/diskstats` row with all required counters:

- `read_ios`
- `read_sectors`
- `read_time_ms`
- `write_ios`
- `write_sectors`
- `write_time_ms`
- `in_flight`
- `time_io_ms`
- `weighted_time_io_ms`

- [ ] **Step 3: Replace the simple delta tracker with a window tracker**

Keep `DiskStatsTracker`, but make it hold the previous full snapshot and timestamp. Add a method that returns explicit state:

```rust
pub enum DiskWindowSample {
    Warmup,
    Sample(DiskWindowMetrics),
    Unavailable { reason: String },
}

impl DiskStatsTracker {
    pub fn read_window(&mut self) -> DiskWindowSample { /* ... */ }
}
```

Rules:

- first successful sample returns `Warmup`
- missing device or malformed row returns `Unavailable`
- `await_ms`, `avg_queue_depth`, `util_pct`, and `in_flight` use `Option`
- legacy `read_mb` / `write_mb` stay available for existing callers

- [ ] **Step 4: Keep `BatchEnvironment` backward-compatible while adding new fields**

Extend `BatchEnvironment` so prefetch throttling can keep using the old fields:

```rust
pub struct BatchEnvironment {
    pub load_avg_1m: f64,
    pub mem_available_mb: u64,
    pub disk_read_mb: f64,
    pub disk_write_mb: f64,
    pub disk_read_mb_s: Option<f64>,
    pub disk_write_mb_s: Option<f64>,
    pub disk_read_iops: Option<f64>,
    pub disk_write_iops: Option<f64>,
    pub disk_util_pct: Option<f64>,
    pub disk_await_ms: Option<f64>,
    pub disk_avg_queue_depth: Option<f64>,
    pub disk_in_flight: Option<u64>,
    pub disk_state: Option<String>,
}
```

`read_batch_environment()` should translate `DiskWindowSample` into this shape without inventing zeros for unavailable values.

- [ ] **Step 5: Run the focused tests**

Run: `cargo test -p ckbadger-indexer sys_info::tests --lib`

Expected:

- all new diskstats and resolver tests pass
- no test relies on `max(1, ...)` or zero fallbacks

- [ ] **Step 6: Commit**

```bash
git add crates/indexer/src/sys_info.rs
git commit -m "feat(indexer): add windowed disk telemetry sampling"
```

---

### Task 2: Thread live disk telemetry through sampler and bulk-build diagnostics

**Files:**
- Modify: `crates/indexer/src/sync/bulk_build/sampler.rs`
- Modify: `crates/indexer/src/sync/diagnostics.rs`
- Modify: `crates/common/src/sync.rs`
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs`
- Test: `crates/indexer/src/sync/bulk_build/sampler.rs`
- Test: `crates/indexer/src/sync/diagnostics.rs`
- Test: `crates/common/src/sync.rs`

- [ ] **Step 1: Write failing diagnostics and serde tests**

Add or extend tests for:

```rust
#[test]
fn sampler_snapshot_default_marks_disk_unavailable() {}

#[test]
fn bulk_build_progress_snapshot_includes_disk_pressure_fields() {}

#[test]
fn bulk_build_progress_json_roundtrips_disk_fields() {}
```

The serde test should ensure optional disk fields serialize as absent or `null`, not fake zeros.

- [ ] **Step 2: Extend `SamplerSnapshot` with optional disk-pressure fields**

In `crates/indexer/src/sync/bulk_build/sampler.rs`:

```rust
#[derive(Clone, Default, Debug)]
pub(crate) struct SamplerSnapshot {
    pub compaction_pending_mb: u64,
    pub l0_files: u64,
    pub imm_memtables: u64,
    pub load_avg_1m: f64,
    pub mem_available_mb: u64,
    pub disk_read_mb: f64,
    pub disk_write_mb: f64,
    pub disk_read_mb_s: Option<f64>,
    pub disk_write_mb_s: Option<f64>,
    pub disk_read_iops: Option<f64>,
    pub disk_write_iops: Option<f64>,
    pub disk_util_pct: Option<f64>,
    pub disk_await_ms: Option<f64>,
    pub disk_avg_queue_depth: Option<f64>,
    pub disk_in_flight: Option<u64>,
    pub disk_state: Option<String>,
}
```

Fill these fields from `BatchEnvironment`.

- [ ] **Step 3: Add live disk fields to bulk-build diagnostics**

In `crates/indexer/src/sync/diagnostics.rs`, add atomic storage for the live sampler view. Follow existing patterns used for `flush_wait_ms` and prefetch channel counters.

Suggested shape:

```rust
last_disk_util_bits: AtomicU64,
last_disk_await_bits: AtomicU64,
last_disk_qd_bits: AtomicU64,
last_disk_wr_mb_s_bits: AtomicU64,
last_disk_wr_iops_bits: AtomicU64,
last_disk_state_code: AtomicU8,
last_disk_telemetry_valid: AtomicBool,
```

Add a small encoder/decoder for disk state strings (`idle`, `active`, `saturated`, `unavailable`) so the shared output remains stable.

- [ ] **Step 4: Extend `BulkBuildProgressData`**

In `crates/common/src/sync.rs`, add optional fields:

```rust
#[serde(default)]
pub disk_state: Option<String>,
#[serde(default)]
pub disk_util_pct: Option<f64>,
#[serde(default)]
pub disk_await_ms: Option<f64>,
#[serde(default)]
pub disk_avg_queue_depth: Option<f64>,
#[serde(default)]
pub disk_write_mb_s: Option<f64>,
#[serde(default)]
pub disk_write_iops: Option<f64>,
```

Keep the shared type minimal. Do not add every raw field to the TUI/API wire format; only ship the fields the heartbeat log and TUI need.

- [ ] **Step 5: Publish sampler values into diagnostics from the bulk-build loop**

In `crates/indexer/src/sync/bulk_build/mod.rs`:

1. Read `let snap = sampler.latest();` exactly once per batch, as it does now.
2. Push the disk-related fields into the diagnostics object before recording the perf sample.
3. Keep using legacy `snap.disk_write_mb` for prefetch throttling; phase 1 is telemetry-only.

If `BulkBuildPerf` already exposes a method for updating per-batch state, add one narrow method for disk telemetry instead of mutating atomics all over the bulk-build loop.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test -p ckbadger-common bulk_build_progress --lib
cargo test -p ckbadger-indexer sampler --lib
cargo test -p ckbadger-indexer diagnostics --lib
```

Expected:

- shared serde remains backward-compatible with missing fields
- diagnostics snapshot exposes disk fields only when valid

- [ ] **Step 7: Commit**

```bash
git add crates/indexer/src/sync/bulk_build/sampler.rs crates/indexer/src/sync/diagnostics.rs crates/common/src/sync.rs crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(sync): wire disk telemetry into bulk-build progress snapshots"
```

---

### Task 3: Persist disk saturation metrics in bulk-sync perf artifacts

**Files:**
- Modify: `crates/indexer/src/bulk_sync_perf.rs`
- Modify: `crates/indexer/src/sync/bulk_build/mod.rs`
- Test: `crates/indexer/src/bulk_sync_perf.rs`

- [ ] **Step 1: Write failing perf-artifact tests**

Add or extend tests covering:

```rust
#[test]
fn batch_samples_write_disk_window_fields_to_samples_jsonl() {}

#[test]
fn metrics_env_aggregates_disk_windows_without_zero_fallbacks() {}

#[test]
fn report_includes_disk_saturation_summary_line() {}

#[test]
fn unavailable_disk_windows_do_not_skew_averages() {}
```

- [ ] **Step 2: Extend `BatchSample` with optional disk metrics**

In `crates/indexer/src/bulk_sync_perf.rs`, add fields such as:

```rust
pub disk_read_mb_s: Option<f64>,
pub disk_write_mb_s: Option<f64>,
pub disk_read_iops: Option<f64>,
pub disk_write_iops: Option<f64>,
pub disk_util_pct: Option<f64>,
pub disk_await_ms: Option<f64>,
pub disk_avg_queue_depth: Option<f64>,
pub disk_in_flight: Option<u64>,
pub disk_state: Option<String>,
```

Update `BatchSample::new()` defaults accordingly and fill the fields from `snap` in `crates/indexer/src/sync/bulk_build/mod.rs`.

- [ ] **Step 3: Add unavailable-aware aggregation helpers**

In `crates/indexer/src/bulk_sync_perf.rs`, add helpers that operate on `Option<f64>` collections:

```rust
fn average_valid(values: &[Option<f64>]) -> Option<f64> { /* ... */ }
fn percentile_valid(values: &[Option<f64>], p: f64) -> Option<f64> { /* ... */ }
fn max_valid(values: &[Option<f64>]) -> Option<f64> { /* ... */ }
```

Compute and persist:

- `avg_disk_util_pct`
- `p95_disk_util_pct`
- `avg_disk_await_ms`
- `p95_disk_await_ms`
- `max_disk_avg_queue_depth`
- `peak_disk_write_mb_s`
- `peak_disk_write_iops`
- `saturated_window_count`
- `saturated_window_ratio`
- `disk_telemetry_status`

Keep the existing `avg_disk_write_mb_per_batch` for backward compatibility, but stop treating it as the primary disk metric.

- [ ] **Step 4: Update `metrics.env`, `report.md`, and `samples.jsonl` writers**

`metrics.env` should stay machine-readable and raw.

Example target keys:

```text
avg_disk_util_pct=87.231
p95_disk_util_pct=103.114
avg_disk_await_ms=9.482
p95_disk_await_ms=18.407
max_disk_avg_queue_depth=3.721
peak_disk_write_mb_s=742.552
peak_disk_write_iops=11342.110
saturated_window_count=411
saturated_window_ratio=0.611
disk_telemetry_status=ok
```

`report.md` should include a one-line summary plus a small table for the new aggregates.

- [ ] **Step 5: Run focused tests**

Run: `cargo test -p ckbadger-indexer bulk_sync_perf --lib`

Expected:

- `samples.jsonl` contains the new disk fields
- `metrics.env` does not contain zero-filled substitutes for unavailable values
- report output stays deterministic

- [ ] **Step 6: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs crates/indexer/src/sync/bulk_build/mod.rs
git commit -m "feat(perf): add disk saturation metrics to bulk sync artifacts"
```

---

### Task 4: Surface live disk pressure in heartbeat logs and TUI

**Files:**
- Modify: `crates/indexer/src/entry.rs`
- Modify: `crates/tui/src/ui.rs`
- Test: `crates/indexer/src/entry.rs`
- Test: `crates/tui/src/ui.rs`

- [ ] **Step 1: Write failing presentation tests**

Add testable helpers before touching the main render/log paths.

In `crates/indexer/src/entry.rs`, add a helper test such as:

```rust
#[test]
fn disk_log_summary_prefers_unavailable_over_fake_numbers() {}
```

In `crates/tui/src/ui.rs`, add tests such as:

```rust
#[test]
fn draw_disk_pressure_renders_saturated_state() {}

#[test]
fn draw_disk_pressure_renders_unavailable_state() {}
```

- [ ] **Step 2: Extract a small heartbeat-log formatter**

Avoid trying to unit-test `info!` macro output directly. Add a pure helper in `crates/indexer/src/entry.rs`:

```rust
struct DiskLogSummary {
    state: String,
    util_pct: Option<String>,
    await_ms: Option<String>,
    queue_depth: Option<String>,
    write_mb_s: Option<String>,
    write_iops: Option<String>,
}

fn summarize_bulk_build_disk(bb: Option<&ckbadger_common::BulkBuildProgressData>) -> Option<DiskLogSummary> {
    /* ... */
}
```

This helper should:

- return `None` when bulk-build progress is absent
- return explicit `unavailable` state when disk telemetry exists but is invalid
- keep numeric formatting stable (`{:.1}` or `{:.2}` once, centrally)

- [ ] **Step 3: Emit disk fields in bulk-sync heartbeat logs**

In `crates/indexer/src/entry.rs`, read `bulk_build_progress_snapshot()` and log:

- `disk_state`
- `disk_util_pct`
- `disk_await_ms`
- `disk_qd`
- `disk_wr_mb_s`
- `disk_wr_iops`

Only emit these fields for bulk sync. If disk telemetry is unavailable, emit a single low-noise warning per run or stage transition instead of repeating it every heartbeat.

- [ ] **Step 4: Add a TUI disk-pressure section**

In `crates/tui/src/ui.rs`, add a compact section driven from `SyncStatusRow.bulk_build`.

Preferred helper shape:

```rust
fn disk_pressure_lines(bb: Option<&BulkBuildProgressData>, width: u16) -> Vec<Line<'static>> {
    /* ... */
}
```

Display:

- `State`
- `util`
- `await`
- `qd`
- `wr MB/s`
- `wr IOPS`

Handle narrow panels by trimming, not by dropping the state line.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test -p ckbadger-indexer entry --lib
cargo test -p ckbadger-tui ui --lib
```

Expected:

- heartbeat formatting helpers are covered by unit tests
- TUI renders `saturated` and `unavailable` deterministically

- [ ] **Step 6: Commit**

```bash
git add crates/indexer/src/entry.rs crates/tui/src/ui.rs
git commit -m "feat(runtime): show disk pressure in bulk sync logs and tui"
```

---

### Task 5: Add phase-2 RocksDB attribution to the perf report

**Files:**
- Modify: `crates/indexer/src/bulk_sync_perf.rs`
- Test: `crates/indexer/src/bulk_sync_perf.rs`

- [ ] **Step 1: Write failing attribution tests**

Add table-driven tests for three cases:

```rust
#[test]
fn report_classifies_device_saturation_when_disk_and_flush_rise_together() {}

#[test]
fn report_classifies_rocksdb_backlog_before_device_saturation() {}

#[test]
fn report_classifies_coordination_gap_when_flush_wait_lacks_disk_pressure() {}
```

Each case should build a short vector of `BatchSample`s with only the relevant fields populated.

- [ ] **Step 2: Implement a narrow attribution classifier**

Keep the logic in `crates/indexer/src/bulk_sync_perf.rs` and make it deterministic. A small enum is enough:

```rust
enum DiskAttribution {
    DeviceSaturated,
    RocksDbBacklog,
    CoordinationGap,
    Inconclusive,
}
```

Use only the inputs approved by the spec:

- `disk_util_pct`
- `disk_await_ms`
- `disk_avg_queue_depth`
- `flush_ms`
- `flush_wait_ms`
- `flush_channel_pending`
- `compaction_pending_mb`
- `l0_files`
- `imm_memtables`

Do not add controller tuning or heuristic retries here.

- [ ] **Step 3: Render a report-only attribution section**

Add a short section to `report.md`, for example:

```markdown
## Disk / Flush Attribution

- Primary classification: device_saturated
- Evidence: 61.1% saturated windows, p95 await 18.4 ms, p95 flush_wait 412 ms
```

Keep `metrics.env` raw. Do not put prose into `metrics.env`.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p ckbadger-indexer bulk_sync_perf --lib`

Expected:

- all three attribution cases classify correctly
- report rendering stays deterministic

- [ ] **Step 5: Commit**

```bash
git add crates/indexer/src/bulk_sync_perf.rs
git commit -m "feat(perf): add disk and flush attribution summary"
```

---

### Task 6: Run full validation and capture one fresh-DB artifact check

**Files:**
- Modify: none
- Verify: working tree changes from Tasks 1-5 only

- [ ] **Step 1: Run crate-level test suites**

Run:

```bash
cargo test -p ckbadger-common --lib
cargo test -p ckbadger-indexer --lib
cargo test -p ckbadger-tui --lib
```

Expected:

- all touched modules pass
- no serde/backward-compat regressions in shared sync types

- [ ] **Step 2: Run static checks for touched Rust code**

Run:

```bash
cargo check
cargo clippy --all-targets --all-features -- -D warnings
```

Expected:

- no type errors
- no lint regressions from the new telemetry types

- [ ] **Step 3: Run one fresh-DB validation flow**

Use the project’s normal workdir and fresh-db workflow:

```bash
cargo run -p ckbadger -- purge --confirm
cargo run -p ckbadger -- run
```

After bulk sync completes, verify:

- `temp/perf/bulk-sync/latest/samples.jsonl` contains disk window metrics
- `temp/perf/bulk-sync/latest/metrics.env` contains the new disk keys
- `temp/perf/bulk-sync/latest/report.md` contains both saturation summary and attribution section
- runtime logs include `disk_state`, `disk_util_pct`, `disk_await_ms`, `disk_qd`, `disk_wr_mb_s`, `disk_wr_iops`
- TUI shows a disk pressure section during bulk sync

If the environment cannot run a full fresh DB sync locally, stop after the automated tests and record that end-to-end artifact validation remains pending.

- [ ] **Step 4: Commit final validation-only changes if needed**

If validation required only code already committed in Tasks 1-5, do not create a new commit. If small follow-up fixes were needed, commit them once:

```bash
git add <touched-files>
git commit -m "fix(perf): address validation issues in disk telemetry rollout"
```

