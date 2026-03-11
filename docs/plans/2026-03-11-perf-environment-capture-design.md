# Perf Environment Capture Design

## Problem

Bulk sync performance varies 30-40% between runs on identical code. The current perf artifacts lack system environment data, making it impossible to attribute variance to hardware conditions (disk I/O, memory pressure, CPU contention) vs code changes.

## Goal

- **Post-hoc attribution**: when a run is slow, artifacts contain enough data to determine root cause without reproducing
- **Automated filtering**: programmatically exclude or normalize runs with abnormal environment conditions

## Design

### 1. Static Environment Snapshot (`environment.env`)

Written once at sync start, alongside `metadata.env`. Captures immutable system context.

```env
# Hardware
cpu_model=AMD Ryzen 9 7950X 16-Core Processor
cpu_cores=24
ram_total_mb=95326
disk_device=nvme0n1
disk_scheduler=none

# OS
kernel=6.19.6-1-cachyos-eevdf
filesystem=btrfs

# RocksDB config
rocksdb_budget_gb=46
block_cache_bulk_mb=7187
wbm_bulk_mb=40726
write_buffer_mega_mb=748
l0_slowdown_bulk=64
l0_stop_bulk=128
max_background_jobs=24
max_subcompactions=6
unordered_write=true
direct_io_reads=true
```

Data sources:

- CPU: `/proc/cpuinfo` (model name, processor count)
- RAM: `/proc/meminfo` (MemTotal)
- Disk: resolve data directory mountpoint -> `/sys/block/{dev}/queue/scheduler`
- Kernel: `uname -r` via `std::process::Command`
- Filesystem: `/proc/mounts` for data directory
- RocksDB: values already computed in `store.rs`, passed through

### 2. Per-Batch Environment Fields in `BatchSample`

New fields added to existing `BatchSample` struct:

```rust
pub struct BatchSample {
    // ... existing fields ...

    // Timing anchor
    pub timestamp_utc: String,        // ISO 8601, enables external correlation

    // System pressure
    pub load_avg_1m: f64,             // /proc/loadavg field 1
    pub mem_available_mb: u64,        // /proc/meminfo MemAvailable

    // Disk I/O deltas (this batch only)
    pub disk_read_mb: f64,            // sectors read delta -> MB
    pub disk_write_mb: f64,           // sectors written delta -> MB
}
```

**Delta tracking**: `DiskStatsTracker` struct holds previous cumulative sector counts from `/proc/diskstats`. Per batch: read current, compute delta, convert to MB (sectors \* 512 / 1048576), store delta, update previous.

**First batch**: `disk_read_mb=0, disk_write_mb=0` (no previous reading).

**Failure mode**: If any procfs read fails (non-Linux), all environment fields get zero values. Never fail the batch over environment telemetry.

**Overhead**: ~10-15us per batch (3 procfs reads). Batches take 500-2000ms. 0.001% overhead.

### 3. Report & Comparison Enhancements

**`report.md`**: Add "Environment" section at top sourced from `environment.env`.

**Baseline comparison**: When both runs have `environment.env`, show diff of changed parameters. If identical, print `Environment: identical to baseline`.

**`metrics.env`**: Add environment pressure aggregates:

```env
avg_load_avg_1m=8.234
max_load_avg_1m=22.410
min_mem_available_mb=4201
avg_disk_write_mb_per_batch=48.231
```

Enables automated filtering: "ignore runs where `max_load_avg_1m > 2 * cpu_cores`".

## Implementation Scope

**Modified files:**

| File                                   | Change                                                                                                                             |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `crates/indexer/src/bulk_sync_perf.rs` | Add env fields to `BatchSample`, `EnvironmentSnapshot` struct, `DiskStatsTracker`, environment.env write, report/metrics additions |
| `crates/indexer/src/sync/pipeline.rs`  | Pass env readings when constructing `BatchSample`                                                                                  |
| `crates/indexer/src/entry.rs`          | Capture RocksDB config at startup, pass to perf run                                                                                |

**New file:**

| File                             | Purpose                                                                                                                                                                         |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/indexer/src/sys_info.rs` | Procfs/sysfs readers: `read_cpu_info()`, `read_mem_available()`, `read_load_avg()`, `read_disk_stats()`, `resolve_block_device()`, `read_kernel_version()`, `read_filesystem()` |

**No changes to**: store layer, API, frontend. New fields are additive to existing samples.jsonl format.

**Testing:**

- Unit tests for `sys_info.rs` parsers with mock procfs content
- Unit test for `DiskStatsTracker` delta computation
- Existing `bulk_sync_perf.rs` tests updated for new `BatchSample` fields
- Integration: next fresh sync run produces `environment.env` and samples with populated env fields
