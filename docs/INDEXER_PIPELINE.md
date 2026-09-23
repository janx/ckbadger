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
4. **Use mode-specific failure handling** - Near-tip batches can drain and retry; fresh-store
   bulk build fails fast and must restart from empty chain stores

## Startup Sync Decision

The RocksDB memtable representation is fixed when a store is opened and cannot be changed
afterwards, so the sync path is decided **once**, before the chain stores are opened for real.
`open_chain_stores_for_startup()` (`crates/indexer/src/entry.rs`) owns that sequence:

1. **Probe open.** The domain store opens with `vector_memtable = false` (SkipList) — the
   representation that is correct for every outcome except a fresh bulk build. The probe handle
   only reads: `fail_fast_if_bulk_build_session_incomplete()` rejects a partial bulk artifact, and
   `get_sync_tip_block()` reads the writer's resume tip from `block_headers` — the authoritative
   source `Repository::get_sync_tip` uses, never `sync_status`, which is repaired later in startup.
2. **Decide.** `decide_startup_sync(chain_tip, sync_tip_block, sync_tip_hash, bulk_sync_threshold)`
   (`crates/indexer/src/sync/indexer.rs`) returns one
   `StartupSyncDecision { path, fresh, blocks_behind, memtable }`.
3. **Open for the decision.** A `Vector` decision drops the probe handle (it is owned here and
   never shared, so the drop closes the DB) and reopens the domain store with VectorRep; every
   other decision keeps the probe handle as the final handle. The append-only store is then opened
   once, with the same decision. The per-process block cache and WriteBufferManager are a
   `OnceLock` (`SHARED_BUDGET`) and `vector_memtable` is not an input to their sizing, so probing
   and reopening provisions exactly one memory budget, not two.
4. **Then write.** `establish_db_network_identity()` — the first domain-store mutation of startup —
   runs on the final handle, after the decision.

`Indexer::new` receives the decision and `Indexer::run` reuses it instead of re-sampling the node
tip. A second sample could name a mode the stores were not opened for, and the memtable can no
longer be changed to match.

### Decision table

| Store state                                   | Lag (`chain_tip - sync_tip`) | Path               | Memtable | Rationale                                                                            |
| --------------------------------------------- | ---------------------------- | ------------------ | -------- | ------------------------------------------------------------------------------------ |
| Fresh (`sync_tip_block == 0` and no tip hash) | `> bulk_sync_threshold`      | `BulkBuild`        | Vector   | Append-only build that never reads back what it writes; sorting is deferred to flush |
| Fresh                                         | `<= bulk_sync_threshold`     | `Pipeline`         | SkipList | Too close to the tip for a build; the pipeline reads back what it writes             |
| Non-fresh                                     | any lag, at any size         | `Pipeline`         | SkipList | "Live catch-up": bulk is fresh-store only (`BULK_SYNC.md` rule 10)                   |
| Non-fresh, node tip **below** the store tip   | negative, reported as-is     | `Pipeline`         | SkipList | Node restarted from a snapshot or reorged; the lag is never clamped to 0             |
| Incomplete bulk-build session marker          | —                            | startup fails fast | —        | A partial bulk artifact is not a supported startup state                             |

The threshold comparison is strict (`blocks_behind > bulk_sync_threshold`), so a fresh store
exactly at the threshold takes the pipeline. The lag is `i64` throughout and a node reporting a tip
below the store's is recorded as a negative number, never clamped to 0 — clamping would make a node
that has fallen behind the store look like an ordinary near-tip startup. A _fresh_ store cannot
produce a negative lag (its resume tip is 0), so that combination is an error, not a path.

### Startup log line

```
INFO Startup sync decision network=mainnet build_version=0.7.4@d00ff2eb sync_path=Pipeline
     fresh=false blocks_behind=76827 memtable=SkipList chain_tip=20530576 bulk_sync_threshold=1000
```

`sync_path` is `BulkBuild` or `Pipeline`, `memtable` is `Vector` or `SkipList`. When the decision
is `Vector`, a second line (`Reopening ckbadger domain store for the decided sync path`) records
the reopen.

Until 2026-09-23 the indexer set `vector_memtable = true` unconditionally, so live sync read its
own unflushed memtable through VectorRep — an unsorted representation whose `Get` sorts the whole
memtable under a write lock. See POSTMORTEM `IDX-007`.

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

3. **Per-block entity daily deltas**: script/token/cluster/spore/object daily contributions
   accumulate into `EntityDailyChanges<K>` keyed by block, in ascending block order — not flattened
   across the batch. The writer needs that block identity to record what a key was worth at the end
   of block N (see [Entity Statistics Write Path](#entity-statistics-write-path)); a block going
   backwards is a fail-fast upstream invariant violation, and `fold_total()` is the single place
   batch-scoped downstream aggregates fold the same collection.
   Object collection identity on both the creation and consumption sides comes from one classifier,
   `classify_object_collection_id` (via `consumed_object_collection_id`), which is also what bulk
   build uses. The consume side used to carry its own predicate list that omitted `.bit Cell`, so a
   consumed `.bit Cell` never subtracted the capacity its creation had added.

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

#### Commit Window Timing

The commit window was one wide `write_commit_ms` measurement that covered a great deal of
_reading_: the address-balance `multi_get`, both tracker preparations, the full script rollup and
the append-only existence probe all ran inside it. During the 2026-09-22 catch-up that made
"commit" report 66-383 s for a 5,000-block batch whose two RocksDB writes were a small part of it.

It is now split into five non-overlapping fields, all milliseconds:

| Field                          | Covers                                                                                                                                        |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `commit_prepare_ms`            | Merging the core and stats batches into `data_batch`, the address-balance prefetch, and the HODL-wave + cell-distribution tracker preparation |
| `script_rollup_ms`             | `materialize_script_versions_and_families` staging the script version/family rollup                                                           |
| `append_only_commit_synced_ms` | `cells_batch.commit_synced()` — the append-only payload commit **and** its fsync                                                              |
| `domain_commit_ms`             | `data_batch.commit()` — the single atomic domain write that advances the sync tip                                                             |
| `commit_phase_total_ms`        | The whole window; the four parts above sum to at most it                                                                                      |

`write_commit_ms` survives in the log under its old name so old and new logs compare directly, and
it is the _same stored value_ as `commit_phase_total_ms` — `BatchWriteMetrics::commit_ms()` is a
method returning that field, not a second accumulator that could drift from the parts.

`precompute_ms` is now a real measurement of the writer's own pre-batch phase. It previously
carried `prefetch_ms`, which the pipeline writer never sets, so every batch logged 0.

`Batch write breakdown` (INFO, one line per batch) carries `precompute_ms`, `write_ms`,
`write_commit_ms`, `commit_phase_total_ms`, `commit_prepare_ms`, `script_rollup_ms`,
`append_only_commit_synced_ms`, `domain_commit_ms`, `tracker_state_bytes`, `finalize_ms`, `txs`,
`cells`, `inputs`. `Batch perf` (the periodic aggregate in `sync/diagnostics.rs`) reports the same
split, with `db_commit_ms` as the wide window and the four parts beside it.

`tracker_state_bytes` is the serialized size of the two tracker states that `sync_meta` rewrites in
full every batch. It exists because the cost had to be measured before anything was changed: at
production scale (2,495 date entries per tracker) it is ~310 KB and ~1.7 ms — 0.15% of the smallest
observed per-block commit window — so the tracker persistence was left exactly as it is, and the
field stays in the log so the number can be re-checked against production.

**Writer-phase heartbeat.** Each of the five commit parts, the finalize step, and every block
inside the staging body bump a monotonic counter (`PerfStats::mark_writer_phase`). A single
`db.write()` cannot be marked from inside — RocksDB gives no progress callback — so a genuinely
wedged commit still trips the watchdog, which is exactly what it is for. See
[Progress Heartbeat and Stall Detection](#progress-heartbeat-and-stall-detection).

#### Entity Statistics Write Path

The eight per-entity daily/hourly stats families are rolled back **only** by the undo log
(`docs/prompts/REORG_HANDLING.md`), and this is the path that feeds it.

One `EntityStatsOverlay` (`crates/indexer/src/db/writer/entity_stats.rs`) exists per committed
batch, shared by the five daily writers and the four hourly write points
(`udt.rs`, `spore.rs`, `mnft.rs`, `dotbit.rs`):

- **One read per key.** The overlay caches the key's current value — warmed by one `multi_get`
  prefetch, then read-through — so no writer re-reads RocksDB inside the loop.
- **First touch per block records the pre-image.** The first mutation of a key inside block N
  writes one `UndoLogEntry::KeyMutation` under `UndoSeqScope::EntityStats` holding the value at
  the **end of block N-1** — not the value RocksDB held when the batch opened, which is what
  makes a rollback into the middle of a multi-thousand-block batch exact. Later mutations of the
  same key in the same block add no entry. Blocks must arrive in ascending order per key; going
  backwards is a hard error rather than a pre-image from the future.
- **One write per key.** `stage_final` writes each dirty key once with its final value
  (`put_stats`, or `delete_stats` when a daily row nets to zero).
- **Bulk records nothing.** In bulk mode the overlay skips the undo entry entirely.

`stage_final` is called **after the last hourly write point and immediately before
`domain_analytics_batch` is merged into `data_batch`**, so the stats rows, their undo pre-images
and the sync tip land in one atomic domain commit.

Two maintenance steps run in the same commit window, after the block writes and before the
commit, and only outside bulk mode:

- **Coverage floor.** `stage_entity_stats_undo_retention` advances
  `entity_stats_undo_contract.coverage_floor_block` to `last_block - 1000` and stages the
  `EntityStats` undo deletions it leaves behind into the same batch.
- **Hourly retention.** A 10-minute task no longer deletes anything; it only sets a
  `hourly_retention_requested` flag (and skips while more than `bulk_sync_threshold` blocks
  remain). The writer swaps that flag and, if set, runs one bounded step per family
  (≤ 5,000 keys), staging the deletions and the advanced `hourly_retention_state:<family>` row
  into the same batch as the blocks — so a deletion can never race an undo replay restoring the
  same key, and the persisted boundary always matches what was actually deleted. The cutoff is
  `min(now − 48 h, hour_of(header(tip − 1000)))`, clamped up by the already-executed cutoff:
  the block-derived bound keeps retention from deleting a bucket that is still inside the undo
  window even when the chain has stalled.

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

| Parameter               | Default | Description                                                           |
| ----------------------- | ------- | --------------------------------------------------------------------- |
| `bulk_sync_threshold`   | `1000`  | Blocks behind tip where bulk-to-near-tip handoff occurs               |
| `poll_interval_ms`      | `1000`  | Live sync new-block poll interval (ms)                                |
| `bulk_memory_budget_gb` | auto    | Optional whole-indexer bulk-sync hard limit                           |
| `ckb.workdir`           | -       | Required CKB node config directory; ckbadger derives its RocksDB path |

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

[indexer]
bulk_sync_threshold = 1000
poll_interval_ms = 1000
# bulk_memory_budget_gb = 32
```

These values are configured in each network's `config.toml`. The top-level
`ckbadger.toml` contains only the `[[network]]` list and shared frontend/log settings.

### Starting One Indexer

```bash
ckbadger -C <orchestrator-root>/<network> run --only indexer
```

`ckbadger-indexer` is a library, not a standalone binary. Pipeline capacity and adaptive batch
controls are internal implementation details rather than public CLI flags.

### Multi-Network Scheduling

At an orchestrator root, APIs, enabled crawlers, and the shared frontend start immediately.
Indexers start in `[[network]]` order. The supervisor waits until the active indexer has crossed
the bulk threshold before admitting the next one, so only one co-resident network performs the
memory-intensive fresh-store bulk build at a time. Indexers already in the near-tip pipeline keep
running independently.

Absent an explicit `[store].memory_budget_gb`, detected host RAM is divided by the number of
networks listed by the governing orchestrator. `[indexer].bulk_memory_budget_gb` overrides the
whole-process guard for that network and must be greater than zero when set.

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
activity buckets, actual-byte queue backpressure, a whole-process memory budget (`VmRSS + VmSwap`
on Linux, process physical footprint on macOS), and byte-bounded finalization. See the Bulk-Build
Engine section for details.

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

### Progress Heartbeat and Stall Detection

A 3-second background loop writes the runtime heartbeat, sync progress, and — on the ticks that
sampled them — memory stats as **one** `StoreBatch` (`CkbadgerStore::commit_heartbeat_tick`),
where it previously issued three separate `put_cf` calls per tick. A run-identity mismatch belongs
to the runtime-status part alone: sync progress and memory stats are still staged, so one bad run
id cannot make the indexer look dead to every reader.

The sweep behind `get_memory_stats()` reads ~8 RocksDB properties across all 60 CFs of **both**
chain stores. Live sync resamples it every 30 s (`MEMORY_STATS_LIVE_SAMPLE_INTERVAL`), while
`SYNC_PROGRESS` — what the TUI and API read — keeps the 3-second cadence. Bulk sync still samples
every tick: its perf heartbeat and memory-pressure log are the point of a build. The `RocksDB
stats` log line only appears on ticks that sampled.

`Sync progress stalled` now requires **two** frozen signals across the whole 60-second window: the
committed tip has not moved **and** the writer-phase counter has not moved. A live catch-up batch
of 5,000 blocks holds one `write_parsed_batch` call for minutes and cannot advance its committed
tip until it commits; on 2026-09-22 the tip-only rule produced 74 (mainnet) / 49 (testnet) false
stall warnings in a single catch-up. The warning line carries `writer_phase_idle_seconds`.

The TUI derives liveness the same way: `stale_age_secs` reads `RuntimeDiagData.heartbeat_age_secs`
— the runtime heartbeat written on every 3-second tick — not the `updated_at` of the memory
sample, which in live mode lags by up to 30 s and says nothing about whether the writer is alive.

### Live-Sync Health CSV

`crates/indexer/src/health_monitor.rs` samples once a minute and appends one averaged row per hour
to `live-sync-health.csv` in the parent directory of `bulk_sync_perf_output_root` (e.g.
`workdir/perf/live-sync-health.csv`).

`csv_header()` is the single definition of the column set, and its first column names the schema
version:

```csv
schema=3,timestamp,current_block,target_block,db_stage_write_ms_avg,db_commit_ms_avg,block_cache_mb_avg,l0_files_avg,l0_max_peak,sst_size_gb_last,chunks_per_hour,slow_chunks_per_hour,timeouts_per_hour,keys_per_hour,avg_us_per_chunk,flush_pending_peak,active_memtable_mb_avg,wbm_usage_mb_avg,wbm_budget_mb_last,flush_observed_in_window,precompute_ms_avg,build_ms_avg,finalize_ms_avg,commit_prepare_ms_avg,script_rollup_ms_avg,append_only_commit_synced_ms_avg,domain_commit_ms_avg,sst_files_last,manifest_mb_last
```

- `db_commit_ms_avg` **is** the wide commit window; `commit_prepare_ms_avg`,
  `script_rollup_ms_avg`, `append_only_commit_synced_ms_avg` and `domain_commit_ms_avg` are its
  non-overlapping parts. There is deliberately no second column for the total.
- `sst_files_last` and `manifest_mb_last` are the standing consequences of flush frequency, and
  the MANIFEST is what an API secondary replays on open.
- There is deliberately **no flush-round column**. RocksDB exposes no cumulative flush counter, so
  flush rounds are counted from `flush_started` in the RocksDB LOG; `flush_pending_peak` and
  `flush_observed_in_window` stay minute-resolution activity signals, not counts.

**Rotation.** Rows of two different column sets must never share a file. If the existing
`live-sync-health.csv` begins with a different header, this run writes
`live-sync-health.schema3.csv` instead of appending. If that schema-suffixed file also exists with
a different header, the monitor fails rather than mixing: the column set changed without a
`CSV_SCHEMA_VERSION` bump.

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

A `Sync progress stalled` warning means both the committed tip and the writer-phase heartbeat have
been still for 60 s. A long catch-up batch does not trigger it: the tip cannot move mid-batch, but
the writer phase keeps advancing per block. See
[Progress Heartbeat and Stall Detection](#progress-heartbeat-and-stall-detection).

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
data must be computed inline on the canonical block path — that document is the single source of
truth for bulk-sync behavior, constraints, and failure handling.

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
  written during bulk sync. `sync_meta` still owns network identity, the genesis baseline,
  build-session state, progress/runtime records, and final completion metadata; it does not become
  a fallback store for skipped domain aggregates.

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

- **BulkMemoryGuard**: checks process `VmRSS + VmSwap` on Linux or process physical footprint on
  macOS before each batch, after each build, and before finalization. It reduces the next
  input-byte cap to preserve transient build headroom and fails with detailed process/owner
  diagnostics if the configured limit is exceeded.
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
    memory_guard.rs  # Portable whole-process budget and transient batch headroom
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

_Last updated: 2026-09-23_
