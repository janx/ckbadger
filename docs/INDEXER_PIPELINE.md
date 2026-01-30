# Indexer Three-Stage Pipeline Architecture

The CKB indexer uses a three-stage pipeline architecture to maximize sync throughput by parallelizing RPC I/O, CPU parsing, and database writes.

## Overview

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│     FETCHER     │────▶│     PARSER      │────▶│     WRITER      │
│   (Async I/O)   │     │  (CPU + Prefetch)│     │    (DB I/O)     │
└─────────────────┘     └─────────────────┘     └─────────────────┘
        │                       │                       │
   RPC requests           Rayon parallel          Batch INSERTs
   to CKB node            block parsing           and UPDATEs
```

### Design Goals

1. **Decouple I/O from computation** - RPC fetching doesn't block parsing; parsing doesn't block DB writes
2. **Maximize parallelism** - Each stage can work on different batches simultaneously
3. **Maintain consistency** - Pipeline mode produces identical data to sequential mode
4. **Handle failures gracefully** - Stale batches are drained on errors; periodic db_tip resync prevents drift

## Pipeline Stages

### Stage 1: Fetcher (Async I/O)

**Location**: `run_pipeline()` fetcher task

**Responsibilities**:

- Query chain tip from CKB RPC
- Fetch blocks in parallel batches (`parallel_fetch_size` concurrent requests)
- Send raw blocks to parser channel

**Key behaviors**:

- Tracks `next_block` locally to avoid re-querying db_tip (prevents race condition - see POSTMORTEM IDX-004)
- Resets `next_block` to `None` every 1000 blocks to resync with writer
- On fetch error, waits 5s and resets `next_block` for recovery

```rust
type FetchedBatch = (u64, u64, Vec<BlockResponseWithCycles>);
//                  start  end   raw blocks with cycles data
```

### Stage 2: Parser (CPU + DB Prefetch)

**Location**: `run_pipeline()` parser task + `parse_blocks_parallel()`

**Responsibilities**:

1. **Parallel parsing** via Rayon:
   - Block headers, transactions, cells
   - Collect all input outpoints for later consumption lookup

2. **Cell info prefetch**:
   - Check LRU cache for input cell info (capacity, lock_script_hash, data_size)
   - Batch-fetch missing cell info from DB (`get_cells_info_batch`)
   - Fetch code_hashes for consumed cells from previous batches (`get_cells_code_hashes_batch`)

**Output structure**:

```rust
type ParsedBatch = (
    u64,                                                // start_block
    u64,                                                // end_block
    Vec<BlockResponseWithCycles>,                       // raw blocks (needed for UDT parsing)
    Vec<ParsedBlock>,                                   // parsed block headers
    Vec<TxData>,                                        // parsed transactions with cells
    HashMap<(Vec<u8>, i16), (i64, i64, Vec<u8>, i32)>,  // input_cell_info: (capacity, created_at_block, lock_hash, data_size)
    HashMap<(Vec<u8>, i16), (Vec<u8>, Option<Vec<u8>>)> // consumed_code_hashes: (lock_code_hash, type_code_hash)
);
```

### Stage 3: Writer (DB I/O)

**Location**: `run_pipeline()` main loop + `write_parsed_batch()`

**Responsibilities**:

1. Validate batch sequence (expected start_block matches db_tip + 1)
2. Check for chain reorgs before processing
3. Write all data to database:
   - Blocks, transactions, cells
   - Cell consumptions with script usage tracking
   - DAO deposits/withdrawals
   - Token transfers (UDT, NFT, DOB)
   - Statistics (hourly, daily, epoch)
4. Update sync_status LAST (crash recovery guarantee)
5. Trigger periodic DAO statistics recalculation

## Data Flow

```
Block N arrives
       │
       ▼
┌──────────────────────────────────────────────────────────────┐
│ PARSER                                                        │
│  1. parse_blocks_parallel() - extract all structured data     │
│  2. Collect input outpoints: [(tx_hash, output_index), ...]   │
│  3. Cache lookup for cell info                                │
│  4. DB batch fetch for cache misses                           │
│  5. DB fetch for consumed code_hashes (script usage tracking) │
└──────────────────────────────────────────────────────────────┘
       │
       ▼ ParsedBatch
       │
┌──────────────────────────────────────────────────────────────┐
│ WRITER                                                        │
│  1. Validate batch sequence                                   │
│  2. Check for reorg                                           │
│  3. Insert blocks, txs, cells (parallel)                      │
│  4. Insert inputs, cell_deps (parallel)                       │
│  5. Consume cells (update status, delete from live_cells)     │
│  6. Update address balances, txs, script usage (parallel)     │
│  7. Process DAO deposits/withdrawals                          │
│  8. Process token transfers                                   │
│  9. Flush batch statistics                                    │
│ 10. Update sync_status (LAST - crash recovery)                │
└──────────────────────────────────────────────────────────────┘
```

## Configuration

| Parameter             | Default | Description                                                     |
| --------------------- | ------- | --------------------------------------------------------------- |
| `pipeline_enabled`    | `true`  | Enable three-stage pipeline (vs sequential sync)                |
| `pipeline_buffer`     | `16`    | Channel capacity between stages                                 |
| `batch_size`          | `10000` | Blocks per batch                                                |
| `parallel_fetch_size` | `64`    | Concurrent RPC requests                                         |
| `bulk_sync_threshold` | `72`    | Blocks behind tip to auto-enable bulk sync (2x DEEP_FORK_DEPTH) |
| `use_copy_bulk_sync`  | `true`  | Use PostgreSQL COPY for bulk sync (5-10x faster)                |
| `copy_pool_size`      | `24`    | Number of COPY connection pool connections                      |
| `fast_sync_mode`      | `true`  | Enable synchronous_commit=off for faster writes                 |

### Environment Variables

```bash
PIPELINE_ENABLED=true
PIPELINE_BUFFER=16
BATCH_SIZE=10000
PARALLEL_FETCH_SIZE=64
BULK_SYNC_THRESHOLD=72
USE_COPY_BULK_SYNC=true
COPY_POOL_SIZE=24
FAST_SYNC_MODE=true
```

### CLI Arguments

```bash
cargo run -p ckbadger-indexer -- \
  --pipeline-enabled \
  --pipeline-buffer 16 \
  --batch-size 10000 \
  --parallel-fetch-size 64 \
  --bulk-sync-threshold 72 \
  --use-copy-bulk-sync \
  --copy-pool-size 24
```

## Error Handling

### Batch Mismatch

When writer receives a batch with unexpected start_block:

```
WARN Pipeline batch mismatch: expected 4086800, got 4086700. Draining stale batches.
```

**Recovery**: Drain all pending batches from channel, fetcher will resync on next db_tip read.

### Write Failure

If `write_parsed_batch()` fails:

1. Log error
2. Drain pending batches
3. Sleep 5 seconds
4. Fetcher resyncs via periodic db_tip refresh

### Reorg Detection

Before processing each batch:

1. Fetch current db_tip and hash
2. Compare with chain's block at that height
3. If mismatch: handle reorg, drain stale batches

### Deep Fork

If reorg depth exceeds `REORG_LIMIT` (36 blocks):

1. Flag `has_unresolved_deep_fork` in database
2. Pause sync with 30s sleep loop
3. Require manual intervention

## Consistency Guarantees

### Pipeline vs Sequential Mode

Both modes MUST produce identical database state. This is enforced by:

1. **Same parsing logic**: `parse_blocks_parallel()` used by both
2. **Same write logic**: `write_parsed_batch()` mirrors `sync_blocks_batch()`
3. **Same features**:
   - DAO deposit/withdrawal tracking
   - Token transfers (UDT mint/transfer/burn)
   - NFT transfers (Spore, MNFT, Dotbit)
   - DOB transfers
   - Script usage statistics
   - All hourly/daily/epoch statistics

### Verified Consistency Points

| Feature                      | Sequential | Pipeline |
| ---------------------------- | ---------- | -------- |
| `insert_cells_batch()`       | Yes        | Yes      |
| `consume_cells_batch()`      | Yes        | Yes      |
| `insert_dao_deposits()`      | Yes        | Yes      |
| `complete_dao_withdrawals()` | Yes        | Yes      |
| `insert_udt_cells_batch()`   | Yes        | Yes      |
| `insert_token_transfer()`    | Yes        | Yes      |
| `insert_nft_transfer()`      | Yes        | Yes      |
| `insert_dob_transfer()`      | Yes        | Yes      |
| `update_script_usage()`      | Yes        | Yes      |
| `flush_batch_stats()`        | Yes        | Yes      |

## Performance Characteristics

### Throughput

With default settings on typical hardware:

| Mode                 | Blocks/sec | Bottleneck |
| -------------------- | ---------- | ---------- |
| Sequential           | ~150-200   | DB writes  |
| Pipeline (buffer=8)  | ~280-320   | DB writes  |
| Pipeline (buffer=16) | ~400-500   | DB writes  |
| Pipeline + COPY      | ~5000-7000 | DB writes  |

**Optimizations**:

1. **Parallel DB writes**: Within each batch, independent operations run concurrently:
   - blocks, transactions, cells inserts (parallel)
   - inputs, cell_deps inserts (parallel)
   - address balances, script usage updates (parallel)

2. **Binary COPY**: When bulk sync is active (blocks_remaining > threshold), the indexer automatically uses PostgreSQL Binary COPY for 5-10x faster writes. See [Binary COPY Infrastructure](#binary-copy-infrastructure) for details.

### Memory Usage

Pipeline mode uses more memory due to buffered batches:

```
Memory ≈ pipeline_buffer × batch_size × (block_size + parsed_data)
       ≈ 16 × 10000 × (~100KB per block)
       ≈ 16GB additional
```

### Channel Backpressure

When writer is slower than fetcher+parser:

- Channels fill to capacity (`pipeline_buffer`)
- Fetcher blocks on send, naturally throttling RPC calls
- No unbounded memory growth

## Monitoring

### Log Messages

```
# Normal operation
INFO Syncing blocks 1000 to 1499 (498501 remaining, 285.32 blocks/sec)
PERF[500blks] RPC=125.3ms DB=1450.2ms

# Batch mismatch (recoverable)
WARN Pipeline batch mismatch: expected 2000, got 1500. Draining stale batches.
INFO Drained 3 stale batches from pipeline

# Write error (recoverable)
ERROR Sync error: database connection failed
INFO Drained 2 stale batches from pipeline

# Deep fork (requires intervention)
WARN Deep fork detected, sync paused
WARN Deep fork unresolved, sync paused. Waiting for manual intervention...
```

### Metrics

Key metrics to monitor:

- `blocks/sec` - overall sync speed
- `RPC time` - fetcher stage latency
- `DB time` - writer stage latency
- `stale batches drained` - indicates mismatch frequency

## Comparison: Pipeline vs Sequential

| Aspect         | Sequential (`sync_blocks_batch`) | Pipeline (`run_pipeline`)    |
| -------------- | -------------------------------- | ---------------------------- |
| Architecture   | Single loop                      | 3 async tasks + channels     |
| Parallelism    | Within batch (DB writes)         | Across stages + within batch |
| Memory         | Lower                            | Higher (buffered batches)    |
| Complexity     | Simpler                          | More complex                 |
| Error recovery | Simpler                          | Drain + resync               |
| Best for       | Small syncs, debugging           | Initial sync, production     |

## Implementation Notes

### Why Raw Blocks in ParsedBatch?

The parsed batch includes raw `BlockResponseWithCycles` because:

1. UDT parsing needs access to witness data (not in `TxData`)
2. Some script detection requires original transaction structure

### Cell Cache Strategy

Two-level lookup for consumed cell info:

1. **LRU Cache** (200k entries): Same-batch and recent block consumptions
2. **DB Batch Query**: Cache misses fetched in single query per batch

### Script Usage Tracking

To track which scripts are used in consumed cells:

1. Parser identifies cells consumed from **previous batches** (not same-batch)
2. Fetches their `lock_code_hash` and `type_code_hash` from DB
3. Writer uses this to update `script_usage` table

Same-batch consumptions get code_hashes from the creating transaction directly.

## Troubleshooting

### Sync Stuck / No Progress

1. Check logs for errors
2. Verify CKB node is synced and responsive
3. Check for `has_unresolved_deep_fork` flag
4. Try restarting indexer

### Data Inconsistency

1. Compare with sequential mode output
2. Check `write_parsed_batch()` vs `sync_blocks_batch()` for divergence
3. Verify all insert/update calls match

### High Memory Usage

1. Reduce `pipeline_buffer` (e.g., to 2-4)
2. Reduce `batch_size`
3. Monitor for memory leaks in channel handling

## Bulk Sync Statistics Optimization

During bulk sync (when `blocks_remaining > bulk_sync_threshold`, default 72 = 2x DEEP_FORK_DEPTH), the indexer skips non-critical statistics updates to reduce DB write time by ~15%.

### Skipped Statistics (during bulk sync)

| Table                     | Rebuilt After | Description                    |
| ------------------------- | ------------- | ------------------------------ |
| `daily_statistics`        | Yes           | Daily block/tx/cell counts     |
| `daily_block_stats`       | Yes           | Daily difficulty/uncle stats   |
| `hourly_statistics`       | Yes           | Hourly activity metrics        |
| `miner_statistics`        | Yes           | Per-miner block counts         |
| `block_time_distribution` | Yes           | Block time histogram           |
| `epoch_time_distribution` | Yes           | Epoch duration histogram       |
| `dao_daily_snapshots`     | Yes           | DAO deposit/issuance snapshots |

### Always Updated (even during bulk sync)

| Table              | Reason                                          |
| ------------------ | ----------------------------------------------- |
| `sync_status`      | Critical for crash recovery                     |
| `epoch_statistics` | Contains epoch metadata (start/end block, etc.) |

### Automatic Rebuild

When bulk sync completes (transitions from `blocks_remaining > threshold` to `<= threshold`):

1. Indexer detects state transition via `was_bulk_sync_active` flag
2. Triggers `rebuild_all_statistics()` which:
   - Truncates each statistics table
   - Rebuilds from raw data (blocks, cells, transactions)
3. Logs progress for each table

```
INFO Bulk sync completed, rebuilding skipped statistics...
INFO Rebuilding daily_statistics...
INFO daily_statistics rebuild completed
INFO Rebuilding daily_block_stats...
...
INFO All statistics rebuild completed
```

### Implementation Details

- State tracking: `was_bulk_sync_active: AtomicBool` in Indexer struct
- Detection: `check_bulk_sync_completion()` called after each batch
- Rebuild entry point: `BatchWriter::rebuild_all_statistics()`

## Binary COPY Infrastructure

The indexer uses PostgreSQL Binary COPY for high-performance bulk data loading during initial sync.

### Auto-Enabled Behavior

COPY is **automatically enabled** when:

- `blocks_remaining > bulk_sync_threshold` (default: 72 blocks behind, 2x DEEP_FORK_DEPTH)
- `use_copy_bulk_sync = true` (default)

When the indexer catches up to the chain tip, it automatically switches back to UNNEST inserts (which support conflict resolution for reorgs).

### COPY Modules

| Module                         | Table                 | Columns | Performance               |
| ------------------------------ | --------------------- | ------- | ------------------------- |
| `copy_format.rs`               | -                     | -       | Core binary serialization |
| `copy_blocks.rs`               | blocks                | 19      | 5x faster than UNNEST     |
| `copy_cells.rs`                | cells                 | 16      | 5-10x faster than UNNEST  |
| `copy_transactions.rs`         | transactions          | 16      | 5-10x faster than UNNEST  |
| `copy_inputs.rs`               | transaction_inputs    | 6       | 5-10x faster than UNNEST  |
| `copy_inputs.rs`               | transaction_cell_deps | 6       | 5-10x faster than UNNEST  |
| `copy_live_cells.rs`           | live_cells            | 10      | 5-10x faster than UNNEST  |
| `copy_address_transactions.rs` | address_transactions  | 6       | 5x faster than UNNEST     |
| `parallel_copy.rs`             | -                     | -       | Partition-aware routing   |

### COPY Pool Configuration

COPY operations use a dedicated `tokio-postgres` connection pool (separate from sqlx):

```rust
CopyConfig {
    max_copy_connections: 24,  // --copy-pool-size
    copy_batch_size: 100_000,
    copy_enabled: true,
}
```

### Log Messages

```
# COPY mode active
INFO Starting indexer (pipeline=true, copy=true, 6000000 blocks behind, threshold=1000)
INFO Bulk sync auto-enabled: 6000000 blocks behind > 1000 threshold, using COPY
INFO Syncing blocks 0 to 999 (5999001 remaining, 450.00 blocks/sec) [COPY]

# Switched to UNNEST (caught up)
INFO Syncing blocks 5999000 to 5999500 (500 remaining, 320.00 blocks/sec)
```

---

_Last updated: 2026-01-28_
