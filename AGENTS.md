# AGENTS.md

Instructions for AI agents working on ckbadger - a CKB blockchain explorer.

## Development Status (IMPORTANT)

**This is a project under active development, NOT running in production.**

- Database can be cleared and rebuilt at any time
- Data can be re-synced from scratch whenever needed
- Schema changes are cheap — no migration compatibility concerns

**Design Implications:**

When solving problems or designing features:

1. **Prefer optimal schema design** over backward compatibility
2. **Feel free to restructure tables** if it produces a cleaner solution
3. **Breaking changes are acceptable** — just update `migrations/postgres/001_init.sql`
4. **If a bug fix requires schema change**, do it properly rather than working around bad structure
5. **Re-sync is always an option** — don't let existing data constrain the right solution

```bash
# Typical workflow after schema changes:
# 1. Edit migrations/postgres/001_init.sql
# 2. Update indexer parser/writer code
# 3. Drop and recreate database
# 4. Re-run indexer to sync from genesis
```

## Commands

```bash
# Rust
cargo check                              # Type check all crates
cargo build -p ckbadger-api              # Build specific crate
cargo clippy                             # Lint

# Rust Testing (213 indexer tests)
cargo test                               # Run all tests
cargo test --lib                         # Unit tests only (fast)
cargo test test_name                     # Single test (partial match)
cargo test -p ckbadger-indexer           # Tests in one crate
cargo test -- --nocapture                # With stdout

# Indexer with Redis cache invalidation (optional feature)
cargo build -p ckbadger-indexer --features redis-cache  # Enable cache feature
REDIS_URL=redis://localhost:6379 cargo run -p ckbadger-indexer --features redis-cache

# Frontend (from root OR frontend/)
pnpm dev                                 # Dev server (:3000)
pnpm build                               # Production build
pnpm lint                                # ESLint
cd frontend && pnpm type-check           # TypeScript (tsc --noEmit)

# Frontend Testing (90 tests)
cd frontend && pnpm test                 # Run Vitest
cd frontend && pnpm test:coverage        # With coverage report
cd frontend && npx vitest run            # Non-interactive

# E2E Testing (requires running services)
pnpm test:e2e                            # Playwright tests

# Pre-commit verification
cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint

# Formatting
pnpm format                              # Prettier (all files)
```

## Project Structure

```
crates/
  api/        # Axum REST/WebSocket server (port 3001)
  indexer/    # Blockchain sync daemon (three-stage pipeline)
  common/     # Shared types (block, cell, tx, script, error)
frontend/     # Next.js 15 App Router + React 19
migrations/postgres/001_init.sql  # Single consolidated schema
docs/POSTMORTEM.md                # Historical bugs - READ BEFORE CKB/DAO WORK
docs/INDEXER_PIPELINE.md          # Pipeline architecture documentation
```

## Indexer Pipeline Configuration

The indexer uses a three-stage pipeline: **Fetcher** (RPC I/O) → **Parser** (CPU + DB prefetch) → **Writer** (DB I/O).

| Parameter             | Default | Description                          |
| --------------------- | ------- | ------------------------------------ |
| `pipeline_enabled`    | `true`  | Enable pipeline mode (vs sequential) |
| `pipeline_buffer`     | `4`     | Channel capacity between stages      |
| `batch_size`          | `10000` | Blocks per batch                     |
| `parallel_fetch_size` | `64`    | Concurrent RPC requests              |
| `copy_pool_size`      | `24`    | Parallel COPY connections            |

```bash
# CLI arguments
cargo run -p ckbadger-indexer -- \
  --pipeline-enabled \
  --pipeline-buffer 4 \
  --batch-size 10000 \
  --copy-pool-size 24

# Environment variables
PIPELINE_ENABLED=true
PIPELINE_BUFFER=4
BATCH_SIZE=10000
COPY_POOL_SIZE=24
```

See `docs/INDEXER_PIPELINE.md` for architecture details.

## Progress Tracking

The indexer uses two complementary log lines:

1. **Batch log** (per batch): `Wrote blocks X to Y (N remaining, 2.34s) [COPY]`
   - Shows DB write duration for the batch
   - Useful for identifying slow batches

2. **Progress log** (every 10s): `Progress: 33.96% (6279999/18491045) - 3465.00 blocks/sec (EMA: 3200.00)`
   - Shows overall sync percentage and throughput
   - `blocks/sec`: 10-second sliding window (real-time, volatile)
   - `EMA`: Exponential Moving Average with α=0.1 (smoothed, stable)
   - ETA: `remaining_blocks / EMA` (simple calculation)

### Redis Sync Data

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
    pub indexes_deferred: bool,
    pub indexes_rebuild_progress: Option<IndexRebuildProgressData>,
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

**Data Flow**:

1. Indexer updates `sync:status` after each batch write
2. Indexer updates `sync:progress` every 10 seconds with ETA
3. Indexer updates `memory:stats` every 10 seconds with RocksDB memory usage
4. API reads `sync:status` for totals (blocks, transactions, cells)
5. API reads `sync:progress` for real-time progress display
6. Task TUI reads `memory:stats` for Memory Usage panel
7. WebSocket broadcaster uses both for `new_block` messages

**Fallback** (when Redis unavailable):

| Data                 | Fallback Query                      |
| -------------------- | ----------------------------------- |
| `tip_block_number`   | `SELECT MAX(number) FROM blocks`    |
| `total_transactions` | `SELECT COUNT(*) FROM transactions` |
| `total_live_cells`   | `SELECT COUNT(*) FROM live_cells`   |
| `sync_ema_rate`      | None (ETA not displayed)            |

**Requires**: `redis-cache` feature enabled on both indexer and API, plus `REDIS_URL` environment variable.

## Deferred Index and Constraint Optimization

For fresh database syncs, the indexer automatically drops non-essential B-tree indexes and UNIQUE constraints to achieve ~3-4x faster write speeds. Both are rebuilt automatically via the task-runner when the sync catches up to the chain tip.

| Parameter                  | Default | Description                                                         |
| -------------------------- | ------- | ------------------------------------------------------------------- |
| `--defer-indexes`          | `false` | Force enable deferred indexes/constraints (non-fresh)               |
| `--no-auto-defer-indexes`  | `false` | Disable auto-optimization for fresh DB                              |
| `--index-rebuild-parallel` | `10`    | Parallel connections per partitioned table (capped at 4 internally) |

**Index Rebuild Lock Contention Handling:**

When rebuilding indexes on partitioned tables, multiple `CREATE INDEX CONCURRENTLY` operations may compete for locks. The task-runner handles this with:

1. **Reduced Parallelism**: Effective parallelism capped at 4 connections per logical index (regardless of `--index-rebuild-parallel` setting) to reduce lock contention
2. **Automatic Retry**: Lock timeout failures retry up to 3 times with 5-second delays
3. **Error Detection**: Detects PostgreSQL lock timeout messages (`lock timeout`, `could not obtain lock`, `canceling statement due to lock timeout`)

**What Gets Deferred:**

| Type               | Items                                           | Reason Safe to Drop                   |
| ------------------ | ----------------------------------------------- | ------------------------------------- |
| B-tree Indexes     | 26 indexes on blocks, transactions, cells, etc. | Query optimization only               |
| UNIQUE Constraints | 5 constraints on cells, inputs, cell_deps, etc. | CKB node already validates uniqueness |

**Deferred UNIQUE Constraints:**

These constraints are redundant during bulk sync because CKB node validates:

- Cell uniqueness: `(tx_hash, output_index)` globally unique
- Input/output indices: Sequential within transaction
- Block structure: Proposals/uncles indexed correctly

| Table                   | Constraint                                  | CKB Guarantee       |
| ----------------------- | ------------------------------------------- | ------------------- |
| `cells`                 | `(created_at_block, tx_hash, output_index)` | Cell outputs unique |
| `transaction_inputs`    | `(tx_block_number, tx_hash, input_index)`   | Sequential indices  |
| `transaction_cell_deps` | `(tx_block_number, tx_hash, dep_index)`     | Sequential indices  |
| `block_proposals`       | `(block_number, proposal_index)`            | Block structure     |
| `uncle_blocks`          | `(block_number, uncle_index)`               | Block structure     |

**Behavior:**

| Scenario                      | Auto-drop indexes/constraints | Auto-submit rebuild task |
| ----------------------------- | ----------------------------- | ------------------------ |
| Fresh DB (tip=0)              | Yes                           | Yes                      |
| Fresh DB + `--no-auto-defer`  | No                            | No                       |
| Resume sync, indexes exist    | No                            | No                       |
| Resume sync, indexes deferred | No                            | Yes                      |
| Any DB + `--defer-indexes`    | Yes                           | Yes                      |

**Task-Based Rebuild Flow:**

When bulk sync completes (catches up to <=1000 blocks behind tip), the indexer automatically submits tasks to the `tasks` table:

1. Indexer detects bulk sync completion
2. Submits `index_rebuild` task (priority 10) if indexes are deferred
3. Submits `cells_status_rebuild` task (priority 9) to rebuild cells.status
4. Submits `live_cells_populate` task (priority 8) to populate PostgreSQL from RocksDB
5. Submits `statistics_rebuild` task (priority 5) to rebuild aggregate statistics
6. Task-runner picks up `index_rebuild`, `cells_status_rebuild`, and `statistics_rebuild` tasks
7. Indexer executes `live_cells_populate` during idle time (requires RocksDB access)
8. Indexes rebuilt with `CREATE INDEX CONCURRENTLY`
9. Cells status rebuilt from transaction_inputs table
10. Statistics tables rebuilt (daily_statistics, hourly_statistics, miner_statistics, etc.)
11. Tasks complete (status: `completed`)

**Available Task Types:**

| Task Type                     | Priority | Description                                                     |
| ----------------------------- | -------- | --------------------------------------------------------------- |
| `index_rebuild`               | 10       | Rebuild deferred indexes and constraints                        |
| `cells_status_rebuild`        | 9        | Rebuild cells.status and consumed*at*\* from transaction_inputs |
| `address_balances_rebuild`    | 8        | Rebuild address_balances from cells GROUP BY                    |
| `live_cells_populate`         | 8        | Populate live_cells table from RocksDB (indexer)                |
| `token_rebuild`               | 7        | Rebuild tokens/token_balances/udt_cells with UDT parsing        |
| `secondary_issuance_backfill` | 7        | Backfill ALL blocks' secondary issuance data (exact)            |
| `activities_rebuild`          | 7        | Rebuild activities table from blocks/transactions               |
| `spore_rebuild`               | 6        | Rebuild spore_cells status from cells table (batched)           |
| `statistics_rebuild`          | 5        | Rebuild all 7 aggregate statistics tables (parallel, up to 4)   |
| `cycles_backfill`             | 0        | Backfill transaction cycles from RPC                            |
| `label_import`                | 0        | Import UDT/script labels from token-labels repo                 |

> **Note:** `consumed_at_backfill` has been merged into `cells_status_rebuild`. Existing pending tasks will be redirected automatically.

**Bulk Sync Protection:**

Tasks that require complete blockchain data are automatically deferred during bulk sync. The task-runner checks sync status before executing each task:

| Task Type                     | Bulk Sync Safe | Reason                                     |
| ----------------------------- | -------------- | ------------------------------------------ |
| `cycles_backfill`             | ✅ Yes         | RPC-based, independent of sync state       |
| `label_import`                | ✅ Yes         | File-based, independent of sync state      |
| `index_rebuild`               | ❌ No          | Would slow down writes by 3-4x during sync |
| `cells_status_rebuild`        | ❌ No          | Requires all transaction_inputs written    |
| `address_balances_rebuild`    | ❌ No          | Requires all cells written                 |
| `live_cells_populate`         | ❌ No          | Requires RocksDB fully populated           |
| `token_rebuild`               | ❌ No          | Requires all cells with UDT type_script    |
| `activities_rebuild`          | ❌ No          | Requires all transactions/cells written    |
| `spore_rebuild`               | ❌ No          | Requires accurate cell status              |
| `statistics_rebuild`          | ❌ No          | Requires complete blockchain data          |
| `secondary_issuance_backfill` | ❌ No          | Requires all blocks to exist               |

When a bulk-sync-unsafe task is claimed during bulk sync:

1. Task-runner detects bulk sync active (blocks_behind > 1000)
2. Task status is set back to `pending`
3. Reason is recorded in `error_message` field
4. Task will be re-attempted on next poll cycle

This prevents incomplete/incorrect results from tasks that depend on having all blockchain data available.

**Label Import Auto-Trigger:**

The `label_import` task is automatically submitted when the indexer starts, if:

1. Token labels directory exists (checks `$TOKEN_LABELS_PATH/information/` or `docs/token-labels/information/`)
2. No pending/running `label_import` task already exists

The path is determined by `TOKEN_LABELS_PATH` environment variable, defaulting to `docs/token-labels` for local development. In Docker, this is set to `/app/token-labels` with a volume mount.

This ensures token labels are refreshed at least once per indexer lifecycle without manual intervention.

**Secondary Issuance Backfill Auto-Trigger:**

The `secondary_issuance_backfill` task is automatically submitted when the indexer crosses the 1000-block threshold (from bulk sync to real-time sync).

**Indexer behavior by sync state:**

| Blocks Behind     | Secondary Issuance Tracking | On State Change      |
| ----------------- | --------------------------- | -------------------- |
| >1000 (bulk)      | Skipped entirely            | -                    |
| ≤1000 (real-time) | Track every block           | Submit backfill task |

The backfill task:

1. Resets `block_secondary_issuance` table and cumulative values to 0
2. Pre-loads all DAO deposit/withdrawal events to memory (eliminates per-batch DB queries)
3. Processes blocks from block 1 to tip (genesis block 0 is skipped - CKB RPC returns null for it)
4. Uses JSON-RPC batch requests (250 blocks per batch, 32 concurrent) to `get_block_economic_state`
5. Writes using PostgreSQL COPY binary protocol (2-3x faster than INSERT)
6. Calculates breakdown using RFC-0015 formula (exact, not sampled)
7. Updates `dao_statistics.cumulative_burnt` with accurate totals

**Performance Configuration:**

| Parameter             | Default | Description                             |
| --------------------- | ------- | --------------------------------------- |
| `batch_size`          | 1000    | Blocks per DB write batch               |
| `concurrent_requests` | 32      | Concurrent RPC batch requests           |
| RPC batch size        | 250     | Blocks per JSON-RPC request (hardcoded) |
| HTTP timeout          | 60s     | Request timeout for stability           |
| HTTP connect timeout  | 10s     | Connection establishment timeout        |

**Performance:** Optimized implementation reduces full-chain backfill time from ~10 hours to ~2-3 hours.

**Why exact calculation matters:** Secondary issuance varies per block (~340-590 CKB). Sampling every N blocks and multiplying produces ~50x under-reporting of burnt amounts.

**Statistics Tables Rebuilt:**

- `daily_statistics` - Daily transaction/cell counts
- `daily_block_stats` - Daily block metrics (uncle rate, block time)
- `hourly_statistics` - Hourly transaction/cell counts
- `miner_statistics` - Miner block counts by day
- `block_time_distribution` - Block time histogram
- `epoch_time_distribution` - Epoch duration histogram
- `dao_daily_snapshots` - Daily DAO metrics

**Token 24h Transfers Refresh:**

The indexer refreshes `tokens.transfers_24h` every 10 minutes via `refresh_token_24h_transfers()`. The query is optimized using a single GROUP BY scan instead of N+1 correlated subqueries:

1. Calculate block number from 24 hours ago (based on max timestamp)
2. Single scan: `GROUP BY type_script_hash` to count all transfers
3. Batch UPDATE via JOIN for active tokens
4. Reset inactive tokens to 0

This reduces query time from ~15s to <1s for 700+ tokens.

## Deferred Activities Write Optimization

For fresh database syncs, the indexer can skip writing to the `activities` table to achieve ~10-20% faster bulk sync speeds. Activities are rebuilt via task-runner after sync completes.

| Parameter                    | Default | Description                                  |
| ---------------------------- | ------- | -------------------------------------------- |
| `--defer-activities`         | `false` | Force enable deferred activities (non-fresh) |
| `--no-auto-defer-activities` | `false` | Disable auto-optimization for fresh DB       |

**What Gets Deferred:**

- All `activities` table INSERT operations during bulk sync
- Activity parsing still occurs (for token/DAO processing), just writes are skipped

**Behavior:**

| Scenario                         | Auto-defer activities | Auto-submit rebuild task |
| -------------------------------- | --------------------- | ------------------------ |
| Fresh DB (tip=0)                 | Yes                   | Yes                      |
| Fresh DB + `--no-auto-defer`     | No                    | No                       |
| Resume sync, activities exist    | No                    | No                       |
| Resume sync, activities deferred | No                    | Yes                      |
| Any DB + `--defer-activities`    | Yes                   | Yes                      |

**Activities Rebuild Task:**

When bulk sync completes, the indexer automatically submits an `activities_rebuild` task (priority 7). The task-runner:

1. Truncates the `activities` table
2. Rebuilds activities in batches (10,000 blocks per batch)
3. Currently rebuilds `CKB_TRANSFER` and `CELLBASE_REWARD` activity types
4. Updates `sync_status.activities_deferred = FALSE` on completion

**Limitations:**

The current SQL-based rebuild only handles basic activity types. Token transfers, DAO operations, and Spore activities require the full Rust ActivityParser and are not rebuilt. For complete activity coverage, avoid using `--defer-activities` or be prepared to re-sync if full activity history is needed.

**Progress Monitoring:**

- **REST API**: `GET /api/v1/tasks/active` returns `activitiesRebuild` object with status and progress
- **Task TUI**: Use `cargo run -p ckbadger-task-tui` to monitor progress

```bash
# Default: auto-defer activities for fresh DB
cargo run -p ckbadger-indexer

# Disable auto-optimization
cargo run -p ckbadger-indexer -- --no-auto-defer-activities

# Check status
psql -c "SELECT activities_deferred FROM sync_status;"
psql -c "SELECT id, task_type, status, progress_current, progress_total FROM tasks WHERE task_type = 'activities_rebuild';"
```

**Progress Monitoring:**

- **Status Page** (`/status`): Shows Index Rebuild panel with progress bar, current index, completed/total counts
- **Homepage Banner**: Amber banner appears for both pending and running rebuild tasks
- **WebSocket**: `new_block` messages include `indexRebuildStatus` field
- **REST API**: `GET /api/v1/tasks/active` returns `indexRebuild` object with status and progress
- **Task TUI**: Use `cargo run -p ckbadger-task-tui` to monitor/manage tasks

```bash
# Default: auto-optimize fresh DB, submit rebuild task when caught up
cargo run -p ckbadger-indexer

# Disable auto-optimization
cargo run -p ckbadger-indexer -- --no-auto-defer-indexes

# Check status
psql -c "SELECT indexes_deferred, indexes_dropped_at FROM sync_status;"
psql -c "SELECT id, task_type, status, progress_current, progress_total FROM tasks WHERE task_type = 'index_rebuild';"

# Monitor tasks via TUI
cargo run -p ckbadger-task-tui
```

## Deferred Address Balances Optimization

For fresh database syncs, the indexer can skip updating the `address_balances` table to achieve ~20-30% faster bulk sync speeds. Address balances are rebuilt via task-runner after sync completes.

| Parameter                          | Default | Description                                        |
| ---------------------------------- | ------- | -------------------------------------------------- |
| `--defer-address-balances`         | `false` | Force enable deferred address balances (non-fresh) |
| `--no-auto-defer-address-balances` | `false` | Disable auto-optimization for fresh DB             |

**What Gets Deferred:**

- All `address_balances` table MERGE operations during bulk sync
- The `address_balances` table remains empty until rebuild completes

**Behavior:**

| Scenario                               | Auto-defer | Auto-submit rebuild task |
| -------------------------------------- | ---------- | ------------------------ |
| Fresh DB (tip=0)                       | Yes        | Yes                      |
| Fresh DB + `--no-auto-defer`           | No         | No                       |
| Resume sync, address_balances exist    | No         | No                       |
| Resume sync, address_balances deferred | No         | Yes                      |
| Any DB + `--defer-address-balances`    | Yes        | Yes                      |

**Address Balances Rebuild Task:**

When bulk sync completes, the indexer automatically submits an `address_balances_rebuild` task (priority 8). The task-runner:

1. Truncates the `address_balances` table
2. Rebuilds from `cells` table using `GROUP BY lock_script_hash`
3. Calculates balance, live_cells_count, total_cells_count, transactions_count
4. Updates `sync_status.address_balances_deferred = FALSE` on completion

**API Fallback:**

When `address_balances_deferred = TRUE`, the API automatically falls back to querying the `cells` table directly:

- `GET /api/v1/addresses/{addr}` - Calculates balance from `SUM(capacity) WHERE status=0`
- `GET /api/v1/addresses/top` - Returns empty array during deferral

```bash
# Check status
psql -c "SELECT address_balances_deferred FROM sync_status;"
psql -c "SELECT id, task_type, status, progress_current FROM tasks WHERE task_type = 'address_balances_rebuild';"
```

## Deferred Token Tables Optimization

For fresh database syncs, the indexer can skip writing to `tokens`, `token_balances`, and `udt_cells` tables to achieve ~10-20% faster bulk sync speeds. Token data is rebuilt via task-runner after sync completes.

| Parameter               | Default | Description                              |
| ----------------------- | ------- | ---------------------------------------- |
| `--defer-token`         | `false` | Force enable deferred tokens (non-fresh) |
| `--no-auto-defer-token` | `false` | Disable auto-optimization for fresh DB   |

**What Gets Deferred:**

- All `tokens` table INSERT/UPDATE operations
- All `token_balances` table INSERT/UPDATE operations
- All `udt_cells` table INSERT/UPDATE operations
- UDT parsing still occurs (for cell data), just writes are skipped

**Behavior:**

| Scenario                     | Auto-defer | Auto-submit rebuild task |
| ---------------------------- | ---------- | ------------------------ |
| Fresh DB (tip=0)             | Yes        | Yes                      |
| Fresh DB + `--no-auto-defer` | No         | No                       |
| Resume sync, tokens exist    | No         | No                       |
| Resume sync, tokens deferred | No         | Yes                      |
| Any DB + `--defer-token`     | Yes        | Yes                      |

**Token Rebuild Task:**

When bulk sync completes, the indexer automatically submits a `token_rebuild` task (priority 7). The task-runner:

1. Truncates `udt_cells`, `token_balances`, `tokens` tables (in order)
2. Scans `cells` table for UDT type_scripts (sUDT/xUDT code hashes)
3. Parses UDT amount from cell data (first 16 bytes, u128 little-endian)
4. Rebuilds `udt_cells`, then aggregates to `tokens` and `token_balances`
5. Updates `sync_status.token_deferred = FALSE` on completion

**API Fallback:**

When `token_deferred = TRUE`, token-related API endpoints return empty data:

- `GET /api/v1/tokens` - Returns empty array
- `GET /api/v1/tokens/{id}` - Returns 404
- `GET /api/v1/addresses/{addr}/tokens` - Returns empty array

```bash
# Check status
psql -c "SELECT token_deferred FROM sync_status;"
psql -c "SELECT id, task_type, status, progress_current, progress_total FROM tasks WHERE task_type = 'token_rebuild';"
```

## Live Cell Store

The LiveCellStore provides O(1) cell lookups during blockchain synchronization using RocksDB for persistent storage. This enables instant restart without rebuilding from database.

| Parameter                    | Default             | Description                                       |
| ---------------------------- | ------------------- | ------------------------------------------------- |
| `--live-cell-db-path`        | `./data/live_cells` | RocksDB data directory                            |
| `--live-cell-flush-interval` | `100`               | Flush dirty cells to database every N batches     |
| `--no-bulk-sync-cell-cache`  | `false`             | Disable consumed cells retention during bulk sync |

**RocksDB Column Families:**

| Column Family      | Key                          | Value             | Purpose                               |
| ------------------ | ---------------------------- | ----------------- | ------------------------------------- |
| `live_cells`       | tx_hash + output_index (34B) | LiveCellInfo      | O(1) lookup for unspent cells         |
| `consumed_cells`   | tx_hash + output_index (34B) | LiveCellInfo      | Recently consumed cells (1000 blocks) |
| `block_headers`    | block_number (8B)            | CachedBlockHeader | Block header + DAO field cache        |
| `block_hash_index` | block_hash (32B)             | block_number (8B) | Reverse lookup: hash → number         |

**Cache Lookup Order:**

1. `get_cells_info_batch(bulk_sync_mode)`: live_cells → consumed_cells → PostgreSQL (skipped if bulk_sync_mode=true)
2. `get_cells_code_hashes_batch(bulk_sync_mode)`: live_cells → consumed_cells → PostgreSQL (skipped if bulk_sync_mode=true)
3. `get_block_dao_field()`: block_headers → PostgreSQL
4. `get_block_number_by_hash()`: block_hash_index → PostgreSQL

**Behavior:**

- **Bulk Sync Mode** (>1000 blocks behind tip): Skips ALL `live_cells` table operations (INSERT/DELETE), writing only to RocksDB for maximum throughput. The `cells` table still receives writes. Additionally, `get_cells_info_batch` and `get_cells_code_hashes_batch` skip PostgreSQL fallback queries when RocksDB is enabled, avoiding expensive partition scans that return 0 rows.
- **Bulk Sync Cell Cache** (default enabled): Retains ALL consumed cells in RocksDB during bulk sync, eliminating PostgreSQL fallback queries. Requires ~15GB extra memory for full chain sync. Disabled automatically on sync completion.
- **Consumed Cell Cache**: When a cell is spent, its info is preserved in `consumed_cells` CF for 1000 blocks (or indefinitely during bulk sync if cell cache enabled).
- **Block Header Cache**: Automatically populated when blocks are written; enables O(1) DAO field lookups.
- **Instant Recovery**: Data persisted to disk, indexer restarts in seconds instead of minutes
- **Graceful Shutdown**: RocksDB data is flushed on shutdown

**Memory Considerations:**

| Machine RAM | Bulk Sync Cell Cache | Expected Usage |
| ----------- | -------------------- | -------------- |
| ≥32GB       | Enabled (default)    | ~22GB peak     |
| <32GB       | Disable recommended  | ~8GB peak      |

```bash
# For low-memory machines
cargo run -p ckbadger-indexer -- --no-bulk-sync-cell-cache
```

**Example Usage:**

```bash
# Default: uses ./data/live_cells
cargo run -p ckbadger-indexer

# Custom path
cargo run -p ckbadger-indexer -- --live-cell-db-path /ssd/live_cells
```

### Live Cells Populate Task

During bulk sync, the indexer skips writing to the PostgreSQL `live_cells` table for performance (writes only to RocksDB). After bulk sync completes, the `live_cells_populate` task copies all live cells from RocksDB to PostgreSQL.

**Why Indexer Executes (not task-runner):**

- Requires direct access to RocksDB live cell store
- Task-runner rejects this task type with error message

**Execution Flow:**

1. Task submitted when bulk sync completes (priority 8)
2. Indexer checks for pending task during pipeline idle time
3. Claims task with `FOR UPDATE SKIP LOCKED`
4. Truncates PostgreSQL `live_cells` table
5. Iterates RocksDB in batches (default 100,000 cells)
6. Writes batches to PostgreSQL using COPY protocol
7. Updates task progress every 5 seconds
8. Marks task completed with `cells_populated` count

**Configuration:**

```rust
LiveCellsPopulateConfig {
    batch_size: 100_000,  // Cells per batch
}
```

**Monitoring:**

```bash
# Check task status
psql -c "SELECT id, status, progress_current, progress_total, rate_ema FROM tasks WHERE task_type = 'live_cells_populate';"

# Monitor via TUI
cargo run -p ckbadger-task-tui
```

## Rust Style

**Imports**: External → internal → stdlib inline:

```rust
use axum::{extract::State, routing::get, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult};
```

**Naming**: `PascalCase` types, `snake_case` functions, `SCREAMING_SNAKE_CASE` constants

**Error Handling**: Indexer uses `anyhow::Result`; API uses `ApiResult<T>` with `ApiError::{not_found, bad_request, internal}()`

**Serde**: Always `#[serde(rename_all = "camelCase")]` for response structs

**API Handler Pattern**:

```rust
async fn get_block(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<BlockResponse> {
    let row = sqlx::query_as::<_, (i64, Vec<u8>, ...)>("SELECT ...")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    match row {
        Some(data) => ok(BlockResponse { ... }),
        None => Err(ApiError::not_found("Block not found")),
    }
}
```

**Routes** (Axum 0.8 uses `{id}` not `:id`):

```rust
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/blocks", get(list_blocks))
        .route("/blocks/{id}", get(get_block))
}
```

## TypeScript/React Style

**Prettier**: semi, singleQuote, tabWidth 2, printWidth 100, trailingComma es5

**Imports**: Always `@/` path alias (not relative):

```typescript
import { cn } from '@/lib/utils';
import { api } from '@/lib/api';
```

**Components**: `'use client'` for interactivity, named exports, Props interface:

```typescript
'use client';

interface HashProps { hash: string; truncate?: boolean; }
export function Hash({ hash, truncate = true }: HashProps) { ... }
```

**Data Fetching**: TanStack Query v5:

```typescript
const { data, isLoading } = useQuery({
  queryKey: ['blocks', page],
  queryFn: () => api.getBlocks({ page }),
});
```

## Key Workflows

### Adding API Endpoint

1. Handler in `crates/api/src/routes/{resource}.rs`
2. Add to module's `routes()`, merge in `mod.rs`
3. TypeScript types + method in `frontend/lib/api.ts`

### Database Changes

1. Edit `migrations/postgres/001_init.sql` directly (single consolidated schema)
2. Update `crates/indexer/src/parser/` and `db/writer.rs`
3. Update API queries in `crates/api/src/routes/`

## Testing Requirements (MANDATORY)

**Every code change MUST include appropriate test coverage. No exceptions.**

### When to Add/Modify Tests

| Change Type            | Required Action                                                      |
| ---------------------- | -------------------------------------------------------------------- |
| New parser function    | Add unit test in same file's `#[cfg(test)]` module                   |
| New API endpoint       | Add test case in `crates/api/tests/api_integration.rs`               |
| New frontend component | Add test in `frontend/__tests__/components/`                         |
| New hook/util function | Add test in `frontend/__tests__/hooks/` or `frontend/__tests__/lib/` |
| Bug fix                | Add regression test that reproduces the bug FIRST, then fix          |
| Refactoring            | Run existing tests BEFORE and AFTER to ensure no regression          |

### Test Patterns by Layer

**Rust Parsers** (inline `#[cfg(test)]`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_something() {
        let input = "...";
        let result = parse_something(input);
        assert_eq!(result.field, expected_value);
    }
}
```

**API Integration** (`crates/api/tests/`):

```rust
#[sqlx::test]
async fn test_get_block_by_hash(pool: PgPool) {
    // Setup test data
    // Call endpoint
    // Assert response
}
```

**Frontend Components** (Vitest + React Testing Library):

```typescript
import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { MyComponent } from '@/components/my-component';

describe('MyComponent', () => {
  it('renders correctly', () => {
    render(<MyComponent prop="value" />);
    expect(screen.getByText('expected')).toBeInTheDocument();
  });
});
```

### Verification Checklist (Before Marking Task Complete)

1. **New code**: `cargo test` / `pnpm test` passes
2. **Modified code**: Related tests still pass
3. **Bug fix**: Regression test added and passes
4. **Coverage**: New functions have at least one test covering happy path

### Test Commands Quick Reference

```bash
# Run tests for changed code only
cargo test test_name_pattern           # Rust - partial match
cd frontend && npx vitest run -t "pattern"  # Frontend - pattern match

# Verify no regressions
cargo test -p ckbadger-indexer         # Test specific crate
cd frontend && pnpm test               # All frontend tests
```

### Anti-Patterns (FORBIDDEN)

| Violation                                           | Why It's Bad                         |
| --------------------------------------------------- | ------------------------------------ |
| Skipping tests for "simple" changes                 | Simple bugs cause production outages |
| Deleting/modifying tests to make them pass          | Hides real bugs                      |
| Writing tests that don't assert anything meaningful | False confidence                     |
| Ignoring test failures with `#[ignore]` / `.skip()` | Technical debt accumulates           |
| Mocking everything (no integration tests)           | Misses interface contract bugs       |

## CKB Domain (CRITICAL)

**BEFORE making changes to CKB-related code, READ the relevant documentation:**

| Topic            | Document                   | Must Read Before                   |
| ---------------- | -------------------------- | ---------------------------------- |
| **Worldview**    | `docs/WORLD_VIEW.md`       | **Any design or implementation**   |
| **Activities**   | `docs/ACTIVITIES.md`       | Activity parsing or API changes    |
| CKB protocol     | `docs/rfcs/`               | Understanding CKB internals        |
| Nervos docs      | `docs/docs.nervos.org/`    | User-facing explanations           |
| DAO, APC, Supply | `docs/DAO_CALCULATIONS.md` | Any DAO/supply/circulation changes |

### Common Knowledge (CKB Core Concept)

**Common Knowledge** refers to state verified by global consensus and accepted by all in the network. The set of all live cells represents the current common knowledge on CKB.

**Common Knowledge Size** = Total occupied capacity of all live cells (NOT just cell data bytes).

A cell's **occupied capacity** includes ALL storage requirements:

| Component      | Size                                  |
| -------------- | ------------------------------------- |
| Capacity field | 8 bytes                               |
| Lock script    | 32 (code_hash) + 1 (hash_type) + args |
| Type script    | 32 (code_hash) + 1 (hash_type) + args |
| Data           | Actual data bytes                     |

**Source**: The `U` field in the DAO header (`dao[24..32]`) stores the cumulative occupied capacity in shannons.

```rust
// DAO field structure (32 bytes, little-endian u64s):
// [0..8]   C = total issuance
// [8..16]  AR = accumulated rate
// [16..24] S = secondary pool (unissued)
// [24..32] U = total occupied capacity  <-- Common Knowledge Size
```

**Official Explorer Formula** (for reference):

```ruby
knowledge_size = dao.U - (BURN_QUOTA * 0.6)
# Where BURN_QUOTA = 8,400,000,000 CKB (genesis burnt tokens)
```

**IMPORTANT**: Do NOT confuse:

- `cell.data.len()` = Only the data field bytes
- `occupied_capacity` = Full storage cost (capacity + scripts + data)
- `U` field = Protocol-level cumulative occupied capacity

**Key domain knowledge in `docs/DAO_CALCULATIONS.md`:**

- Genesis issued 33.6B but only 25.2B circulating (8.4B burnt)
- `total_issuance` (dao field) ≠ `circulating` (subtract burnt)
- APC formula: `secondary_issuance_per_year / circulating_supply * 100`
- When to use `total_issuance` vs `circulating` for different calculations

### Numerical Precision (MANDATORY)

**All numerical calculations MUST be exact. NO estimation, interpolation, or sampling-based approximations.**

| Approach                     | Status       | Example                                   |
| ---------------------------- | ------------ | ----------------------------------------- |
| Exact per-block calculation  | ✅ REQUIRED  | Track every block's secondary issuance    |
| Sampling with multiplication | ❌ FORBIDDEN | Sample every N blocks, multiply by N      |
| Interpolation between points | ❌ FORBIDDEN | Estimate values between known data points |
| Average-based estimation     | ❌ FORBIDDEN | Use average rate × time period            |

**Why this matters:**

- Blockchain data is deterministic - there's always an exact value
- Sampling errors compound over millions of blocks
- Users expect explorer data to match on-chain reality exactly
- Small per-block errors become massive cumulative errors (e.g., 50x under-reporting)

**If exact calculation is expensive (e.g., RPC calls for every block):**

1. Defer during bulk sync, backfill via task-runner after sync completes
2. Use cumulative on-chain values (e.g., DAO field differences) instead of per-block sampling
3. Never sacrifice accuracy for performance - correctness is non-negotiable

### Script Identification

```rust
// code_hash = script TYPE (what kind), script_hash = script INSTANCE (unique)
// CORRECT: Compare code_hash for type detection
let code_hash = parse_hex_to_bytes(&type_script.code_hash);
DaoParser::is_dao_code_hash(&code_hash)
// WRONG: Computing full script_hash then comparing to code_hash
```

### DAO Constants

```rust
const DAO_CODE_HASH: &str = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
const DAO_OCCUPIED_CAPACITY: u64 = 102_00000000; // 102 CKB
// DAO field: bytes 8-15 = AR (accumulated rate, u64 LE)
// Compensation: free_capacity * ar_withdraw / ar_deposit - free_capacity
```

### DAO Lifecycle

1. **Deposit**: Creates DAO cell → `dao_deposits(tx_hash=deposit_tx)`
2. **Withdraw Request**: Consumes deposit → set `withdraw_request_tx`
3. **Withdraw Completion**: Lookup by `withdraw_request_tx` (NOT request cell's tx_hash)

## Gotchas

| Issue                         | Solution                                                          |
| ----------------------------- | ----------------------------------------------------------------- |
| SQLx NOT compile-time checked | Using `query_as` with tuples - verify SQL manually                |
| Hex parsing                   | Use `parse_hex_to_bytes()`, `parse_capacity()` in `rpc/client.rs` |
| Script hashing                | `ckb-hash::new_blake2b()` with CKB personalization                |
| WebSocket Text (Axum 0.8)     | Needs `Utf8Bytes` - use `.into()` from String                     |
| react-force-graph-2d          | No SSR - `next/dynamic` with `ssr: false`                         |
| API casing                    | Backend `camelCase` via serde, frontend types match               |
| SQL aggregates                | Cast explicitly: `::float8`, `::numeric`                          |
| FK constraints                | Insert parents before children                                    |
| Daily charts                  | Exclude incomplete current day                                    |
| Next.js standalone            | Monorepo path: `.next/standalone/frontend/`                       |
| Docker + host CKB             | Use `network_mode: host`                                          |
| Vitest globals                | Add `vitest/globals` to tsconfig types                            |
| MSW handlers                  | Must start server in setup.ts `beforeAll`                         |
| sqlx::test                    | Requires `MIGRATOR` constant in lib.rs                            |
| Spore molecule `Bytes`        | Size field = content length (NOT total size including header)     |

## File Locations

| What                | Where                                      |
| ------------------- | ------------------------------------------ |
| API routes          | `crates/api/src/routes/*.rs`               |
| Activities API      | `crates/api/src/routes/activities.rs`      |
| Response types      | `crates/api/src/response.rs`               |
| WebSocket           | `crates/api/src/ws/`                       |
| RPC client          | `crates/indexer/src/rpc/client.rs`         |
| Parsers             | `crates/indexer/src/parser/*.rs`           |
| Activity parser     | `crates/indexer/src/parser/activity.rs`    |
| Activity writer     | `crates/indexer/src/db/copy_activities.rs` |
| DB writes           | `crates/indexer/src/db/writer.rs`          |
| Activity types      | `crates/common/src/activity.rs`            |
| Spore parser        | `crates/indexer/src/parser/spore.rs`       |
| Spore writer        | `crates/indexer/src/db/writer/spore.rs`    |
| Frontend API        | `frontend/lib/api.ts`                      |
| UI components       | `frontend/components/ui/`                  |
| Activity components | `frontend/components/activity/`            |
| Pages               | `frontend/app/`                            |
| Rust tests          | Inline `#[cfg(test)]` in parser files      |
| API integration     | `crates/api/tests/api_integration.rs`      |
| Frontend tests      | `frontend/__tests__/**/*.test.{ts,tsx}`    |
| MSW handlers        | `frontend/__tests__/msw/handlers.ts`       |
| E2E tests           | `e2e/*.spec.ts`                            |
| CI workflow         | `.github/workflows/ci.yml`                 |

## Dependencies

**Rust**: axum 0.8, sqlx 0.8, tokio 1.42, serde, ckb-types/ckb-hash 0.119, anyhow/thiserror
**Frontend**: next 15.1, react 19, @tanstack/react-query 5, zustand 5, tailwindcss 3.4
