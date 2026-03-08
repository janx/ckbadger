# Bulk Sync Wall-Clock Optimization Design

## Goal

Reduce fresh-db bulk sync wall-clock time from ~5,445s (~1:30:45) toward ~3,100-3,600s by addressing the four largest bottlenecks: controller regression, commit overhead, cell write cost, and compaction stalls.

## Principle Alignment

- **CKB Native**: No change to chain data semantics
- **Local First**: Faster rebuild = cheaper experiments, aligns with local-first philosophy
- **Agent Friendly**: Perf artifacts already track wall-clock; optimization is measurable

## Current Baseline

| Metric              | Best Run (ba7f132) | Latest Run (10df88e) |
| ------------------- | ------------------ | -------------------- |
| Wall clock          | 4,774s             | 5,445s               |
| Batches             | 3,807              | 4,348                |
| Avg blocks/batch    | 4,934              | 4,321                |
| Small batches (<2K) | 45 (1.2%)          | 540 (12.4%)          |
| Total commit time   | 2,739s             | 3,172s               |
| blocks/sec (wall)   | 3,935              | 3,450                |

### Time Breakdown (latest run, 4,833s batch-level)

| Phase           | Seconds | % of Batch |
| --------------- | ------- | ---------- |
| RocksDB commit  | 3,172   | 65.6%      |
| T1 cell writes  | 2,442   | 50.5%      |
| Precompute      | 1,778   | 36.8%      |
| Activity writes | 1,209   | 25.0%      |
| NFT precompute  | 956     | 19.8%      |
| Parse           | 380     | 7.9%       |

Phases overlap (parallel threads), so percentages exceed 100%.

### Chain Region Throughput

| Region          | Blocks/sec | Character          |
| --------------- | ---------- | ------------------ |
| Early (0-5M)    | 11,751     | Light workload     |
| Mid (5-10M)     | 3,222      | Heavy cells/inputs |
| Late (10-15M)   | 2,668      | Slowest, NFT-dense |
| Recent (15-20M) | 5,096      | Moderate           |

## Constraints

- All data must be written inline per BULK_SYNC.md (no deferred index rebuild)
- Live sync path must remain unchanged
- Fail-fast on invariant violations
- Single calculation path for derived data

## Design

### 1. Controller Policy Rollback

**Problem**: Commits `10d8627` and `e9aa560` changed the adaptive controller to use `l0_files_total` in pressure decisions and added `far_bulk_cost_backoff_allowed` gating. This produces 540 small batches (12.4%) vs 45 (1.2%) in the best run. Root cause: `l0_files_total >= 48` triggers `rocksdb_moderate_pressure` too easily — with 40 CFs, even 2 L0 files per CF = 80 total, which is normal.

**Change**: Revert `update_after_write()` policy to `ba7f132` behavior:

- Remove `l0_files_total` from `rocksdb_severe_pressure` and `rocksdb_moderate_pressure` conditions
- Remove `healthy_absolute_write_cost` and `far_bulk_cost_backoff_allowed` gates
- Revert `severe_floor_relaxation` to simple `severe_pressure` check
- Revert `min_target_batch_txs` clamp from `ADAPTIVE_BATCH_MAX_TXS` back to `ADAPTIVE_BATCH_BASE_MIN_TXS`
- Keep `l0_files_total` field in `AdaptiveBatchInput` and perf samples for diagnostics only

**Files**: `crates/indexer/src/sync/adaptive.rs`

**Expected gain**: ~670s (14%), recovering best-run batch count

### 2. Bulk Sync Commit Consolidation

**Problem**: The parallel write path commits ~13 times per batch (9 domain + 4 append). Each `commit_no_wal()` triggers RocksDB write group serialization, memtable insert, and potential flush. Commit is 65.6% of batch time.

**Current commit structure per batch**:

| Thread                | Domain | Append |
| --------------------- | ------ | ------ |
| T1_cells              | 1      | 0      |
| T2_txs_addr           | 1      | 1      |
| T4_dao                | 1      | 0      |
| T5_udt                | 1      | 0      |
| T6a_spore             | 1      | 1      |
| T6b_mnft_dotbit       | 1      | 1      |
| T_ACT                 | 1      | 1      |
| Finalize (core+stats) | 2      | 0      |
| **Total**             | **~9** | **~4** |

**Change**: Threads build StoreBatch but don't commit. Main thread merges and commits once per store.

```
Before:  Thread → build batch → commit → return timing
After:   Thread → build batch → return (batch, timing)
         Main   → merge domain batches → 1 domain commit
                → merge append batches → 1 append commit
```

Implementation:

1. `StoreBatch::into_parts()` returns `(WriteBatch, Vec<AppendBatchOp>, &CkbadgerStore)`
2. Domain merge: `WriteBatch::append(&mut other)` for all domain batches
3. Append merge: concatenate `append_ops` vectors, run dedup + validation once
4. Finalize batch (block headers + stats) written into the merged domain WriteBatch
5. Only bulk sync path changes; live sync retains existing multi-commit structure

**Safety**: Bulk sync crash = rebuild from genesis. Single WriteBatch is more atomic than multi-commit (all-or-nothing). Threads write non-overlapping CFs, no key conflicts.

**Files**: `crates/ckbadger-store/src/batch.rs`, `crates/indexer/src/sync/batch.rs`

**Expected gain**: ~270-540s (commit fixed overhead reduced ~80%)

### 3. T1 Raw-Key Reuse

**Problem**: T1_cells is 50.5% of batch time. Each cell's outpoint key (34 bytes) is encoded repeatedly across `put_cell`, `put_live_cell`, `consume_cell` operations.

**Change**:

1. Add raw-key payload methods to StoreBatch:
   - `put_cell_raw_key(raw_key, info)`
   - `put_live_cell_raw_key(raw_key)`
   - `delete_live_cell_raw_key(raw_key)`
   - `put_consumed_cell_raw_key(raw_key, ...)`

2. In T1 thread: pre-encode outpoint key once per cell, reuse across all operations

3. Update `insert_cells_batch` and `consume_cells_batch_preloaded` to use pre-encoded keys

**Files**: `crates/ckbadger-store/src/batch.rs`, `crates/indexer/src/db/writer/cells.rs`, `crates/indexer/src/sync/batch.rs`

**Expected gain**: ~120-240s (5-10% of T1 phase)

### 4. L0 Threshold Tuning

**Problem**: Bulk sync L0 slowdown at 64 files, stop at 128. Peak observed: 112-119 files. Writer spends significant time in slowdown zone.

**Change**: In `enter_bulk_sync_mode()`:

- Slowdown: 64 → 96
- Stop: 128 → 192

Reverted in `exit_bulk_sync_mode()`.

**Synergy with Section 2**: Commit consolidation reduces L0 file production rate (fewer commits = fewer memtable flushes). Raised thresholds provide additional headroom.

**Trade-off**: More space amplification during bulk sync; slightly longer compaction drain at exit. Acceptable since bulk sync completion already waits for compaction drain.

**Files**: `crates/ckbadger-store/src/store.rs`

**Expected gain**: ~100-200s

## Combined Expected Gain

| Optimization         | Estimated Gain    | Cumulative        |
| -------------------- | ----------------- | ----------------- |
| Controller rollback  | ~670s             | ~4,775s           |
| Commit consolidation | ~270-540s         | ~4,235-4,505s     |
| T1 raw-key reuse     | ~120-240s         | ~3,995-4,385s     |
| L0 threshold tuning  | ~100-200s         | ~3,795-4,285s     |
| **Total**            | **~1,160-1,650s** | **~3,795-4,285s** |

Target: ~3,100-3,600s wall-clock (35-43% improvement over latest 5,445s).

Note: Estimates assume independent gains. Actual improvement may differ due to interactions (e.g., commit consolidation + L0 tuning synergy may exceed sum of parts, but controller fix + commit consolidation may partially overlap if some small-batch overhead was commit-dominated).

## Testing Strategy

- Controller: unit tests in `adaptive.rs` verifying best-run policy behavior
- Commit consolidation: unit test proving merged batch produces same data as separate commits; integration test in `reorg_handling.rs`
- T1 raw-key: unit test comparing `put_cell_raw_key` output against `put_cell`
- L0 thresholds: verify values in `enter_bulk_sync_mode()` unit test
- Full validation: fresh-db bulk sync run with perf artifact comparison

## Validation

- Fresh-db run beats 4,774s (best run baseline)
- Perf artifact `blocks_per_sec_wall` improves
- Batch count returns to ~3,800 range (controller fix)
- No correctness regression in `cargo test`

## Rollout Order

1. Controller policy rollback (independent, biggest single win)
2. Commit consolidation (independent, structural fix)
3. T1 raw-key reuse (depends on understanding commit consolidation's batch structure)
4. L0 threshold tuning (independent, can be done anytime)
5. Fresh-db verification run
