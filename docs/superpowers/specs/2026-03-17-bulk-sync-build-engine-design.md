# Bulk Sync Build Engine Design

Date: 2026-03-17
Status: Draft approved in chat, written for review

## Goal

Redesign fresh bulk sync around a different invariant:

- CKB chain data is read once
- each required derived fact is computed once
- intermediate state stays in memory
- generated RocksDB rows are written once
- ckbadger RocksDB is treated as the final artifact, not the working-memory substrate

This design only targets fresh bulk rebuild from genesis.

## Starting Point

The mandatory worldview and bulk-sync rules already point in this direction:

- `docs/prompts/WORLD_VIEW.md`
- `docs/prompts/BULK_SYNC.md`

The current pipeline already moved part of the hot path away from DB lookups, but bulk sync still
uses RocksDB as working memory in both parser and writer stages:

- parser resolves missing inputs with `get_full_cells_info_batch`
- parser still reads token/object/spore metadata from RocksDB during precompute
- writer prefetches DAO deposits, address balances, script info, UDT input info, and object IDs
- many domain CFs are repeatedly updated/deleted during sync even though fresh rebuild only needs
  the final correct state

That means bulk sync is still paying for read-modify-write behavior that is only necessary when the
DB itself is the authoritative mutable state.

## Hard Constraints

- Bulk sync is fresh DB only. Resume-from-partial is unsupported.
- Mid-run API queryability is not required.
- Crash recovery is not required beyond "delete DB and re-sync".
- Temporary stage files / sorted runs are not allowed.
- All calculations remain exact. No estimation/sampling/interpolation.
- `CF_CELLS` remains append-only and is never updated/deleted.
- If available memory is insufficient for this mode, fail fast. Do not fall back to DB hot-path
  reads that reintroduce a second calculation path.

## Non-Goals

- This design does not change live-sync or reorg handling.
- This design does not introduce backward-compatible partial-build repair flows.
- This design does not try to preserve the current per-batch crash-consistency semantics inside
  bulk sync, because bulk sync no longer exposes partial state as a supported artifact.

## Core Model

Bulk sync becomes a build engine with three kinds of state:

1. Chain facts read from CKB RocksDB
2. In-memory canonical build state owned by reducers
3. Final RocksDB materialization buffers

The canonical mutable state during bulk sync is no longer the domain store. It is the in-memory
owner state.

### Build-Time Invariant

For bulk sync, no reducer may query ckbadger RocksDB for correctness-critical data that can be
derived from:

- current block/tx/output facts
- the in-memory live-cell state
- reducer-owned in-memory state

If some reducer still needs RocksDB lookups in the hot path, the fact graph is incomplete and the
design is not finished.

## FactsArena

The parser stage should emit a single shared `FactsArena` per batch/range. It should contain all
facts needed by downstream reducers so that no writer module needs to "补上下文" from RocksDB.

`FactsArena` should be split into compact, interned structures:

- `BlockFacts`
  - block number, hash, parent hash, epoch, timestamp, DAO field, tx count
- `TxFacts`
  - tx hash, block number, tx index, cellbase flag, fee, total input/output capacity
  - input outpoints
  - output cell refs
  - protocol/action hints already parsed from witnesses
- `CellFacts`
  - outpoint
  - created_at_block
  - capacity
  - occupied_capacity
  - data_size
  - optional `udt_amount`
  - interned lock/type/data identities
  - exact semantic tags: DAO / sUDT / xUDT / Spore / Cluster / mNFT / .bit / Fiber / plain cell
- `ResolvedInputFacts`
  - for each tx input, the fully resolved consumed cell view from in-memory live state
- `ResolvedTxFacts`
  - one canonical tx-level structure that contains all resolved inputs, outputs, and semantic facts
  - all reducers consume this structure instead of re-scanning/re-resolving independently

### Required Interning

To make an in-memory live set viable, `FactsArena` and owner state must avoid repeated `Vec<u8>`
  payloads for common identities:

- lock script hash / code hash / args
- type script hash / code hash / args
- collection IDs
- token type hashes
- frequently repeated small strings / enums

The build path should prefer `u32`/`u64` IDs into intern tables over repeated heap vectors.

## Runtime Architecture

### Stage 1: Chain Reader

- Read canonical blocks from CKB RocksDB exactly once
- Hand them to parser in order
- Never re-read ckbadger DB state for bulk correctness

### Stage 2: Fact Extractor

- Parse blocks/txs/cells/witnesses
- Compute `occupied_capacity`, `udt_amount`, script identities, protocol tags, DAO tags
- Intern repeated identities
- Emit `FactsArena`

This stage may be parallel inside the batch, but its output must be a single canonical fact graph.

### Stage 3: Sequencer + LiveCellOwner

This is the authoritative state transition engine for bulk sync.

Responsibilities:

- maintain the live-cell set in memory
- resolve every input directly from the live-cell set
- insert every output directly into the live-cell set
- emit `ResolvedTxFacts` in canonical tx order
- emit immutable history records when a state transition is final

This stage replaces the current bulk-sync dependency on:

- `get_full_cells_info_batch`
- `get_udt_cells_info_batch`
- `find_consumed_dao_deposits_batch`
- `get_*_by_outpoints_batch`

Those become in-memory lookups against owner state.

### Stage 4: Owner Reducers

Reducers own the mutable build state for their domain. They consume `ResolvedTxFacts` and may not
  query RocksDB for current-state context.

Recommended owners:

- `AddressOwner`
  - current address balance
  - used capacity
  - live/total cell counts
- `ScriptOwner`
  - script usage state
  - script daily deltas / compatibility metadata
- `TokenOwner`
  - token metadata final row
  - holder balances
  - ranked holder indexes
  - transfer history rows
- `DaoOwner`
  - deposit lifecycle state
  - DAO final indexes
  - DAO snapshot inputs
- `ObjectOwner`
  - spore / mNFT / object final state and collection indexes
- `IdentityOwner`
  - .bit / did:ckb identity state and collection indexes
- `FiberOwner`
  - fiber channel registry and indexes
- `ActivityOwner`
  - per-tx activity bundles
  - collection activity rows
- `StatsOwner`
  - chain/hour/day/epoch/miner stats
  - token/script/spore/object/identity rollups
  - HODL / cell distribution / cohort snapshots

### Stage 5: Materializer

Writes RocksDB in write-once mode according to CF class.

Bulk sync should stop thinking in terms of "write current block's DB mutations". It should think in
  terms of "this row has reached final value; write it once".

## Column Family Reclassification

The key optimization is to reclassify domain writes by write policy.

### Class A: Append-Only / Immutable Event Rows

These rows never need a later correction inside fresh bulk sync. Write them once when finalized.

- `cells` (append-only store only)
- `block_headers`
- `block_hash_index`
- `tx_index`
- `tx_hash_map`
- `consumed_cells`
- `addr_txs`
- `token_transfers`
- `activities`
- `object_collection_activities`
- `identity_collection_activities`

Notes:

- `cells` may still be streamed during sync, because it is append-only and never re-read by the
  build engine
- `consumed_cells` is immutable once a consumption is observed
- these writes can be flushed in large sequential batches without any read-modify-write step

### Class B: Final Snapshot State

These rows represent the final chain-tip view and should not be maintained incrementally in
RocksDB during fresh bulk sync. Keep them in reducer memory and write once during final
materialization.

- `live_cells`
- `cell_by_lock`
- `cell_by_type`
- `cell_by_lock_code`
- `cell_by_type_code`
- `cell_by_data_hash`
- `addr_balance`
- `dao_deposits`
- `dao_by_withdraw_tx`
- `dao_by_block`
- `dao_by_lock_block`
- `dao_by_status_block`
- `tokens`
- `token_holders`
- `token_holders_by_balance`
- `addr_tokens_by_balance`
- `spore_data`
- `spore_by_cluster`
- `object_data`
- `object_by_collection`
- `identity_data`
- `identity_by_collection`
- `object_collection_agg`
- `identity_agg`
- `stats_identity`
- `fiber_channels`
- `fiber_channel_by_commitment`
- `fiber_channel_by_funding_args`
- `addr_fiber_channels`
- `cluster_agg`
- `script_info`

These are the biggest win because current bulk sync often does:

- write output state
- later read it back as input context
- later delete/update related indexes

Fresh rebuild does not need that loop.

### Class C: Sealed Aggregate Buckets

These are mutable while a bucket is open, but final once the time bucket / epoch / collection
window is sealed. They should be written once per sealed bucket, not updated per block.

- `stats_chain`
- `stats_dao`
- `stats_hodl`
- `stats_script`
- `stats_token`
- `stats_spore`
- `stats_object`

Examples:

- daily/hourly buckets can flush once the reducer has moved beyond that bucket
- epoch buckets can flush once the epoch is closed
- HODL/cell distribution snapshots flush on the date transition where they are defined

### Class D: Bulk-Sync Disabled or Minimal Metadata

- `reorg_undo_log_by_block` is not part of bulk sync
- `pending_proposals` is live-sync only
- `sync_meta` only stores bulk-build metadata and final completion state, not a full partially
  queryable mutable progress model

## LiveCellOwner Design

`LiveCellOwner` is the crucial structure because it replaces RocksDB for input resolution.

It should keep only the minimal state required to:

- resolve future consumes
- build final live-cell indexes
- feed downstream reducers

Recommended compact slot content:

- outpoint
- created_at_block
- capacity
- occupied_capacity
- data_size
- optional `udt_amount`
- interned lock/type/data identifiers
- small semantic flags

Do not keep full duplicated script/data payloads per live cell if an intern table can hold them
once.

### Important Consequence

`CF_CELLS` can still receive append-only writes during sync, but the build engine must never read it
back during bulk sync. The live set is authoritative until final materialization completes.

## Write Schedule

### During Bulk Sync

Allowed streamed writes:

- append-only `cells`
- immutable event/history CFs from Class A
- sealed aggregate buckets from Class C once sealed
- minimal `sync_meta` heartbeat/build metadata

Forbidden bulk-sync hot-path behavior:

- DB reads for input resolution
- DB reads for address/script/token/object current state
- repeated update/delete cycles for final snapshot CFs

### End of Bulk Sync

Freeze reducers and materialize final snapshot CFs in owner order:

1. `LiveCellOwner`
   - `live_cells`
   - `cell_by_*`
2. `DaoOwner`
   - `dao_*`
3. `AddressOwner`
   - `addr_balance`
4. `TokenOwner`
   - `tokens`
   - `token_holders*`
   - `addr_tokens_by_balance`
5. `ObjectOwner` / `IdentityOwner` / `FiberOwner`
   - final object/identity/fiber state and indexes
6. `ScriptOwner`
   - `script_info`
7. `StatsOwner`
   - any remaining unsealed aggregate buckets
8. `sync_meta`
   - final completion record and tip metadata

The exact order can be tuned, but the rule is fixed: final snapshot CFs are written only after the
owners have converged on final values.

## Memory Strategy

This design is intentionally memory-first and should use adaptive budgets.

Adaptive knobs:

- parser batch size
- pipeline depth
- per-owner flush thresholds for Class A / Class C buffers
- intern-table growth strategy
- reducer sharding width

Non-adaptive rule:

- the authoritative live set and required owner state stay in memory

If the compact in-memory state cannot fit the machine budget, bulk sync should fail fast with
actionable output. It must not silently fall back to RocksDB current-state reads, because that
would recreate the exact second path this design is eliminating.

## Expected Read/Write Elimination

If this design is followed correctly, bulk sync should remove or drastically shrink:

- parser-time `get_full_cells_info_batch`
- parser-time `get_token`, `get_spores_batch`, `get_*_index` current-state lookups
- writer-time `read_address_balances`
- writer-time `read_script_info`
- writer-time `find_consumed_dao_deposits_batch`
- writer-time `get_udt_cells_info_batch`
- writer-time `get_spore_ids_by_outpoints_batch`
- writer-time `get_mnft_token_ids_by_outpoints_batch`
- writer-time `get_dotbit_account_ids_by_outpoints_batch`

The remaining RocksDB traffic during build should be dominated by:

- sequential append-only writes
- periodic sealed-bucket writes
- final one-shot snapshot materialization

## Failure Handling

- unresolved input in `LiveCellOwner`: fail immediately with outpoint / block / tx context
- memory budget exhausted: fail immediately with budget and owner breakdown
- reducer invariant violation: fail immediately with owner-specific context
- materialization failure: fail and require fresh rebuild

No repair path is added for bulk mode.

## Migration Plan

1. Introduce explicit CF write-policy classification in code: `append_only`, `final_snapshot`,
   `sealed_aggregate`, `bulk_disabled`.
2. Introduce `FactsArena` and `ResolvedTxFacts` as the only correctness path for bulk reducers.
3. Build `LiveCellOwner` and move all input resolution onto it.
4. Move current bulk prefetch readers out of writer modules and into reducer-owned memory state.
5. Convert final snapshot CFs from incremental DB mutation to end-of-build materialization.
6. Convert stats CFs to sealed-bucket flushing where possible.
7. Remove obsolete bulk-mode DB prefetch/read helpers after the new path is validated.

## Validation Requirements

The implementation plan for this design should include:

- regression tests proving bulk input resolution no longer depends on DB reads
- append-only invariant checks for `CF_CELLS`
- consistency tests comparing old/new bulk-sync outputs on fresh DB
- per-owner invariant tests
- wall-clock and RocksDB read/write counters before/after
- memory profile output with owner-level attribution

## Open Question Kept Explicit

The decisive feasibility question is not algorithmic correctness. It is peak memory for:

- compact live-cell state at chain tip
- token/object/identity final snapshot maps
- open aggregate buckets

The implementation must measure and publish these numbers early. If the compact live-state budget
is still too large for the target machines, the correct outcome is to tighten the in-memory data
layout further or require more RAM for this bulk path, not to reintroduce RocksDB hot-path reads.
