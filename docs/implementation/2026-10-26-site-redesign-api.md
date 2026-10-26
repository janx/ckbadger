# Site redesign API lane

## Goal

Implement round-one API corrections and the read contracts used by the homepage, block, transaction, and activities pages.

## Principle Alignment

All changes in this lane are read-time. The API does not write domain, append-only, or network stores. Existing chain-derived classifications and persisted participant deltas are the source of truth. Missing node RocksDB evidence produces an explicit error, not a second RPC path.

## Field decisions (recorded before implementation)

| Items       | Decision               | Source                                                                                                |
| ----------- | ---------------------- | ----------------------------------------------------------------------------------------------------- |
| Fix 1       | Read-time              | Existing identity classification in chart series                                                      |
| Fix 2, 3    | Read-time              | Collection aggregates: omit token transfer count on collections, expose liveCount and activitiesCount |
| Fix 4       | Read-time              | Existing token transfer sequence + transaction lookup for actual tx index                             |
| Fix 5       | Read-time              | Object live outpoint index; separate createdAtTx                                                      |
| Fix 9       | Read-time              | Indexed TokenInfo and exact cell bytes; no heuristic beside deterministic interpretation              |
| Fix 10      | Read-time verification | Existing persisted exact rolling_24h snapshot path already on main                                    |
| Fix 12, 13  | Read-time              | Fiber channel fields and decoded since epoch fraction                                                 |
| Fix 14      | Read-time              | Node RocksDB orphan block bytes                                                                       |
| Gap 1       | Read-time/lazy         | Node serialized block size; existing lazy transaction cycles sum only when complete                   |
| Gap 2       | Read-time              | Node economic state / canonical reward formulas                                                       |
| Gaps 3–5    | Read-time              | Transaction inputs, outputs, witnesses, node dependency cells                                         |
| Gap 6       | Lazy                   | Existing cycles worker; coordinated with indexer owner before adding persisted fields                 |
| Gap 9       | Read-time              | Shared store participant positive-delta function; cellbase excluded                                   |
| Gaps 10, 11 | Read-time              | Immutable pool mirror snapshot, interpreted records, cursor paging                                    |
| Gap 15      | Read-time              | Stored/pool lock script + registry; nullable only when unresolved                                     |
| Gaps 21, 28 | Read-time              | Existing exact item deltas and protocol actions                                                       |

## Result

Implementation and validation are recorded below as work lands. Re-sync required: no for this lane; the coordinated indexer lane changes daily CKB moved semantics.

## Implemented read contracts

- Collection rows expose live/activities counts and omit the token-only transfer count. `.bit` remains an identity in utilization charts. Token history exposes both transfer sequence and canonical transaction index.
- Object detail distinguishes creation transaction from its exact current live outpoint. Identity history reads strict persisted item actions; `.cell` reuses the indexer's exhaustive action mapping. The only indexer edits in this lane publicly export that existing pure mapping; there is no changed write path.
- Cell detail reads nonempty data from the node's original transaction, fails with context if unavailable, and enriches deterministic UDT decoding with exact integer decimal formatting and token metadata. Deterministic decoding suppresses competing heuristics.
- Fiber keeps raw delay epochs and serves one validated decoded epoch fraction; timeline fields use camelCase. Fork detail walks the actual abandoned branch in node RocksDB and exposes writer-recorded event reversal links.
- Transaction lists expose participant-positive Value through `TxActions::ckb_moved()`, exact output capacity, shared activity, serialized size, explicit cycle state, and consensus-window proposal origin for block-filtered rows. Cellbase has Value/cycles zero. Pending/failed cycles serialize null; malformed negative cycle state errors with the transaction hash.
- Transaction detail exposes script membership, exact occupied bytes per input/output and their totals, current output state, molecule witness fields, CoBuild and CKBFS byte ranges. Dependency expansion resolves dep groups from node bytes and matches actual executed script code/data hashes.
- `/activities?blockNumber=N` and `/transactions/{hash}/activity` reuse canonical transaction actions. Participant lock kind comes from the script registry display name; an unresolved script remains null.
- `/transactions/pool` reads one immutable interpreted pool snapshot. Canonical-indexed records are excluded, paging is stable by time/hash, incomplete interpretation remains explicit, and size/cycles/fee-rate/time summary is computed from all eligible records before slicing.

## Validation

Regressions were demonstrated before fixes in `/tmp/ckbadger-api-red.log`, `ckbadger-api-cell-required-red.log`, and `ckbadger-api-cycles-red.log`. Final targeted runs cover 637 API unit tests and 328 endpoint tests across activities, assets, cells, charts, DAO, Fiber, Spore, statistics, tokens, and transactions. All passed (`/tmp/ckbadger-api-suite7.log` plus the corrected and expanded `/tmp/ckbadger-api-suite8.log`). Tests include an uncle proposal origin, strict missing-node errors, identity update vs transfer, exact participant Value, full pool summary across pages, and empty healthy pool null ranges.

`cargo check --workspace -j4` and `cargo clippy --all-targets -j4 -- -D warnings` passed. Frontend focused contracts and general fixtures passed 104 tests. Local frontend typecheck's remaining errors belong exclusively to parent-owned block/transaction pages and fixtures not merged into this isolated lane; integration must run the combined typecheck. No live database or service was touched.

Primary format evidence: CKBFS `RFC.v3.md` from code-monad/ckbfs and CoBuild `schemas/basic.mol` / `top_level.mol` from cryptape/ckb-transaction-cobuild-poc. Decoder tests verify field boundaries and reject malformed nested molecules.

Per-group cycles enrichment is a coordinated follow-up after the indexer-owned in-process VM result contract lands. Block list/detail evidence and transaction class filters are parent-owned integration changes. API changes alone require no re-sync; the strict identity-history and shared derived-counter write contracts require the coordinated indexer rebuild.
