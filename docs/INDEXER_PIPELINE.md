# Indexer Three-Stage Pipeline Architecture

The CKB indexer uses a three-stage pipeline architecture to maximize sync throughput by parallelizing block fetching, CPU parsing, and database writes.

## Overview

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│     FETCHER     │────▶│     PARSER      │────▶│     WRITER      │
│    (RocksDB)    │     │  (CPU + Prefetch)│     │    (DB I/O)     │
└─────────────────┘     └─────────────────┘     └─────────────────┘
        │                       │                       │
   Direct RocksDB         Rayon parallel          RocksDB batch
   reads (~0.1ms)         block parsing           writes
```

### Design Goals

1. **Decouple I/O from computation** - Block fetching doesn't block parsing; parsing doesn't block DB writes
2. **Maximize parallelism** - Each stage can work on different batches simultaneously
3. **Maintain consistency** - Pipeline produces deterministic, correct database state
4. **Handle failures gracefully** - Stale batches are drained on errors; periodic db_tip resync prevents drift

## Pipeline Stages

### Stage 1: Fetcher (Async I/O)

**Location**: `run_pipeline()` fetcher task

**Responsibilities**:

- Query chain tip from the local CKB RocksDB
- Read blocks from CKB's RocksDB (~0.1ms per block) using the RocksDB path resolved from `[ckb].workdir`
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

2. **Cell info prefetch** (single DB read replaces two):
   - Check LRU cache for full input cell info (all `LiveCellInfo` fields)
   - Batch-fetch missing cell info from DB (`get_full_cells_info_batch`) — returns complete `LiveCellInfo` structs, replacing both the old `get_cells_info_batch` (4 fields) and `get_cells_code_hashes_batch` (2 fields) with a single read

**Output structure**:

```rust
type ParsedBatch = (
    u64,                                        // start_block
    u64,                                        // end_block
    u64,                                        // chain_tip
    Arc<Vec<BlockResponseWithCycles>>,           // raw blocks (needed for UDT parsing)
    Vec<ParsedBlock>,                            // parsed block headers
    Vec<TxData>,                                 // parsed transactions with cells
    HashMap<(Vec<u8>, i16), LiveCellInfo>,        // input_cell_info: full cell data for all consumed inputs
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
│  3. Cache lookup for full LiveCellInfo                        │
│  4. Single DB batch fetch for cache misses (full_cells_info)  │
└──────────────────────────────────────────────────────────────┘
       │
       ▼ ParsedBatch
       │
┌──────────────────────────────────────────────────────────────┐
│ WRITER                                                        │
│  1. Validate batch sequence                                   │
│  2. Check for reorg                                           │
│  3. Build same-batch LiveCellInfo map from ParsedCell data    │
│  4. 4-way prefetch: DAO + UDT + addr balances + script info   │
│  5. Parallel write threads:                                   │
│     T1:  Cell data + consumption (CELLS, LIVE/CONSUMED)       │
│     T1b: Cell indexes (BY_LOCK, BY_TYPE, BY_*_CODE)           │
│     T2:  Txs + addr deltas + script deltas + addr_tx index    │
│     T4:  DAO deposits/withdrawals                             │
│     T5:  Token transfers (UDT/NFT/Spore)                      │
│     T6:  Spore NFT data                                       │
│     T7:  Statistics + block-level aggregation                  │
│     T_ACT: Per-owner activity entries (see ACTIVITY_SYSTEM.md)│
│  6. Finalize: block headers + stats commit                    │
│  7. Update sync_status (LAST - crash recovery)                │
└──────────────────────────────────────────────────────────────┘
```

## Configuration

| Parameter             | Default | Description                                              |
| --------------------- | ------- | -------------------------------------------------------- |
| `pipeline_buffer`     | `16`    | Channel capacity between stages                          |
| `batch_size`          | `10000` | Blocks per batch                                         |
| `parallel_fetch_size` | `64`    | Concurrent block fetch work units in the pipeline        |
| `bulk_sync_threshold` | `72`    | Blocks behind tip to treat sync as bulk mode             |
| `ckb.workdir`         | -       | CKB node config directory; ckbadger derives RocksDB path |

### Relevant Config

```bash
[store]
domain_data_path = "data/domain"
append_only_data_path = "data/append-only"

[ckb]
rpc_url = "http://127.0.0.1:8114"
workdir = "/var/lib/ckb"
```

`pipeline_buffer`, `batch_size`, `parallel_fetch_size`, and
`bulk_sync_threshold` are configured via CLI flags in current builds.

### CLI Arguments

```bash
cargo run -p ckbadger-indexer -- \
  --pipeline-buffer 16 \
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

1. Set `deep_fork_detected` in sync status
2. Pause sync with 30s sleep loop
3. Require manual intervention

## Consistency Guarantees

### Data Consistency

The pipeline produces deterministic database state. All domain operations go through `write_parsed_batch()`:

- Cell insertion and consumption
- DAO deposit/withdrawal tracking
- Token transfers (UDT mint/transfer/burn)
- NFT transfers (Spore, MNFT, Dotbit)
- DOB transfers
- Script usage statistics
- All hourly/daily/epoch statistics

## Performance Characteristics

### Throughput

With default settings on typical hardware:

| Configuration        | Blocks/sec   | Bottleneck         |
| -------------------- | ------------ | ------------------ |
| Pipeline (buffer=8)  | ~280-320     | RocksDB writes     |
| Pipeline (buffer=16) | ~400-500     | RocksDB writes     |
| Pipeline (optimized) | ~5000-7000   | DB reads in Writer |
| Pipeline (preloaded) | ~15000-20000 | RocksDB commits    |

**Optimizations**:

1. **Preloaded cell consumption**: Writer uses `consume_cells_batch_preloaded()` with zero DB reads — cell info is passed from the Parser stage via `LiveCellInfo` maps, and same-batch cells are resolved from the in-memory `batch_cell_infos` map. This also **fixes the same-batch consumption bug** where cells created and consumed within the same WriteBatch would not be found by `multi_get_cf`.

2. **Single Parser DB read**: `get_full_cells_info_batch()` returns complete `LiveCellInfo` structs in one read, replacing two separate reads (`get_cells_info_batch` + `get_cells_code_hashes_batch`).

3. **4-way prefetch + split write threads**: Address balance and script info DB reads are prefetched in parallel with DAO/UDT reads via nested `rayon::join` (4-way). The write phase splits work across T1 (cells + consumption) and T2 (transactions + address deltas + script deltas + addr_tx index), with zero CF overlap. This hides the read latency in the prefetch phase and halves T1's write time.

4. **SST Bulk Ingest** (bulk-build): Batch flushes bypass the memtable/WAL path entirely via `SstFileWriter` + `IngestExternalFile`. Rows are sorted by (CF, key), written as one SST file per CF, and ingested directly into L0 with `move_files=true` (zero-copy rename). This eliminates memtable pressure and WAL overhead during bulk sync. Live-sync continues to use `WriteBatch` for atomic per-batch writes.

### Memory Usage

Pipeline mode uses more memory due to buffered batches:

```
Memory ≈ pipeline_buffer × batch_size × (block_size + parsed_data)
       ≈ 16 × 10000 × (~100KB per block)
       ≈ 16GB additional
```

Bulk-build mode adds in-memory state for the live-cell set (LiveCellOwner), intern tables, and
reducer-owned domain state. See the Bulk-Build Engine section for details.

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

## Implementation Notes

### Why Raw Blocks in ParsedBatch?

The parsed batch includes raw `BlockResponseWithCycles` because:

1. UDT parsing needs access to witness data (not in `TxData`)
2. Some script detection requires original transaction structure

### Cell Cache Strategy

Three-level lookup for consumed cell info:

1. **LRU Cache** (200k entries, full `CachedCellInfo`): Recent block cells with all fields (capacity, lock_script_hash, lock_code_hash, lock_args, type_script_hash, type_code_hash, data_size)
2. **DB Batch Query**: Cache misses fetched via `get_full_cells_info_batch()` — returns complete `LiveCellInfo` in one read
3. **Same-batch map**: Cells created in the current batch are available via `batch_cell_infos` HashMap built from `ParsedCell` data

### Script Usage Tracking

All code hash data is now available from `LiveCellInfo` — no separate DB reads needed:

1. Parser provides `input_cell_info: HashMap<..., LiveCellInfo>` with all fields including `lock_code_hash` and `type_code_hash`
2. Writer builds `batch_cell_infos: HashMap<..., LiveCellInfo>` for same-batch cells
3. Script usage changes look up consumed cells from either map directly

## Troubleshooting

### Sync Stuck / No Progress

1. Check logs for errors
2. Verify CKB node is synced and responsive
3. Check for `deep_fork_detected` in sync status
4. Try restarting indexer

### Data Inconsistency

1. Check `write_parsed_batch()` for correctness
2. Verify all insert/update calls match expected behavior
3. Run `ckbadger verify --depth sampling` to check data integrity

### High Memory Usage

1. Reduce `pipeline_buffer` (e.g., to 4-8 from default 16)
2. Reduce `batch_size`
3. Monitor for memory leaks in channel handling

## Bulk-Build Engine

Fresh-db bulk sync uses a dedicated build engine that treats RocksDB as a write-once artifact
rather than working memory. Per [docs/prompts/BULK_SYNC.md](./prompts/BULK_SYNC.md), all required
data must be computed inline on the canonical block path. See
[docs/superpowers/specs/2026-03-17-bulk-sync-build-engine-design.md](./superpowers/specs/2026-03-17-bulk-sync-build-engine-design.md)
for the full design rationale.

### Architecture

```
┌───────────────────────────────────────────────────────────────────────────┐
│ Batch N                                                                   │
│                                                                           │
│  1. Chain Reader ─── read blocks from CKB RocksDB                        │
│  2. Fact Extractor ─ parallel parse (rayon) → FactsArena                 │
│     └─ concurrent IdentityInterner (DashMap + Mutex<Vec>)                │
│  3. Sequencer ────── LiveCellOwner resolves inputs from memory           │
│  4. Owner Reducers ─ address (serial) │ script,token,dao,fiber,object    │
│     └─ address returns cell_dist      │ (parallel via rayon::join)       │
│  5. Materializer ─── Class A/C rows → SST ingest → RocksDB              │
│                                                                           │
│  Pipelining: fetch batch N+1 overlaps with build N                       │
│  SST overlap: SST ingest for batch N runs as background task during N+1  │
└───────────────────────────────────────────────────────────────────────────┘
```

**Key data structures**:

- **FactsArena**: per-batch fact graph with `BlockFacts`, `TxFacts`, `CellFacts`, interned identities
- **IdentityInterner**: `DashMap<Vec<u8>, u32>` for concurrent insert during parallel parsing;
  freezes to `FrozenIdentityView` (lock-free `Vec` snapshot) for the reduce phase
- **LiveCellOwner**: `FxHashMap<OutPointKey, LiveCellSlot>` — authoritative in-memory live-cell set;
  resolves all consumed inputs without DB reads. Protocol facts stored in a separate
  `FxHashMap<OutPointKey, CellProtocolFacts>` side-map (only ~1-2% of cells have protocol facts)
- **FxHashMap**: replaces `std::HashMap` in hot structures for 2-5x faster hashing on fixed-size keys

### Performance Optimizations

1. **FxHashMap**: non-cryptographic hash for `OutPointKey` (36B), lock/type hashes, and all reducer maps
2. **Protocol facts side-map**: removed `Option<CellProtocolFacts>` from every `LiveCellSlot`, saving ~24B per entry
3. **Parallel reducers**: address runs serially (produces cell distribution deltas), then script + token + dao + fiber + object run in parallel via nested `rayon::join`
4. **Inter-batch pipelining**: `tokio::spawn` fetches batch N+1 from CKB RocksDB while batch N is being built (~200ms fetch hidden behind ~3-4s build)
5. **SST ingest overlap**: after materializing batch N, SST file writes and ingestion are dispatched as `spawn_blocking` and run concurrently with batch N+1 compute. SST files bypass memtables entirely, so there is no flush latency.
6. **Parallel block parsing**: `rayon::par_iter` parses blocks within a batch, merges output ranges for global cell indices post-merge

### Bulk-Build Write Classes

- **Class A** (immutable event rows): streamed immediately as each batch completes — `cells`,
  `block_headers`, `tx_index`, `addr_txs`, `token_transfers`, `activities`, collection activity feeds
- **Class B** (final snapshot): held in reducer memory, written once after all batches —
  `live_cells`, `cell_by_*`, `addr_balance`, `tokens`, `token_holders*`, `dao_*`,
  `script_info`, object/identity/fiber state
- **Class C** (sealed aggregates): flushed once the time bucket/epoch closes — `stats_chain`,
  `stats_dao`, `stats_hodl`, `stats_script`, `stats_token`, `stats_spore`, `stats_object`
- **Class D** (bulk-disabled): `reorg_undo_log_by_block`, `pending_proposals` — not written during
  bulk sync; `sync_meta` holds only build metadata and final completion state

### Bulk-Build Stats Coverage

- `stats_dao`: daily DAO snapshots are materialized during bulk build; latest/top DAO summaries are
  refreshed after sync tip metadata is finalized.
- `stats_script`: daily per-code-hash deltas are written during bulk build and read directly by the
  script chart APIs.
- `stats_token`: transfer totals, hourly transfer buckets, and token daily deltas are written
  during bulk build and become the starting point for later live-sync accumulation.
- `stats_chain` / `stats_hodl` / object-related sealed stats are also written inline; bulk build
  does not rely on a post-sync backfill pass.

### Still Skipped In Bulk Build

- Reorg detection/rollback paths
- Partial-state recovery flows
- Live-sync-only metadata such as `pending_proposals`

### Bulk-Sync Completion Behavior

When bulk sync completes (transitions from `blocks_remaining > threshold` to `<= threshold`):

1. Indexer detects state transition via `was_bulk_sync_active` flag
2. Marks bulk sync completed in sync status/cache metadata
3. Finalizes the active bulk-sync perf artifact run under `workdir/perf/bulk-sync/<run_id>/`
4. Invalidates chart caches
5. Restores normal compaction options and triggers background compaction

### Implementation Details

- State tracking: `was_bulk_sync_active: AtomicBool` in Indexer struct
- Detection: `check_bulk_sync_completion()` called after each batch
- No automatic call to `BatchWriter::rebuild_all_statistics()` in current runtime path
- Fresh-db bulk sync writes perf artifacts directly from the indexer runtime under `workdir/perf/bulk-sync/`; failed runs keep their own directory and only completed runs refresh `workdir/perf/bulk-sync/latest/`
- `metadata.env` records both `run_id` and `build_version`, so artifact comparisons can separate one runtime execution from another binary build

### Module Structure

```
crates/indexer/src/sync/bulk_build/
  mod.rs           # Build loop, inter-batch pipelining, flush overlap
  facts.rs         # FactsArena — per-batch fact graph
  interner.rs      # IdentityInterner (DashMap) + FrozenIdentityView
  live_cells.rs    # LiveCellOwner — in-memory UTXO set + protocol facts side-map
  sequencer.rs     # Canonical tx-order sequencing + input resolution
  accounting.rs    # Fee/capacity accounting
  materialize.rs   # SST ingest assembly + flush_rows_to_stores()
  owners/
    mod.rs         # ReducerContext, parallel reducer dispatch
    address.rs     # AddressOwner — balances, cell counts, addr_stats
    dao.rs         # DaoOwner — deposit lifecycle, DAO indexes
    token.rs       # TokenOwner — UDT metadata, holders, transfers
    script.rs      # ScriptOwner — script usage, daily deltas
    object.rs      # ObjectOwner — spore/mNFT/object/identity/cluster state
    fiber.rs       # FiberOwner — fiber channel registry
```

## Crash Recovery

The indexer implements crash recovery to handle failures during batch writes. During live-sync, RocksDB WriteBatch provides atomicity within a single batch. During bulk-build, SST ingest provides per-CF atomicity. A crash between batches can leave the store in an inconsistent state.

### Write Ordering Strategy

**Sync status is written LAST** as the "commit marker". The write order is:

1. T1: Cells + consumption via preloaded lookup (no DB reads)
2. T2: Transactions + address balance deltas + script usage deltas + addr_tx index (using prefetched data, no DB reads)
3. T4: DAO deposits/withdrawals (using prefetched data)
4. T5: Token transfers, NFT data (using prefetched data)
5. Statistics updates
6. **Sync status (LAST)** - only after all other data succeeds

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

## Progress Tracking

The indexer uses two complementary log lines:

1. **Batch log** (per batch): `Wrote blocks X to Y (N remaining, 2.34s)`
   - Shows DB write duration for the batch
   - Useful for identifying slow batches

2. **Progress log** (every 10s): `Progress: 33.96% (6279999/18491045) - 3465.00 blocks/sec (EMA: 3200.00)`
   - Shows overall sync percentage and throughput
   - `blocks/sec`: 10-second sliding window (real-time, volatile)
   - `EMA`: Exponential Moving Average with α=0.1 (smoothed, stable)
   - ETA: `remaining_blocks / EMA` (simple calculation)

## Redis Sync Data

The indexer publishes sync data to Redis for API/WebSocket consumption:

| Key             | TTL | Contents                        |
| --------------- | --- | ------------------------------- |
| `sync:status`   | 60s | JSON: `SyncStatusData` struct   |
| `sync:progress` | 30s | JSON: `SyncProgressData` struct |
| `memory:stats`  | 30s | JSON: `MemoryStatsData` struct  |

**`sync:status`** - Core sync state (`crates/common/src/sync.rs`):

```rust
pub struct SyncStatusData {
    pub tip_block_number: i64,
    pub tip_block_hash: String,
    pub total_transactions: i64,
    pub total_cells: i64,
    pub total_live_cells: i64,
    pub total_addresses: i64,
    pub last_synced_at: i64,
    pub sync_ema_rate: Option<f64>,
    pub sync_started_at: Option<i64>,
    pub bulk_sync_completed_at: Option<i64>,
    // ... other fields
}
```

**`sync:progress`** - Real-time progress with ETA:

```rust
pub struct SyncProgressData {
    pub current_block: u64,
    pub target_block: u64,
    pub blocks_per_second: f64,
    pub ema_blocks_per_second: f64,
    pub eta_seconds: Option<f64>,
    pub eta_formatted: String,
    pub progress_percentage: f64,
    pub updated_at: i64,
}
```

**`memory:stats`** - RocksDB and cell store memory usage:

```rust
pub struct MemoryStatsData {
    pub live_cells_count: u64,           // Live cells in RocksDB
    pub consumed_cells_count: u64,       // Consumed cells cache count
    pub consumed_cells_bytes: u64,       // Consumed cells cache size
    pub rocksdb_memtable_bytes: u64,     // RocksDB memtable usage
    pub rocksdb_block_cache_bytes: u64,  // RocksDB block cache usage
    pub rocksdb_table_readers_bytes: u64,// RocksDB table readers
    pub rocksdb_total_bytes: u64,        // Total RocksDB memory
    pub block_headers_count: u64,        // Cached block headers
    pub bulk_sync_cell_cache_enabled: bool, // Bulk sync cache flag
    pub bulk_sync_mode: bool,            // Currently in bulk sync
    pub updated_at: i64,                 // Unix timestamp
}
```

### Data Flow

1. Indexer updates `sync:status` after each batch write
2. Indexer updates `sync:progress` every 10 seconds with ETA
3. Indexer updates `memory:stats` every 10 seconds with RocksDB memory usage
4. API reads `sync:status` for totals (blocks, transactions, cells)
5. API reads `sync:progress` for real-time progress display
6. Operational tools can read `memory:stats` for memory monitoring
7. WebSocket broadcaster uses both for `new_block` messages

### Fallback (when Redis unavailable)

| Data                 | Fallback Source                        |
| -------------------- | -------------------------------------- |
| `tip_block_number`   | `store.get_sync_tip()` from RocksDB    |
| `total_transactions` | `store.get_sync_status()` from RocksDB |
| `total_live_cells`   | `store.get_sync_status()` from RocksDB |
| `sync_ema_rate`      | None (ETA not displayed)               |

**Requires**: `redis-cache` feature enabled on both indexer and API, plus `REDIS_URL` environment variable.

---

_Last updated: 2026-03-21_
