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
| `bulk_sync_threshold` | `1000`  | Blocks behind tip to treat sync as bulk mode             |
| `poll_interval_ms`    | `1000`  | Live sync new-block poll interval (ms)                   |
| `ckb.workdir`         | -       | CKB node config directory; ckbadger derives RocksDB path |

Pipeline channel capacity (16) and batch span are hardcoded constants.
Live batch span is density-adaptive: `40,000 txs / tx_per_block_ema`, clamped to [1, 5000] blocks.

### Relevant Config

```bash
[store]
domain_data_path = "data/domain"
append_only_data_path = "data/append-only"

[ckb]
rpc_url = "http://127.0.0.1:8114"
workdir = "/var/lib/ckb"
```

`bulk_sync_threshold` and `poll_interval_ms` are configured in `ckbadger.toml`.

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

4. **RocksDB WriteBatch**: All writes within a batch are grouped into atomic WriteBatch operations for maximum throughput.

### Memory Usage

Pipeline mode uses more memory due to buffered batches:

Pipeline memory is bounded by channel capacity (16) × batch span × block size.
Live batch span adapts to chain density (~20-5000 blocks per batch).

Bulk-build mode adds in-memory state for the live-cell set (LiveCellOwner), intern tables, and
reducer-owned domain state. Its growth is controlled by compact fixed-size state, MTP-sealed
activity buckets, actual-byte queue backpressure, a whole-process `VmRSS + VmSwap` budget, and
byte-bounded finalization. See the Bulk-Build Engine section for details.

### Channel Backpressure

When writer is slower than fetcher+parser:

- Channels fill to capacity (16 batches)
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

1. Pipeline channel capacity and batch span are auto-managed
2. Monitor for memory leaks in channel handling

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
│  4. 3-way parallel tree (nested rayon::join):                           │
│     LEFT:   history materialization → activity_stats accumulation       │
│     MIDDLE: chain_stats (reads only immutable arena + resolved)        │
│     RIGHT:  hodl → rayon::join(address+cell_dist, 5 reducers)          │
│  5. Materializer ─── Class A/C rows → StoreBatch → RocksDB              │
│                                                                           │
│  Pipelining: fetch batch N+1 overlaps with build N                       │
│  Flush overlap: RocksDB flush N runs as background task during build N+1 │
└───────────────────────────────────────────────────────────────────────────┘
```

**Key data structures**:

- **FactsArena**: per-batch fact graph with `BlockFacts`, `TxFacts`, `CellFacts`, interned identities
- **IdentityInterner**: `DashMap<Arc<[u8]>, u32>` for concurrent insert during parallel parsing;
  the lookup map and ID table share each byte payload, and it freezes to an O(1)
  `FrozenIdentityView` for the reduce phase
- **LiveCellOwner**: `FxHashMap<OutPointKey, LiveCellSlot>` — authoritative in-memory live-cell set;
  resolves all consumed inputs without DB reads. The outpoint exists only as the map key, while
  rare data hash, UDT, and DAO fields live in a sparse `FxHashMap<OutPointKey, LiveCellExtras>`
  side-map
- **AddressOwner**: fixed-size `[u8; 32]` keys and transaction hashes with in-place updates; the
  stable `AddressBalance` representation is created only while final rows are streamed
- **FxHashMap**: replaces `std::HashMap` in hot structures for 2-5x faster hashing on fixed-size keys

### Performance Optimizations

1. **FxHashMap**: non-cryptographic hash for `OutPointKey` (36B), lock/type hashes, and all reducer maps
2. **Sparse live-cell extras**: outpoints are not duplicated in values, and rare protocol fields
   are removed from every `LiveCellSlot` into `LiveCellExtras`
3. **3-way parallel build tree**: history materialization + activity_stats (LEFT), chain_stats (MIDDLE), and hodl + owner reducers (RIGHT) run concurrently via nested `rayon::join`; within RIGHT, address + cell_dist run in parallel with 5 independent reducers (script, token, dao, fiber, object)
4. **Inter-batch pipelining**: prefetch worker reads batch N+1 from CKB RocksDB while batch N is being built; fetch uses `std::thread::scope` (not rayon) so blocking RocksDB reads don't starve CPU-bound build work
5. **RocksDB flush overlap**: materialized rows are sent to a flush channel; a dedicated worker
   commits them to RocksDB concurrently with the next batch's build. Queue permits are held for
   the actual retained row-vector bytes until commit completes
6. **Parallel block parsing**: `rayon::par_iter` parses blocks within a batch, merges output ranges for global cell indices post-merge
7. **Bottleneck-driven resource control**: a single `BottleneckController` measures per-batch timing (fetch wait, build CPU, flush wait) and dynamically adjusts `target_cells`, `fetch_threads`, and `bg_jobs`. Batch sizing uses a build-time band [2s, 5s]: below band → grow, above band → shrink, in-band with build > IO → grow (IO headroom), in-band with IO ≥ build → hold (physical limit). Supply cap at 4× actual cells prevents divergence when supply-limited. Drain uses cell count as primary budget with RAM-derived bytes as safety cap
8. **Bounded finalization**: live-cell indexes and owner rows are emitted sequentially through
   32 MiB domain-store batches, so finalization does not duplicate the entire in-memory snapshot
9. **Clean process handoff**: after durable bulk finalization, the indexer exits successfully and
   the supervisor immediately starts a fresh process for the near-tip pipeline, allowing the OS
   to reclaim allocator arenas and reducer state

### Bulk-Build Write Classes

- **Class A** (immutable event rows): streamed immediately as each batch completes — `cells`,
  `block_headers`, `tx_index`, `addr_txs`, `token_transfers`, `activities`, collection activity feeds
- **Class B** (final snapshot): held in reducer memory, written once after all batches —
  `live_cells`, `cell_by_*`, `addr_balance`, `tokens`, `token_holders*`, `dao_*`,
  `script_info`, object/identity/fiber state
- **Class C** (sealed aggregates): flushed once the time bucket/epoch closes — `stats_chain`,
  `stats_dao`, `stats_hodl`, `stats_script`, `stats_token`, `stats_spore`, `stats_mnft`
- **Class D** (bulk-disabled): `reorg_undo_log_by_block`, `pending_proposals`, `dob_decoded` — not
  written during bulk sync; `sync_meta` holds only build metadata and final completion state

### Bulk-Build Stats Coverage

- `stats_dao`: daily DAO snapshots are materialized during bulk build; latest/top DAO summaries are
  refreshed after sync tip metadata is finalized.
- `stats_script`: daily per-code-hash deltas are written during bulk build and read directly by the
  script chart APIs.
- `stats_token`: transfer totals, hourly transfer buckets, and token daily deltas are written
  during bulk build and become the starting point for later live-sync accumulation.
- `stats_chain` / `stats_hodl` / object-related sealed stats are also written inline; bulk build
  does not rely on a post-sync backfill pass.
- Activity hourly/daily buckets use the CKB median-time-past watermark (37 headers including the
  current header, upper median). Only buckets whose exact UTC+8 end is at or below that watermark
  are emitted; later actions targeting a sealed bucket are an invariant violation.

### Still Skipped In Bulk Build

- Reorg detection/rollback paths
- Partial-state recovery flows
- Live-sync-only metadata such as `pending_proposals`
- DOB decoding (`dob_decoded` CF) — populated by background worker after sync catches up to tip

### DOB Background Worker

After sync catches up to the chain tip (bulk sync completes), the indexer spawns a background DOB
decode worker that processes Spore NFTs with DOB0/DOB1 content types:

1. Worker scans `spore_data` CF for undecoded DOB spores (those with no `dob_decoded` entry yet)
2. Fetches decoder binaries from CKB RPC (cached to filesystem via `dob-decoder` crate)
3. Executes decoders in CKB-VM sandbox to extract DNA/trait data
4. Writes a `DecodeOutcome` to `dob_decoded` CF (domain store, Class D — bulk-disabled):
   - success → `Decoded(DobDecodedEntry)`
   - deterministic failure (bad/dangling on-chain data, or a decoder that rejects the DNA) → `Failed(DobDecodeFailure)`, recorded once so the spore is not re-attempted (a `failed_recorded` count is added to the end-of-run summary)
   - transient failure (RPC/node fetch) → nothing written; the spore stays undecoded and is retried next run
5. API reads the outcome from `dob_decoded` for spore detail pages (`decoded` / `failed` / `pending` status)

Failures are classified via a typed `DobDecodeError` (`crates/indexer/src/sync/dob_decode_error.rs`); see `docs/OBJECT_SYSTEM.md` for the failure taxonomy. The decoder crate (`crates/dob-decoder/`) handles CKB-VM execution, binary caching, and RPC fetching.

### Bulk-Build Performance Infrastructure

- **BottleneckController**: unified resource controller with two independent dimensions.
  Located in `crates/indexer/src/sync/bottleneck.rs`.

  **Dimension 1 — Batch sizing** (build-time band [2s, 5s]):
  - Primary objective: keep `build_ema` within [BUILD_TIME_MIN=2s, BUILD_TIME_MAX=5s]
  - Below band → grow (batch too small regardless of IO)
  - Above band → shrink (build genuinely too large)
  - In-band, build > IO → grow (IO has headroom for larger batches)
  - In-band, IO ≥ build → hold (physical IO limit reached)
  - IO wait (recv + flush) is excluded from the band check because shrinking batch size cannot reduce IO-bound time
  - Supply cap: `target_cells` capped at 4× actual delivered cells to prevent runaway
    when prefetch rate is the bottleneck
  - `drain_by_cells(target_cells, max_batch_bytes)`: cell count is primary budget, RAM-derived bytes is safety cap
  - Prefetch fill estimate uses `cell_density()` (actual cells/byte from buffer) for accurate byte budget

  **Dimension 2 — I/O resources** (waste classification):

  | Knob            | Range      | Fetch-bound | Build-bound      | Flush-bound |
  | --------------- | ---------- | ----------- | ---------------- | ----------- |
  | `fetch_threads` | [2, cores] | +25%        | hold             | -25%        |
  | `bg_jobs`       | [N/4, N]   | -1          | -1 (if waste<5%) | +1          |

  Proactive L0 compensation: +1 bg_jobs when L0 EMA > 40 without Flush classification.
  Channel depth (prefetch + flush) is derived from system RAM (16GB→2, 32GB→4, 64GB+→8, max 8).
  Depth controls scheduling only: prefetched chunks are split by actual Molecule block bytes, and
  the flush queue also reserves permits for actual retained row-vector bytes.

- **BackgroundSampler**: periodic background thread that samples RocksDB stats and system metrics
  (via cross-platform POSIX APIs) on a configurable interval, decoupling stat collection from the
  hot batch path. Located in `crates/indexer/src/sync/bulk_build/sampler.rs`.
- **PrefetchChannelHandle**: bounded channel for inter-batch block prefetching. Depth and
  concurrency are controlled by the bottleneck controller, while fetched blocks are split into
  messages by their actual encoded bytes. Fetch uses `std::thread::scope` (temporary threads, not
  rayon) to avoid starving CPU-bound build work.
  Located in `crates/indexer/src/sync/bulk_build/prefetch.rs`.

- **BulkMemoryGuard**: checks process `VmRSS + VmSwap` before each batch, after each build, and
  before finalization. It reduces the next input-byte cap to preserve transient build headroom and
  fails with detailed process/owner diagnostics if the configured limit is exceeded.
  `[indexer].bulk_memory_budget_gb` is optional; without it, the store's per-network RAM share is
  used.

### Bulk-Sync Completion Behavior

When bulk sync completes (transitions from `blocks_remaining > threshold` to `<= threshold`):

1. The bulk engine drains history writes and streams all sealed/final snapshot rows to their
   owning stores
2. It flushes memtables, persists sync totals, clears the bulk session marker, and marks bulk sync
   completed in sync status/cache metadata
3. It finalizes the active bulk-sync perf artifact under `workdir/perf/bulk-sync/<run_id>/`
4. The indexer exits with success; the supervisor treats this as a planned handoff and immediately
   starts a new indexer process without crash backoff
5. The fresh process sees a non-fresh store, selects the normal near-tip pipeline, restores normal
   compaction behavior, invalidates caches as required, and starts live-sync background work

### Implementation Details

- Handoff state is persisted before the successful process exit; the next process cannot re-enter
  the fresh-store-only bulk route
- Successful indexer exit is a planned supervisor handoff; unsuccessful exits retain normal crash
  backoff behavior
- No automatic call to `BatchWriter::rebuild_all_statistics()` in current runtime path
- Fresh-db bulk sync writes perf artifacts directly from the indexer runtime under `workdir/perf/bulk-sync/`; failed runs keep their own directory and only completed runs refresh `workdir/perf/bulk-sync/latest/`
- `metadata.env` records both `run_id` and `build_version`, so artifact comparisons can separate one runtime execution from another binary build

### Module Structure

```
crates/indexer/src/sync/
  bottleneck.rs    # BottleneckController — unified adaptive resource control
  bulk_build/
    mod.rs           # Build loop, 3-way parallel tree, inter-batch pipelining, flush overlap
    binary_facts.rs  # Binary-format fact serialization for prefetch channel
    facts.rs         # FactsArena — per-batch fact graph
    interner.rs      # IdentityInterner (DashMap) + FrozenIdentityView
    live_cells.rs    # LiveCellOwner — compact UTXO set + sparse extras side-map
    memory_guard.rs  # Whole-process RSS + swap budget and transient batch headroom
    sequencer.rs     # Canonical tx-order sequencing + input resolution
    accounting.rs    # Fee/capacity accounting
    materialize.rs   # Byte-bounded domain finalization + dual-store history writes
    sampler.rs       # BackgroundSampler — periodic RocksDB + system stats sampling
    prefetch.rs      # PrefetchChannelHandle — bounded block prefetching
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

The indexer implements crash recovery to handle failures during batch writes. RocksDB WriteBatch provides atomicity within a single batch, but a crash between batches can leave the store in an inconsistent state.

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

## Sync Data Storage

Sync progress and status are stored directly in RocksDB (no external dependencies):

| Data          | RocksDB Access              | Contents                         |
| ------------- | --------------------------- | -------------------------------- |
| Sync tip      | `store.get_sync_tip()`      | Current synced block number/hash |
| Sync status   | `store.get_sync_status()`   | Totals: blocks, txs, cells       |
| Sync progress | `store.get_sync_progress()` | ETA, throughput, percentage      |
| Memory stats  | `store.get_memory_stats()`  | RocksDB memory usage             |

### Data Flow

1. Indexer writes sync status/progress to RocksDB after each batch
2. API reads from RocksDB secondary (read-only) for totals and progress
3. WebSocket broadcaster reads sync data for `new_block` messages
4. TUI reads progress and memory stats for monitoring display

---

_Last updated: 2026-03-23_
