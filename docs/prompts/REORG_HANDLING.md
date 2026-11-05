# Chain Reorganization (Reorg) Handling

This document describes how ckbadger handles blockchain reorganizations (reorgs) and deep forks in the current RocksDB architecture.

## Overview

A chain reorganization occurs when the CKB node switches to a different fork with higher accumulated proof-of-work. When this happens, the indexer must:

1. Detect the fork point (last common ancestor)
2. Roll back data from orphaned blocks
3. Re-sync from the fork point on the new canonical chain

CKBadger only needs to handle shallow fork, which means the reorg's impact has an explicit small upper bound (36 blocks). The reorg handling should use simple mechanisms and CF design, because the computation burden is very small.

Deep forks should cause failure and alert, and the way to fix a deep reorg is simply rebuild the whole db. Luckily db rebuild is very fast.

## Reorg Detection

The indexer compares the stored tip hash with the chain hash at the same height:

```text
DB Block N hash: 0xabc...
Chain Block N hash: 0xdef...  <- mismatch => reorg
```

On mismatch, it walks backward to find the fork point.

### Sync Phase Boundary (MANDATORY)

- Reorg handling runs only in **live sync** (near tip).
- During **bulk sync**, reorg handling is disabled by design: no reorg detection, no fork-point search, no rollback path execution.
- Bulk-sync behavior and failure policy are defined in `docs/prompts/BULK_SYNC.md`.
- When transitioning from bulk to live sync, reorg detection resumes automatically.

## Handling Strategies

### Automatic Reorg (depth <= 36)

For reorgs up to 36 blocks deep, the indexer:

1. Records a reorg event in `CF_SYNC_META` (`reorg:<timestamp_ms>`), including the first block it
   drops (`first_orphan_hash`, read from the stored headers before the rollback)
2. Calls `rollback_to_block(fork_point)` for atomic multi-CF rollback (through
   `BatchWriter::rollback_to`, see [Rollback Mechanisms](#rollback-mechanisms))
3. Range-deletes the rolled-back blocks' `block_headers`, `tx_index`, `activities` and journal rows
4. Writes every other domain row the rolled-back blocks changed — `live_cells`, `addr_txs`,
   `addr_balance`, `token_transfers`, collection activities and their counts, aggregates — back to
   its value at the end of the fork point from the block journal, in the same atomic write; nothing
   is scanned, reversed from deleted rows or rebuilt from canonical state
5. Recomputes the DAO tip summary at the new tip
6. Clears deep-fork flag if it was set
7. Updates sync cache status and continues syncing
8. Notifies pipeline fetcher via `reorg_notify_flag` and drains stale batches

### Pipeline Coordination

The indexer uses a three-stage pipeline (Fetcher -> Parser -> Writer). On reorg:

1. Writer performs rollback
2. Writer sets `reorg_notify_flag = true`
3. Writer drains stale parser/output batches
4. Fetcher sees the flag and resets local `next_block`
5. Fetcher re-reads DB tip and resumes from correct height

### Deep Fork (depth > 36)

For reorgs deeper than 36 blocks:

1. Writes deep-fork info into `sync_status`
2. Sets `deep_fork_detected = true`
3. Pauses sync in a wait loop
4. Broadcasts deep-fork status via WebSocket
5. Requires operator intervention and full DB rebuild before resuming normal correctness guarantees

## API Endpoints

### `GET /api/v1/forks`

Returns the persisted reorg-event history (`sync_meta` `reorg:<ms>:<uuid>` rows), newest first:
automatic switches and deep-fork detections. Each row carries its D7 address facts
(`firstOrphanHash`, `firstOrphanStatus`) and names related events (`revertedByEvent`,
`reversesEvents`) by D7 address, never by millisecond id.

### `GET /api/v1/forks/{fork_point_height}/{orphan_hash_prefix8}` (D7)

Returns one event's detail: the fork point height plus the first 8 lowercase hex digits of the first
block the switch dropped (height `fork_point + 1` on the abandoned branch). Exactly one event may
match; two matches are an error naming both events. Every automatic switch persists its first orphan
(`ReorgEvent.first_orphan_hash`): `execute_reorg` reads it from the indexer's own headers before the
rollback deletes them, after proving the branch links — its parent is the fork point hash, each later
header's parent is the header below, and the header at the old tip is the recorded old tip hash; a
break is an error before anything is rolled back. So every row's address resolves from the store
alone, without the node reader (`firstOrphanStatus` is `recorded`). A deep detection rolled nothing
back, so it has no address (`not_observed`). Without the node reader the detail is the persisted
event with `evidenceStatus: "node_reader_unavailable"` and empty branch evidence; with it, the node's
old branch must start at the persisted first orphan. The old millisecond route `/forks/{id}` is
removed.

### `GET /api/v1/forks/recent`

Returns current deep-fork status and the newest persisted event.

## WebSocket Events

Subscribe to `reorg` channel for fork-related notifications.

Current broadcaster behavior in RocksDB mode emits `deep_fork` state changes:

```json
{
  "type": "deep_fork",
  "data": {
    "detected": true,
    "depth": 50,
    "dbTip": 1000,
    "chainTip": 1050,
    "forkPoint": 1000,
    "timestamp": "2024-01-15T10:30:00Z"
  }
}
```

And resolution event:

```json
{
  "type": "deep_fork",
  "data": {
    "detected": false,
    "depth": 0,
    "dbTip": 0,
    "chainTip": 0,
    "forkPoint": 0,
    "timestamp": "2024-01-15T10:35:00Z"
  }
}
```

## Why 36 Blocks?

The 36-block limit balances:

- Safety: natural reorgs are usually shallow
- Performance: rollback remains bounded
- Practicality: deeper forks usually indicate exceptional network conditions

With ~10s block time, 36 blocks is about 6 minutes.

## Rollback Mechanisms

There is one: **the block journal**. Every column family has exactly one rollback class, the
`rollback_class` column of `CF_SPECS` (`crates/ckbadger-store/src/store.rs`), and a rollback treats
each class one way:

| Class          | Column families                                                                                            | Rollback                                                                                                                              |
| -------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `Journal`      | every domain CF not listed below                                                                           | each rolled-back block's journal entries write the key's previous value back; an absent previous value deletes the key                |
| `RangeByBlock` | `block_headers`, `tx_index`, `tx_group_cycles` (the lazy cycles records, keyed by the tx slot), `activities` (block-descending keys), `reorg_undo_log_by_block` (the journal) | the rolled-back blocks' key range is deleted; nothing is journaled                                                                    |
| `Never`        | `cells` (append-only store); `net_nodes`, `net_stats`, `net_crawl` (network store)                         | not rolled back: an orphaned cell payload is unreachable once its live / consumed marker is gone; the network store is not chain data |
| `Validated`    | `dob_decoded`                                                                                              | not rolled back: readers validate each row against canonical state                                                                    |

`RangeByBlock` is only for CFs whose key opens with the block number;
`rollback_classes_follow_the_key_layout_rule` pins the table.

### Write side: one overlay per live batch

A live batch spans many blocks and a fork can land on any of them, so the journal is kept per
block. `BlockOverlay` (`crates/ckbadger-store/src/block_overlay.rs`), one per live batch, sees every
write the batch makes to a `Journal`-class CF. The first time block N writes a key it records a
journal entry `(cf, key, previous)`: the key's value **at the end of block N-1** — which an earlier
block of the same batch may have written — or absent when the key did not exist.

- **One entry per (block, key)**, holding the pre-block value. A block that leaves a key at the
  value it found (a `lock_scripts` row of a known lock, a key created and deleted in one block)
  journals nothing.
- **One write per key**: each key is staged into the commit once, with its final value.
- A previous value the batch did not read is resolved by one `multi_get` per CF when the overlay
  is staged.

The writers reach it through `StoreBatch`: `StoreBatch::journaled(store, &overlay, origin)` makes
each sub-batch of the live batch route its `Journal`-class writes into the overlay (every other
class goes to the `WriteBatch` as before), and `StoreBatch::at(block)` attributes the writes made
through it to one block. A writer whose rows are batch aggregates applies them **block by block**
(the parser keeps per-block deltas, `PerBlock`) with the same per-row function, so every block's
end state is journaled exactly; a writer that reads back a key the batch already wrote reads it
through the overlay (`StoreBatch::get_stats`, `StoreBatch::get_current_many`). `write_parsed_batch`
stages the overlay (`stage_journal`) right before the atomic domain commit, so a block's rows and
its journal rows land in one write; `Live batch journal staged` (debug) logs `journal_entries`,
`journal_bytes` and `journal_keys_written`. Row layout: `docs/STORE_SCHEMA.md` →
`reorg_undo_log_by_block`.

Fail fast — each of these is an error that fails the batch before anything is committed:

- a `Journal`-class write without a block scope (`StoreBatch::at`);
- a write to a key from an older block than the key's last one (blocks reach a key in ascending
  order);
- a key written from two sub-batches, whose merge order would decide the committed value;
- a range delete on a `Journal`-class CF of a journaled batch (the journal needs every key);
- a commit before `stage_journal`;
- a write of the one key kept outside the journal, the coverage contract (below).

Bulk sync records no journal (`docs/prompts/BULK_SYNC.md` rule 12): an overlay opened on a store in
bulk mode stages final values and journals nothing.

### `rollback_to_block(target)`

`CkbadgerStore::rollback_to_block` (`crates/ckbadger-store/src/reorg_ops.rs`) rolls the domain store
back to the end of block `target` (`-1`: before genesis). For the blocks from the tip (the highest
stored header) down to `target + 1` it stages, into **one** `WriteBatch`:

1. the deletion of their `RangeByBlock` rows: one range delete per CF covering every rolled-back
   block — `block_headers`, `tx_index` and `reorg_undo_log_by_block` from `target + 1` up, and
   `activities`, whose keys open with the block number descending, from the tip's key through
   `target + 1`'s;
2. their journal entries' previous values, written back block by block from the tip down, each
   block's entries in reverse sequence order (an absent previous value deletes the key).

Each key has one entry per block holding its value at the end of the previous block, so the last
write of every key is its value at the end of `target`. There is no other stage: nothing is
reversed from deleted rows, rebuilt from surviving rows, or found by scanning a column family.
Because the rollback is one atomic write it is either committed or absent; there is no
in-progress marker and nothing to resume after a crash.

Errors, with nothing written: a target below -1 or above the tip; an undo-log row of a scope other
than `Journal`, or one that cannot be decoded; a journal row that names an unknown CF, a CF that is
not `Journal` (`cells` included — the append-only invariant on this path) or the coverage
contract; a `tx_index` row or journal row of a block above the tip. The result counts the headers
and `tx_index` rows removed, the `live_cells` markers removed and restored, and the journal entries
written back (`Rollback committed`, info).

### One entry point

The three callers — `execute_reorg` (automatic reorg, target = the fork point), startup cleanup
(`init_sync_start_with_options`, target = the startup tip, or -1 when its header is missing) and
partial-batch cleanup (`cleanup_batch_range`, target = the batch's first block - 1) — share
`BatchWriter::rollback_to` (`crates/indexer/src/db/writer/reorg.rs`), which runs, in order:

1. the coverage-floor check (below);
2. the caller's reads of the store as it is — `execute_reorg` proves the stored orphaned branch and
   reads its first orphan here, before the headers are deleted;
3. `derive::invalidate_for_rollback(target)` (below);
4. `begin_rollback`, which marks a pending fork reconciliation interrupted unless the rollback is a
   cleanup that keeps its fork point;
5. `rollback_to_block(target)`;
6. `refresh_latest_dao_statistics`, the DAO tip summary at the new tip (below).

None of them touches the append-only store.

### Tip summaries and the counters journaled per block

Rows that are a function of the tip are recomputed, not journaled. `refresh_latest_dao_statistics`
(`crates/indexer/src/db/writer/statistics.rs`) writes the DAO singletons (`stats_dao`
`DAO_LATEST_STATS` 0x1B, `DAO_TOP_DEPOSITORS` 0x20) and the daily snapshot of the tip's still-open
UTC+8 day (`DAO_DAILY_SNAPSHOT` 0x08) outside the journal, from one `dao_deposits` scan, and removes
every 0x08 row of a day after the tip's (only a former tip's open-day row, which a rollback can
write back as the previous value of a sealed write). It runs after every tip change: each forward
live batch, every rollback, startup and bulk finalize. A sealed day's row (a later day's block
exists) is the forward writer's, journaled at its sealer block, the first block of the next UTC+8
day.

`sync_status` (the tip number and hash, and the running totals `total_transactions`,
`total_cells_created`, `total_cells_consumed`) and `live_cell_summary:current` (the tip and its four
live-cell class counters) are **journaled per block** instead of recomputed at the new tip: their
counters cannot be derived from headers. The live writer writes both at every block of a batch, and
the rollback writes the target block's values back like any other row.

### Coverage floor (rebuild-required below it)

The journal is pruned behind a moving floor, and the store states how far back it can still roll:
`sync_meta` → `entity_stats_undo_contract` (`EntityStatsUndoContract { version,
coverage_floor_block, updated_at_block }`).

- Every live commit advances `coverage_floor_block` to
  `committed_tip - ENTITY_STATS_UNDO_RETAIN_BLOCKS` (1000) and deletes every journal row of the
  blocks it leaves behind (one range delete) in the same batch, so the floor and its deletions are
  never separately durable. `committed_tip` is the durable tip the batch builds on (the block before
  its first block), because the prune sees only committed rows; a batch longer than 1000 blocks
  leaves its own below-floor rows to the next batch (POSTMORTEM IDX-012). The floor never moves
  down.
- The contract describes which journal rows exist, so it is written **outside** the journal
  (`StoreBatch::put_entity_stats_undo_contract` stages it straight into the commit; the overlay
  refuses the key and the rollback refuses a journal row that names it): rolling back the block
  that advanced the floor leaves the floor where the prune put it.
- A rollback target **below** the floor fails with a rebuild-required error at **all three** entry
  points rather than deleting the rolled-back blocks' headers and transactions while every
  journaled row keeps their effects, which the re-sync would then apply a second time.
- A live reorg on a store with **no** contract at all is likewise refused; startup already rejects
  any non-empty store without one.

There is no repair path here on purpose: fabricating a row the store cannot reconstruct is exactly
the silent repair this design removes.

**Bulk → live handoff.** Bulk sync records no journal, so the first floor is the **handoff tip** —
the last block bulk actually wrote, not the chain tip it was racing. Live sync then has
`bulk_sync_threshold` blocks in which to build coverage before a legal shallow fork could reach
below the floor, which is why `indexer.bulk_sync_threshold >= DEEP_FORK_DEPTH` (36) is validated at
config load (`test_bulk_sync_threshold_below_deep_fork_depth_is_rejected`). The generated
`config.toml` writes 1000. See `docs/prompts/BULK_SYNC.md` rule 12.

### Derived aggregates (journaled, or reset)

`dao_live_agg`, `object_collection_activity_count` and `bit_cell_by_account` are the indexer's
background-derived aggregates (`docs/STORE_SCHEMA.md` → Derived Aggregates). Their live maintenance
writes every row per block through the journal like any `Journal` CF, so a rollback restores them
exactly — but only rows that were exact when their entries were recorded. Before every rollback
`derive::invalidate_for_rollback(target)` therefore resets:

- a derivation still **running**, by any rollback that removes a block (its rows mix snapshot
  contributions with live deltas, and rebuild chunks are not journaled);
- a **complete** derivation when `target` is below `completed_at_tip`, the tip at which its last
  rebuild chunk committed; at or above it, the journal owns the rollback.

A reset (`NotStarted` + range delete of the CF) commits before the journal rollback, together with
the deletion of the journal entries above the target of every aggregate the journal does not own —
the reset ones and any that are not maintained (not started, or left `Failed`) — so the rollback
cannot write stale rows into a cleared CF, and a crash anywhere in the rollback leaves the
aggregate reset and empty, never half-restored. The scheduler derives it again; meanwhile the API
serves the fields it feeds as unknown (`null`, or `503 derive_pending` for `/dao/evidence` and
`/dao/depositors`).

### The pin: the rollback property suite

`crates/indexer/tests/rollback_property.rs` writes one chain through the real live path — Spore
cluster and Spore, xUDT, mNFT, did:ckb, `.bit`, `.cell`, NervosDAO deposit / request / completion,
Fiber open / close, UTC+8 day and UTC hour boundaries, a data cell created and consumed above the
fork — rolls it back through `execute_reorg`, and requires every domain CF to be byte-equal, key by
key, to a store that synced only to the fork point with the same batch boundaries: single-block
batches, forks inside multi-block batches, 36 blocks deep, derived aggregates `Running` or
`Complete` above and below the target, the open-day DAO snapshot at forks inside batches, and plain
transfer chains. The rolled-back store must then replay to the tip byte-equal. The suite does not
know which mechanism restores a row, only that every row is restored; a change to any writer or to
the rollback is checked against it.

### Key Insights

1. One mechanism, chosen per CF by its key layout: a new `Journal` CF needs no rollback code — its
   live writer only has to write in a block scope, which the batch enforces.
2. The append-only store contains only `CF_CELLS` (immutable cell payloads). All other CFs (activities, addr_txs, collection activities, indexes, stats) are in the domain store.
3. In normal sync, append-store keys (`CF_CELLS`) are expected to be first-write-only; if a key already exists, that is an upstream bug signal. Duplicate append key writes are treated as correctness violations and should fail immediately.
