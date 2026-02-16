# CLAUDE.md

Instructions for AI agents working on ckbadger - a CKB blockchain explorer.

## Development Status (IMPORTANT)

**This is a project under active development, NOT running in production.**

- Database can be cleared and rebuilt at any time
- Data can be re-synced from scratch whenever needed
- Schema changes are cheap — no migration compatibility concerns

**Design Implications:**

When solving problems or designing features:

1. **Prefer optimal data design** over backward compatibility
2. **Feel free to restructure column families** if it produces a cleaner solution
3. **Breaking changes are acceptable** — just update the store types/ops in `crates/ckbadger-store/`
4. **If a bug fix requires storage change**, do it properly rather than working around bad structure
5. **Re-sync is always an option** — don't let existing data constrain the right solution

```bash
# Typical workflow after storage changes:
# 1. Update types/ops in crates/ckbadger-store/src/
# 2. Update indexer writer code in crates/indexer/src/db/writer/
# 3. Delete RocksDB data directory
# 4. Re-run indexer to sync from genesis
```

## Commands

```bash
# Rust
cargo check                              # Type check all crates
cargo build -p ckbadger-api              # Build specific crate
cargo clippy                             # Lint

# Rust Testing (533 tests across all crates)
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
  api/            # Axum REST/WebSocket server (port 3001)
  indexer/        # Blockchain sync daemon (three-stage pipeline)
  ckbadger-store/ # Embedded RocksDB storage engine (25 column families)
  common/         # Shared types (block, cell, tx, script, error)
  task-runner/    # Background task executor
  task-tui/       # Terminal UI for task management
frontend/         # Next.js 15 App Router + React 19
docs/POSTMORTEM.md                # Historical bugs - READ BEFORE CKB/DAO WORK
docs/INDEXER_PIPELINE.md          # Pipeline architecture documentation
```

## Indexer Pipeline Configuration

The indexer uses a three-stage pipeline: **Fetcher** (RPC I/O) → **Parser** (CPU + DB prefetch) → **Writer** (DB I/O).

| Parameter             | Default | Description                          |
| --------------------- | ------- | ------------------------------------ |
| `pipeline_enabled`    | `true`  | Enable pipeline mode (vs sequential) |
| `pipeline_buffer`     | `8`     | Channel capacity between stages      |
| `batch_size`          | `10000` | Blocks per batch                     |
| `parallel_fetch_size` | `64`    | Concurrent RPC requests              |

```bash
# CLI arguments
cargo run -p ckbadger-indexer -- \
  --pipeline-enabled \
  --pipeline-buffer 4 \
  --batch-size 10000

# Environment variables
PIPELINE_ENABLED=true
PIPELINE_BUFFER=4
BATCH_SIZE=10000
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

| Data                 | Fallback Source                        |
| -------------------- | -------------------------------------- |
| `tip_block_number`   | `store.get_sync_tip()` from RocksDB    |
| `total_transactions` | `store.get_sync_status()` from RocksDB |
| `total_live_cells`   | `store.get_sync_status()` from RocksDB |
| `sync_ema_rate`      | None (ETA not displayed)               |

**Requires**: `redis-cache` feature enabled on both indexer and API, plus `REDIS_URL` environment variable.

## Deferred Write Optimization

For fresh syncs, the indexer defers certain non-critical writes to RocksDB during bulk sync to maximize throughput. Deferred data is maintained inline by the indexer (no separate rebuild needed) since RocksDB writes are fast enough.

**Note:** Deferred states are stored in `sync_status` within the RocksDB store. The indexer reads these flags on startup.

**Available Task Types:**

| Task Type                  | Priority | Description                                     |
| -------------------------- | -------- | ----------------------------------------------- |
| `label_import`             | 0        | Import UDT/script labels from token-labels repo |
| `statistics_rebuild`       | 5        | No-op (statistics maintained inline by indexer) |
| `token_rebuild`            | 7        | No-op (tokens maintained inline by indexer)     |
| `spore_rebuild`            | 6        | No-op (spore data maintained inline by indexer) |
| `address_balances_rebuild` | 8        | No-op (balances maintained inline by indexer)   |

> **Note:** Most rebuild tasks are now no-ops because the RocksDB indexer maintains all data inline during sync. Only `label_import` performs actual work.

**Bulk Sync Protection:**

Tasks that require complete blockchain data are automatically deferred during bulk sync. The task-runner checks sync status before executing each task.

**Label Import Auto-Trigger:**

The `label_import` task is automatically submitted when the indexer starts, if:

1. Token labels directory exists (checks `$TOKEN_LABELS_PATH/information/` or `docs/token-labels/information/`)
2. No pending/running `label_import` task already exists

The path is determined by `TOKEN_LABELS_PATH` environment variable, defaulting to `docs/token-labels` for local development. In Docker, this is set to `/app/token-labels` with a volume mount.

**Progress Monitoring:**

- **REST API**: `GET /api/v1/tasks/active` returns task status and progress
- **Task TUI**: Use `cargo run -p ckbadger-task-tui` to monitor/manage tasks

## ckbadger-store (Embedded Storage Engine)

All data is stored in a single RocksDB instance (`ckbadger-store` crate) with 25 column families. The indexer opens it read-write; the API opens a secondary (read-only) instance.

| Parameter            | Default                 | Description            |
| -------------------- | ----------------------- | ---------------------- |
| `CKBADGER_DATA_PATH` | `./data/ckbadger-store` | RocksDB data directory |

**Key Column Families:**

| Column Family      | Key                          | Value                | Purpose                          |
| ------------------ | ---------------------------- | -------------------- | -------------------------------- |
| `live_cells`       | tx_hash + output_index (34B) | LiveCellInfo         | O(1) lookup for unspent cells    |
| `consumed_cells`   | tx_hash + output_index (34B) | LiveCellInfo         | Recently consumed cells          |
| `block_headers`    | block_number (8B)            | CachedBlockHeader    | Block header + DAO field cache   |
| `block_hash_index` | block_hash (32B)             | block_number (8B)    | Reverse lookup: hash → number    |
| `dao_deposits`     | tx_hash + output_index (34B) | DaoDepositCacheEntry | DAO deposit lifecycle cache      |
| `tasks`            | task UUID (16B)              | TaskEntry            | Background task state            |
| `sync_status`      | fixed key                    | SyncStatus           | Sync progress and deferred flags |
| `addr_balance`     | lock_script_hash (32B)       | AddressBalance       | Address balance and cell counts  |
| `tokens`           | type_script_hash (32B)       | TokenInfo            | UDT token metadata               |
| `daily_stats`      | date string                  | DailyStats           | Daily aggregate statistics       |

**Key Design:**

- `CkbadgerStore::open(path)` — primary read-write mode for indexer and task-runner
- `CkbadgerStore::open_secondary(path)` — read-only mode for API
- All store operations are synchronous (RocksDB reads are fast)
- `TaskEntry` (store type) converts to `Task` (common type) via `task_entry_to_task()`

**Memory Considerations:**

| Machine RAM | Expected Usage |
| ----------- | -------------- |
| ≥32GB       | ~22GB peak     |
| <32GB       | ~8GB peak      |

```bash
# Default: uses ./data/ckbadger-store
cargo run -p ckbadger-indexer

# Custom path
CKBADGER_DATA_PATH=/ssd/ckbadger-store cargo run -p ckbadger-indexer
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
    let block = state.store.get_block_header(block_num)
        .ok_or_else(|| ApiError::not_found("Block not found"))?;
    ok(BlockResponse { ... })
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

### Storage Changes

1. Update types/ops in `crates/ckbadger-store/src/` (column families, key encoding, value types)
2. Update `crates/indexer/src/db/writer/` for write path changes
3. Update store method calls in `crates/api/src/routes/`

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
#[tokio::test]
async fn test_get_block_by_hash() {
    let store = CkbadgerStore::open_temp().unwrap();
    // Setup test data in store
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
// [16..24] S = cumulative non-miner secondary issuance (depositor + treasury)
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

| Issue                     | Solution                                                          |
| ------------------------- | ----------------------------------------------------------------- |
| Hex parsing               | Use `parse_hex_to_bytes()`, `parse_capacity()` in `rpc/client.rs` |
| Script hashing            | `ckb-hash::new_blake2b()` with CKB personalization                |
| WebSocket Text (Axum 0.8) | Needs `Utf8Bytes` - use `.into()` from String                     |
| react-force-graph-2d      | No SSR - `next/dynamic` with `ssr: false`                         |
| API casing                | Backend `camelCase` via serde, frontend types match               |
| Daily charts              | Exclude incomplete current day                                    |
| Next.js standalone        | Monorepo path: `.next/standalone/frontend/`                       |
| Docker + host CKB         | Use `network_mode: host`                                          |
| Vitest globals            | Add `vitest/globals` to tsconfig types                            |
| MSW handlers              | Must start server in setup.ts `beforeAll`                         |
| RocksDB secondary mode    | API uses `open_secondary()` — read-only, no write locks           |
| Spore molecule `Bytes`    | Size field = content length (NOT total size including header)     |

## File Locations

| What             | Where                                   |
| ---------------- | --------------------------------------- |
| Storage engine   | `crates/ckbadger-store/src/`            |
| Store types      | `crates/ckbadger-store/src/types.rs`    |
| Store operations | `crates/ckbadger-store/src/*_ops.rs`    |
| API routes       | `crates/api/src/routes/*.rs`            |
| Response types   | `crates/api/src/response.rs`            |
| WebSocket        | `crates/api/src/ws/`                    |
| RPC client       | `crates/indexer/src/rpc/client.rs`      |
| Parsers          | `crates/indexer/src/parser/*.rs`        |
| DB writers       | `crates/indexer/src/db/writer/*.rs`     |
| Spore writer     | `crates/indexer/src/db/writer/spore.rs` |
| Task runner      | `crates/task-runner/src/executor/*.rs`  |
| Frontend API     | `frontend/lib/api.ts`                   |
| UI components    | `frontend/components/ui/`               |
| Pages            | `frontend/app/`                         |
| Rust tests       | Inline `#[cfg(test)]` in source files   |
| API integration  | `crates/api/tests/api_integration.rs`   |
| Frontend tests   | `frontend/__tests__/**/*.test.{ts,tsx}` |
| MSW handlers     | `frontend/__tests__/msw/handlers.ts`    |
| E2E tests        | `e2e/*.spec.ts`                         |
| CI workflow      | `.github/workflows/ci.yml`              |

## Dependencies

**Rust**: axum 0.8, rocksdb, tokio 1.42, serde, ckb-types/ckb-hash 0.119, anyhow/thiserror
**Frontend**: next 15.1, react 19, @tanstack/react-query 5, zustand 5, tailwindcss 3.4
