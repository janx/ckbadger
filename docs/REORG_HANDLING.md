# Chain Reorganization (Reorg) Handling

This document describes how ckbadger handles blockchain reorganizations (reorgs) and deep forks in the current RocksDB architecture.

## Overview

A chain reorganization occurs when the CKB node switches to a different fork with higher accumulated proof-of-work. When this happens, the indexer must:

1. Detect the fork point (last common ancestor)
2. Roll back data from orphaned blocks
3. Re-sync from the fork point on the new canonical chain

## Configuration

| Parameter             | Value                       | Description                                                              |
| --------------------- | --------------------------- | ------------------------------------------------------------------------ |
| `DEEP_FORK_DEPTH`     | `36` blocks                 | Maximum depth for automatic rollback handling                            |
| `bulk_sync_threshold` | `1000` blocks (CLI default) | When farther behind than this, reorg checks are skipped during bulk sync |

## Reorg Detection

The indexer compares the stored tip hash with the chain hash at the same height:

```text
DB Block N hash: 0xabc...
Chain Block N hash: 0xdef...  <- mismatch => reorg
```

On mismatch, it walks backward to find the fork point.

### Bulk Sync Optimization

During bulk sync (when `blocks_remaining > bulk_sync_threshold`), reorg checks are skipped because historical blocks are already finalized in practice. Reorg detection resumes automatically near tip.

## Handling Strategies

### Automatic Reorg (depth <= 36)

For reorgs up to 36 blocks deep, the indexer:

1. Records a reorg event in `CF_SYNC_META` (`reorg:<timestamp_ms>`)
2. Calls `rollback_to_block(fork_point)` for atomic multi-CF rollback
3. Removes rolled-back entries from core CFs (`block_headers`, `tx_index`, `live_cells`, `token_transfers`, `activities`, etc.)
4. Rebuilds `addr_balance` from remaining `live_cells`
5. Clears deep-fork flag if it was set
6. Updates sync cache status and continues syncing
7. Notifies pipeline fetcher via `reorg_notify_flag` and drains stale batches

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
5. Requires manual intervention

## RocksDB State

### `sync_status`

Deep fork state is persisted in `SyncStatus`:

- `deep_fork_detected: bool`
- `deep_fork_info: Option<DeepForkInfo>`

`DeepForkInfo` fields:

- `db_tip`
- `db_tip_hash`
- `chain_tip`
- `chain_tip_hash`
- `depth`
- `fork_point`

### `sync_meta`

Reorg events are serialized and stored as key-value entries:

- key: `reorg:<timestamp_ms>`
- value: `ReorgEvent { detected_at, rollback_from, rollback_to, depth }`

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

### `POST /api/v1/admin/resolve-deep-fork`

Resolves current deep fork with action `dismiss` (requires `ADMIN_TOKEN`).

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

## Resolving Deep Forks

When a deep fork is detected:

1. Investigate node/network state
2. Verify canonical chain
3. Resolve via API (`dismiss`) or rebuild DB and re-sync from genesis

After resolution, indexer resumes from stored tip and continues normal reorg handling.

## Why 36 Blocks?

The 36-block limit balances:

- Safety: natural reorgs are usually shallow
- Performance: rollback remains bounded
- Practicality: deeper forks usually indicate exceptional network conditions

With ~10s block time, 36 blocks is about 6 minutes.

## Monitoring

Recommended checks:

1. Logs for `Deep fork detected` / `Deep fork unresolved`
2. `GET /api/v1/forks/recent`
3. `GET /api/v1/statistics/network` (`deepForkStatus` fields)

---

_Last updated: 2026-02-19_
