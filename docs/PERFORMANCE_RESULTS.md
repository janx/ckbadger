# Performance Benchmark Results

## Overview

This document tracks the performance of the CKB indexer with the in-memory LiveCellStore optimization.

**Target**: 5000+ blocks/sec sustained at 10M block height

## Benchmark Configuration

### Hardware

- CPU: [To be filled]
- RAM: [To be filled]
- Storage: [To be filled]

### Software

- PostgreSQL version: [To be filled]
- CKB node version: [To be filled]
- Indexer configuration:
  - `--live-cell-memory-limit`: 8GB (default)
  - `--live-cell-flush-interval`: 100 batches (default)
  - `--pipeline-enabled`: true
  - `--batch-size`: 10000

## Benchmark Methodology

Run the benchmark script:

```bash
# Quick benchmark (first 100K blocks)
./scripts/benchmark_sync.sh --quick

# Full benchmark (0-10M blocks)
./scripts/benchmark_sync.sh --start 0 --end 10000000
```

## Results

### Checkpoint Performance

| Checkpoint | Blocks Synced | Duration (sec) | Blocks/sec | Memory (GB) |
| ---------- | ------------- | -------------- | ---------- | ----------- |
| 1M         | -             | -              | -          | -           |
| 5M         | -             | -              | -          | -           |
| 10M        | -             | -              | -          | -           |

**Status**: PENDING - Requires running benchmark with DATABASE_URL and CKB_RPC_URL

### Memory Usage

- Peak memory during sync: [To be measured]
- LiveCellStore memory: [To be measured]
- PostgreSQL shared_buffers: [To be measured]

### Crash Recovery

- Time to rebuild LiveCellStore from DB: [To be measured]
- Target: <5 minutes for 50M cells

## Expected Improvements

Based on the implementation:

1. **Bulk Sync Mode**: Skips DB UPDATE operations when >1000 blocks behind tip
2. **In-Memory Lookups**: O(1) cell lookups via HashMap instead of DB queries
3. **Periodic Flush**: Dirty cells flushed every 100 batches (configurable)
4. **Deferred Indexes**: Non-essential indexes dropped during initial sync

### Theoretical Performance Gains

| Operation        | Before          | After           | Improvement |
| ---------------- | --------------- | --------------- | ----------- |
| Cell lookup      | ~1ms (DB)       | ~1μs (memory)   | ~1000x      |
| Cell consumption | UPDATE + DELETE | Memory only     | ~10x        |
| Batch processing | Sequential DB   | Parallel memory | ~5x         |

## How to Run Benchmark

```bash
# 1. Set up PostgreSQL
export DATABASE_URL=postgres://user:pass@localhost/ckbadger

# 2. Set up CKB node
export CKB_RPC_URL=http://localhost:8114

# 3. Run benchmark
./scripts/benchmark_sync.sh --quick  # Quick test
./scripts/benchmark_sync.sh --start 0 --end 10000000  # Full benchmark

# 4. Update this document with results
```

## Conclusion

[To be filled after benchmark completion]

---

_Last updated: 2026-01-28_
_Status: Awaiting benchmark execution_
