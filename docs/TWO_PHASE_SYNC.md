# Two-Phase Sync Architecture

This document describes the two-phase sync optimization for ckbadger's indexer, designed to reduce full database sync time from ~13 hours to under 1 hour (13x improvement).

## Overview

Traditional blockchain indexers write all derived tables (balances, statistics, indexes) during the initial sync. This creates significant I/O overhead because:

1. **Random writes** - Balance updates require reading current values before writing
2. **Index maintenance** - Every insert updates multiple B-tree indexes
3. **Lock contention** - Statistics tables cause row-level locks
4. **Cascading updates** - One block can trigger hundreds of balance recalculations

The two-phase approach separates **core data ingestion** from **derived data computation**.

## Architecture

```
                        PHASE 1: Core Sync                          PHASE 2: Rebuild
                    (High-throughput ingestion)                   (Batch computation)
                              │                                          │
                              ▼                                          ▼
┌─────────────────────────────────────────────┐     ┌─────────────────────────────────────────────┐
│                  WRITES                      │     │                  REBUILDS                   │
│                                             │     │                                             │
│  blocks ─────────────────── ✓               │     │  cell_status ◄─────── rebuild_cell_status   │
│  transactions ──────────── ✓               │     │                       (partition-parallel)  │
│  cells (status=0 only) ─── ✓               │     │                                             │
│  transaction_inputs ────── ✓               │     │  live_cells ◄──────── rebuild_live_cells    │
│  transaction_cell_deps ─── ✓               │     │                                             │
│  epoch_statistics ──────── ✓               │     │  address_balances ◄── rebuild_address_balances
│  sync_status ───────────── ✓               │     │                                             │
│                                             │     │  script_usage_stats ◄ rebuild_script_usage  │
│                  SKIPS                      │     │                                             │
│                                             │     │  dao_deposits ◄────── rebuild_dao_deposits  │
│  cells.status UPDATE ──── ✗                │     │  udt_cells ◄───────── rebuild_udt_cells     │
│  live_cells ───────────── ✗                │     │                                             │
│  dao_deposits ─────────── ✗                │     │  daily_statistics ◄── rebuild_daily_stats   │
│  udt_cells ─────────────── ✗                │     │  hourly_statistics ◄─ rebuild_hourly_stats  │
│  address_balances ─────── ✗                │     │  epoch_statistics ◄── rebuild_epoch_stats   │
│  address_transactions ─── ✗                │     │  miner_statistics ◄── rebuild_miner_stats   │
│  script_usage_stats ───── ✗                │     │                                             │
│  hourly_statistics ────── ✗                │     │  indexes ◄─────────── recreate_sync_indexes │
│  daily_statistics ─────── ✗                │     │                       (concurrent)          │
│  miner_statistics ─────── ✗                │     │                                             │
│  spore/nft processing ─── ✗                │     │  address_transactions (background,optional) │
│  dao_daily_snapshots ──── ✗                │     │                                             │
│  block_time_distribution ─ ✗               │     └─────────────────────────────────────────────┘
│                                             │
└─────────────────────────────────────────────┘
```

## Phase 1: Core Sync

### What Gets Written

| Table                   | Purpose                    | Why Written              |
| ----------------------- | -------------------------- | ------------------------ |
| `blocks`                | Block headers and metadata | Primary data             |
| `transactions`          | Transaction records        | Primary data             |
| `cells`                 | All cells (status=0 only)  | Primary data             |
| `transaction_inputs`    | Input references           | Primary data             |
| `transaction_cell_deps` | Cell dependencies          | Primary data             |
| `epoch_statistics`      | Epoch-level metrics        | Needed for sync progress |
| `sync_status`           | Indexer sync state         | Critical for operation   |

### What Gets Skipped

| Table/Operation           | Purpose                         | Why Skipped              |
| ------------------------- | ------------------------------- | ------------------------ |
| `cells.status` UPDATE     | Mark cells as consumed          | Computed in Phase 2      |
| `live_cells`              | Currently unspent cells         | Expensive random I/O     |
| `dao_deposits`            | DAO deposit tracking            | Per-tx queries expensive |
| `udt_cells`               | UDT/xUDT token cells            | Per-tx queries expensive |
| `spore_*/mnft_*/dotbit_*` | NFT tables                      | Per-tx queries expensive |
| `address_balances`        | Per-address balance totals      | Frequent updates         |
| `address_transactions`    | Transaction history per address | Many rows per tx         |
| `script_usage_stats`      | Script deployment counts        | Aggregate updates        |
| `hourly_statistics`       | Hourly network metrics          | Aggregate updates        |
| `daily_statistics`        | Daily network metrics           | Aggregate updates        |
| `miner_statistics`        | Per-miner block counts          | Aggregate updates        |
| `dao_daily_snapshots`     | DAO state over time             | Daily aggregates         |
| `block_time_distribution` | Block time histograms           | Statistical updates      |

### Enabling Bulk Sync Mode

Bulk sync mode is controlled by the `bulk_sync_mode` configuration:

```rust
// In Config (crates/indexer/src/config.rs)
Config {
    bulk_sync_mode: true,       // Enable two-phase sync
    bulk_sync_threshold: 1000,  // Blocks behind tip to trigger bulk sync
    batch_size: 3000,           // Larger batches for throughput
    parallel_fetch_size: 64,    // More concurrent RPC calls
    pipeline_buffer: 4,
    // ... other fields
}
```

When `bulk_sync_mode` is enabled and blocks remaining > threshold, the indexer automatically:

- Skips `cells.status` UPDATE (consumed cells marked in Phase 2)
- Skips `live_cells` inserts/deletes
- Skips `dao_deposits` processing (rebuilt in Phase 2)
- Skips `udt_cells` processing (rebuilt in Phase 2)
- Skips all Spore/NFT/dotbit processing (TODO: rebuild in Phase 2)
- Skips `address_balances`, `address_transactions`, `script_usage_stats` updates
- Skips hourly/daily/miner statistics writes
- Drops non-essential indexes at startup

**Automatic detection**: The indexer checks if the current block is far behind tip:

```rust
fn is_bulk_sync_active(&self) -> bool {
    self.config.bulk_sync_mode
        && self.progress.blocks_remaining() > self.config.bulk_sync_threshold
}
```

**Automatic Phase 2 trigger**: When `is_bulk_sync_active()` transitions from `true` to `false`, the indexer automatically starts the rebuild process in the background.

## Phase 2: Rebuild

After Phase 1 completes (all blocks synced), Phase 2 rebuilds derived tables using efficient batch operations.

### Rebuild Order

The rebuild runs tasks in dependency order:

```
1.  CellStatus         - Mark consumed cells (status=1) from transaction_inputs
2.  LiveCells          - Foundation for balance calculation (depends on cell_status)
3.  AddressBalances    - Depends on cells.status
4.  ScriptUsageStats   - Depends on cells.status
5.  DaoDeposits        - Rebuild DAO deposits from cells with DAO type script
6.  UdtCells           - Rebuild UDT cells from cells with sUDT/xUDT type script
7.  DailyStatistics    - Depends on all blocks
8.  HourlyStatistics   - Depends on all blocks
9.  EpochStatistics    - May update existing records
10. MinerStatistics    - Depends on all blocks
11. Indexes            - After all data written
12. AddressTransactions - Background (optional, can run while API active)
```

### Partition-Based Parallelism

Large tables use partition-based rebuilds for progress tracking and resumability:

```sql
-- Live cells rebuild processes 1M blocks at a time
SELECT rebuild_live_cells_partition($1, $2);
-- Where $1 = partition_start (0, 1000000, 2000000, ...)
-- And $2 = partition_end
```

Benefits:

- **Progress tracking**: Know exactly how far along the rebuild is
- **Resumability**: Can restart from last completed partition
- **Memory efficiency**: Don't load entire dataset into memory
- **Parallelization potential**: Could run partitions concurrently

### Rebuild Functions

Located in `migrations/postgres/001_init.sql`:

| Function                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- |
| `rebuild_cell_status_partition(start, end)` | Mark consumed cells from transaction_inputs |
| `rebuild_live_cells_partition(start, end)`  | Rebuild live_cells for block range          |
| `rebuild_address_balances()`                | Full rebuild from cells                     |
| `rebuild_script_usage_stats()`              | Count script deployments                    |
| `rebuild_dao_deposits()`                    | Rebuild DAO deposits from cells             |
| `rebuild_udt_cells()`                       | Rebuild UDT cells from cells                |
| `rebuild_daily_statistics()`                | Aggregate daily metrics                     |
| `rebuild_hourly_statistics()`               | Aggregate hourly metrics                    |
| `rebuild_epoch_statistics()`                | Update epoch-level metrics                  |
| `rebuild_miner_statistics()`                | Count blocks per miner                      |
| `drop_sync_indexes()`                       | Drop indexes for fast writes                |
| `recreate_sync_indexes()`                   | Recreate indexes concurrently               |

## Control Plane

The control plane (`migrations/control/001_init.sql`) manages multiple database instances and tracks sync progress.

### Schema

```sql
-- Track multiple database instances
CREATE TABLE instances (
    id UUID PRIMARY KEY,
    name TEXT,
    database_url TEXT,
    status TEXT,           -- created, syncing, rebuilding, ready, active
    sync_phase TEXT,       -- pending, core_sync, rebuild_*, completed
    current_block BIGINT,
    target_block BIGINT,
    sync_speed FLOAT8
);

-- Track which instance the API uses
CREATE TABLE active_instance (
    id INTEGER PRIMARY KEY DEFAULT 1,
    instance_id UUID REFERENCES instances(id)
);

-- Individual sync/rebuild jobs
CREATE TABLE sync_jobs (
    id UUID PRIMARY KEY,
    instance_id UUID REFERENCES instances(id),
    job_type TEXT,
    status TEXT,           -- pending, running, completed, failed
    progress_current BIGINT,
    progress_total BIGINT
);

-- Audit log
CREATE TABLE sync_events (
    id BIGSERIAL PRIMARY KEY,
    instance_id UUID,
    event_type TEXT,
    severity TEXT,
    message TEXT
);
```

### Sync Phases

The indexer tracks progress through the two-phase sync via the `sync_phase` column.
Phase names are generated dynamically as `format!("rebuild_{}", RebuildTask.name())`:

```
core_sync → rebuild_cell_status → rebuild_live_cells → rebuild_address_balances →
rebuild_script_usage_stats → rebuild_dao_deposits → rebuild_udt_cells →
rebuild_daily_statistics → rebuild_hourly_statistics → rebuild_epoch_statistics →
rebuild_miner_statistics → rebuild_indexes → completed
```

Phase transitions are automatic:

- When the indexer starts with `bulk_sync_mode=true` and is far behind, it sets phase to `core_sync`
- When caught up (blocks_remaining ≤ threshold), it automatically triggers rebuild and updates phases
- After all rebuild tasks complete, phase becomes `completed` and status becomes `ready`

## TUI Dashboard

The `ckbadger-tui` crate provides a terminal dashboard for monitoring and managing database instances.

### Prerequisites

1. **Control plane database**: Create a PostgreSQL database for the control plane:

```bash
createdb ckbadger_control
psql ckbadger_control < migrations/control/001_init.sql
```

2. **Build the TUI**:

```bash
cargo build -p ckbadger-tui --release
```

### Running the TUI

```bash
# Option 1: Environment variable
export CONTROL_DATABASE_URL=postgres://user:pass@localhost/ckbadger_control
cargo run -p ckbadger-tui

# Option 2: Command line argument
cargo run -p ckbadger-tui -- --control-db-url postgres://localhost/ckbadger_control

# Option 3: .env file (create .env in project root)
echo "CONTROL_DATABASE_URL=postgres://localhost/ckbadger_control" >> .env
cargo run -p ckbadger-tui
```

### Interface Overview

```
┌─ CKBadger Control Plane ──────────────────────────────────────────┐
│ Active: mainnet-prod (mainnet) | Syncing: 15,234,567 | 1,234 blk/s│
├───────────────────────────────────────────────────────────────────┤
│ [Instances] [Jobs] [Events] [Config]                              │
├───────────────────────────────────────────────────────────────────┤
│   Name          Status     Phase            Block          Speed  │
│ ─────────────────────────────────────────────────────────────────│
│ * mainnet-prod  Syncing    Core Sync        15.2M / 16.0M  1,234  │
│   mainnet-new   Created    Pending          0              -      │
│   testnet       Ready      Completed        8.8M           -      │
├───────────────────────────────────────────────────────────────────┤
│ Last refresh: 3s ago                                              │
│ q:Quit  Tab/←→:Switch tabs  j/k/↑↓:Navigate  a:Activate  r:Refresh│
└───────────────────────────────────────────────────────────────────┘
```

### Tabs

#### Instances Tab

Displays all registered database instances with:

| Column  | Description                                                     |
| ------- | --------------------------------------------------------------- |
| `*`     | Active instance indicator                                       |
| Name    | Instance name                                                   |
| Status  | `Created`, `Syncing`, `Rebuilding`, `Ready`, `Active`, `Failed` |
| Phase   | Current sync phase (Core Sync, Rebuild: \*, Completed)          |
| Block   | Current block / target block (with percentage)                  |
| Speed   | Sync speed in blocks/second                                     |
| Network | `mainnet` or `testnet`                                          |

**Status Colors:**

- 🟢 Green: `Active`, `Ready`
- 🟡 Yellow: `Syncing`, `Rebuilding`
- 🔴 Red: `Failed`

**Actions:**

- Press `a` to activate the selected instance (makes it the API's data source)
- Only `Ready` or `Active` instances can be activated

#### Jobs Tab

Shows currently running sync/rebuild jobs:

| Column   | Description                                 |
| -------- | ------------------------------------------- |
| Instance | Instance name                               |
| Type     | Job type (core*sync, rebuild*\*, etc.)      |
| Status   | `pending`, `running`, `completed`, `failed` |
| Progress | Completion percentage                       |
| Speed    | Processing speed (rows/second)              |

**Actions:**

- Press `c` to cancel the selected job

#### Events Tab

Real-time audit log showing recent sync events:

```
[14:32:15] INFO     phase_transition: Entering RebuildLiveCells
[14:32:10] INFO     core_sync_complete: Synced 16,000,000 blocks
[14:30:05] WARNING  slow_batch: Batch took 5.2s (expected <2s)
[14:25:00] ERROR    rpc_timeout: CKB node not responding
```

**Severity Levels:**

- `INFO` (white): Normal operations
- `WARNING` (yellow): Performance issues, retries
- `ERROR` (red): Failures requiring attention
- `CRITICAL` (red): System failures

#### Config Tab

Displays current sync configuration and two-phase sync architecture info:

- Default batch size, parallel fetch settings
- Tables skipped during bulk sync
- Current instance/job counts
- Architecture overview

### Keyboard Shortcuts

| Key         | Context        | Action                     |
| ----------- | -------------- | -------------------------- |
| `q`         | Any            | Quit TUI                   |
| `Tab`       | Any            | Next tab                   |
| `Shift+Tab` | Any            | Previous tab               |
| `←` / `→`   | Any            | Switch tabs                |
| `↑` / `k`   | Instances/Jobs | Select previous row        |
| `↓` / `j`   | Instances/Jobs | Select next row            |
| `a`         | Instances      | Activate selected instance |
| `c`         | Jobs           | Cancel selected job        |
| `r`         | Any            | Force refresh data         |

### Common Workflows

#### Starting a Fresh Sync

1. Create instance via SQL (or future TUI feature):

```sql
INSERT INTO instances (name, database_url, ckb_rpc_url, network)
VALUES ('mainnet-new', 'postgres://localhost/ckbadger_new',
        'http://localhost:8114', 'mainnet');
```

2. Open TUI to monitor:

```bash
cargo run -p ckbadger-tui
```

3. Watch the instance progress through phases:
   - `Pending` → `Core Sync` → `Rebuild: live_cells` → ... → `Completed`

4. Once `Ready`, press `a` to activate the instance

#### Monitoring Active Sync

1. Open TUI - header shows active instance status
2. Check `Jobs` tab for detailed job progress
3. Check `Events` tab for any warnings/errors
4. Data auto-refreshes every 5 seconds (or press `r`)

#### Troubleshooting Failed Instance

1. Check `Events` tab for error messages
2. Look at instance's `last_error` in Instances tab
3. Common issues:
   - CKB node unreachable: Check `ckb_rpc_url`
   - Database connection failed: Check `database_url`
   - Disk full: Check PostgreSQL storage
4. Fix issue, then restart indexer - it will resume from last checkpoint

#### Switching Active Instance (Zero-Downtime)

1. Ensure new instance shows `Ready` status
2. Select new instance, press `a`
3. API immediately starts serving from new instance
4. Old instance can be archived or deleted

## Performance Expectations

| Metric         | Traditional   | Two-Phase        | Improvement |
| -------------- | ------------- | ---------------- | ----------- |
| Full sync time | ~13 hours     | ~1 hour          | **13x**     |
| Blocks/second  | ~300          | ~4,000           | 13x         |
| I/O operations | High (random) | Low (sequential) | 5-10x       |
| Memory usage   | Variable      | Bounded          | More stable |

### Why It's Faster

1. **Sequential writes** - Phase 1 only appends; no read-modify-write cycles
2. **No index overhead** - Indexes dropped during Phase 1, rebuilt in Phase 2
3. **Batch aggregation** - Statistics computed once over final data, not incrementally
4. **Efficient scans** - Rebuild functions use sequential scans, not random lookups

## Usage

### Starting a New Sync

```bash
# Create new instance via TUI or directly:
INSERT INTO instances (name, database_url, ckb_rpc_url, network)
VALUES ('mainnet-new', 'postgres://...', 'http://...', 'mainnet');

# Start indexer with bulk sync mode
BULK_SYNC_MODE=true cargo run -p ckbadger-indexer -- \
    --batch-size 3000 \
    --parallel-fetch-size 64
```

### Monitoring Progress

```bash
# Via TUI
cargo run -p ckbadger-tui

# Or via SQL
SELECT sync_phase, current_block, target_block,
       round(current_block::numeric / target_block * 100, 2) as progress_pct
FROM instances WHERE name = 'mainnet-new';
```

### Manual Rebuild (if needed)

```rust
use ckbadger_indexer::RebuildRunner;

let runner = RebuildRunner::new(pool);

// Full rebuild
runner.run_full_rebuild().await?;

// Or individual tasks
runner.run_task(RebuildTask::LiveCells).await?;
runner.run_task(RebuildTask::AddressBalances).await?;
```

## Troubleshooting

### Rebuild Fails Mid-Way

Check progress table:

```sql
SELECT * FROM rebuild_progress ORDER BY task_name;
```

Resume from failed task:

```rust
// Skip completed tasks, restart from failed
for task in RebuildTask::all_ordered() {
    let progress = runner.get_progress(task.name()).await?;
    if progress.map(|p| p.status != "completed").unwrap_or(true) {
        runner.run_task(task).await?;
    }
}
```

### API Queries Slow During Rebuild

Indexes are dropped during Phase 1. Either:

1. Wait for Phase 2 `Indexes` task to complete
2. Or manually recreate critical indexes:

```sql
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_cells_lock_hash
ON cells (lock_script_hash);
```

### Sync Speed Drops

Check for:

- Network latency to CKB node
- Disk I/O saturation
- PostgreSQL vacuum running

```sql
-- Check for long-running queries
SELECT pid, now() - pg_stat_activity.query_start AS duration, query
FROM pg_stat_activity
WHERE state != 'idle' AND duration > interval '1 minute';
```

## Related Documentation

- [AGENTS.md](../AGENTS.md) - Development guidelines
- [INDEXER_PIPELINE.md](./INDEXER_PIPELINE.md) - Pipeline architecture
- [DAO_CALCULATIONS.md](./DAO_CALCULATIONS.md) - DAO-specific logic
