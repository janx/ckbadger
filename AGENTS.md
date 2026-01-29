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
| `pipeline_buffer`     | `16`    | Channel capacity between stages      |
| `batch_size`          | `10000` | Blocks per batch                     |
| `parallel_fetch_size` | `64`    | Concurrent RPC requests              |
| `copy_pool_size`      | `24`    | Parallel COPY connections            |

```bash
# CLI arguments
cargo run -p ckbadger-indexer -- \
  --pipeline-enabled \
  --pipeline-buffer 16 \
  --batch-size 10000 \
  --copy-pool-size 24

# Environment variables
PIPELINE_ENABLED=true
PIPELINE_BUFFER=16
BATCH_SIZE=10000
COPY_POOL_SIZE=24
```

See `docs/INDEXER_PIPELINE.md` for architecture details.

## Deferred Index Optimization

For fresh database syncs, the indexer automatically drops non-essential B-tree indexes to achieve ~3x faster write speeds. Indexes are rebuilt automatically when the sync catches up to the chain tip.

| Parameter                  | Default | Description                                      |
| -------------------------- | ------- | ------------------------------------------------ |
| `--defer-indexes`          | `false` | Force enable deferred indexes (for non-fresh DB) |
| `--no-auto-defer-indexes`  | `false` | Disable auto-optimization for fresh DB           |
| `--rebuild-indexes-only`   | `false` | Only rebuild indexes, don't sync                 |
| `--index-rebuild-parallel` | `10`    | Parallel connections per partitioned table       |

**Behavior:**

| Scenario                      | Auto-drop indexes | Auto-rebuild |
| ----------------------------- | ----------------- | ------------ |
| Fresh DB (tip=0)              | Yes               | Yes          |
| Fresh DB + `--no-auto-defer`  | No                | No           |
| Resume sync, indexes exist    | No                | No           |
| Resume sync, indexes deferred | No                | Yes          |
| Any DB + `--defer-indexes`    | Yes               | Yes          |

```bash
# Default: auto-optimize fresh DB, rebuild when caught up
cargo run -p ckbadger-indexer

# Disable auto-optimization
cargo run -p ckbadger-indexer -- --no-auto-defer-indexes

# Manual index rebuild only
cargo run -p ckbadger-indexer -- --rebuild-indexes-only

# Check status
psql -c "SELECT indexes_deferred, indexes_dropped_at FROM sync_status;"
```

## In-Memory Live Cell Store

The LiveCellStore provides O(1) cell lookups during blockchain synchronization by maintaining an in-memory cache of live cells. This significantly improves performance during bulk sync operations.

| Parameter                    | Default      | Description                                        |
| ---------------------------- | ------------ | -------------------------------------------------- |
| `--live-cell-memory-limit`   | `8589934592` | Maximum memory for in-memory live cell store (8GB) |
| `--live-cell-flush-interval` | `100`        | Flush dirty cells to database every N batches      |

**Behavior:**

- **Bulk Sync Mode** (>1000 blocks behind tip): Skips ALL `live_cells` table operations (INSERT/DELETE), writing only to the in-memory store for maximum throughput. The `cells` table still receives writes.
- **Periodic Flushing**: Dirty cells are flushed to the `live_cells` table every N batches (default 100)
- **Graceful Shutdown**: All pending cells are flushed to database on shutdown
- **Crash Recovery**: On restart, the indexer rebuilds the in-memory store from the `live_cells` table, ensuring no data loss

**Database Schema:**

The `live_cells` table is **hash-partitioned by `tx_hash`** into 16 partitions for parallel write distribution:

```sql
CREATE TABLE live_cells (...) PARTITION BY HASH (tx_hash);
CREATE TABLE live_cells_p00 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 0);
-- ... 15 more partitions
```

**Example Usage:**

```bash
# Default: 8GB memory limit, flush every 100 batches
cargo run -p ckbadger-indexer

# Custom memory limit (16GB) and flush interval (50 batches)
cargo run -p ckbadger-indexer -- \
  --live-cell-memory-limit 17179869184 \
  --live-cell-flush-interval 50

# Environment variables
LIVE_CELL_MEMORY_LIMIT=17179869184
LIVE_CELL_FLUSH_INTERVAL=50
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

| Topic            | Document                          | Must Read Before                   |
| ---------------- | --------------------------------- | ---------------------------------- |
| **Worldview**    | `docs/WORLD_VIEW.md`              | **Any design or implementation**   |
| DAO, APC, Supply | `docs/DAO_CALCULATIONS.md`        | Any DAO/supply/circulation changes |
| Historical bugs  | `docs/POSTMORTEM.md`              | Any CKB domain changes             |
| CKB protocol     | `docs/rfcs/`                      | Understanding CKB internals        |
| Nervos docs      | `docs/docs.nervos.org/`           | User-facing explanations           |
| DOB/Spore        | `docs/dob-cookbook/`              | DOB protocol, Spore NFT rendering  |
| Script names     | `docs/script-name-overrides.json` | Script label corrections           |

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

| What            | Where                                   |
| --------------- | --------------------------------------- |
| API routes      | `crates/api/src/routes/*.rs`            |
| Response types  | `crates/api/src/response.rs`            |
| WebSocket       | `crates/api/src/ws/`                    |
| RPC client      | `crates/indexer/src/rpc/client.rs`      |
| Parsers         | `crates/indexer/src/parser/*.rs`        |
| DB writes       | `crates/indexer/src/db/writer.rs`       |
| Frontend API    | `frontend/lib/api.ts`                   |
| UI components   | `frontend/components/ui/`               |
| Pages           | `frontend/app/`                         |
| Rust tests      | Inline `#[cfg(test)]` in parser files   |
| API integration | `crates/api/tests/api_integration.rs`   |
| Frontend tests  | `frontend/__tests__/**/*.test.{ts,tsx}` |
| MSW handlers    | `frontend/__tests__/msw/handlers.ts`    |
| E2E tests       | `e2e/*.spec.ts`                         |
| CI workflow     | `.github/workflows/ci.yml`              |

## Dependencies

**Rust**: axum 0.8, sqlx 0.8, tokio 1.42, serde, ckb-types/ckb-hash 0.119, anyhow/thiserror
**Frontend**: next 15.1, react 19, @tanstack/react-query 5, zustand 5, tailwindcss 3.4
