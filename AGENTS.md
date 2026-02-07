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
3. **Breaking changes are acceptable** — just update `migrations/clickhouse/001_init.sql`
4. **If a bug fix requires schema change**, do it properly rather than working around bad structure
5. **Re-sync is always an option** — don't let existing data constrain the right solution

```bash
# Typical workflow after schema changes:
# 1. Edit migrations/clickhouse/001_init.sql
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

# Performance Testing
cargo bench -p ckbadger-indexer          # Run Criterion benchmarks
cargo bench -p ckbadger-indexer -- cache # Run specific benchmark group
k6 run perf/k6/smoke-test.js             # API smoke test (requires k6)

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
migrations/clickhouse/001_init.sql  # ClickHouse schema
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

**`memory:stats`** - LRU cache memory usage:

```rust
pub struct MemoryStatsData {
    pub cell_cache_count: u64,           // Cells in LRU cache
    pub cell_cache_capacity: u64,        // LRU cache capacity
    pub bulk_sync_mode: bool,            // Currently in bulk sync
    pub updated_at: i64,                 // Unix timestamp
}
```

**Data Flow**:

1. Indexer updates `sync:status` after each batch write
2. Indexer updates `sync:progress` every 10 seconds with ETA
3. Indexer updates `memory:stats` every 10 seconds with cache stats
4. API reads `sync:status` for totals (blocks, transactions, cells)
5. API reads `sync:progress` for real-time progress display
6. WebSocket broadcaster uses both for `new_block` messages

**Fallback** (when Redis unavailable):

| Data                 | Fallback Query                      |
| -------------------- | ----------------------------------- |
| `tip_block_number`   | `SELECT MAX(number) FROM blocks`    |
| `total_transactions` | `SELECT COUNT(*) FROM transactions` |
| `total_live_cells`   | `SELECT COUNT(*) FROM live_cells`   |
| `sync_ema_rate`      | None (ETA not displayed)            |

**Requires**: `redis-cache` feature enabled on both indexer and API, plus `REDIS_URL` environment variable.

## ClickHouse Architecture

The indexer uses ClickHouse as the sole data store with the following design principles:

1. **Immutable Fact Tables**: All blockchain data (blocks, transactions, cells) is append-only
2. **Versioned Canonical Mapping**: `canonical_blocks` table tracks the current chain with monotonic versions
3. **Versioned Cell State**: `cell_state` table tracks live/consumed cells with version history
4. **LRU Cell Cache**: In-memory cache (~1M entries) for O(1) cell lookups during sync

### Table Categories

| Category              | Tables                                                                          | Engine             | Purpose                               |
| --------------------- | ------------------------------------------------------------------------------- | ------------------ | ------------------------------------- |
| **Immutable Facts**   | blocks_all, transactions_all, cell_outputs_all, cell_inputs_all, activities_all | MergeTree          | Store ALL data (canonical + orphaned) |
| **Canonical Mapping** | canonical_blocks                                                                | ReplacingMergeTree | Track current canonical chain         |
| **State Snapshots**   | cell_state, dao_deposits                                                        | ReplacingMergeTree | Track current cell/deposit state      |

### Sync Performance

| Parameter    | Default | Description      |
| ------------ | ------- | ---------------- |
| `batch_size` | `10000` | Blocks per batch |

Current sync speed: ~2,500-3,000 blocks/sec.

### Bulk Sync Mode

When >1000 blocks behind tip:

- Activities are written normally
- All data goes to ClickHouse (no deferred writes)
- LRU cache handles cell lookups

### Reorg Handling

Reorgs are handled by versioning, not deletion:

1. Orphaned blocks remain in `blocks_all` (preserved for analysis)
2. New `canonical_blocks` entries are written with higher `canon_version`
3. `cell_state` entries are versioned to reflect the new canonical state
4. Queries use `ORDER BY canon_version DESC LIMIT 1 BY (key)` pattern

### Canonical Blocks Query Pattern (IMPORTANT)

When joining with `canonical_blocks` (ReplacingMergeTree), **ALWAYS use FINAL**:

```sql
-- CORRECT: Use FINAL to get deduplicated results
SELECT ... FROM transactions_all t
INNER JOIN canonical_blocks FINAL c ON t.block_number = c.number AND t.block_hash = c.block_hash

-- WRONG: May return duplicate rows during merge
SELECT ... FROM transactions_all t
INNER JOIN canonical_blocks c ON t.block_number = c.number AND t.block_hash = c.block_hash
```

**Why FINAL is required:**

- `canonical_blocks` uses `ReplacingMergeTree(canon_version)` for reorg handling
- Without FINAL, queries may see multiple versions of the same block number
- FINAL forces ClickHouse to return only the latest version per key
- Performance impact is negligible since `canonical_blocks` is small (~18M rows, <1ms overhead)

## Deferred Activities Write Optimization

For fresh database syncs, the indexer can skip writing to the `activities` table to achieve ~10-20% faster bulk sync speeds. Activities are rebuilt via task-runner after sync completes.

| Parameter                    | Default | Description                            |
| ---------------------------- | ------- | -------------------------------------- |
| `--no-auto-defer-activities` | `false` | Disable auto-optimization for fresh DB |

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

**Activities Rebuild Task:**

When bulk sync completes, the indexer automatically submits an `activities_rebuild` task (priority 7). The task-runner:

1. Truncates the `activities` table
2. Rebuilds activities in batches (10,000 blocks per batch)
3. Currently rebuilds `CKB_TRANSFER` and `CELLBASE_REWARD` activity types
4. Updates `sync_status.activities_deferred = FALSE` on completion

**Limitations:**

The current SQL-based rebuild only handles basic activity types. Token transfers, DAO operations, and Spore activities require the full Rust ActivityParser and are not rebuilt. For complete activity coverage, use `--no-auto-defer-activities` on fresh DB or be prepared to re-sync if full activity history is needed.

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

| Parameter                          | Default | Description                            |
| ---------------------------------- | ------- | -------------------------------------- |
| `--no-auto-defer-address-balances` | `false` | Disable auto-optimization for fresh DB |

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

| Parameter               | Default | Description                            |
| ----------------------- | ------- | -------------------------------------- |
| `--no-auto-defer-token` | `false` | Disable auto-optimization for fresh DB |

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

## Deferred Spore Tables Optimization

For fresh database syncs, the indexer can skip writing to `spore_clusters` and `spore_cells` tables to achieve ~10-15% faster bulk sync speeds. Spore data is rebuilt via task-runner after sync completes.

| Parameter               | Default | Description                            |
| ----------------------- | ------- | -------------------------------------- |
| `--no-auto-defer-spore` | `false` | Disable auto-optimization for fresh DB |

**What Gets Deferred:**

- All `spore_clusters` table INSERT/UPDATE operations
- All `spore_cells` table INSERT/UPDATE operations
- Spore parsing still occurs (for cell data), just writes are skipped

**Behavior:**

| Scenario                     | Auto-defer | Auto-submit rebuild task |
| ---------------------------- | ---------- | ------------------------ |
| Fresh DB (tip=0)             | Yes        | Yes                      |
| Fresh DB + `--no-auto-defer` | No         | No                       |
| Resume sync, spore exists    | No         | No                       |
| Resume sync, spore deferred  | No         | Yes                      |

**Spore Rebuild Task:**

When bulk sync completes, the indexer automatically submits a `spore_rebuild` task (priority 6). The task-runner:

1. Truncates `spore_cells`, then `spore_clusters` tables
2. Scans `cells` table for Spore type_scripts (Spore code hash)
3. Parses Spore metadata from cell data using molecule codec
4. Rebuilds `spore_cells`, then aggregates to `spore_clusters`
5. Updates `sync_status.spore_deferred = FALSE` on completion

**API Fallback:**

When `spore_deferred = TRUE`, Spore-related API endpoints return empty data:

- `GET /api/v1/spores` - Returns empty array
- `GET /api/v1/spores/{id}` - Returns 404
- `GET /api/v1/addresses/{addr}/spores` - Returns empty array

```bash
# Check status
psql -c "SELECT spore_deferred FROM sync_status;"
psql -c "SELECT id, task_type, status, progress_current, progress_total FROM tasks WHERE task_type = 'spore_rebuild';"
```

## LRU Cell Cache

The indexer uses an in-memory LRU cache for O(1) cell lookups during blockchain synchronization.

| Parameter      | Default     | Description            |
| -------------- | ----------- | ---------------------- |
| Cache capacity | ~1M entries | Maximum cells in cache |
| Memory usage   | ~200MB      | Approximate RAM usage  |

**Cache Behavior:**

- **On cell creation**: Cell info added to cache
- **On cell consumption**: Cell info retrieved from cache (or ClickHouse if miss)
- **Cache eviction**: LRU (Least Recently Used) policy

**Lookup Order:**

1. Check LRU cache (O(1))
2. If miss, batch query ClickHouse `cell_state` table
3. Add result to cache for future lookups

**Memory Considerations:**

| Machine RAM | Expected Usage                  |
| ----------- | ------------------------------- |
| ≥8GB        | Comfortable                     |
| <8GB        | May need smaller cache capacity |

## API Statistics Caching

The API uses Redis caching to reduce ClickHouse query load for statistics endpoints.

**Cache Configuration:**

| Endpoint Type      | Cache Key Prefix      | TTL    | Description                     |
| ------------------ | --------------------- | ------ | ------------------------------- |
| Network stats      | `stats:network`       | 5 sec  | Network stats (hash rate, etc)  |
| Transaction stats  | `stats:tx_stats`      | 1 min  | Hourly/daily transaction counts |
| Recent blocks      | `stats:recent_blocks` | 10 sec | Latest 10 blocks                |
| Chart data         | `chart:*`             | 5 min  | Historical charts (30 days)     |
| Transaction list   | `transactions:list`   | 5 sec  | First page of transactions      |
| Transaction detail | `tx:detail`           | 60 sec | Individual transaction pages    |
| Block list         | `blocks:list`         | 5 sec  | First page of blocks            |
| Block detail       | `block:detail`        | 60 sec | Individual block pages          |

**Cached Endpoints:**

- `GET /statistics/network` - Network stats (cached 5s, reduces RPC calls)
- `GET /statistics/tx-stats` - Transaction count charts
- `GET /statistics/recent-blocks` - Recent blocks list
- `GET /charts/transaction-count` - Daily transaction counts
- `GET /charts/cell-count` - Daily cell creation counts
- `GET /charts/average-block-time` - Daily average block time
- `GET /charts/hash-rate` - Daily hash rate

**Mempool Summary Endpoint:**

- `GET /mempool/summary` - Combined endpoint for homepage ChainWave visualization
  - Returns pending transactions, proposals, tip block, and tip block transactions
  - Reduces 4 API calls to 1 for the ChainWave component

**Cache Warmup:**

On API startup, all chart caches are pre-populated via `warmup::warmup_chart_caches()`. This runs 5 seconds after startup to ensure ClickHouse is ready.

**ClickHouse Memory Limits:**

Configured in `docker/clickhouse-users.xml`:

| Setting                     | Value | Description            |
| --------------------------- | ----- | ---------------------- |
| `max_memory_usage`          | 4GB   | Per-query memory limit |
| `max_memory_usage_for_user` | 16GB  | Total memory per user  |
| `max_concurrent_queries`    | 10    | Max concurrent queries |
| `max_execution_time`        | 60s   | Query timeout          |

**Query Optimizations:**

1. **Window functions**: Average block time uses `leadInFrame()` instead of O(n²) self-join
2. **Time range filters**: Chart queries limited to last 30 days (`- 2592000000` ms)
3. **Materialized views**: `migrations/clickhouse/002_materialized_views.sql` defines pre-aggregated views

**Materialized Views:**

| View                   | Purpose                       | Used By                 |
| ---------------------- | ----------------------------- | ----------------------- |
| `mv_daily_tx_count`    | Daily transaction counts      | Transaction count chart |
| `mv_daily_cell_count`  | Daily cell creation counts    | Cell count chart        |
| `mv_daily_block_stats` | Daily block stats (hash rate) | Hash rate/difficulty    |
| `mv_hourly_tx_count`   | Hourly transaction counts     | TX Stats (24h chart)    |
| `mv_five_min_tx_count` | 5-minute transaction counts   | TX Stats (hourly chart) |

```bash
# Build API with Redis cache support
cargo build -p ckbadger-api --features redis-cache

# Run with Redis
REDIS_URL=redis://localhost:6379 cargo run -p ckbadger-api --features redis-cache
```

## Known Scripts

The explorer maintains a database of known CKB scripts (lock scripts and type scripts) imported from the `docs/token-labels` submodule.

**Data Source:**

- Script definitions: `docs/token-labels/information/script/*/index.json`
- Name overrides: `docs/script-name-overrides.json`
- Import script: `scripts/import-known-scripts.js`

**Database Table:** `known_scripts` (ClickHouse)

| Column                   | Type            | Description                           |
| ------------------------ | --------------- | ------------------------------------- |
| `code_hash`              | FixedString(32) | Script code hash (primary identifier) |
| `network`                | String          | `mainnet` or `testnet`                |
| `name`                   | String          | Human-readable script name            |
| `script_kind`            | String          | `lock` or `type` (auto-detected)      |
| `decoder_type`           | String          | `udt`, `spore`, `dao`, `ckbfs`, etc.  |
| `deprecated`             | UInt8           | Whether the script is deprecated      |
| `is_system`              | UInt8           | Genesis/system script flag            |
| `code_cell_tx_hash`      | FixedString(32) | Transaction containing code cell      |
| `code_cell_output_index` | Int16           | Output index of code cell             |

**API Endpoints:**

| Endpoint                    | Description                             |
| --------------------------- | --------------------------------------- |
| `GET /scripts`              | List scripts (paginated, searchable)    |
| `GET /scripts/{name}`       | Get script by name (all deployments)    |
| `GET /scripts/{name}/usage` | Get usage stats (cell counts, capacity) |
| `POST /scripts/lookup`      | Batch lookup by code hashes             |
| `GET /scripts/code-cell`    | Get code cell location for a script     |

**Query Parameters for `GET /scripts`:**

| Param          | Type   | Description                            |
| -------------- | ------ | -------------------------------------- |
| `limit`        | int    | Page size (default: 20)                |
| `cursor`       | string | Pagination cursor (script name)        |
| `network`      | string | Filter by network (default: `mainnet`) |
| `decoder_type` | string | Filter by decoder type                 |
| `search`       | string | Search by name or code hash            |

**Importing/Updating Script Data:**

```bash
# Import all scripts from token-labels
node scripts/import-known-scripts.js

# Dry run (preview SQL without executing)
node scripts/import-known-scripts.js --dry-run

# Custom ClickHouse URL
node scripts/import-known-scripts.js --clickhouse-url http://localhost:8123
```

**Script Kind Detection:**

The import script auto-detects `script_kind` based on:

1. `decoder_type` - `dao`, `udt`, `spore` → `type`
2. Name patterns - `/lock/i`, `/secp256k1/i` → `lock`; `/udt/i`, `/nft/i` → `type`

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

1. Edit `migrations/clickhouse/001_init.sql` directly (ClickHouse schema)
2. Update `crates/indexer/src/parser/` and `db/writer/`
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
| Scripts API         | `crates/api/src/routes/scripts.rs`         |
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
