# CLAUDE.md

Instructions for AI agents working on ckbadger - a CKB blockchain explorer.

## Project Principles

- **CKB Native** - Make CKB concepts tangible instead of just-another-explorer
- **Local First** - Optimized for decentralized deployment on localhosts
- **Agent Friendly** - Prefer clear, automation-friendly structure and workflows

### Local First (Expanded)

- Local-first aligns with Web5 and Unix philosophy. Files and executable binaries are the foundation of composability, and ckbadger is designed around files and executable binaries.
- Local-first means ckbadger optimizes for writes (building data indexes), not reads (serving API and web page requests), unlike typical blockchain explorers. This enables extremely fast database sync, so local experiments remain cheap: if the DB is broken, rebuild it instead of protecting a 60-hour sync artifact. DB reads remain very fast, just not the top optimization target.

## Design Starting Point (MANDATORY)

- Documents under `docs/prompts/` capture the deep understanding and thinking principles of ckbadger.
- Treat `docs/prompts/` as the starting point for all design reasoning and architecture decisions.

## Agent Task Template (MANDATORY)

For any non-trivial task, use this structure in the final summary or PR description:

```md
## Goal

- What problem is being solved

## Principle Alignment

- CKB Native: / Local First: / Agent Friendly:

## Scope

- Files changed and why / Any storage/schema impact

## Validation

- Commands run: / Tests added/updated: / Verify checks:

## Result

- Behavior change summary / Re-sync required: yes/no / Next items on plan (if any) / What to do next suggestions (think deep think broad, if plan completed)
```

**Principle Sync Rule**: If principle wording changes, update both `README.md` and `CLAUDE.md` in the same commit.

## Coding Principles (MANDATORY)

- **Fail Fast, Fail Early** - Never hide invariant violations with silent fallbacks, lower-bound clamps, or default-zero repairs; fail immediately with actionable context
- Do not add silent guards to mask bad states on correctness-critical paths (for example `max(0)`, `saturating_sub`, `unwrap_or(0)`).
- If an invariant is violated, return/raise an error with enough context (block/tx/key/date) to locate the upstream bug quickly.

## DB Responsibility Boundary (MANDATORY)

- **Indexer owns all RocksDB writes**: any operation that creates/updates/deletes persistent DB state must be executed by `ckbadger-indexer`.
- **API is read-only for RocksDB**: `ckbadger-api` must only read from store (secondary/open_secondary path) and must not write persistent state.
- If API needs missing derived data, API must trigger indexer to compute and write it, then wait/poll for result instead of writing DB directly.

## Development Status & Sync Policies (IMPORTANT)

**This is a project under active development, NOT running in production.** Database can be cleared and rebuilt at any time. Schema changes are cheap.

**Design implications**: Prefer optimal data design over backward compatibility. Feel free to restructure column families. Breaking changes are acceptable — just update `crates/ckbadger-store/`. Re-sync is always an option.

**Sync Bug Policy (MANDATORY):** No rebuild task/workflow as primary fix for sync bugs. Fix indexer logic first, then delete RocksDB and re-sync from genesis. Prefer dropping and rebuilding DB over complex compatibility/backfill paths.

**Bulk Sync Failure Policy (MANDATORY):** Bulk sync is single-shot: finish successfully or fail fast. No auto-cleanup partial state. No defer + refill/rebuild patterns; write derived data inline. If it fails: fix bug, delete RocksDB, restart from genesis.

**Bulk Sync Write Policy:** Required user-facing derived data must be written inline with block processing. Any correctness bug should be fixed in the write/read logic with a full DB rebuild + re-sync from genesis.

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
cargo test                               # Run all tests
cargo test --lib                         # Unit tests only (fast)
cargo test test_name                     # Single test (partial match)
cargo test -p ckbadger-indexer           # Tests in one crate
cargo test -- --nocapture                # With stdout

# Indexer with Redis cache invalidation
cargo build -p ckbadger-indexer --features redis-cache
REDIS_URL=redis://localhost:6379 cargo run -p ckbadger-indexer --features redis-cache

# Frontend
pnpm dev                                 # Dev server (:3000)
pnpm build                               # Production build
pnpm lint                                # ESLint
cd frontend && pnpm type-check           # TypeScript (tsc --noEmit)
cd frontend && pnpm test                 # Vitest
cd frontend && npx vitest run            # Non-interactive

# Verify (requires running API at localhost:3001)
cargo run -p ckbadger-indexer -- verify --depth fast
cargo run -p ckbadger-indexer -- verify --depth sampling

# Pre-commit
cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint

# Formatting
pnpm format                              # Prettier (all files)

# Make shortcuts
make up                                  # Start local stack
make rebuild SERVICES=api                # Rebuild + restart service(s)
make tui                                 # Run monitoring TUI
make reset CONFIRM=1                     # Delete local RocksDB + redis data
make verify                              # Run verify --depth fast
```

`up` default mode from `.env`: `COMPOSE_PROFILES=internal` => internal CKB node, unset => external CKB.

## AI-Friendly Page Output

Frontend pages support `md` (markdown) and `raw` (structured JSON) formats. Format negotiation: `query.format` > URL suffix (`.md`/`.raw`) > `Accept` header. When adding/changing routes or formats, see `docs/AI_FORMATS.md` for the full checklist (MANDATORY).

## Project Structure

```
crates/
  api/            # Axum REST/WebSocket server (port 3001)
  indexer/        # Blockchain sync daemon (three-stage pipeline)
    src/verify/   #   Data integrity verification suite (54 checks)
  ckbadger-store/ # Embedded RocksDB storage engine (dual-store, 37 canonical CFs)
  common/         # Shared types (block, cell, tx, script, error)
  ckb-store-reader/ # Read-only CKB RocksDB reader (optional direct read mode)
  tui/            # Terminal monitoring UI (sync/memory/throughput)
frontend/         # Next.js 15 App Router + React 19
docs/AI_FORMATS.md           # AI-friendly page output details
docs/ACTIVITY_SYSTEM.md      # Activity system design - READ BEFORE ACTIVITY CHANGES
docs/ARCHITECTURE_MAP.md     # Module ownership and entry points
docs/POSTMORTEM.md           # Historical bugs - READ BEFORE CKB/DAO WORK
docs/INDEXER_PIPELINE.md     # Pipeline architecture + Redis sync data
docs/PERFORMANCE_RESULTS.md  # Benchmark snapshots for perf work
docs/REORG_HANDLING.md       # Chain reorganization handling
docs/STORE_SCHEMA.md         # Column families reference (37 CFs)
docs/VERIFY.md               # Data integrity verification details
```

## Indexer Pipeline Configuration

Three-stage pipeline: **Fetcher** (RPC I/O) -> **Parser** (CPU + DB prefetch) -> **Writer** (DB I/O). See `docs/INDEXER_PIPELINE.md` for architecture details, Redis sync data structs, and progress tracking.

| Parameter             | Default | Description                             |
| --------------------- | ------- | --------------------------------------- |
| `pipeline_enabled`    | `true`  | Enable pipeline mode (vs sequential)    |
| `pipeline_buffer`     | `8`     | Channel capacity between stages         |
| `batch_size`          | `10000` | Blocks per batch                        |
| `parallel_fetch_size` | `64`    | Concurrent RPC requests                 |
| `bulk_sync_threshold` | `1000`  | Blocks behind tip to treat as bulk sync |

Redis keys: `sync:status` (60s TTL), `sync:progress` (30s), `memory:stats` (30s). Fallback: RocksDB `get_sync_tip()`/`get_sync_status()`. Requires `redis-cache` feature + `REDIS_URL`.

## Label Import

`label_import` auto-runs on indexer start if `$TOKEN_LABELS_PATH/information/` exists (default: `docs/token-labels`). Manual: `cargo run -p ckbadger-indexer -- label-import`. Flags: `--token-labels-path`, `--network`, `--import-udt`, `--import-scripts`.

## ckbadger-store (Embedded Storage Engine)

Two logical RocksDB stores: domain (`CKBADGER_DOMAIN_DATA_PATH`, default `./data/ckbadger-store`) and append-only (`CKBADGER_APPEND_ONLY_DATA_PATH`, default `./data/ckbadger-store-append-only`). Indexer opens read-write; API opens secondary (read-only). See `docs/STORE_SCHEMA.md` for full column family reference (37 CFs, including split stats CFs).

Memory: ~22GB peak (>=32GB RAM), ~8GB peak (<32GB RAM).

## Data Integrity Verification

54 checks across 3 tiers: Fast (6, seconds), Sampling (21, minutes), Explorer (27, minutes). See `docs/VERIFY.md` for full details.

```bash
cargo run -p ckbadger-indexer -- verify --depth fast      # Quick sanity
cargo run -p ckbadger-indexer -- verify --depth sampling   # Full validation
cargo run -p ckbadger-indexer -- verify --list-checks      # List all checks
```

## Rust Style

**Imports**: External -> internal -> stdlib inline. **Naming**: `PascalCase` types, `snake_case` functions, `SCREAMING_SNAKE_CASE` constants. **Serde**: Always `#[serde(rename_all = "camelCase")]` for response structs. **Routes**: Axum 0.8 uses `{id}` not `:id`.

**Error Handling**: Indexer uses `anyhow::Result`; API uses `ApiResult<T>` with `ApiError::{not_found, bad_request, internal}()`.

**API Handler Pattern**: `async fn handler(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult<T>`

## TypeScript/React Style

**Prettier**: semi, singleQuote, tabWidth 2, printWidth 100, trailingComma es5. **Imports**: Always `@/` path alias (not relative). **Components**: `'use client'` for interactivity, named exports, Props interface. **Data Fetching**: TanStack Query v5.

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

| Change Type            | Required Action                                                      |
| ---------------------- | -------------------------------------------------------------------- |
| New parser function    | Add unit test in same file's `#[cfg(test)]` module                   |
| New API endpoint       | Add test case in `crates/api/tests/api_integration.rs`               |
| New frontend component | Add test in `frontend/__tests__/components/`                         |
| New hook/util function | Add test in `frontend/__tests__/hooks/` or `frontend/__tests__/lib/` |
| Bug fix                | Add regression test that reproduces the bug FIRST, then fix          |
| Refactoring            | Run existing tests BEFORE and AFTER to ensure no regression          |

**Verification**: New code passes `cargo test`/`pnpm test`. Bug fixes have regression tests. New functions have at least one happy-path test.

**FORBIDDEN**: Skipping tests for "simple" changes. Deleting/modifying tests to make them pass. Writing tests that don't assert anything meaningful. Ignoring failures with `#[ignore]`/`.skip()`.

## CKB Domain (CRITICAL)

**BEFORE making changes to CKB-related code, READ the relevant documentation:**

| Topic            | Document                      | Must Read Before                     |
| ---------------- | ----------------------------- | ------------------------------------ |
| **Worldview**    | `docs/WORLD_VIEW.md`          | **Any design or implementation**     |
| CKB protocol     | `docs/rfcs/`                  | Understanding CKB internals          |
| Nervos docs      | `docs/docs.nervos.org/`       | User-facing explanations             |
| DAO, APC, Supply | `docs/DAO_CALCULATIONS.md`    | Any DAO/supply/circulation changes   |
| Activity system  | `docs/ACTIVITY_SYSTEM.md`     | Activity feed or activity CF changes |
| Reorg handling   | `docs/REORG_HANDLING.md`      | Reorg or fork-related changes        |
| Architecture     | `docs/ARCHITECTURE_MAP.md`    | Module ownership questions           |
| Performance      | `docs/PERFORMANCE_RESULTS.md` | Performance optimization work        |

### Common Knowledge (CKB Core Concept)

**Common Knowledge Size** = Total occupied capacity of all live cells (NOT just cell data bytes). A cell's occupied capacity includes: capacity field (8B) + lock script (33B + args) + type script (33B + args) + data bytes. Source: DAO header `U` field (`dao[24..32]`).

```rust
// DAO field structure (32 bytes, little-endian u64s):
// [0..8]   C = total issuance
// [8..16]  AR = accumulated rate
// [16..24] S = cumulative non-miner secondary issuance (depositor + treasury)
// [24..32] U = total occupied capacity  <-- Common Knowledge Size
```

**Do NOT confuse**: `cell.data.len()` (data only) vs `occupied_capacity` (full storage cost) vs `U` field (protocol-level cumulative).

**Key domain knowledge** (`docs/DAO_CALCULATIONS.md`): Genesis issued 33.6B, only 25.2B circulating (8.4B burnt). `total_issuance` (dao field) != `circulating` (subtract burnt). APC = `secondary_issuance_per_year / circulating_supply * 100`.

### Numerical Precision (MANDATORY)

**All numerical calculations MUST be exact. NO estimation, interpolation, or sampling-based approximations.** Blockchain data is deterministic. Sampling errors compound over millions of blocks.

- Exact per-block calculation: REQUIRED
- Sampling with multiplication / interpolation / average-based estimation: FORBIDDEN
- Use cumulative on-chain values (DAO field differences) instead of sampling
- Do NOT write approximate values into persistent user-facing aggregates

### Script Identification

```rust
// code_hash = script TYPE (what kind), script_hash = script INSTANCE (unique)
// CORRECT: Compare code_hash for type detection
let code_hash = parse_hex_to_bytes(&type_script.code_hash);
DaoParser::is_dao_code_hash(&code_hash)
// WRONG: Computing full script_hash then comparing to code_hash
```

### DAO Constants & Lifecycle

```rust
const DAO_CODE_HASH: &str = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
const DAO_OCCUPIED_CAPACITY: u64 = 102_00000000; // 102 CKB
// Compensation: free_capacity * ar_withdraw / ar_deposit - free_capacity
```

1. **Deposit**: Creates DAO cell -> `dao_deposits(tx_hash=deposit_tx)`
2. **Withdraw Request**: Consumes deposit -> set `withdraw_request_tx`
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

| What             | Where                                                                                                      |
| ---------------- | ---------------------------------------------------------------------------------------------------------- |
| Storage engine   | `crates/ckbadger-store/src/` (types, store, keys, \*\_ops.rs)                                              |
| API routes       | `crates/api/src/routes/*.rs` (15 modules)                                                                  |
| Response types   | `crates/api/src/response.rs`                                                                               |
| WebSocket        | `crates/api/src/ws/`                                                                                       |
| RPC client       | `crates/indexer/src/rpc/client.rs`                                                                         |
| Parsers          | `crates/indexer/src/parser/*.rs` (block, cell, dao, script, spore, dotbit, mnft, udt, rgbpp, media_source) |
| DB writers       | `crates/indexer/src/db/writer/*.rs` (17 modules)                                                           |
| Label import     | `crates/indexer/src/label_import.rs`                                                                       |
| Verify checks    | `crates/indexer/src/verify/*.rs`                                                                           |
| TUI              | `crates/tui/src/`                                                                                          |
| Frontend API     | `frontend/lib/api.ts`                                                                                      |
| AI format routes | `frontend/app/ai-md/`, `frontend/app/ai-raw/`, `frontend/lib/ai/`                                          |
| Capabilities     | `frontend/app/capabilities/route.ts`, `frontend/lib/ai/capabilities.ts`                                    |
| LLM discovery    | `frontend/public/llms.txt`, `frontend/public/llms-full.txt`                                                |
| UI components    | `frontend/components/ui/`                                                                                  |
| Pages            | `frontend/app/`                                                                                            |
| Tests (Rust)     | Inline `#[cfg(test)]`, `crates/api/tests/api_integration.rs`                                               |
| Tests (Frontend) | `frontend/__tests__/**/*.test.{ts,tsx}`, `frontend/__tests__/msw/handlers.ts`                              |
| CI               | `.github/workflows/ci.yml`                                                                                 |

## Dependencies

**Rust**: axum 0.8, rocksdb, tokio 1.42, serde, ckb-types/ckb-hash 0.119, anyhow/thiserror
**Frontend**: next 15.1, react 19, @tanstack/react-query 5, zustand 5, tailwindcss 3.4
