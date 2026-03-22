# Bulk Sync Disk Telemetry: Linux-First Saturation and Attribution

**Date**: 2026-03-22
**Status**: Draft

## Goal

Add real disk telemetry for fresh DB / bulk sync runs so artifacts, runtime logs, and TUI can answer four questions with evidence instead of guesswork:

1. Is the underlying block device saturated during bulk sync?
2. How often is the run disk-bound versus compute-bound?
3. When flush/compaction slows down, is the limiting factor the physical device or RocksDB backlog?
4. Which next optimization direction is justified by the run data?

## Principle Alignment

- **CKB Native**: Sync performance is measured from real chain ingestion and RocksDB write behavior, not synthetic benchmarks.
- **Local First**: Fresh DB rebuild wall clock is the primary optimization target, so telemetry must explain local write-path limits.
- **Agent Friendly**: Perf artifacts should make bottlenecks machine-readable and explicit, so future optimization loops do not depend on ad hoc shell work.

## Problem Statement

The current perf pipeline already captures a small amount of host telemetry:

- Static environment metadata in `EnvironmentSnapshot`
- Per-window `disk_read_mb` / `disk_write_mb`
- RocksDB backlog signals such as `compaction_pending_mb`, `l0_files`, and `imm_memtables`

This is not enough to answer whether hardware is fully utilized. The latest fresh DB artifacts showed:

- `disk_device=` and `filesystem=` were unresolved
- `avg_disk_write_mb_per_batch=0.000` was misleading
- there was no direct signal equivalent to `iostat -x` fields such as `%util`, `await`, or queue depth

As a result, a run can clearly look write-path bound while the artifacts still cannot prove whether:

- the physical disk is saturated
- RocksDB backlog is building before device saturation
- the controller is overfeeding flush/compaction without hitting hardware limits

## Scope

### In Scope

- Linux-first telemetry for bulk sync, with bulk-build as the first fully supported path
- Device saturation metrics derived from `/proc/diskstats`
- Aggregated reporting in `perf/bulk-sync/*`
- Runtime heartbeat log fields during bulk sync
- TUI surfacing of current disk pressure
- A follow-up attribution layer that aligns device saturation with RocksDB backlog metrics

### Out of Scope

- eBPF, perf-event, or per-process block I/O tracing
- A general observability platform redesign
- Non-Linux parity in phase 1
- Per-column-family RocksDB hotspot attribution in phase 1

## Current State

### Existing Telemetry

- `crates/indexer/src/sys_info.rs`
  - resolves static environment metadata
  - parses `/proc/diskstats`
  - tracks cumulative read/write sector deltas through `DiskStatsTracker`
- `crates/indexer/src/sync/bulk_build/sampler.rs`
  - periodically samples RocksDB backlog and environment values
- `crates/indexer/src/bulk_sync_perf.rs`
  - persists batch samples, heartbeats, environment metadata, and summary metrics
- `crates/indexer/src/entry.rs`
  - emits bulk sync heartbeat logs and RocksDB stats logs
- `crates/tui/src/ui.rs`
  - has runtime health panels but no dedicated disk pressure visualization

### Observed Gap

The current disk signal is only throughput-like byte deltas. It does not express:

- device busy time
- average I/O latency
- queue depth
- concurrent in-flight I/O
- whether a window should be considered saturated

This leaves the most important performance question unanswered: did wall clock stop improving because storage was maxed out, or because the write path was internally inefficient before reaching device limits?

## Design Overview

The design is intentionally staged:

1. **Phase 1: device saturation telemetry**
   - Make the artifacts answer whether the block device was busy, queued, and latency-bound during each sampling window.
2. **Phase 2: RocksDB attribution**
   - Align the device-level signal with flush/compaction backlog to distinguish hardware saturation from internal write-path pressure.
3. **Phase 3: device resolution hardening**
   - Improve robust mapping from RocksDB path to the correct block device for layered storage setups.

The priority order is deliberate:

- first prove device saturation
- then explain it against RocksDB backlog
- then harden edge-case resolution beyond the minimum required for Linux-first support

## Phase 1: Device Saturation Telemetry

### Data Source

Phase 1 uses Linux `/proc/diskstats` only. This keeps the implementation:

- rootless
- dependency-free
- cheap enough to sample continuously during bulk sync

The tracker changes from a simple sector-delta accumulator into a windowed device stats tracker.

### New Window Metrics

For each sampling window, collect:

| Metric | Meaning |
|--------|---------|
| `disk_read_mb_s` | Read throughput in MB/s |
| `disk_write_mb_s` | Write throughput in MB/s |
| `disk_read_iops` | Completed read I/O operations per second |
| `disk_write_iops` | Completed write I/O operations per second |
| `disk_util_pct` | Fraction of wall time the device reported active I/O |
| `disk_await_ms` | Average service+queue wait time per completed I/O |
| `disk_avg_queue_depth` | Average queue depth over the window |
| `disk_in_flight` | Instantaneous in-flight I/O count at sample time |
| `disk_state` | Classified state: `idle`, `active`, `saturated`, or `unavailable` |

### Metric Formulas

Assume a window duration `window_ms` from two snapshots of the same diskstats row.

- Throughput
  - `read_mb_s = read_sectors_delta * 512 / 1024 / 1024 / window_seconds`
  - `write_mb_s = write_sectors_delta * 512 / 1024 / 1024 / window_seconds`
- IOPS
  - `read_iops = read_ios_delta / window_seconds`
  - `write_iops = write_ios_delta / window_seconds`
- Utilization
  - `util_pct = time_io_ms_delta / window_ms * 100`
  - This is the closest direct analogue to `iostat %util` for a single device.
- Await
  - `await_ms = (read_time_ms_delta + write_time_ms_delta) / (read_ios_delta + write_ios_delta)`
  - If no I/O completed in the window, the metric is unavailable, not zero, and no fallback denominator is introduced.
- Average queue depth
  - `avg_queue_depth = weighted_time_io_ms_delta / window_ms`
  - This is equivalent in spirit to `avgqu-sz`.
- In-flight
  - direct point-in-time value from the diskstats row

### Saturation Classification

Phase 1 uses a deliberately simple, explicit classification model.

- `unavailable`
  - device unresolved, kernel fields missing, or window invalid
- `idle`
  - negligible throughput and low utilization
- `active`
  - meaningful I/O with no strong saturation signal
- `saturated`
  - one of:
    - `disk_util_pct >= 85` and `disk_avg_queue_depth >= 1.0`
    - `disk_util_pct >= 90`
    - `disk_await_ms` materially elevated while write throughput remains high

The implementation should keep the rule table centralized and documented. The threshold values are intentionally conservative and may be tuned later, but the classification logic itself should remain explicit and auditable.

### Data Model Rules

The telemetry must distinguish missing data from true zero values.

- Do not serialize unresolved telemetry as `0.0`.
- Prefer explicit absence or `unavailable` state over numeric zero when the device cannot be read.
- Summary reports must not average unresolved windows into zero-heavy aggregates.

This is required to avoid repeating the misleading `avg_disk_write_mb_per_batch=0.000` situation seen in current artifacts.

## Phase 1 Outputs

### Perf Artifacts

Extend `BatchSample` and the underlying sample stream to include the new disk window metrics.

Artifacts should expose:

- per-sample metrics in `samples.jsonl`
- run-level aggregation in `report.md`
- machine-readable summary in `metrics.env`

Add at least these run-level aggregates:

| Aggregate | Meaning |
|----------|---------|
| `avg_disk_util_pct` | Average utilization across valid windows |
| `p95_disk_util_pct` | High-end device busy level |
| `avg_disk_await_ms` | Average device latency across valid windows |
| `p95_disk_await_ms` | Tail latency during the run |
| `max_disk_avg_queue_depth` | Peak backlog at the device |
| `peak_disk_write_mb_s` | Peak sustained write throughput |
| `peak_disk_write_iops` | Peak sustained write IOPS |
| `saturated_window_count` | Number of windows classified as saturated |
| `saturated_window_ratio` | Fraction of valid windows classified as saturated |
| `disk_telemetry_status` | `ok`, `partial`, or `unavailable` |

`report.md` should also include a short conclusion line, for example:

> Device saturation observed in 61% of valid windows; p95 await 18.4 ms; peak write throughput 742 MB/s.

### Runtime Logs

Bulk sync heartbeat logs should include current disk pressure fields:

- `disk_state`
- `disk_util_pct`
- `disk_await_ms`
- `disk_qd`
- `disk_wr_mb_s`
- `disk_wr_iops`

These values should be emitted only when bulk sync is active and telemetry is available. If telemetry is unavailable, log the reason once and keep subsequent heartbeat noise low.

### TUI

Add a dedicated disk pressure panel or equivalent section near runtime health. Phase 1 should show:

- current `state`
- current `util%`
- current `await`
- current `queue depth`
- current `write MB/s`
- recent or peak values

Text-first presentation is sufficient. Phase 1 does not need sparklines or history graphs.

## Phase 2: RocksDB Attribution

Phase 2 keeps the same sampling windows and overlays existing write-path backlog metrics:

- `compaction_pending_mb`
- `l0_files`
- `imm_memtables`
- `flush_ms`
- `flush_wait_ms`
- `flush_channel_pending`

The purpose is not to build a new dashboard. It is to support interpretation of a single run.

### Target Interpretations

The report should make these distinctions possible:

- **Physical device saturation**
  - high `disk_util_pct`
  - elevated `disk_await_ms`
  - sustained queue depth
  - flush wait and write-path latency rise in the same windows
- **RocksDB backlog before hardware saturation**
  - low or moderate disk utilization
  - but high `compaction_pending_mb`, `l0_files`, or `imm_memtables`
  - flush/compaction time rises without the device looking fully busy
- **Pipeline/controller coordination issue**
  - high `flush_wait_ms`
  - but neither device saturation nor strong RocksDB backlog fully explains it

Phase 2 only needs enough aggregation and summary text to support those judgments. Per-CF attribution remains a later enhancement.

## Phase 3: Device Resolution Hardening

Phase 1 still requires a minimum viable device resolver so the telemetry can attach to the correct block device for common Linux setups.

The minimum supported cases are:

- raw partition mounts such as ext4/xfs
- NVMe namespaces with partition suffixes
- dm-crypt / LUKS style mapped devices
- common btrfs mount layouts

Resolution should produce two outputs:

- `disk_device`
  - the device name used to look up `/proc/diskstats`
- `filesystem`
  - the filesystem label/type for environment metadata

If the resolver cannot determine a valid device, the system should mark disk telemetry unavailable instead of silently writing zeroed metrics.

More ambitious resolution work, such as multi-device filesystem awareness, is deferred.

## Component Changes

### `crates/indexer/src/sys_info.rs`

- Expand the diskstats parser beyond read/write sectors
- Replace the simple delta tracker with a windowed stats tracker
- Add explicit result types for:
  - valid window
  - first sample / warmup
  - unavailable / unresolved
- Keep parser helpers directly testable with string fixtures

### `crates/indexer/src/sync/bulk_build/sampler.rs`

- Extend `SamplerSnapshot` with disk pressure fields
- Preserve cheap periodic sampling semantics
- Ensure unresolved telemetry does not collapse to misleading zeros

### `crates/indexer/src/bulk_sync_perf.rs`

- Extend `BatchSample`
- Add run-level aggregation for disk saturation metrics
- Update `report.md` and `metrics.env` writers
- Add disk telemetry status to environment or run summary output

### `crates/indexer/src/entry.rs`

- Extend bulk sync heartbeat logs with disk pressure fields
- Emit a low-noise warning when disk telemetry is unavailable

### `crates/tui/src/ui.rs`

- Add a disk pressure section
- Display `unavailable` explicitly when telemetry cannot be collected

## Error Handling

Follow the project’s fail-fast rules for correctness, but do not make sync fail just because telemetry is unavailable.

Expected behavior:

- malformed parser assumptions in test fixtures or internal invariants should fail tests immediately
- runtime inability to resolve or read telemetry should degrade to explicit `unavailable`
- no silent lower-bound repair such as treating unavailable latency as `0 ms`

The reason is operational: telemetry is diagnostic, not canonical chain state. It must be truthful, but it is not allowed to break a healthy sync.

## Testing Strategy

Every code change must add or update tests.

### `sys_info.rs`

- parse complete diskstats rows
- compute deltas across windows
- derive `util`, `await`, `queue depth`, throughput, and IOPS correctly
- handle first-sample warmup behavior
- handle missing device rows
- handle zero-I/O windows without inventing latency
- cover common device naming patterns

### `bulk_sync_perf.rs`

- aggregate sample streams into the new summary metrics
- exclude unavailable windows from numeric aggregates
- produce correct `saturated_window_ratio`
- keep report rendering stable when only partial telemetry is available

### Logging / TUI

- formatting tests for heartbeat disk fields
- rendering tests for `active`, `saturated`, and `unavailable` states

## Acceptance Criteria

A fresh DB run is considered telemetry-complete for phase 1 when:

1. `samples.jsonl` includes windowed disk saturation metrics for valid windows.
2. `report.md` can state whether device saturation occurred and how often.
3. `metrics.env` exposes machine-readable utilization, latency, queue depth, and saturation ratios.
4. runtime heartbeat logs make current disk pressure visible during bulk sync.
5. TUI makes the current disk state visible without needing to inspect raw logs.
6. unresolved telemetry is labeled explicitly and never presented as zero activity.

## Rejected Alternatives

### Keep Only Read/Write MB Deltas

Rejected because throughput alone cannot distinguish:

- a saturated device
- a lightly loaded fast device
- a backlog building above the device

### Add eBPF or Per-Process Tracing First

Rejected for phase 1 because it adds operational cost, permissions complexity, and portability burden before the simpler `/proc/diskstats` path has been exhausted.

### Solve Device Resolution First and Delay Saturation Metrics

Rejected because the main user question is whether hardware is saturated. Resolution hardening is necessary, but only as the smallest prerequisite for that answer.

## Rollout Plan

1. Implement phase 1 for Linux bulk-build sampling, artifacts, logs, and TUI.
2. Validate on a fresh DB run and verify the artifacts can classify disk-bound windows.
3. Add phase 2 report-level attribution against RocksDB backlog metrics.
4. Harden device resolution for more Linux storage topologies.

## Open Questions

- Whether pipeline-mode sampling should adopt the full disk pressure model in the same patch or immediately after bulk-build support lands.
- Whether run summaries should keep both raw numeric fields and higher-level text conclusions in `metrics.env`, or reserve prose for `report.md` only.
