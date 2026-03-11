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

CKBadger uses two rollback mechanisms depending on the column family:

### Undo-Log Replay (for stateful CFs)

CFs with delta-based state (e.g. `addr_balance`, `script_info`, `token_holders`, cell indexes) use undo-log replay:

- Write path records undo entries into `reorg_undo_log_by_block`
- Key: `block_number + seq`
- Value: `UndoLogEntry { target_store, cf_name, key, previous_value }`
- Rollback replays entries for `block > rollback_to` in reverse order

### Direct Deletion (for activity/event CFs)

Activity and event CFs are rolled back via full-CF scan and direct deletion of entries belonging to rolled-back blocks:

- `CF_ACTIVITIES`, `CF_ADDR_TXS` — scan all keys, delete where `block_num > rollback_to`
- `CF_OBJECT_COLLECTION_ACTIVITIES`, `CF_IDENTITY_COLLECTION_ACTIVITIES` — same approach
- Stats CFs (`ACTIVITY_DAILY`, `ACTIVITY_HOURLY` prefixes in `CF_STATS_CHAIN`) — deleted via `should_delete_stats_for_replay`

No ghost entries, no canonical filtering needed — direct deletion keeps the domain store clean.

### Key Insights

1. CF ownership isolation alone is not enough; write semantics must also be isolated.
2. The append-only store contains only `CF_CELLS` (immutable cell payloads). All other CFs (activities, addr_txs, collection activities, indexes, stats) are in the domain store.
3. In normal sync, append-store keys (`CF_CELLS`) are expected to be first-write-only; if a key already exists, that is an upstream bug signal. Duplicate append key writes are treated as correctness violations and should fail immediately.
