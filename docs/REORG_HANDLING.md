# Chain Reorganization (Reorg) Handling

This document describes how ckbadger handles blockchain reorganizations (reorgs) and deep forks.

## Overview

A chain reorganization occurs when the CKB node switches to a different fork of the blockchain. This happens when a competing chain accumulates more proof-of-work than the current chain. When this occurs, the indexer must:

1. Detect the fork point (common ancestor block)
2. Roll back data from orphaned blocks
3. Re-sync from the fork point on the new canonical chain

## Configuration

| Parameter       | Value       | Description                                               |
| --------------- | ----------- | --------------------------------------------------------- |
| `REORG_LIMIT`   | 36 blocks   | Maximum depth for automatic reorg handling                |
| `confirmations` | 0 (default) | Blocks are indexed immediately without confirmation delay |

## Reorg Detection

The indexer detects reorgs by comparing the stored block hash at the DB tip with the chain's block hash at the same height:

```
DB Block N hash: 0xabc...
Chain Block N hash: 0xdef...  <- Mismatch indicates reorg
```

When a mismatch is detected, the indexer walks backwards to find the fork point (last common ancestor).

### Bulk Sync Optimization

During bulk sync (when more than `bulk_sync_threshold` blocks behind the chain tip, default 72), reorg checks are **skipped**. This is safe because:

- CKB finalizes blocks after 24 confirmations
- `bulk_sync_threshold = 72` (2 × DEEP_FORK_DEPTH) ensures only finalized blocks are synced
- Historical blocks cannot be reorganized

Reorg detection resumes automatically when the indexer approaches the chain tip.

## Handling Strategies

### Automatic Reorg (depth <= 36)

For reorgs up to 36 blocks deep, the indexer automatically:

1. Archives orphaned blocks to `orphaned_blocks` table
2. Archives orphaned transactions to `orphaned_transactions` table
3. Rolls back network statistics (must happen before deleting blocks):
   - Decrements `hourly_statistics` (blocks_count, transactions_count, cells_created, cells_consumed)
   - Decrements `daily_statistics` (blocks*count, transactions_count, cells*\*, total_live_cells, total_data_size)
   - Decrements `miner_statistics` (blocks_count per miner)
4. Reverts cell consumption (marks consumed cells as live again)
5. Deletes rolled-back data (blocks, transactions, cells created after fork point)
6. Reverts DAO deposit states (withdraw requests and completions)
7. Rolls back token statistics:
   - Reverses `total_supply` changes from mints/burns
   - Decrements `transfers_count`
   - Rebuilds `token_balances` from remaining transfers
   - Recalculates `holders_count`
8. Deletes `token_transfers` in rollback range
9. Updates `sync_status` to fork point
10. Records event in `reorg_events` table
11. **Notifies fetcher task** via `reorg_notify_flag` to reset its state
12. Drains stale batches from pipeline channels
13. Continues syncing from the new chain

All operations execute in a single database transaction for atomicity.

### Pipeline Coordination

The indexer uses a three-stage pipeline (Fetcher → Parser → Writer). When a reorg is detected:

1. **Writer stage** handles the database rollback
2. **Writer stage** sets `reorg_notify_flag = true`
3. **Writer stage** drains stale batches from the parse channel
4. **Fetcher stage** checks the flag on each iteration
5. **Fetcher stage** resets its internal `next_block` state when flag is set
6. **Fetcher stage** re-queries DB for correct sync position

This coordination prevents the fetcher from continuing to send batches starting from outdated block numbers after a reorg.

### Deep Fork (depth > 36)

For reorgs deeper than 36 blocks:

1. Records deep fork event in `reorg_events` table
2. Sets `deep_fork_detected = TRUE` in `sync_status`
3. **Pauses sync** - indexer enters wait loop
4. Broadcasts alert via WebSocket
5. Frontend displays prominent alert banner

**Manual intervention required** to resolve deep forks.

## Database Schema

### reorg_events

Records all reorganization events:

```sql
CREATE TABLE reorg_events (
    id SERIAL PRIMARY KEY,
    event_type VARCHAR(20) NOT NULL,      -- 'auto', 'deep', 'resolved'
    depth INTEGER NOT NULL,
    fork_point_number BIGINT NOT NULL,
    fork_point_hash BYTEA NOT NULL,
    old_tip_number BIGINT NOT NULL,
    old_tip_hash BYTEA NOT NULL,
    new_tip_number BIGINT NOT NULL,
    new_tip_hash BYTEA NOT NULL,
    orphaned_blocks_count INTEGER DEFAULT 0,
    orphaned_txs_count INTEGER DEFAULT 0,
    detected_at TIMESTAMPTZ DEFAULT NOW(),
    resolved_at TIMESTAMPTZ,
    resolved_by VARCHAR(50),
    resolution_action VARCHAR(50),
    resolution_notes TEXT
);
```

### orphaned_blocks / orphaned_transactions

Preserve data from abandoned fork for historical reference:

```sql
CREATE TABLE orphaned_blocks (
    id SERIAL PRIMARY KEY,
    reorg_event_id INTEGER REFERENCES reorg_events(id),
    number BIGINT NOT NULL,
    hash BYTEA NOT NULL,
    parent_hash BYTEA NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    transactions_count INTEGER NOT NULL,
    miner_lock_hash BYTEA
);

CREATE TABLE orphaned_transactions (
    id SERIAL PRIMARY KEY,
    reorg_event_id INTEGER REFERENCES reorg_events(id),
    hash BYTEA NOT NULL,
    block_number BIGINT NOT NULL,
    block_hash BYTEA NOT NULL,
    tx_index INTEGER NOT NULL,
    inputs_count INTEGER NOT NULL,
    outputs_count INTEGER NOT NULL,
    total_capacity BIGINT
);
```

### sync_status (deep fork fields)

```sql
ALTER TABLE sync_status ADD COLUMN
    deep_fork_detected BOOLEAN DEFAULT FALSE,
    deep_fork_at TIMESTAMPTZ,
    deep_fork_db_tip BIGINT,
    deep_fork_db_tip_hash BYTEA,
    deep_fork_chain_tip BIGINT,
    deep_fork_chain_tip_hash BYTEA,
    deep_fork_depth INTEGER,
    deep_fork_fork_point BIGINT,
    last_reorg_at TIMESTAMPTZ,
    last_reorg_depth INTEGER;
```

## API Endpoints

### GET /api/v1/forks

List all reorg events (paginated).

**Response:**

```json
{
  "data": [
    {
      "id": 1,
      "eventType": "auto",
      "depth": 3,
      "forkPointNumber": 1000,
      "oldTipNumber": 1003,
      "newTipNumber": 1004,
      "orphanedBlocksCount": 3,
      "orphanedTxsCount": 15,
      "detectedAt": "2024-01-15T10:30:00Z"
    }
  ],
  "total": 1,
  "hasMore": false
}
```

### GET /api/v1/forks/{id}

Get reorg detail with orphaned blocks and transactions.

### GET /api/v1/forks/recent

Get recent reorg (last 24h) and current deep fork status.

**Response:**

```json
{
  "hasRecentReorg": false,
  "reorg": null,
  "deepFork": {
    "detected": false,
    "detectedAt": null,
    "depth": null,
    "dbTip": null,
    "chainTip": null,
    "forkPoint": null
  }
}
```

### POST /api/v1/admin/resolve-deep-fork

Manually resolve a deep fork (requires `ADMIN_TOKEN` env var).

**Request:**

```json
{
  "adminToken": "your-secret-token",
  "action": "dismiss",
  "notes": "Resolved after manual verification"
}
```

## WebSocket Events

Subscribe to `reorg` channel for real-time notifications:

```javascript
ws.send(
  JSON.stringify({
    action: 'subscribe',
    channel: 'reorg',
  })
);
```

**Reorg Event:**

```json
{
  "type": "reorg",
  "data": {
    "depth": 3,
    "oldTip": 1003,
    "newTip": 1004,
    "forkPoint": 1000,
    "orphanedBlocks": 3,
    "orphanedTxs": 15,
    "timestamp": "2024-01-15T10:30:00Z"
  }
}
```

**Deep Fork Event:**

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

## Frontend Components

### DeepForkAlert

Red alert banner displayed on homepage when `deepForkStatus.detected === true`:

- Shows depth, DB tip, chain tip
- Links to `/forks` page
- Auto-updates via network stats polling

### /forks Page

Lists all reorg events with:

- Event type badges (auto=blue, deep=red, resolved=green)
- Depth, fork point, old/new tips
- Orphaned block/transaction counts
- Links to detail page

### /forks/[id] Detail Page

Shows complete reorg information:

- Event metadata
- List of orphaned blocks
- List of orphaned transactions

## Resolving Deep Forks

When a deep fork is detected:

1. **Investigate**: Check CKB node logs, network status
2. **Verify**: Confirm the new chain is canonical
3. **Resolve** via one of:
   - **API**: POST to `/admin/resolve-deep-fork` with action `dismiss`
   - **Database**: Manually clear `deep_fork_detected` flag
   - **Re-sync**: Drop database and re-index from genesis

After resolution, the indexer will:

1. Clear deep fork flags
2. Resume syncing from the stored tip
3. Handle any remaining reorg automatically

## Why 36 Blocks?

The 36-block limit balances:

- **Safety**: Most natural reorgs are 1-3 blocks
- **Performance**: Rolling back 36 blocks is fast
- **Practicality**: Deeper reorgs likely indicate serious network issues

CKB's ~10 second block time means 36 blocks ≈ 6 minutes of chain history.

## Monitoring

Check for reorg issues:

```sql
-- Recent reorgs
SELECT * FROM reorg_events ORDER BY detected_at DESC LIMIT 10;

-- Unresolved deep forks
SELECT * FROM sync_status WHERE deep_fork_detected = TRUE;

-- Orphaned data counts
SELECT
    re.id,
    re.event_type,
    re.depth,
    COUNT(DISTINCT ob.id) as orphaned_blocks,
    COUNT(DISTINCT ot.id) as orphaned_txs
FROM reorg_events re
LEFT JOIN orphaned_blocks ob ON re.id = ob.reorg_event_id
LEFT JOIN orphaned_transactions ot ON re.id = ot.reorg_event_id
GROUP BY re.id;
```
