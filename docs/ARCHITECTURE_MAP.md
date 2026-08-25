# Architecture Map

Quick navigation map for humans and agents working in `ckbadger`.

## Runtime Data Flow

1. `crates/cli` reads either a single-network `config.toml` or an orchestrator
   `ckbadger.toml` whose `[[network]]` entries point at per-network work directories.
2. In orchestrator mode, one supervisor starts every API, every enabled crawler, and one shared
   frontend immediately. Indexers are admitted in `[[network]]` order so only one fresh network
   performs bulk sync at a time.
3. Each indexer validates its configured network against the CKB node, derives the exact
   `GenesisBaseline` from block 0 when absent, fetches chain data, parses it, and writes its own
   domain and append-only RocksDB stores.
4. Each API opens those two chain stores as read-only secondaries, refreshing them on a catch-up
   loop. A direct per-network API serves `/api/v1/*` and `/ws`.
5. The shared frontend serves pages under `/{network}/...` and proxies
   `/api/{network}/v1/*` → that network's `/api/v1/*`, plus
   `/ws/{network}` → that network's `/ws`.
6. The opt-in crawler observes the selected CKB p2p network and is the sole writer of that
   network's separate, TTL-retained network store. The API reads it as a secondary.

## Store Ownership

| Store       | Sole writer         | Readers                   | Contents                                                                  |
| ----------- | ------------------- | ------------------------- | ------------------------------------------------------------------------- |
| Domain      | Per-network indexer | API/TUI secondary readers | Mutable canonical chain state, indexes, aggregates, sync metadata         |
| Append-only | Per-network indexer | API secondary reader      | Immutable cell payloads keyed by outpoint                                 |
| Network     | Per-network crawler | API secondary reader      | Non-chain p2p observations; TTL-retained and not rebuildable from genesis |

A RocksDB secondary has no snapshots, so a reader's view changes only when catch-up runs.
`crates/ckbadger-store/src/read_view.rs` makes catch-up exclusive with pinned read scopes, and the
API pins one view per request: any new read path that resolves an index row and then loads the row
it points at is coherent by default, and a handler that waits for the indexer to write new data
(long-poll) must release its pin first. See
[docs/STORE_SCHEMA.md](STORE_SCHEMA.md#read-consistency-secondary-readers).

## Module Map

| Layer                    | Entry points                                              | Core files                                                                                             | Tests                                                     |
| ------------------------ | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | --------------------------------------------------------- |
| CLI / supervisor         | `crates/cli/src/main.rs`                                  | `supervisor.rs`, `sequencer.rs`                                                                        | `cargo test -p ckbadger`                                  |
| Configuration            | `crates/config/src/lib.rs`                                | `orchestrator.rs`                                                                                      | `cargo test -p ckbadger-config`                           |
| Indexer library          | `crates/indexer/src/lib.rs`, `entry.rs`                   | `network_guard.rs`, `genesis_baseline.rs`, `lifecycle.rs`, `sync/`, `parser/`, `db/writer/`, `verify/` | `cargo test -p ckbadger-indexer`                          |
| Store                    | `crates/ckbadger-store/src/lib.rs`                        | `store.rs`, `read_view.rs`, `types.rs`, `keys.rs`, `*_ops.rs`, `network_*`                             | `cargo test -p ckbadger-store`                            |
| API / frontend server    | `crates/api/src/lib.rs`, `entry.rs`                       | `routes/`, `ws/`, `frontend_proxy.rs`, `embedded_frontend.rs`, `response.rs`                           | `cargo test -p ckbadger-api`, `crates/api/tests/api_*.rs` |
| Crawler                  | `crates/crawler/src/lib.rs`, `rpc_observer.rs`            | round engine, p2p prober, configured-node RPC session observer, network-store writer                   | `cargo test -p ckbadger-crawler`                          |
| TUI                      | `crates/tui/src/lib.rs`                                   | `multi.rs`, service/sync/memory/network panels                                                         | `cargo test -p ckbadger-tui`                              |
| Shared domain logic      | `crates/common/src/lib.rs`                                | `network.rs`, `burn_policy.rs`, `dao.rs`, `token.rs`, `types/`                                         | `cargo test -p ckbadger-common`                           |
| Optional direct CKB read | `crates/ckb-store-reader/src/lib.rs`                      | `convert.rs`                                                                                           | `cargo test -p ckb-store-reader`                          |
| DOB decoder              | `crates/dob-decoder/src/lib.rs`                           | `vm.rs`, `cache.rs`, `fetch.rs`, `types.rs`                                                            | `cargo test -p ckbadger-dob-decoder`                      |
| Frontend                 | `frontend/src/main.tsx`, `frontend/src/routes/router.tsx` | `frontend/app/`, `frontend/lib/api.ts`, `frontend/lib/active-network.ts`, `frontend/components/`       | `cd frontend && pnpm type-check && pnpm test`             |

## Canonical Detection Metadata

Protocol detection starts from `docs/metadata/scripts/*.toml`. The indexer build bundles that
metadata, and `crates/indexer/src/parser/registry.rs` constructs one fail-fast,
network-agnostic `ProtocolRegistry` keyed by `code_hash`. Parser, bulk-build, activity, and API
classification paths must consume that shared registry or a single derived lookup; do not add a
second hardcoded protocol-hash list.

## Common Change Entry Points

| Task                           | Start here                                                                                   | Also update                                                                                             |
| ------------------------------ | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Orchestrator/network behavior  | `crates/config/src/orchestrator.rs`, `crates/cli/src/main.rs`, `crates/cli/src/sequencer.rs` | `crates/api/src/frontend_proxy.rs`, `frontend/lib/active-network.ts`, README/config docs                |
| Storage/key design change      | `crates/ckbadger-store/src/types.rs`, `keys.rs`, `*_ops.rs`                                  | Indexer owning writer, API readers, `docs/STORE_SCHEMA.md`; state logical store and re-sync requirement |
| Script/protocol detection      | `docs/metadata/scripts/*.toml`, `crates/indexer/src/parser/registry.rs`                      | Relevant parsers/writers, activity builder, metadata tests                                              |
| Script reference/version model | `docs/SCRIPTS_CODE_CELLS_AND_REFS.md`, store types/keys                                      | `db/writer/addresses.rs`, API script resolution/routes, frontend script pages                           |
| Bulk-build engine              | `docs/prompts/BULK_SYNC.md`, `crates/indexer/src/sync/bulk_build/mod.rs`                     | `bulk_build/owners/`, `live_cells.rs`, `materialize.rs`, memory/perf tests                              |
| Parser capability              | `crates/indexer/src/parser/*.rs`                                                             | Writer/bulk owner, parser registry when protocol-bound, inline parser tests                             |
| API endpoint                   | `crates/api/src/routes/*.rs`                                                                 | `routes/mod.rs`, `frontend/lib/api.ts`, API/frontend tests, `docs/API.md`, agent discovery files        |
| Frontend route/view            | `frontend/src/routes/router.tsx`, `frontend/app/`                                            | components, API client, network-aware route helpers, frontend tests                                     |
| Verification logic             | `crates/indexer/src/verify/*.rs`                                                             | registration tests and `docs/TESTING.md` counts                                                         |

## Fast Validation Shortcuts

```bash
cargo check
cargo test -p ckbadger-indexer
cargo test -p ckbadger-api
pnpm --dir frontend type-check
pnpm --dir frontend test
ckbadger verify --depth fast
```
