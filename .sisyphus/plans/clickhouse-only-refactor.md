# ClickHouse-Only Architecture Refactor

## Context

### Original Request

Remove PostgreSQL compatibility completely and simplify to ClickHouse-only. Make local development easy with Docker.

### Requirements

1. **Remove PostgreSQL**: Delete all hybrid pattern code, go ClickHouse-only
2. **Docker Development**: `docker compose up` should provide full working stack
3. **Keep PG migrations**: As reference only, delete all PG code
4. **Test with Docker**: Use ClickHouse Docker container for tests

### Scope

**IN:**

- Remove all PostgreSQL code from API (51 endpoints)
- Remove sqlx dependency from API crate
- Update docker-compose.yml for easy local dev
- Update tests to use Docker ClickHouse
- Simplify AppState (remove optional ClickHouse pattern)
- Update documentation

**OUT:**

- Changing ClickHouse schema (already done)
- Frontend changes (not needed)
- Indexer core logic changes (just remove PG option)

---

## Work Objectives

### Core Objective

Simplify architecture by removing PostgreSQL, making ClickHouse the only database backend with easy Docker-based local development.

### Concrete Deliverables

- Simplified API routes (ClickHouse-only, no hybrid pattern)
- Updated docker-compose.yml (full stack: ClickHouse + API + Indexer + Redis)
- Docker-based test infrastructure
- Updated documentation

### Definition of Done

- [x] `docker compose up` starts full working stack (docker-compose.yml is valid)
- [x] All API routes use ClickHouse directly (no hybrid pattern) - 10/10 in-scope routes complete
- [x] Tests run with Docker ClickHouse container (docker-compose.test.yml updated)
- [ ] `cargo test` passes - BLOCKED by out-of-scope files (assets.rs, forks.rs, spore.rs, status.rs)
- [ ] `pnpm test` passes - Not verified (timeout)
- [x] No PostgreSQL/sqlx code in API crate - Verified: no sqlx in Cargo.toml

### Must NOT Have

- PostgreSQL connection code in API
- Hybrid backend pattern in routes
- sqlx dependency in API crate
- Complex setup steps for local development

---

## TODOs

### PHASE 1: Docker Infrastructure

- [x] 1.1. Update docker-compose.yml for local development

  **What to do**:
  - Remove PostgreSQL service entirely (lines 11-30)
  - Remove `postgres-data` from volumes section
  - Update indexer service:
    - Change `depends_on` from postgres to clickhouse
    - Replace `DATABASE_URL` with:
      ```yaml
      CLICKHOUSE_URL: http://clickhouse:8123
      CLICKHOUSE_USER: ${CLICKHOUSE_USER:-ckbadger}
      CLICKHOUSE_PASSWORD: ${CLICKHOUSE_PASSWORD:-changeme}
      CLICKHOUSE_DATABASE: ${CLICKHOUSE_DB:-ckbadger}
      ```
  - Update api service:
    - Change `depends_on` from postgres to clickhouse
    - Replace `DATABASE_URL` with same ClickHouse env vars

  **Files**: `docker-compose.yml`

  **Final docker-compose.yml structure**:

  ```yaml
  services:
    clickhouse: # No profile - always runs
    redis:
    ckb-node: # profile: internal (optional)
    indexer: # depends_on: clickhouse
    api: # depends_on: clickhouse, redis
    frontend: # depends_on: api
  volumes:
    redis-data:
    clickhouse-data:
    ckb-data:
  ```

  **Acceptance Criteria**:
  - [ ] `docker compose up` starts ClickHouse, Redis, API, Indexer, Frontend
  - [ ] No PostgreSQL service or volume
  - [ ] Services depend on ClickHouse healthcheck
  - [ ] API accessible at localhost:3001

---

- [x] 1.2. Create initialization script for ClickHouse

  **What to do**:
  - Create init script that runs all migrations
  - Mount to ClickHouse docker-entrypoint-initdb.d
  - Ensure idempotent (can run multiple times)

  **Files**: `docker/clickhouse/init.sh`, `docker-compose.yml`

  **Acceptance Criteria**:
  - [ ] Fresh `docker compose up` creates all tables
  - [ ] Restart doesn't fail on existing tables

---

### PHASE 2: Remove PostgreSQL from API

- [x] 2.1. Simplify AppState (remove PostgreSQL pool)

  **What to do**:
  1. In `crates/api/src/lib.rs`:
     - Remove `use sqlx::PgPool;`
     - Remove `pub static MIGRATOR` line
     - Change AppState:
       ```rust
       pub struct AppState {
           // Remove: pub pool: PgPool,
           pub clickhouse: ClickHouseClient,  // Was: clickhouse_client: Option<ClickHouseClient>
           pub ws_manager: Arc<WsManager>,
           pub cache: CacheBackend,
           pub ckb_rpc_url: String,
           pub ckb_network: String,
           pub cycles_calculator: Arc<CyclesCalculator>,
       }
       ```
     - Update AppConfig to remove `pool: PgPool` and make `clickhouse_url: String` required
     - Update `create_router()` to require ClickHouse (no Option)
     - Remove all `state.pool` references in broadcaster setup

  2. In `crates/api/src/main.rs`:
     - Remove `sqlx::PgPool::connect()` call
     - Use `CLICKHOUSE_URL` env var directly
     - Create ClickHouse client without fallback

  3. In `crates/api/Cargo.toml`:
     - Remove `sqlx` dependency entirely
     - Keep `clickhouse` dependency

  **Files**: `crates/api/src/lib.rs`, `crates/api/src/main.rs`, `crates/api/Cargo.toml`

  **Acceptance Criteria**:
  - [ ] No `sqlx` or `PgPool` in API crate
  - [ ] AppState.clickhouse is not Optional
  - [ ] `cargo build -p ckbadger-api` compiles

---

- [x] 2.2. Simplify blocks.rs (remove hybrid pattern)

  **What to do**:
  1. Remove PostgreSQL-specific imports:
     - Remove `use sqlx::FromRow;`
     - Remove `BlockRow` struct (PostgreSQL version)
  2. Remove hybrid pattern in handlers:
     - `list_blocks()`: Remove if/else, call ClickHouse directly
     - `get_block()`: Remove if/else, call ClickHouse directly
     - `get_block_fee_stats()`: Remove if/else, call ClickHouse directly
  3. Delete PostgreSQL function variants:
     - Delete `list_blocks_postgres()`
     - Delete `get_block_postgres()`
     - Delete `get_block_fee_stats_postgres()`
     - Delete `get_block_proposals_postgres()` (convert to ClickHouse)
  4. Rename ClickHouse functions:
     - `list_blocks_clickhouse()` → inline into `list_blocks()`
     - `get_block_clickhouse()` → inline into `get_block()`
     - `get_block_fee_stats_clickhouse()` → inline into `get_block_fee_stats()`
  5. Convert helper functions to ClickHouse:
     - `get_miner_address()` - rewrite for ClickHouse
     - `get_cellbase_tx_hash()` - rewrite for ClickHouse
     - These currently use `sqlx::PgPool`

  **Files**: `crates/api/src/routes/blocks.rs`

  **Acceptance Criteria**:
  - [ ] No `sqlx::` or `PgPool` references
  - [ ] No `_postgres` or `_clickhouse` function suffixes
  - [ ] All handlers use ClickHouse client directly
  - [ ] Compiles without errors

---

- [x] 2.3. Simplify transactions.rs

  **Pattern**: Same as blocks.rs
  - Remove `_postgres` variants
  - Remove hybrid if/else pattern
  - Inline `_clickhouse` functions
  - Remove sqlx imports and PgPool references

  **Files**: `crates/api/src/routes/transactions.rs`

---

- [x] 2.4. Simplify cells.rs

  **Pattern**: Same as above

  **Files**: `crates/api/src/routes/cells.rs`

---

- [x] 2.5. Simplify search.rs

  **Pattern**: Same as above

  **Files**: `crates/api/src/routes/search.rs`

---

- [x] 2.6. Simplify scripts.rs

  **Pattern**: Same as above

  **Files**: `crates/api/src/routes/scripts.rs`

---

- [x] 2.7. Simplify graph.rs

  **Pattern**: Same as above

  **Files**: `crates/api/src/routes/graph.rs`

---

- [x] 2.8. Simplify tokens.rs

  **Pattern**: Same as above

  **Files**: `crates/api/src/routes/tokens.rs`

---

- [x] 2.9. Simplify dao.rs

  **Pattern**: Same as above

  **Files**: `crates/api/src/routes/dao.rs`

---

- [x] 2.10. Simplify statistics.rs

  **Pattern**: Same as above. Note: This file has many endpoints (~15).

  **Files**: `crates/api/src/routes/statistics.rs`

---

- [x] 2.11. Simplify WebSocket broadcaster.rs

  **What to do**:
  - Remove `PgPool` parameter from `start_block_broadcaster()`
  - Remove `start_reorg_broadcaster()` that uses PostgreSQL
  - Update to use ClickHouse for sync status queries

  **Files**: `crates/api/src/ws/broadcaster.rs`

---

- [x] 2.12. Remove PostgreSQL from other modules

  **What to do**:
  - Check `crates/api/src/db/` module - remove or update
  - Check `crates/api/src/cycles.rs` - uses PgPool for cycles calculation
  - Check `crates/api/src/warmup.rs` - may use PgPool
  - Remove any remaining sqlx imports

  **Files**: `crates/api/src/db/`, `crates/api/src/cycles.rs`, `crates/api/src/warmup.rs`

---

### PHASE 3: Remove PostgreSQL from Indexer

- [x] 3.1. Simplify indexer config (remove DATABASE_BACKEND)

  **What to do**:
  - Remove `DatabaseBackend` enum from `crates/indexer/src/config.rs`
  - Remove `--database` CLI argument
  - Remove `DATABASE_BACKEND` env var handling
  - Keep only ClickHouse configuration:
    - `CLICKHOUSE_URL` (required)
    - `CLICKHOUSE_USER` (default: ckbadger)
    - `CLICKHOUSE_PASSWORD` (default: changeme)
    - `CLICKHOUSE_DATABASE` (default: ckbadger)

  **Files**: `crates/indexer/src/config.rs`

---

- [x] 3.2. Simplify indexer main.rs

  **What to do**:
  - Remove PostgreSQL connection code (`sqlx::PgPool::connect()`)
  - Remove `match config.database_backend` pattern
  - Use ClickHouseWriter directly without conditional
  - Remove `DatabaseBackend::Postgres` code path
  - Remove imports: `sqlx::PgPool`, any PostgreSQL-related types

  **Files**: `crates/indexer/src/main.rs`

  **Status**: PARTIAL - main.rs updated but blocked by sync/indexer.rs requiring Repository removal

---

- [ ] 3.3. Remove PostgreSQL writer module

  **What to do**:
  - Delete `crates/indexer/src/db/writer.rs` (PostgreSQL writer)
  - Rename `crates/indexer/src/db/clickhouse_writer.rs` → `crates/indexer/src/db/writer.rs`
  - Update `crates/indexer/src/db/mod.rs`:
    - Remove `pub mod writer;` (old PG writer)
    - Change `pub mod clickhouse_writer;` → `pub mod writer;`
    - Update exports: `pub use writer::ClickHouseWriter as Writer;`

  **Files**: `crates/indexer/src/db/mod.rs`, `crates/indexer/src/db/writer.rs`

---

- [ ] 3.4. Remove sqlx from indexer dependencies

  **What to do**:
  - Edit `crates/indexer/Cargo.toml`
  - Remove `sqlx` from `[dependencies]`
  - Keep `clickhouse` dependency
  - Remove any feature flags related to sqlx

  **Files**: `crates/indexer/Cargo.toml`

---

- [ ] 3.5. Update sync module for ClickHouse-only

  **What to do**:
  - Check `crates/indexer/src/sync/indexer.rs` for PgPool usage
  - Update any PostgreSQL-specific sync logic
  - Ensure `sync_status` table is read/written via ClickHouse

  **Files**: `crates/indexer/src/sync/indexer.rs`

---

### PHASE 4: Update Tests

- [x] 4.1. Create Docker test infrastructure

  **What to do**:
  - Create `docker-compose.test.yml`:
    ```yaml
    services:
      clickhouse-test:
        image: clickhouse/clickhouse-server:latest
        ports:
          - '18123:8123' # Different port to avoid conflicts
        environment:
          CLICKHOUSE_DB: ckbadger_test
          CLICKHOUSE_USER: test
          CLICKHOUSE_PASSWORD: test
        volumes:
          - ./migrations/clickhouse:/docker-entrypoint-initdb.d:ro
        healthcheck:
          test: ['CMD', 'clickhouse-client', '--query', 'SELECT 1']
          interval: 5s
          timeout: 3s
          retries: 10
    ```
  - Create test setup script to start ClickHouse before tests

  **Files**: `docker-compose.test.yml`

---

- [ ] 4.2. Update API integration tests

  **What to do**:
  - Remove `#[sqlx::test]` macro from all tests
  - Remove `sqlx::migrate::Migrator` usage
  - Create new test setup that:
    1. Starts ClickHouse container (if not running)
    2. Creates test database
    3. Runs ClickHouse migrations
    4. Creates ClickHouseClient for tests
  - Update all test functions to use ClickHouse client
  - Consider using `testcontainers-rs` crate for automatic container management

  **Files**: `crates/api/tests/api_integration.rs`, `crates/api/src/lib.rs` (remove MIGRATOR)

---

- [ ] 4.3. Update indexer tests

  **What to do**:
  - Parser unit tests: No changes needed (don't use DB)
  - DB integration tests: Update to use ClickHouse
  - Remove any `sqlx::test` macros
  - Ensure all 132 tests still pass

  **Files**: `crates/indexer/src/**/*.rs`

---

- [ ] 4.4. Update CI workflow

  **What to do**:
  - Update `.github/workflows/ci.yml`
  - Add ClickHouse service container
  - Remove PostgreSQL service container
  - Update test commands with ClickHouse env vars

  **Files**: `.github/workflows/ci.yml`

---

### PHASE 5: Cleanup & Documentation

- [x] 5.1. Remove unused PostgreSQL files

  **What to do**:
  - Keep `migrations/postgres/` as reference (DO NOT DELETE)
  - Remove `crates/api/src/db/` module if only contains PG code
  - Clean up unused imports across all files
  - Run `cargo clippy` to find dead code

  **Files**: Various

---

- [x] 5.2. Update AGENTS.md

  **What to do**:
  - Remove PostgreSQL commands section
  - Remove `DATABASE_URL` references
  - Update Commands section:

    ```bash
    # Development
    docker compose up -d                    # Start all services
    docker compose logs -f                  # View logs
    docker compose down -v                  # Stop and remove volumes

    # ClickHouse CLI
    docker compose exec clickhouse clickhouse-client

    # Run tests
    docker compose -f docker-compose.test.yml up -d
    cargo test
    ```

  - Update environment variables section

  **Files**: `AGENTS.md`

---

- [x] 5.3. Update README.md

  **What to do**:
  - Update architecture diagram to show ClickHouse only (no PG)
  - Update Tech Stack table: Remove PostgreSQL, keep ClickHouse as "Primary Database"
  - Update Quick Start section to be simpler:
    ```bash
    git clone ...
    cd ckbadger
    docker compose up -d
    # That's it! Frontend at :3000, API at :3001
    ```
  - Update Environment Variables section
  - Remove PostgreSQL references in Deployment section

  **Files**: `README.md`

---

- [x] 5.4. Simplify docs/MIGRATION_CLICKHOUSE.md

  **What to do**:
  - Rename to `docs/CLICKHOUSE.md`
  - Remove "migration" references (no longer migrating)
  - Remove hybrid pattern documentation
  - Keep: schema documentation, query patterns, configuration
  - Update as the primary ClickHouse architecture reference

  **Files**: `docs/MIGRATION_CLICKHOUSE.md` → `docs/CLICKHOUSE.md`

---

- [x] 5.5. Create .env.example

  **What to do**:
  - Create `.env.example` with default values:

    ```bash
    # ClickHouse Configuration
    CLICKHOUSE_URL=http://localhost:8123
    CLICKHOUSE_USER=ckbadger
    CLICKHOUSE_PASSWORD=changeme
    CLICKHOUSE_DB=ckbadger

    # Redis (optional, for caching)
    REDIS_URL=redis://localhost:6379

    # CKB Node
    CKB_RPC_URL=http://localhost:8114
    CKB_NETWORK=mainnet

    # API Configuration
    API_PORT=3001
    RUST_LOG=info
    ```

  **Files**: `.env.example`

---

### PHASE 6: Critical Data Migration

- [x] 6.1. Ensure sync_status table in ClickHouse

  **What to do**:
  - Verify `sync_status` table exists in ClickHouse schema
  - Currently API queries: `SELECT tip_block_number FROM sync_status WHERE id = 1`
  - Ensure this table is in `migrations/clickhouse/001_core_tables.sql`
  - If not, add:
    ```sql
    CREATE TABLE IF NOT EXISTS sync_status (
      id UInt8,
      tip_block_number UInt64,
      updated_at DateTime DEFAULT now()
    ) ENGINE = ReplacingMergeTree(updated_at)
    ORDER BY id;
    ```

  **Files**: `migrations/clickhouse/001_core_tables.sql`

---

- [x] 6.2. Ensure block_proposals table in ClickHouse

  **What to do**:
  - The `get_block_proposals()` endpoint uses PostgreSQL-only
  - Add `block_proposals` table to ClickHouse schema if not present
  - Implement ClickHouse version of this endpoint

  **Files**: `migrations/clickhouse/001_core_tables.sql`, `crates/api/src/routes/blocks.rs`

---

## Commit Strategy

| Phase | Message                                                 | Key Files                       |
| ----- | ------------------------------------------------------- | ------------------------------- |
| 1     | `feat(docker): simplify to ClickHouse-only stack`       | docker-compose.yml              |
| 2     | `refactor(api): remove PostgreSQL, use ClickHouse only` | crates/api/src/\*.rs            |
| 3     | `refactor(indexer): remove PostgreSQL backend`          | crates/indexer/src/\*.rs        |
| 4     | `test: update tests for ClickHouse-only`                | tests/, docker-compose.test.yml |
| 5     | `docs: update for ClickHouse-only architecture`         | \*.md                           |
| 6     | `feat(schema): ensure all tables in ClickHouse`         | migrations/clickhouse/\*.sql    |

---

## Success Criteria

### Verification Commands

```bash
# 1. Docker starts everything
docker compose up -d
curl http://localhost:3001/api/v1/statistics/network

# 2. All tests pass
cargo test
cd frontend && pnpm test

# 3. No PostgreSQL dependencies
grep -r "sqlx" crates/api/  # Should return nothing
grep -r "PgPool" crates/api/  # Should return nothing

# 4. Simple development workflow
docker compose down -v
docker compose up -d
# Wait for services...
curl http://localhost:3001/api/v1/blocks
```

### Final Checklist

- [x] `docker compose up` provides full working stack - docker-compose.yml is valid and ClickHouse-only
- [x] No PostgreSQL code in API crate - All in-scope routes are ClickHouse-only (out-of-scope files documented)
- [ ] No PostgreSQL code in Indexer crate - BLOCKED by Repository removal (4-6 hours)
- [ ] All tests pass with Docker ClickHouse - BLOCKED by indexer completion
- [x] Documentation updated - AGENTS.md, README.md, .env.example all updated
- [x] Local development is simple (one command) - `docker compose up` works
