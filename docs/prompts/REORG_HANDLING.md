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

1. Records a reorg event in `CF_SYNC_META` (`reorg:<timestamp_ms>`)
2. Calls `rollback_to_block(fork_point)` for atomic multi-CF rollback
3. Removes rolled-back entries from all domain CFs: `block_headers`, `tx_index`, `live_cells`, `token_transfers`, `activities`, `addr_txs`, collection activity CFs, mutable aggregates, etc.
4. Activity/addr_txs/collection activity entries are directly deleted via range scan (no ghost entries, no canonical filtering needed)
5. Rebuilds `addr_balance` and collection activity counts from remaining canonical state
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

Returns current deep-fork event list derived from `sync_status`:

- deep fork active: one synthetic event
- no deep fork: empty list

### `GET /api/v1/forks/{id}`

Returns deep-fork detail only when:

- `id == 1`
- deep fork is currently active

`orphaned_blocks` and `orphaned_transactions` are empty in RocksDB mode.

### `GET /api/v1/forks/recent`

Returns current deep-fork status and optional synthetic reorg object when deep fork is active.

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

Every column family (and every stats prefix within one) is rolled back by exactly **one** of
these mechanisms. Two owners for the same rows means one of them silently loses data:

### Undo-Log Replay (for stateful CFs)

CFs with delta-based state (e.g. `addr_balance`, `script_info`, `token_holders`, cell indexes) use undo-log replay:

- Write path records undo entries into `reorg_undo_log_by_block`
- Key: `block_number + seq`
- Value: `UndoLogEntry { target_store, cf_name, key, previous_value }`
- Rollback replays entries for `block > rollback_to` in reverse order

### Direct Deletion (for activity/event CFs)

Activity and event CFs are rolled back via full-CF scan and direct deletion of entries belonging to rolled-back blocks:

- `CF_ACTIVITIES`, `CF_ADDR_TXS` — scan all keys, delete where `block_num > rollback_to`
- `CF_ADDR_TXS_BY_PREFIX` — same, and with tx-contexts available the exact keys are derived from
  the rolled-back `TxActions` rows' `LockPrefix` participants (cells cannot enumerate a party that
  holds none); a missing row aborts the rollback
- `CF_OBJECT_COLLECTION_ACTIVITIES`, `CF_IDENTITY_COLLECTION_ACTIVITIES` — same approach
- Stats CFs (`ACTIVITY_DAILY`, `ACTIVITY_HOURLY` prefixes in `CF_STATS_CHAIN`) — deleted via `should_delete_stats_for_replay`

No ghost entries, no canonical filtering needed — direct deletion keeps the domain store clean.

### Prefix Participation Counters (reversed by deleted rows)

`addr_prefix_stats` counts the transactions a protocol named a 20-byte lock-hash prefix in without
that party holding a cell. It carries **no** undo pre-image. Stage 8c' counts the
`addr_txs_by_prefix` rows it deletes per prefix, subtracts that from the counter, and asserts the
result **equals** the rows that survive; a counter reaching zero has its row deleted, and a prefix
with deleted rows but no counter row aborts the rollback. This is the same contract
`addr_balance.txs_count` uses against `addr_txs`, and it is the only exact one: a live batch spans
many blocks, so a pre-image recorded on one block of the batch is not replayed when the fork point
lands on a later block of that same batch, while the rows written after it are still deleted.

### Entity Statistics (undo-log owned, never swept)

The eight per-entity daily/hourly stats families are restored **only** by undo replay. They are
not in `STATS_REPLAY_CANDIDATE_PREFIXES`, and `should_delete_stats_for_replay` answers `false` for
them at any date or hour:

- `SCRIPT_DAILY`, `TOKEN_DAILY`, `CLUSTER_DAILY`, `SPORE_DAILY`, `OBJECT_DAILY`
- `TOKEN_HOURLY`, `SPORE_HOURLY`, `OBJECT_HOURLY`

These rows are keyed `(entity, time bucket)`, so one bucket carries contributions from **both**
sides of the fork point. Deleting the bucket threw away the surviving main-chain part for good,
because replay only re-applies blocks after the fork point (POSTMORTEM STATS-010). The forward
path records a per-block pre-image instead: `EntityStatsOverlay`
(`crates/indexer/src/db/writer/entity_stats.rs`) records the value at the end of the previous
block the first time a block touches a key, under `UndoSeqScope::EntityStats`, and writes each key
exactly once per batch. Rollback replays those entries and gets the exact bytes back.

Undo replay runs **before** canonical rollback at every entry point (`execute_reorg`,
`init_sync_start`, partial-batch cleanup), so the stages that recompute aggregates from surviving
rows — cluster capacity, for instance — already see restored values.

### Stats Prefix Rollback Ownership

Every stats prefix the rollback path can reach has exactly one owner. The table is executable:
`test_stats_prefix_rollback_owner_table` in `crates/ckbadger-store/src/reorg_ops.rs` asserts it
prefix by prefix.

| Prefix                                                                         | Owner              | Mechanism                                                                                |
| ------------------------------------------------------------------------------ | ------------------ | ---------------------------------------------------------------------------------------- |
| `DAILY`, `HOURLY`, `ACTIVITY_DAILY`, `ACTIVITY_HOURLY`, `DAILY_BLOCK`, `MINER` | Cutoff sweep       | Cutoff bucket delta-repaired, later buckets deleted and regenerated by replay            |
| `ACTIVITY_DAILY_ADDR_SET`, `ACTIVITY_HOURLY_ADDR_SET`                          | Cutoff sweep       | Strict `>` deletion; the cutoff bucket's address set is rebuilt from the surviving chain |
| `DAO_DAILY_SNAPSHOT`                                                           | Cutoff sweep       | Cutoff day recomputed forward (`recompute_dao_daily_snapshots`)                          |
| `EPOCH`                                                                        | Cutoff sweep       | Boundary epoch truncated to the fork point; epochs beginning inside the range deleted    |
| `HODL_WAVE`, `CELL_DISTRIBUTION`, `ADDR_COHORT`                                | Cutoff sweep       | Day-boundary snapshots; the cutoff day's row does not exist yet when the fork happens    |
| `SCRIPT_DAILY`, `TOKEN_DAILY`, `CLUSTER_DAILY`, `SPORE_DAILY`, `OBJECT_DAILY`  | `EntityStats` undo | Per-block pre-image replay; never deleted by the sweep                                   |
| `TOKEN_HOURLY`, `SPORE_HOURLY`, `OBJECT_HOURLY`                                | `EntityStats` undo | Per-block pre-image replay; expiry is the separate forward-only retention sweep          |

The three day-boundary snapshot families are sweep-owned and correct as such: a day's snapshot is
sealed on the **first block of the next day** and keyed by the **previous** day, so the cutoff
day's row does not exist yet at fork time and the previous day's row sorts below the cutoff. This
is pinned by `test_day_boundary_snapshots_survive_same_day_and_sealer_rollback`.

### Coverage Floor (rebuild-required below it)

`EntityStats` undo entries are pruned behind a moving floor, so the store states how far back it
can still roll these families: `sync_meta` → `entity_stats_undo_contract`
(`EntityStatsUndoContract { version, coverage_floor_block, updated_at_block }`).

- Every live commit advances `coverage_floor_block` to
  `committed_tip - ENTITY_STATS_UNDO_RETAIN_BLOCKS` (1000) and stages the matching deletions into
  the same batch, so the floor and its deletions are never separately durable.
- A rollback target **below** the floor fails with a rebuild-required error at **all three** entry
  points — `execute_reorg` (fork point), `init_sync_start` (startup cleanup target), and
  partial-batch cleanup — rather than rolling back blocks, cells and indexes while leaving these
  buckets in place, which would have the re-sync add the same deltas a second time, undetectably.
- A live reorg on a store with **no** contract at all is likewise refused; startup already rejects
  any non-empty store without one.

There is no repair path here on purpose: fabricating a bucket the store cannot reconstruct is
exactly the silent repair this design removes.

**Bulk → live handoff.** Bulk sync records no undo entries, so the first floor is the **handoff
tip** — the last block bulk actually wrote, not the chain tip it was racing. Live sync then has
`bulk_sync_threshold` blocks in which to build coverage before a legal shallow fork could reach
below the floor, which is why `indexer.bulk_sync_threshold >= DEEP_FORK_DEPTH` (36) is validated at
config load (`test_bulk_sync_threshold_below_deep_fork_depth_is_rejected`). The generated
`config.toml` writes 1000. See `docs/prompts/BULK_SYNC.md` rule 12.

### Key Insights

1. CF ownership isolation alone is not enough; write semantics must also be isolated.
2. The append-only store contains only `CF_CELLS` (immutable cell payloads). All other CFs (activities, addr_txs, collection activities, indexes, stats) are in the domain store.
3. In normal sync, append-store keys (`CF_CELLS`) are expected to be first-write-only; if a key already exists, that is an upstream bug signal. Duplicate append key writes are treated as correctness violations and should fail immediately.
