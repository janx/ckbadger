# Site redesign API lane

## Goal

Implement round-one API corrections and the read contracts used by the homepage, block, transaction, and activities pages.

## Principle Alignment

API changes in this lane are read-time. The API does not write domain, append-only, or network stores. The delegated cycles-worker exception below only uses its existing indexer-owned domain write path. Existing chain-derived classifications and persisted participant deltas are the source of truth. Missing node RocksDB evidence produces an explicit error, not a second RPC path.

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

Per-group cycles enrichment and transaction class filtering are completed by the follow-up below. Block list/detail evidence remains parent-owned integration work. API changes alone require no re-sync; the strict identity-history and shared derived-counter write contracts require the coordinated indexer rebuild.

## Follow-up read decisions

- Transaction class filtering reuses the activity classifier before paging. Non-cellbase rows use the existing canonical action scan; cellbase uses canonical transaction indexes because production intentionally does not persist its action rows. Filtered global totals are omitted rather than reporting an unrelated count. Pool partial records appear only in All, and summaries use the full filtered set.
- Group cycles join the indexer's canonical result by group kind and script hash. Displayed output-only lock groups do not receive execution cycles. Calculation completion requires the validated group record; a pre-existing positive scalar remains visible but does not mean group calculation has completed. Each long-poll attempt pins only its local reads.
- Cellbase block/detail activity is read-time interpretation of original node outputs through the existing shared indexer interpreter. Global activity semantics remain unchanged. This adds no persistent action rows or index.
- Genesis and cellbase have no script execution. API reads return complete zero and clear executed-group flags; nonzero persisted values fail with transaction context. The owning cycles worker explicitly persists zero for genesis through its existing domain-only path and refuse contradictory nonzero state. No append-only write path is introduced or changed.

Follow-up regressions were run after the coordinated performance quiet window and before fixes: `/tmp/ckbadger-api-class-filter-red.log`, `ckbadger-api-class-client-red.log`, `ckbadger-api-group-unit-red.log`, `ckbadger-api-group-red.log`, `ckbadger-api-followup-endpoints-red.log`, and `ckbadger-worker-genesis-red.log`. They reproduced incorrect scalar-only completion, hidden contradictory exempt cycles, genesis VM execution/pending state, and missing cellbase activity.

Follow-up validation passed 638 API unit tests, 77 transaction/activity endpoint tests, 9 cycles-worker tests, and 75 frontend API contract tests. Coverage includes sparse filtering across chunks, cellbase pagination without persisted actions, class-filtered pool summaries before pagination, exact group joining, canonical long-poll completion, real-node genesis type groups marked unexecuted, and genesis block-worker completion with positive-cache rejection. Workspace check, all-target clippy with `-D warnings`, formatting, and frontend lint passed. The isolated frontend typecheck still only reports parent-owned block/transaction page fixtures and nullable-cycle rendering changes absent from this lane; combined integration owns that final gate. All temporary RocksDB stores were placed under the worktree; no live store was touched. Re-sync required: no for this follow-up. Lazy writes target only the domain store; append-only paths are unchanged.

## Delegated lazy-store invariants

The final follow-up owns `tx_ops.rs` and `block_ops.rs`, coordinated with the indexer lane. Existing lazy writes remain domain-only and under the canonical write gate. A native group result must match an already positive scalar; invalid values below the explicit `-1` failure marker are rejected. Block publication requires every ordinary transaction to have positive cycles, while genesis accepts only absent/zero transaction values. Any existing block aggregate, including zero, must agree with the validated exact sum. Contradictory state errors without changing the stored total, groups, or header. The existing append-only boundary remains enforced and covered by temporary-store assertions. No new schema, bulk path, or append-only mutation is introduced.

The store RED run (`/tmp/ckbadger-api-store-cycles-red.log`, repeated with the shared API feature graph) reproduced all four regressions before implementation. After the fix, the complete API/store unit suites passed 638 + 707 tests, all 9 cycles-worker tests and 40 transaction endpoint tests passed against the tightened writer contract. Workspace check and formatting passed. Assertions cover unchanged state after errors, preserved `-1` failure-marker semantics, and rejection of both scalar/group writes and block publication on an append-only store. Re-sync required: no; these are lazy write invariants, with no bulk-sync path change.
