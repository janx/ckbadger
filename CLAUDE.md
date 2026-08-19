# AGENTS.md

Instructions for AI agents working on ckbadger - a CKB blockchain explorer.

## Project Principles

- **CKB Native** - Make CKB concepts tangible instead of just-another-explorer. CKB chain data is the only source of truth, all other data are derived from it.
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

## Result

- Behavior change summary
- Re-sync required: yes/no
- What to do next
```

**Principle Sync Rule**: If principle wording changes, update both `README.md` and `CLAUDE.md` in the same commit.

## Coding Principles (MANDATORY)

- **Fail Fast, Fail Early** - Never hide invariant violations with silent fallbacks, lower-bound clamps, or default-zero repairs; fail immediately with actionable context
- **Refactor First When It Helps** - Before implementing new code, evaluate whether a focused refactor will reduce complexity or risk; if yes, refactor first and then implement.
- **Single Calculation Path for Read Data** - For any data that must be read/derived, keep exactly one computation path and make that single path correct.
- **No Fallback Calculation Chains** - Reject defensive multi-path computation such as "if path A is wrong, fallback to B, then fallback to C"; do not add path B/C, fix path A.
- **No Workaround Fixes for Bugs** - Do not ship bypasses, route detours, temporary guards, degraded-mode switches, or UX-level evasions as bug fixes. Identify and fix the upstream root cause in the owning computation/write path.
- Do not add silent guards to mask bad states on correctness-critical paths (for example `max(0)`, `saturating_sub`, `unwrap_or(0)`).
- If an invariant is violated, return/raise an error with enough context (block/tx/key/date) to locate the upstream bug quickly.

## Debug & Fix Principles (MANDATORY)

- **Trace Root Cause** - Do not stop at shallow/near-surface symptoms; track the true upstream root cause.
- **Fix Root Cause, Not With Fallbacks** - If you find some data incorrect, don't be satisfy, don't use recalculation code to correct it, instead you should check why it's incorrect in the first place, fix the bug there. Do not patch incorrect pre-computation with extra fallback paths; fix the original computation logic that produced the wrong state.

## DB Responsibility Boundary (MANDATORY)

- **Indexer owns all chain-store RocksDB writes**: any operation that creates/updates/deletes persistent domain- or append-only-store state must be executed by `ckbadger-indexer`. The one exception is the separate **network store**, whose sole writer is the opt-in `ckbadger-crawler` service (see Network store responsibility below).
- **API is read-only for RocksDB**: `ckbadger-api` must only read from store (secondary/open_secondary path) and must not write persistent state.
- If API needs missing derived data, API must trigger indexer to compute and write it, then wait/poll for result instead of writing DB directly.
- **Domain store responsibility**: domain store (`[store].domain_data_path`, 59 CFs) holds all mutable canonical/query state including activities, addr_txs, live/consumed cell markers, and all indexes. May perform create/update/delete as required by chain progression and reorg handling, but only via indexer.
- **Append-only store responsibility**: append-only store (`[store].append_only_data_path`, 1 CF: `CF_CELLS`) holds only immutable cell payloads, content-addressed by outpoint. Write-once, never updated or deleted.
- **Append-only correction policy**: if cell payload data in the append-only store is wrong, fix indexer logic and rebuild from genesis; do not patch cell data with in-place update/delete.
- **Cross-store cell reads**: live/consumed markers live in domain store; cell payloads live in append-only store. Reading a full cell requires both the domain and append-only stores.
- **Network store responsibility** (third store class): the network store (`[store].network_data_path`, default `data/network`, 2 CFs: `CF_NET_NODES`, `CF_NET_STATS`) holds whole-network p2p-crawler observations — non-chain, non-deterministic, TTL-retained network-topology data. Written exclusively by the opt-in `ckbadger-crawler` service (never the indexer); the API opens it secondary (read-only). It is the ONLY store EXEMPT from the rebuild-from-genesis invariant, because its contents derive from live p2p observation rather than chain replay.

## Store Boundary Check Rules (MANDATORY)

- `CF_CELLS` is the only append-only CF. The 59 domain CFs are the canonical mutable chain view; the 2 network CFs (`CF_NET_NODES`, `CF_NET_STATS`) belong to the separate network store (mutable, TTL-retained, non-chain).
- Every storage PR must explicitly state which logical store each new/changed write path targets (`domain`, `append-only`, or `network`).
- Any write path to the append-only store (`CF_CELLS`) must be reviewed as append-only semantics: new-key append only, no update, no delete, no overwrite.
- Do not add helper APIs that allow generic mutation on append-only store (for example update-by-key or delete-by-key operations).
- If a feature requires mutable behavior, it belongs in domain store, not append-only store.
- Validation for storage changes must include at least one check/test proving append-only invariants are enforced on touched paths.

## Development Status & Sync Policies (IMPORTANT)

**This is a project under active development, NOT running in production.** Database can be cleared and rebuilt at any time. Schema changes are cheap.

**Design implications**: Prefer optimal data design over backward compatibility. Feel free to restructure column families. Breaking changes are acceptable — just update `crates/ckbadger-store/`. Re-sync is always an option.

**Sync Bug Policy (MANDATORY):** No rebuild task/workflow as primary fix for sync bugs. Fix indexer logic first, then delete RocksDB and re-sync from genesis. Prefer dropping and rebuilding DB over complex compatibility/backfill paths.

**Bulk Sync Policy (MANDATORY):** Follow `docs/prompts/BULK_SYNC.md` as the single source of truth for bulk sync behavior, constraints, and failure handling.

```bash
# Typical workflow after storage changes:
# 1. Update types/ops in crates/ckbadger-store/src/
# 2. Update indexer writer code in crates/indexer/src/db/writer/
# 3. Delete RocksDB data directory
# 4. Re-run indexer to sync from genesis
```

## Commands

```bash
# CLI usage
cargo build -p ckbadger                  # Build CLI binary
ckbadger init                            # Create orchestrator ckbadger.toml + per-network config.toml (add --with-testnet)
ckbadger run                             # Start supervisor (indexer + api per network + shared frontend)
ckbadger tui                             # Run monitoring TUI
ckbadger status                          # Show service and sync status
ckbadger verify --depth fast             # Data integrity verification
ckbadger label-import                    # Import token/script labels
ckbadger crawl [--once]                  # Run whole-network p2p peer crawler (opt-in; --once = single round then exit)
ckbadger purge                           # Delete local RocksDB data

# Rust development
cargo check                              # Type check all crates
cargo clippy                             # Lint
cargo test                               # Run all tests
cargo test --lib                         # Unit tests only (fast)
cargo test test_name                     # Single test (partial match)
cargo test -p ckbadger-indexer           # Tests in one crate
cargo test -- --nocapture                # With stdout

# Frontend development
pnpm dev                                 # Dev server (:3000)
pnpm build                               # Vite SPA build to dist/
pnpm lint                                # ESLint
cd frontend && pnpm type-check           # TypeScript (tsc --noEmit)
cd frontend && pnpm test                 # Vitest
cd frontend && npx vitest run            # Non-interactive

# Pre-commit
cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint

# Formatting
pnpm format                              # Prettier (all files)

# Make shortcuts
make build                               # cargo build -p ckbadger
make check                               # cargo check && cargo clippy
make test                                # All tests (Rust + frontend)
make lint                                # Frontend lint + type-check
make verify                              # Run verify --depth fast
```

## Performance Notes

- Performance-affecting PRs should include before/after numbers.
- Benchmark snapshots are generated on demand; no committed `docs/PERFORMANCE_RESULTS.md` baseline is required.

## Project Structure

```
crates/
  cli/            # Single CLI binary (ckbadger) with subcommands + supervisor
  config/         # config parsing: per-network config.toml + orchestrator ckbadger.toml (ckbadger-config)
  ipc/            # Unix socket IPC protocol (ckbadger-ipc)
  api/            # Axum REST/WebSocket server library (port 8101)
  indexer/        # Blockchain sync daemon library (three-stage pipeline)
    src/sync/bulk_build/ # Bulk-build engine (in-memory reducers, FactsArena, LiveCellOwner)
    src/verify/   #   Data integrity verification suite (57 checks)
  ckbadger-store/ # Embedded RocksDB storage engine (three store classes: 59 domain + 1 append-only + 2 network CFs)
  dob-decoder/    # CKB-VM DOB decoder (DNA extraction from Spore NFTs)
  common/         # Shared types (block, cell, tx, script, error)
  ckb-store-reader/ # Read-only CKB RocksDB reader (optional direct read mode)
  tui/            # Terminal monitoring UI library (sync/memory/throughput)
  bench/          # API stress testing and benchmarking tool
  crawler/        # Opt-in whole-network CKB L1 p2p peer crawler (ckbadger-crawler); sole writer of the network store
frontend/         # Vite + React SPA
docs/ARCHITECTURE_MAP.md     # Module ownership and entry points
docs/POSTMORTEM.md           # Historical bugs - READ BEFORE CKB/DAO WORK
docs/INDEXER_PIPELINE.md     # Pipeline architecture and progress tracking
docs/STORE_SCHEMA.md         # Column families reference (59 domain + 1 append-only + 2 network)
docs/TESTING.md               # Data integrity verification details
```

## Indexer Pipeline Configuration

Three-stage pipeline: **Fetcher** (RPC I/O) -> **Parser** (CPU + DB prefetch) -> **Writer** (DB I/O). See `docs/INDEXER_PIPELINE.md` for architecture details and progress tracking.

| Parameter               | Default | Description                             |
| ----------------------- | ------- | --------------------------------------- |
| `bulk_sync_threshold`   | `1000`  | Blocks behind tip to treat as bulk sync |
| `poll_interval_ms`      | `1000`  | Live sync new-block poll interval (ms)  |
| `bulk_memory_budget_gb` | auto    | Optional whole-indexer bulk memory cap  |

Bulk-build mode uses a `BottleneckController` (`crates/indexer/src/sync/bottleneck.rs`) with two independent dimensions: (1) batch sizing via build-time band + build/IO overlap — primary objective is build time in [2s, 5s]; below band → grow, above band → shrink; IO wait (recv + flush) is excluded from the band check because shrinking batch size cannot reduce IO-bound time; in-band the controller grows while build > IO (IO headroom exists), holds when IO ≥ build (physical limit); (2) I/O resources governed by waste classification (recv wait vs flush wait) adjusting `fetch_threads` and `bg_jobs`. Drain uses `drain_by_cells(target_cells, max_batch_bytes)` with cell count as primary budget and RAM-derived bytes as safety cap. Prefetch fill estimate uses actual cell density from the buffer (`cell_density()`) instead of hardcoded assumptions. Channel depths (prefetch + flush) are derived from system RAM.

In orchestrator mode, APIs/crawlers/frontend start immediately, while indexers enter fresh-store
bulk sync sequentially in `[[network]]` order. Only one network bulk-syncs at a time; an indexer
past the bulk threshold no longer blocks the next network.

Sync progress and memory stats are stored in RocksDB (`get_sync_tip()`/`get_sync_status()`/`get_sync_progress()`/`get_memory_stats()`).

## Label Import

`label_import` auto-runs on indexer start. Labels are bundled at compile time from `docs/metadata/`. Optional workdir override: place TOML files in `<work_dir>/metadata/`. Manual: `ckbadger label-import`.

## ckbadger-store (Embedded Storage Engine)

Three logical RocksDB store classes: domain (`[store].domain_data_path`, default `data/domain`, 59 CFs), append-only (`[store].append_only_data_path`, default `data/append-only`, 1 CF: `CF_CELLS`), and network (`[store].network_data_path`, default `data/network`, 2 CFs: `CF_NET_NODES`, `CF_NET_STATS`). For the two chain stores the indexer opens read-write and the API opens secondary (read-only); the append-only store holds only immutable cell payloads keyed by outpoint, while all other chain state (activities, indexes, stats, etc.) lives in the domain store. The network store is written solely by the opt-in `ckbadger-crawler` service (configured via the `[crawler]` section, default `enabled = false`; API opens it secondary) and holds non-chain p2p-crawler observations — it is the only store EXEMPT from rebuild-from-genesis. See `docs/STORE_SCHEMA.md` for full column family reference.

Memory is budgeted per network. `[store].memory_budget_gb` is an explicit per-network override;
otherwise detected host RAM is divided by the number of co-resident orchestrator networks. The
domain and append-only stores in one process share one RocksDB block cache and
WriteBufferManager. `[indexer].bulk_memory_budget_gb` optionally sets the whole-indexer bulk-sync
hard limit; otherwise bulk sync uses the same per-network RAM share.

## Data Integrity Verification

57 checks across 3 tiers: Fast (7, seconds), Sampling (24, minutes), Explorer (26, minutes). See `docs/TESTING.md` for full details.

```bash
ckbadger verify --depth fast              # Quick sanity
ckbadger verify --depth sampling          # Full validation
ckbadger verify --list-checks             # List all checks
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
| New API endpoint       | Add test case in the matching `crates/api/tests/api_*.rs` file       |
| New frontend component | Add test in `frontend/__tests__/components/`                         |
| New hook/util function | Add test in `frontend/__tests__/hooks/` or `frontend/__tests__/lib/` |
| Bug fix                | Add regression test that reproduces the bug FIRST, then fix          |
| Refactoring            | Run existing tests BEFORE and AFTER to ensure no regression          |

**Verification**: New code passes `cargo test`/`pnpm test`. Bug fixes have regression tests. New functions have at least one happy-path test.

**FORBIDDEN**: Skipping tests for "simple" changes. Deleting/modifying tests to make them pass. Writing tests that don't assert anything meaningful. Ignoring failures with `#[ignore]`/`.skip()`.

## CKB Domain (CRITICAL)

**BEFORE making changes to CKB-related code, READ the relevant documentation:**

| Topic            | Document                              | Must Read Before                              |
| ---------------- | ------------------------------------- | --------------------------------------------- |
| **Worldview**    | `docs/prompts/WORLD_VIEW.md`          | **Any design or implementation**              |
| Bulk sync rules  | `docs/prompts/BULK_SYNC.md`           | Bulk sync logic or sync-mode boundary changes |
| Reorg handling   | `docs/prompts/REORG_HANDLING.md`      | Reorg or fork-related changes                 |
| Activity system  | `docs/prompts/ACTIVITY_DESIGN.md`     | Activity feed or activity CF changes          |
| CKB protocol     | https://github.com/nervosnetwork/rfcs | Understanding CKB internals                   |
| Nervos docs      | https://docs.nervos.org               | User-facing explanations                      |
| DAO, APC, Supply | `docs/DAO_CALCULATIONS.md`            | Any DAO/supply/circulation changes            |
| Architecture     | `docs/ARCHITECTURE_MAP.md`            | Module ownership questions                    |

The RFC and Nervos-docs sources are upstream links, not vendored copies — they were
git submodules and were removed; nothing under `docs/` mirrors them.

### Common Knowledge (CKB Core Concept)

**Common Knowledge Size** = Total real occupied capacity of all live cells (NOT just cell data
bytes). A cell's occupied capacity includes: capacity field (8B) + lock script (33B + args) +
type script (33B + args) + data bytes. It is derived from DAO header `U` (`dao[24..32]`) minus the
network's genesis-derived `GenesisBaseline.virtual_occupied`.

```rust
// DAO field structure (32 bytes, little-endian u64s):
// [0..8]   C = total issuance
// [8..16]  AR = accumulated rate
// [16..24] S = complete unissued secondary pool (DAO interest + treasury)
// [24..32] U = protocol occupied capacity (includes genesis virtual occupied)
```

**Do NOT confuse**: `cell.data.len()` (data only) vs `occupied_capacity` (full storage cost) vs `U` field (protocol-level cumulative).

**Key domain knowledge** (`docs/DAO_CALCULATIONS.md`): mainnet's familiar rounded genesis
figures are 33.6B issued, 8.4B burnt, and 25.2B circulating, but code must derive the exact
per-network `GenesisBaseline` from block 0. Protocol circulation is `C - baseline.burnt - S`.
Estimated APC uses the explorer-compatible continuous-compounding model seeded by
`baseline.total_issuance`; the separate nominal APC chart uses
`baseline.total_issuance - baseline.burnt`.

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
// 102 CKB is only the standard secp DAO-cell occupied capacity.
// Persist and use each deposit cell's exact occupied capacity.
// Compensation: free_capacity * ar_withdraw / ar_deposit - free_capacity,
// where free_capacity = capacity - exact_occupied_capacity.
```

1. **Deposit**: Creates DAO cell → key `dao_deposits` by deposit outpoint.
2. **Withdraw Request**: Consumes the deposit and creates a request outpoint; persist that outpoint
   and its AR.
3. **Withdraw Completion**: Resolve through `dao_by_withdraw_tx` keyed by the request outpoint, not
   by treating the completion transaction hash as the original deposit key.

## Gotchas

| Issue                     | Solution                                                          |
| ------------------------- | ----------------------------------------------------------------- |
| Hex parsing               | Use `parse_hex_to_bytes()`, `parse_capacity()` in `rpc/client.rs` |
| Script hashing            | `ckb-hash::new_blake2b()` with CKB personalization                |
| WebSocket Text (Axum 0.8) | Needs `Utf8Bytes` - use `.into()` from String                     |
| react-force-graph-2d      | Use `frontend/lib/dynamic-client.tsx` for client-only graph loads |
| API casing                | Backend `camelCase` via serde, frontend types match               |
| Daily charts              | Exclude incomplete current day                                    |
| Vite SPA deep links       | Rust frontend server must fall back to `index.html` for non-files |
| Vitest globals            | Add `vitest/globals` to tsconfig types                            |
| MSW handlers              | Must start server in setup.ts `beforeAll`                         |
| RocksDB secondary mode    | API uses `open_secondary()` — read-only, no write locks           |
| Spore molecule `Bytes`    | Size field = content length (NOT total size including header)     |

## File Locations

| What             | Where                                                                                                                                                                           |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CLI binary       | `crates/cli/src/main.rs` (subcommands, supervisor)                                                                                                                              |
| Config           | `crates/config/src/lib.rs` (per-network `config.toml`); `crates/config/src/orchestrator.rs` (orchestrator `ckbadger.toml`, `[[network]]`)                                       |
| IPC protocol     | `crates/ipc/src/` (Unix socket server/client)                                                                                                                                   |
| Storage engine   | `crates/ckbadger-store/src/` (types, store, keys, \*\_ops.rs)                                                                                                                   |
| API routes       | `crates/api/src/routes/*.rs` (18 mounted modules + `tx_lookup`/`proposal_window` helpers)                                                                                       |
| Response types   | `crates/api/src/response.rs`                                                                                                                                                    |
| WebSocket        | `crates/api/src/ws/`                                                                                                                                                            |
| RPC client       | `crates/indexer/src/rpc/client.rs`                                                                                                                                              |
| Parsers          | `crates/indexer/src/parser/*.rs` (bit_cell, block, cell, dao, did_ckb, dotbit, fiber, media_source, mnft, registry, rgbpp, script, spore, stablepp, transaction, udt, utxoswap) |
| DB writers       | `crates/indexer/src/db/writer/*.rs` (20 modules)                                                                                                                                |
| DOB decoder      | `crates/dob-decoder/src/` (lib, vm, cache, fetch, types)                                                                                                                        |
| Label import     | `crates/indexer/src/label_import.rs`                                                                                                                                            |
| Verify checks    | `crates/indexer/src/verify/*.rs`                                                                                                                                                |
| TUI              | `crates/tui/src/`                                                                                                                                                               |
| Bench            | `crates/bench/src/` (stress testing, endpoint benchmarks, reports)                                                                                                              |
| Frontend API     | `frontend/lib/api.ts`                                                                                                                                                           |
| LLM discovery    | `frontend/public/llms.txt`, `frontend/public/llms-full.txt`                                                                                                                     |
| UI components    | `frontend/components/ui/`                                                                                                                                                       |
| Pages            | `frontend/app/` (dynamic routes split: `page.tsx` wrapper + `client-page.tsx`)                                                                                                  |
| Tests (Rust)     | Inline `#[cfg(test)]`, per-resource `crates/api/tests/api_*.rs` (shared helpers in `crates/api/tests/common/mod.rs`)                                                            |
| Tests (Frontend) | `frontend/__tests__/**/*.test.{ts,tsx}`, `frontend/__tests__/msw/handlers.ts`                                                                                                   |
| CI               | `.github/workflows/ci.yml`                                                                                                                                                      |

## Dependencies

**Rust**: axum 0.8, rocksdb, tokio 1.42, serde, ckb-types/ckb-hash 0.119, anyhow/thiserror
**Frontend**: vite 5, react 19, react-router-dom 7, @tanstack/react-query 5, zustand 5, tailwindcss 3.4
