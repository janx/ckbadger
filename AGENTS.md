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
   - ETA uses trend-based prediction for improved accuracy (see below)

### ETA Calculation (Trend-Based)

The indexer uses linear regression on speed history to predict future sync speed:

1. **Speed History**: Records last 30 speed samples (~5 minutes of data)
2. **Trend Detection**: Uses linear regression to calculate speed change rate (slope)
3. **Segmented Prediction**: Divides remaining blocks into 10 segments, predicts speed for each
4. **Safety Clamp**: Predicted speeds are clamped to `[10%, 200%]` of current EMA

**Why this matters**: Block sizes grow over time (more transactions, more cells). Early blocks sync at ~5000 blocks/sec, recent blocks at ~1000 blocks/sec. Simple `remaining/rate` underestimates ETA when speed is declining.

**Fallback**: If insufficient trend data (< 3 samples), uses simple `remaining / EMA` calculation.

### Redis Sync Data

The indexer publishes sync data to Redis for API/WebSocket consumption:

| Key             | TTL | Contents                        |
| --------------- | --- | ------------------------------- |
| `sync:status`   | 60s | JSON: `SyncStatusData` struct   |
| `sync:progress` | 30s | JSON: `SyncProgressData` struct |

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

**Data Flow**:

1. Indexer updates `sync:status` after each batch write
2. Indexer updates `sync:progress` every 10 seconds with ETA
3. API reads `sync:status` for totals (blocks, transactions, cells)
4. API reads `sync:progress` for real-time progress display
5. WebSocket broadcaster uses both for `new_block` messages

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
3. Submits `live_cells_populate` task (priority 8) to populate PostgreSQL from RocksDB
4. Submits `statistics_rebuild` task (priority 5) to rebuild aggregate statistics
5. Task-runner picks up `index_rebuild` and `statistics_rebuild` tasks
6. Indexer executes `live_cells_populate` during idle time (requires RocksDB access)
7. Indexes rebuilt with `CREATE INDEX CONCURRENTLY`
8. Statistics tables rebuilt (daily_statistics, hourly_statistics, miner_statistics, etc.)
9. Tasks complete (status: `completed`)

**Available Task Types:**

| Task Type             | Priority | Description                                      |
| --------------------- | -------- | ------------------------------------------------ |
| `index_rebuild`       | 10       | Rebuild deferred indexes and constraints         |
| `live_cells_populate` | 8        | Populate live_cells table from RocksDB (indexer) |
| `statistics_rebuild`  | 5        | Rebuild all 7 aggregate statistics tables        |
| `cycles_backfill`     | 0        | Backfill transaction cycles from RPC             |
| `label_import`        | 0        | Import UDT/script labels from token-labels repo  |

**Label Import Auto-Trigger:**

The `label_import` task is automatically submitted when the indexer starts, if:

1. `docs/token-labels/information/` directory exists
2. No pending/running `label_import` task already exists

This ensures token labels are refreshed at least once per indexer lifecycle without manual intervention.

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

## Live Cell Store

The LiveCellStore provides O(1) cell lookups during blockchain synchronization using RocksDB for persistent storage. This enables instant restart without rebuilding from database.

| Parameter                    | Default             | Description                                   |
| ---------------------------- | ------------------- | --------------------------------------------- |
| `--live-cell-db-path`        | `./data/live_cells` | RocksDB data directory                        |
| `--live-cell-flush-interval` | `100`               | Flush dirty cells to database every N batches |

**RocksDB Column Families:**

| Column Family      | Key                          | Value             | Purpose                               |
| ------------------ | ---------------------------- | ----------------- | ------------------------------------- |
| `live_cells`       | tx_hash + output_index (34B) | LiveCellInfo      | O(1) lookup for unspent cells         |
| `consumed_cells`   | tx_hash + output_index (34B) | LiveCellInfo      | Recently consumed cells (1000 blocks) |
| `block_headers`    | block_number (8B)            | CachedBlockHeader | Block header + DAO field cache        |
| `block_hash_index` | block_hash (32B)             | block_number (8B) | Reverse lookup: hash → number         |

**Cache Lookup Order:**

1. `get_cells_info_batch()`: live_cells → consumed_cells → PostgreSQL
2. `get_block_dao_field()`: block_headers → PostgreSQL
3. `get_block_number_by_hash()`: block_hash_index → PostgreSQL

**Behavior:**

- **Bulk Sync Mode** (>1000 blocks behind tip): Skips ALL `live_cells` table operations (INSERT/DELETE), writing only to RocksDB for maximum throughput. The `cells` table still receives writes.
- **Consumed Cell Cache**: When a cell is spent, its info is preserved in `consumed_cells` CF for 1000 blocks, reducing PostgreSQL fallback queries by 10-20%.
- **Block Header Cache**: Automatically populated when blocks are written; enables O(1) DAO field lookups.
- **Instant Recovery**: Data persisted to disk, indexer restarts in seconds instead of minutes
- **Graceful Shutdown**: RocksDB data is flushed on shutdown

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

**Key domain knowledge in `docs/DAO_CALCULATIONS.md`:**

- Genesis issued 33.6B but only 25.2B circulating (8.4B burnt)
- `total_issuance` (dao field) ≠ `circulating` (subtract burnt)
- APC formula: `secondary_issuance_per_year / circulating_supply * 100`
- When to use `total_issuance` vs `circulating` for different calculations

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
