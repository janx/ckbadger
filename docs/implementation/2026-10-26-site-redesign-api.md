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

## TypeID retirement read contract

Independent review of the indexer's retirement fix found two API assumptions incompatible with its canonical contract: historical registered references without a current binding caused family endpoints to fail, and current-code lookup could promote a retired non-executable typed payload. The read-time decision is to use observed `type_id_deployments` as historical provenance and the canonical reference mapping as the sole current binary binding for those references. No binding is a legitimate retired/burned state; direct binary-hash history remains queryable. Registered canonical metadata without observed lineage retains its existing missing-mapping invariant check. Generic non-TypeID typed-code resolution is outside this fix. No schema, writer, bulk, or append-only mutation is added.

RED runs reproduced both incorrect current-binary resolution and family HTTP 500s (`/tmp/ckbadger-api-typeid-red.log`, `ckbadger-api-typeid-endpoints-red.log`). After the fix, all 639 API unit tests and 37 script endpoint tests passed, including retirement, burn, subsequent reactivation, preserved historical metadata, and the unchanged missing-mapping invariant regression. API all-target clippy with `-D warnings` and formatting passed. The full unit run needed permission to bind temporary mock-server sockets; its first sandbox run failed solely with socket EPERM. Re-sync required: no for this API-only follow-up.

## Family history after TypeID retirement

Follow-up review found that current executable membership incorrectly also selected family history: retiring a TypeID hid all its historical daily deltas and surviving user cells. Family membership now comes from the shared store provenance index (observed TypeID lineage plus registered family), independently of the optional current binary mapping. Family capacity history and utilization retain those references; version references and `byDeployment` remain restricted to current binary bindings. No retired reference is assigned to its last binary. API `/usage` uses the writer's shared exact reference aggregation operator, validates it against persisted family counters, and reads the additional nonpersisted raw capacity totals from that same operator. The writer owns the corresponding domain rollup correction; no new schema or append-only write is needed.

Family metadata, indexes, current bindings, and mutable version counters are read from one pinned domain view. The version reader no longer mixes an older warmup-cache row with a newly committed family index. RED tests reproduced lost nonzero daily history (`/tmp/ckbadger-api-family-history-red.log`) and same-binary reactivation reporting zero users from stale cached counters (`/tmp/ckbadger-api-family-cache-red.log`). The fixtures preserve six live reference users and exact two-day capacity/occupied changes of 1000/600 to 800/500. They verify family detail, usage, both charts, retirement/burn, different- and same-binary reactivation, no duplicate family membership, no fabricated version attribution, and rejection of inconsistent persisted family totals. The existing usage fixture was completed with its corresponding canonical reference rows and persisted family totals; all original numerical assertions remain unchanged.

The affected full suites passed 639 API unit tests and 121 endpoint tests (37 scripts, 23 charts, 61 statistics). Workspace check and strict all-target workspace clippy passed. A final same-view regression reproduced HTTP 500 with an older empty family-name cache (`/tmp/ckbadger-api-family-names-red.log`); utilization now joins pinned family metadata as well as pinned versions, and the regression passes. Formatting and diff checks pass. Root owns the combined final gates after the writer/API commits. Re-sync required: no for these API-only reads; the coordinated writer rollup correction uses the already-planned rebuild. The API adds no persistent writes, schema, or append-only mutation.

## USDI metadata provenance correction

The mainnet USDI Type ID `0xbfa35a9c38a676682b65ade8f02be164d48632281477e36f8dc2f41f79e56bfc` has one complete indexed deployment history entry: `0xf6a5eef65101899db9709c8de1cc28f23c1bee90d857ebe176f6647ef109e20d:0`, block 14,861,099. Its 33,696-byte ELF hashes exactly to the standard xUDT binary `0x50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95`; it remains live with no consumption or upgrade through node indexer tip `0x13dfe48`. Read-only `get_cells`, `get_transactions`, and original transaction data established these facts; raw evidence is retained in the scratch performance results as `api-usdi-metadata-node-proof.json`. The mainnet anchor moves to xUDT, preserving one binary per family and the separate USDI token identity. The existing distinct testnet binary remains unchanged: its local node on port 8124 was unavailable, so it was not independently revalidated.

The regression first reproduced the incorrect USDI Asset script label (`/tmp/ckbadger-api-usdi-metadata-red.log`), then checks the xUDT binary/label and absence of a fictitious separate mainnet USDI script family. The coordinated importer change owns lossless multiple registered anchors and rejects conflicting families before mutation. No API writer or append-only change is introduced.

## Explicit registered anchors versus copied deployments

Real-chain TypeID `0xe305abb02155db4393497ac45377808b7210bee5a0fe9b28f3e793d35714fec2` exposed a second distinction: a third-party TypeID may copy binaries from different registered families over time. Observing those deployments does not register that reference permanently to either family. The API consumes the shared `registered_references` contract; only explicitly registered anchors with observed lineage keep a family after retirement. Unregistered copies follow the current binary's family, and have no family when retired. All observed native TypeIDs still use canonical executable mappings, so a retired non-ELF payload cannot become code even when its reference is unregistered.

The API's missing-mapping invariant checks every registered anchor, including a second anchor that is not the preferred display reference. Synthetic version rows contain no registrations. Retired test fixtures register only imported seed versions; discovered versions retain observations without creating registrations. Family charts aggregate the capacity history of their selected reference members; this does not attribute historical transaction execution to today's binary, and no historical-execution index is added.

The new endpoint regression first reproduced the production-shaped conflicting-family HTTP 500 (`/tmp/ckbadger-api-unregistered-family-red.log`). It checks family detail, usage, capacity history, utilization, and executable lookup while an unregistered TypeID selects each of two binaries and then retires. Existing six-live-user retirement and reactivation regressions remain in place for explicitly registered references.

The second-anchor RED run returned HTTP 200 instead of the required invariant error (`/tmp/ckbadger-api-second-anchor-red.log`); after enumerating all registered anchors it passes. Mainnet USDI now also classifies as native xUDT through the unchanged shared protocol registry. The prior compatible-only test used a premise disproved by the node's exact code hash. Its negative assertion is retained using the existing distinct testnet USDI metadata, and a new positive assertion covers the verified mainnet xUDT deployment. This is a metadata-driven classification correction, not a special-case hash rule or a fresh claim about the unavailable testnet node.

Final affected suites pass against the coordinated writer/importer: 640 API unit tests, 40 script endpoint tests, 23 chart endpoint tests, and 61 statistics endpoint tests (`/tmp/ckbadger-api-registered-anchors-suite.log`). The complete run uses temporary local mock-server sockets and lane-local temporary RocksDB stores; the earlier sandbox chart attempt failed only its two socket-binding tests, both pass in this final run. Workspace check, formatting and strict all-target clippy pass. Re-sync required: yes for the coordinated `registered_references` domain schema; it uses the plan's rebuild rather than a backfill. These API changes add no persistent write or append-only mutation.
