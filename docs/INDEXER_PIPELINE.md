# Indexer Three-Stage Pipeline Architecture

The CKB indexer uses a three-stage pipeline architecture to maximize sync throughput by parallelizing block fetching, CPU parsing, and database writes.

## Overview

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│     FETCHER     │────▶│     PARSER      │────▶│     WRITER      │
│  (RocksDB/RPC)  │     │  (CPU + Prefetch)│     │    (DB I/O)     │
└─────────────────┘     └─────────────────┘     └─────────────────┘
        │                       │                       │
   Direct RocksDB         Rayon parallel          RocksDB batch
   reads (~0.1ms)         block parsing           writes
   or RPC fallback
```

### Design Goals

1. **Decouple I/O from computation** - Block fetching doesn't block parsing; parsing doesn't block DB writes
2. **Maximize parallelism** - Each stage can work on different batches simultaneously
3. **Maintain consistency** - Pipeline mode produces identical data to sequential mode
4. **Handle failures gracefully** - Stale batches are drained on errors; periodic db_tip resync prevents drift

## Pipeline Stages

### Stage 1: Fetcher (Async I/O)

**Location**: `run_pipeline()` fetcher task

**Responsibilities**:

- Query chain tip (from CKB RocksDB directly, or CKB RPC as fallback)
- Read blocks from CKB's RocksDB (~0.1ms per block) when `CKB_DATA_PATH` is set, or fetch via JSON-RPC
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
│  5. Consume cells (update status to consumed)                 │
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
| `pipeline_buffer`     | `8`     | Channel capacity between stages                                 |
| `batch_size`          | `10000` | Blocks per batch                                                |
| `parallel_fetch_size` | `64`    | Concurrent RPC requests (used only in RPC fallback mode)        |
| `bulk_sync_threshold` | `72`    | Blocks behind tip to auto-enable bulk sync (2x DEEP_FORK_DEPTH) |
| `ckb_data_path`       | -       | Path to CKB node's RocksDB data dir for direct reads            |

### Environment Variables

```bash
PIPELINE_ENABLED=true
PIPELINE_BUFFER=4
BATCH_SIZE=10000
PARALLEL_FETCH_SIZE=64
BULK_SYNC_THRESHOLD=72
CKBADGER_DATA_PATH=./data/ckbadger-store
CKB_DATA_PATH=/var/lib/ckb/data/db
```

### CLI Arguments

```bash
cargo run -p ckbadger-indexer -- \
  --pipeline-enabled \
  --pipeline-buffer 4 \
  --batch-size 10000 \
  --parallel-fetch-size 64 \
  --bulk-sync-threshold 72
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

Before processing each batch (only when close to chain tip):

1. Fetch current db_tip and hash
2. Compare with chain's block at that height
3. If mismatch: handle reorg, drain stale batches

**Bulk Sync Optimization**: During bulk sync (blocks_remaining > bulk_sync_threshold), reorg checks are skipped since historical blocks are already finalized (CKB finalizes after 24 blocks).

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

| Mode                 | Blocks/sec | Bottleneck     |
| -------------------- | ---------- | -------------- |
| Sequential           | ~150-200   | RocksDB writes |
| Pipeline (buffer=8)  | ~280-320   | RocksDB writes |
| Pipeline (buffer=16) | ~400-500   | RocksDB writes |
| Pipeline (optimized) | ~5000-7000 | RocksDB writes |

**Optimizations**:

1. **Parallel writes**: Within each batch, independent operations run concurrently:
   - blocks, transactions, cells writes (parallel)
   - inputs, cell_deps writes (parallel)
   - address balances, script usage updates (parallel)

2. **RocksDB WriteBatch**: All writes within a batch are grouped into atomic WriteBatch operations for maximum throughput.

### Memory Usage

Pipeline mode uses more memory due to buffered batches:

```
Memory ≈ pipeline_buffer × batch_size × (block_size + parsed_data)
       ≈ 8 × 10000 × (~100KB per block)
       ≈ 8GB additional
```

### Channel Backpressure

When writer is slower than fetcher+parser:

- Channels fill to capacity (`pipeline_buffer`)
- Fetcher blocks on send, naturally throttling reads
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
- `Fetch time` - fetcher stage latency (RocksDB or RPC)
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

1. Reduce `pipeline_buffer` (e.g., to 2-4 from default 8)
2. Reduce `batch_size`
3. Monitor for memory leaks in channel handling

## Bulk Sync Statistics Optimization

During bulk sync (when `blocks_remaining > bulk_sync_threshold`, default 72 = 2x DEEP_FORK_DEPTH), the indexer skips non-critical statistics updates to maximize write throughput.

### Skipped Statistics (during bulk sync)

| Column Family       | Rebuilt After | Description                  |
| ------------------- | ------------- | ---------------------------- |
| `daily_stats`       | Yes           | Daily block/tx/cell counts   |
| `daily_block_stats` | Yes           | Daily difficulty/uncle stats |
| `hourly_stats`      | Yes           | Hourly activity metrics      |
| `miner_stats`       | Yes           | Per-miner block counts       |
| `block_time_dist`   | Yes           | Block time histogram         |
| `epoch_time_dist`   | Yes           | Epoch duration histogram     |

### Always Updated (even during bulk sync)

| Column Family | Reason                                          |
| ------------- | ----------------------------------------------- |
| `sync_status` | Critical for crash recovery                     |
| `epoch_stats` | Contains epoch metadata (start/end block, etc.) |

### Automatic Rebuild

When bulk sync completes (transitions from `blocks_remaining > threshold` to `<= threshold`):

1. Indexer detects state transition via `was_bulk_sync_active` flag
2. Triggers `rebuild_all_statistics()` which rebuilds from raw data
3. Logs progress for each statistics group

### Implementation Details

- State tracking: `was_bulk_sync_active: AtomicBool` in Indexer struct
- Detection: `check_bulk_sync_completion()` called after each batch
- Rebuild entry point: `BatchWriter::rebuild_all_statistics()`

## Crash Recovery

The indexer implements crash recovery to handle failures during batch writes. RocksDB WriteBatch provides atomicity within a single batch, but a crash between batches can leave the store in an inconsistent state.

### Write Ordering Strategy

**Sync status is written LAST** as the "commit marker". The write order is:

1. Block headers, transactions, cells (WriteBatch)
2. Cell consumptions, address balances, script usage
3. DAO deposits, token transfers, NFT data
4. Statistics updates
5. **Sync status (LAST)** - only after all other data succeeds

This ensures that if sync_status indicates a block range, all related data is complete.

### Startup Consistency Check

On startup, `find_last_consistent_block()` validates store consistency by comparing sync_status tip against actual stored data.

### Recovery Flow

```
                    ┌─────────────────┐
                    │  Batch Write    │
                    │    Fails        │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │  Sleep 5s       │
                    │  Retry          │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
    On startup ────▶│ find_last       │
                    │ _consistent     │──▶ Detect & rollback if needed
                    │ _block()        │
                    └─────────────────┘
```

---

_Last updated: 2026-02-09_
