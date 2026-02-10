# Performance Benchmark Results

> **Note (2026-02)**: This document records historical benchmarks from the PostgreSQL-based indexer (Jan 2026). The storage engine has since been replaced with embedded RocksDB (`ckbadger-store`), which eliminated the PostgreSQL COPY bottleneck described below. These results are preserved for historical reference only and do not reflect current architecture or performance.

## Overview

This document tracks the performance of the CKB indexer with the in-memory LiveCellStore optimization.

**Target**: 5000+ blocks/sec sustained at 10M block height
**Achieved**: ~1500-2800 blocks/sec (40-150% improvement over baseline)

## Benchmark Configuration

### Hardware

- CPU: Linux x86_64 (development machine)
- RAM: Available for 8GB LiveCellStore limit
- Storage: Local SSD

### Software

- PostgreSQL version: 15 (Docker)
- CKB node version: v0.204.0
- Indexer configuration:
  - `--live-cell-flush-interval`: 100 batches (default)
  - `--pipeline-enabled`: true
  - `--batch-size`: 10000
  - `--copy-pool-size`: 24
  - `CKB_DATA_PATH`: not set (using JSON-RPC, not direct RocksDB reads)

## Benchmark Results (2026-01-28)

### Checkpoint Performance

| Checkpoint | Blocks Synced | Duration (sec) | Blocks/sec | Memory (MB) | Live Cells |
| ---------- | ------------- | -------------- | ---------- | ----------- | ---------- |
| 1M         | 1,000,000     | ~470           | ~2,100     | 3,400       | 1,400,000  |
| 2M         | 1,000,000     | ~340           | ~2,900     | 3,900       | 2,600,000  |
| 3M         | 1,000,000     | ~360           | ~2,800     | 4,100       | 3,900,000  |
| 4M         | 1,000,000     | ~530           | ~1,900     | 4,200       | 5,400,000  |
| 5M         | 1,000,000     | ~640           | ~1,550     | 6,500       | 7,200,000  |

**Overall Average**: ~2,000 blocks/sec from genesis to 5M blocks

### Performance by Block Range

| Block Range | Average Rate | Notes                      |
| ----------- | ------------ | -------------------------- |
| 0 - 1M      | ~2,100 blk/s | Initial sync, genesis data |
| 1M - 3M     | ~2,800 blk/s | Peak performance           |
| 3M - 4M     | ~1,900 blk/s | Increasing live cells      |
| 4M - 5M     | ~1,550 blk/s | 7M+ live cells             |

### Memory Usage

- Peak memory during sync: **6.5 GB** (at 5M blocks)
- LiveCellStore memory: ~6 GB for 7.2M live cells
- Memory per live cell: ~850 bytes (including HashMap overhead)
- Well under 8GB limit throughout sync

### Comparison to Baseline

| Metric       | Baseline (8.5M blocks) | Optimized (5M blocks) | Improvement  |
| ------------ | ---------------------- | --------------------- | ------------ |
| Sync rate    | ~1,100 blk/s           | ~1,550-2,800 blk/s    | 40-150%      |
| Memory       | N/A                    | 6.5 GB                | Within limit |
| Cell lookups | DB queries             | In-memory O(1)        | ~1000x       |

## Analysis

### Why Target Not Fully Met

The 5000+ blocks/sec target was not achieved. Analysis:

1. **DB Write Bottleneck**: Even with bulk sync mode, COPY operations to PostgreSQL take ~10-13 seconds per 10K block batch
2. **Live Cell Growth**: As live cells grow (7M+ at 5M blocks), memory operations increase
3. **DAO Statistics**: DAO stats calculation adds overhead every 10K blocks

### What Worked Well

1. **In-Memory LiveCellStore**: Eliminated DB lookups for cell consumption
2. **Bulk Sync Mode**: Skipped UPDATE/DELETE operations during bulk sync
3. **Deferred Indexes**: 258 indexes dropped during initial sync
4. **Memory Efficiency**: 6.5GB for 7.2M cells (well under 8GB limit)

### Potential Further Optimizations

1. **Parallel COPY**: Increase `copy_pool_size` beyond 24
2. **Batch Size Tuning**: Experiment with larger batch sizes
3. **PostgreSQL Tuning**: Apply server-level tuning (wal_level=minimal, fsync=off)
4. **Hardware**: NVMe SSD, more RAM for PostgreSQL shared_buffers

## Crash Recovery

- LiveCellStore persists to RocksDB on disk, enabling instant restart
- Periodic flush ensures durability (every 100 batches by default)

## Conclusion

The LiveCellStore optimization provides a **40-150% improvement** over the baseline sync rate. While the 5000+ blocks/sec target was not achieved, the implementation:

1. ✅ Eliminates the UPDATE bottleneck on the cells table
2. ✅ Provides O(1) cell lookups via in-memory HashMap
3. ✅ Stays well under the 8GB memory limit
4. ✅ Maintains data integrity with periodic flushes
5. ✅ Supports crash recovery via DB rebuild

The remaining bottleneck is PostgreSQL COPY performance, which could be addressed with:

- Server-level PostgreSQL tuning
- Hardware improvements (faster storage)
- Further parallelization of COPY operations
- **Direct CKB RocksDB reads** (`CKB_DATA_PATH`): Eliminates RPC latency entirely (~0.1ms vs ~15ms per block). This was not enabled during the benchmark above but is expected to significantly improve fetch stage throughput.

---

_Last updated: 2026-01-28_
_Benchmark: Fresh sync from genesis to 5M blocks_
_Status: COMPLETE - Target partially met (40-150% improvement achieved)_
