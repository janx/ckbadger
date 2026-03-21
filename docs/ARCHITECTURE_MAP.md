# Architecture Map

Quick navigation map for humans and agents working in `ckbadger`.

## Runtime Data Flow

1. `crates/indexer` fetches blocks and transactions from CKB RPC.
2. `crates/indexer/src/parser/` converts raw chain data into domain models.
3. `crates/indexer/src/db/writer/` writes indexed data into `crates/ckbadger-store` (RocksDB) and sync
   progress into Redis (optional `redis-cache` feature).
4. `crates/api` serves REST and WebSocket data from RocksDB (+ Redis cache when enabled).
5. `frontend` consumes `/api/v1` + `/ws` and renders explorer pages.

## Module Map

| Layer                    | Entry Points                                              | Core Files                                                                                                                                                                                       | Tests                                                                                     |
| ------------------------ | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| Indexer                  | `crates/indexer/src/main.rs`, `crates/indexer/src/lib.rs` | `crates/indexer/src/sync/indexer.rs`, `crates/indexer/src/sync/bulk_build/mod.rs`, `crates/indexer/src/parser/*.rs`, `crates/indexer/src/db/writer/*.rs`, `crates/indexer/src/verify/mod.rs`     | `cargo test -p ckbadger-indexer` + inline `#[cfg(test)]`                                  |
| Store                    | `crates/ckbadger-store/src/lib.rs`                        | `crates/ckbadger-store/src/store.rs`, `crates/ckbadger-store/src/sst_ingest.rs`, `crates/ckbadger-store/src/types.rs`, `crates/ckbadger-store/src/keys.rs`, `crates/ckbadger-store/src/*_ops.rs` | `cargo test -p ckbadger-store` + inline `#[cfg(test)]`                                    |
| API                      | `crates/api/src/main.rs`, `crates/api/src/lib.rs`         | `crates/api/src/routes/mod.rs`, `crates/api/src/routes/*.rs`, `crates/api/src/ws/*.rs`, `crates/api/src/response.rs`                                                                             | `cargo test -p ckbadger-api`, `crates/api/tests/api_integration.rs`                       |
| Shared types             | `crates/common/src/lib.rs`                                | `crates/common/src/types.rs`, `crates/common/src/sync.rs`, `crates/common/src/dao.rs`                                                                                                            | `cargo test -p ckbadger-common`                                                           |
| Optional direct CKB read | `crates/ckb-store-reader/src/lib.rs`                      | `crates/ckb-store-reader/src/convert.rs`                                                                                                                                                         | `cargo test -p ckb-store-reader`                                                          |
| Frontend                 | `frontend/src/main.tsx`, `frontend/src/routes/router.tsx` | `frontend/app/**/page.tsx`, `frontend/lib/api.ts`, `frontend/components/**/*.tsx`                                                                                                                | `cd frontend && pnpm build`, `cd frontend && pnpm test`, `cd frontend && pnpm type-check` |

## Common Change Entry Points

| Task                            | Start Here                                                                                                      | Also Update                                                                                                                                                                       |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Storage/key design change       | `crates/ckbadger-store/src/types.rs`, `crates/ckbadger-store/src/keys.rs`, `crates/ckbadger-store/src/*_ops.rs` | `crates/indexer/src/db/writer/*.rs`, API readers in `crates/api/src/routes/*.rs`                                                                                                  |
| Script reference/version change | `docs/SCRIPTS_CODE_CELLS_AND_REFS.md`, `crates/ckbadger-store/src/types.rs`, `crates/indexer/src/sync/*.rs`     | `crates/indexer/src/db/writer/addresses.rs`, `crates/api/src/utils/script_resolution.rs`, `crates/api/src/routes/scripts.rs`, `frontend/app/script/**`, `frontend/app/scripts/**` |
| Bulk-build engine change        | `crates/indexer/src/sync/bulk_build/mod.rs`                                                                     | Reducers in `crates/indexer/src/sync/bulk_build/owners/*.rs`, `live_cells.rs`, `materialize.rs`                                                                                   |
| New parser capability           | `crates/indexer/src/parser/*.rs`                                                                                | Writer modules in `crates/indexer/src/db/writer/*.rs`, inline parser tests                                                                                                        |
| New API endpoint                | `crates/api/src/routes/*.rs`                                                                                    | `crates/api/src/routes/mod.rs`, `frontend/lib/api.ts`, `crates/api/tests/api_integration.rs`                                                                                      |
| Frontend page or data view      | `frontend/src/routes/router.tsx`, `frontend/app/**/page.tsx`                                                    | `frontend/components/**`, `frontend/lib/api.ts`, `frontend/__tests__/**`                                                                                                          |
| Verification logic              | `crates/indexer/src/verify/*.rs`                                                                                | `crates/indexer/src/verify/mod.rs`, report formatting in `crates/indexer/src/verify/report.rs`                                                                                    |

## Fast Validation Shortcuts

```bash
# Rust core
cargo check
cargo test -p ckbadger-indexer
cargo test -p ckbadger-api

# Frontend
cd frontend && pnpm type-check
cd frontend && pnpm test

# Data integrity
cargo run -p ckbadger-indexer -- verify --depth fast
```
