# Data exactness execution record

## Goal

Execute `2026-10-25-data-exactness-fix-plan.md` against the completed homepage integration baseline `fce23b8d`. Product implementation is delegated to GPT-6 Sol agents; the root agent performs coordination, independent audit, integration and validation.

## Principle Alignment

- Canonical chain facts remain the source of truth. Raw block timestamps remain available, while all statistics use the same monotonic chain-time definition.
- Store schema changes are rebuilt from genesis; no compatibility backfill or repair calculation path is introduced.
- Indexer is the only chain-store writer. New aggregates belong to domain; immutable cell payloads remain append-only.
- Missing required values are errors. Legitimately unavailable values stay explicit rather than becoming numeric zero.

## Store write boundaries

- Domain: indexer writes canonical headers with `chain_time_ms`, exact daily work sums, the unified day/hour aggregates, DAO daily/latest and rolling-window data, transaction-index hashes, and optional runtime observations. Rollback restores/reduces this mutable state.
- Append-only: immutable `CF_CELLS` payloads retain new-key append semantics. No repair, overwrite, or delete path was added. Store and indexer rollback integration tests verify that canonical rollback preserves append-only payloads.
- Network: only crawler publishes optional GeoIP coordinates in `CF_NET_NODES`; this separate observation store is recreated by crawling, not by chain replay.
- API remains read-only. Rolling activity reads indexed transactions under the request's pinned view and caches by canonical tip hash, including same-height reorg invalidation.

## Accepted decisions

1. All daily/hourly statistics use `t*(b) = max(ts(b), t*(b−1))`.
2. DAO “Change in 24 h” is a real rolling 24-hour change.
3. Homepage Addresses is the number of distinct full lock addresses in the rolling window, with the existing activity scope excluding cellbase.
4. Capacity bars use the real ratio; a zero segment is not drawn.
5. The user confirmed there is no alternative testnet RPC and explicitly retained testnet verification as an outstanding acceptance item.

## Isolation and evidence

Integration branch: `integration/data-exactness` in `.worktrees/data-exactness`. Main and the existing services/data directories are not modified. No push, main merge, purge or re-sync has been performed.

The independent mainnet header scan covered heights 0 through 20,824,832 inclusive and verified every parent link and the captured tip hash. No raw timestamp reversals or changes to daily/hourly membership were found in that captured prefix. Integer difficulty calculations still differ from the old bulk floating-point algorithm on 2,459 complete dates. The audit report is in [mainnet-header-audit.md](2026-10-25-data-exactness-evidence/mainnet-header-audit.md). Large raw JSON/progress artifacts and the temporary scanner are retained under `test_outputs/data-exactness/`.

Local testnet RPC `127.0.0.1:8124` was unavailable. The user confirmed no alternative endpoint is available and accepted retaining testnet full-header verification as pending. No node was started to bypass the plan’s service boundary.

The connected browser surface was unavailable, but a local Chromium executable was present. A separate ephemeral static server and headless browser verified CommandPalette at 1440×900 and 390×844: overlay equals the entire viewport, its parent is BODY, the panel stays within viewport width, and Escape removes it. Both screenshots were inspected. This is layout validation using an explicitly unavailable API response; it does not claim post-resync homepage data acceptance. Artifacts: `test_outputs/data-exactness/command-palette-{1440,390}.png` and `command-palette-visual.json`.

## Final candidate and validation

Final integration candidate: `23f59e31` on `integration/data-exactness`; product source is unchanged since `0a86a3a8`, with the final commit completing API integration-test fixtures. All product and test implementation was performed by GPT-6 Sol agents. Root reviewed, integrated and validated it.

- Frontend: **164 files / 1,352 tests passed** (`pnpm exec vitest run --minWorkers=1 --maxWorkers=2`); type-check, lint and production build passed. Final logs: `/tmp/ckbadger-exactness-{vitest,build}-final-confirmed.log`; type-check/lint also passed in the `95b0728c` commit hook.
- Rust: **4,441 passed, zero failed, three pre-existing ignored across 65 test/doc-test targets** at `23f59e31`, exit status 0 (`CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test --workspace --no-fail-fast`). Log: `/tmp/ckbadger-exactness-workspace-final-23f59.log`. No new ignore/skip markers were added. The commit hook passed workspace all-targets Clippy with warnings denied and formatting.
- Canonical guard inventory: **1,589 baseline records + 98 added matching locations = 1,687 classified rows**, with no unclassified entries. The [triage table](2026-10-25-silent-guards-triage.md) also records semantic defects outside the search patterns.
- Independent review: API/frontend confirmation closed its original four findings; a separate runtime/core reviewer confirmed `ba1a446b`, `95b0728c` and the exact bench patch committed as `0a86a3a8`. No unresolved findings remained in those reviewed scopes. Reviewers inspected code, regression assertions and logs; they did not claim to rerun the final workspace suite themselves.

## Validation history

The following records are intermediate snapshots, including failed runs and their fixes. They do not supersede the final candidate results above.

- Frontend first full pass: 161 files / 1,318 tests; type-check, lint and production build passed. Follow-up contract changes have targeted tests and require the final integrated full run.
- API hotspot batch: 592 library tests passed in the Sol API worktree.
- Exact daily work: common helpers, store reorg tests, indexer boundary fixture, API chart/network tests passed before integration.
- Monotonic live writer: a backward-midnight DAO regression first reproduced the old depositor underflow, then passed single-block versus multi-block batch equivalence.
- Intermediate integrated `cargo check --workspace` passed after removing copied workspace-package artifacts; native dependency build artifacts were retained. This was before the second monotonic-time and remaining exact-ratio changes and does not replace the final checks.
- Integrated frontend pass at `e3dfb13c`: type-check and build passed. Vitest ran 1,320 tests; 1,316 passed on the first sandboxed run. The two local-listener failures and two load-sensitive UI waits passed together when rerun outside the socket-restricted sandbox with at most two workers (13/13 tests). A final integrated full run remains required after subsequent changes.
- First integrated Rust workspace run at `3a32da59`: all test targets compiled, then 17 targets failed. Failures include strict-state fixtures requiring canonical header prefixes/histograms/nonzero difficulty, absent embedded frontend build resources, and one timing-sensitive RPC test. The frontend resources have now been built; Sol agents own fixture fixes without relaxing product invariants. Detailed temporary output: `/tmp/ckbadger-exactness-workspace-tests-first.log`. This is not a passing acceptance run.
- Rolling activity fixture: 524 transactions / 4 blocks covers a 512-row page boundary, cutoff equality, cross-hour deduplication and same-height reorg cache invalidation. Local measured old aggregate read 211 µs; exact fresh scan 9,324 µs; tip-hash cache hit 437 µs. These are synthetic-test measurements, not production performance claims.
- Store rollback fixtures now build the exact forward histogram from adjacent canonical chain-time headers. The Sol store run passed all 688 library tests; this does not replace the final integrated workspace run.
- Integrated `cargo clippy --all-targets -- -D warnings` and fmt passed during commits `4233ba6d` and `d2297490`. Later commits still require the final checks. Frontend production build passed at `e6be4f9b`. Its complete API test run finished with 1,100 passing and 15 failing tests across 25 targets; the 611 library tests passed. Thirteen failures were old fixtures missing the now-required canonical transaction hash; the Sol follow-up passed all 152 tests in the four affected targets. The remaining timestamp/warmup fixtures are being corrected without weakening the production checks. This is an intermediate run, not final acceptance.

- Integrated frontend at `6d17719f`: all 162 files / 1,326 tests passed with at most two workers; type-check and lint also passed. Further commits still require the final run. Temporary logs: `/tmp/ckbadger-exactness-vitest-fees.log`, `/tmp/ckbadger-exactness-typecheck-fees.log`, `/tmp/ckbadger-exactness-lint-fees.log`.

- CommandPalette browser check at `6f3f926d`: desktop and mobile assertions passed; production frontend build passed before the check. No existing service or DB was accessed.

- Integrated store run at `d5cef26e`: 694 library tests passed, including append-only retention on rollback; `cargo check --workspace` also passed. Logs: `/tmp/ckbadger-exactness-store-telemetry.log`, `/tmp/ckbadger-exactness-check-telemetry.log`.
- Integrated indexer integration run at `f9d92b45`: all 90 tests passed across bulk engine, materialization, crash, DAO daily, entity rollback, object protocol and reorg targets. Log: `/tmp/ckbadger-exactness-indexer-integration-fixtures.log`.
- Independent RPC verification now reconstructs chain time from the complete canonical header prefix and binds the scanned final hash to the exported anchor. Insufficient explicit RPC/time budget is Inconclusive, not a successful sparse approximation.
- GeoIP coordinates are optional and lookup decode errors propagate. This changes only the crawler-owned network store schema; an existing network store must be recreated by crawling, independently of chain re-sync. No existing store was changed during implementation.

- Integrated frontend at `5fc1c78f`: all 162 files / 1,334 tests passed; type-check and lint passed. A first Vitest invocation exited before running tests because its implicit minimum worker count exceeded the explicit maximum; rerun with `--minWorkers=1 --maxWorkers=2` passed. Logs: `/tmp/ckbadger-exactness-{vitest,typecheck,lint}-charts.log`.
- The nonempty DAO rollback/replay differential found an upstream cohort accumulator drift that the prior code called negligible. The canonical rollback path now reverses the exact cohort delta (`8b7468e9`, `d83d8192`), with regression evidence for negative/duplicate state and zero-cohort representation; API reads do not compensate for a wrong accumulator.

- Second integrated Rust workspace run (Rust snapshot `33acb2c5`): 4,388 passed, 18 failed, 3 existing ignored tests across 65 test/doc-test targets. Four targets failed: API statistics (one node-tip mock missing), common sync (one required-vs-optional telemetry fixture), indexer library (two optional diagnostic formatting assertions and one shared DotCell error text assertion), and entity-statistics verification (13 mock sources lacking the newly required complete canonical prefix). No assertions are being relaxed to accept incorrect data; the fixtures must express the new contracts. Full output: `/tmp/ckbadger-exactness-workspace-33ac.log`. Final acceptance remains pending.

- Final frontend candidate at `4e06d5b5`: all 163 files / 1,344 tests passed; type-check, lint and production build passed. Logs: `/tmp/ckbadger-exactness-{vitest,typecheck,lint,build}-final.log`. The only later frontend change is a comment replacing the obsolete median-time description with the chain-time watermark.
- Final TUI lane check: 153 tests and clippy passed before integration of the overlap/OOM and clock/client-init changes. The final integrated Rust run remains required.

- Third integrated Rust workspace run at `d3c4dbb0`: 4,423 passed, one failed, three existing ignored across 65 test/doc-test targets. The only failure was an API fixture expecting the old invalid-tip-timestamp diagnostic; the API returned the correct contextual 500. `a26014b9` updates the exact diagnostic assertion (including `9223372036854775807 ms`), and that regression passes. Log: `/tmp/ckbadger-exactness-workspace-d3c4.log`. Later rollback/parser/performance changes still require final validation.
- Expanded DAO live single/multi-batch and rollback/replay comparisons cover query-domain CFs, excluding only operational sync metadata and batch-dependent undo preimages. They exposed missing reversal of created-and-consumed cell counts and cumulative ScriptInfo/ScriptReference capacity facts. The owning rollback deltas are fixed in `a7c7ebb6`; targeted differences and 90 store reorg tests passed in the lane. Append-only payloads are compared unchanged. `e59d18fd` additionally merges lock/type updates by the actual persistent key and rejects a missing reference row; the same-hash regression first failed and then passed. Older indexer fixtures now supply the complete forward script deltas rather than omit them (`f5427b95`). The subsequent indexer library run passed all 1,777 tests; strengthened exact undo-retention assertions also passed.

- Parser entity deltas now contain undated identities (`6c4d4caf`, `00bfbbc4`). Only the writer assigns the day from canonical chain time. The former raw-timestamp date computation in the parser is removed, and live daily/hourly undo, long-batch retention and DAO rollback/replay targets pass after integration.

- Combined nonempty-chain differential (`b2f49d2a`): one continuous genesis→3 chain includes DAO deposit/request/completion, xUDT mint/transfer, .cell create/register, and two backward timestamps across day/hour boundaries. Every `stats_*` and `dao_*` CF is compared byte-for-byte among bulk, one-block live, multi-block live, and rollback/replay; DAO/token/object rows are asserted nonempty. The test exposed the bulk object owner's missing type index, now fixed at its output writer. It also compares bulk/live immutable cell payloads and asserts payloads remain unchanged through rollback/replay. Integrated targeted regression passed after conflict resolution; it also passed in the fourth workspace run recorded below.
- Final date-call sweep reviewed daily/hourly helper calls and raw timestamp conversions in writer, rollback and API boundaries. Production entity deltas are undated until the writer supplies canonical chain time; the remaining raw-time helper is explicitly diagnostic and raw timestamps remain in block/transaction display. Search evidence: `/tmp/ckbadger-date-call-final-sweep.txt` and `/tmp/ckbadger-raw-time-final-sweep.txt`.
- At candidate `b2f49d2a`, the canonical triage table covers 1,589 baseline matches plus 86 added matching locations, with no unclassified rows. No new test ignore/skip markers were added.

- Fourth integrated Rust workspace run at `b2f49d2a`: 4,419 passed, nine failed, three existing ignored across 65 targets. All nine failures are in `reorg_handling.rs`: its manual forward fixtures omitted cumulative ScriptInfo counts now required by strict rollback. The complete 1,778-test indexer library run passed, including the combined nonempty-chain differential. The fixture correction is integrated as `a624ba64`; all 18 reorg integration tests and targeted clippy pass. Final rerun waits for independent-review fixes. Log: `/tmp/ckbadger-exactness-workspace-final-b2f49.log`.

- Independent API/frontend review corrections: `5fca54dd` selects the collection's actual statistics family for detail/chart and distinguishes legitimate net-zero history from missing live history; API assets 60/60 and API library 625/625 passed. `33390610` preserves signed Knowledge net-flow values in real LineChart rendering/tooltips and replaces the homepage allocation's minimum/rounded/residual widths with raw-amount geometry and omitted zero segments. Targeted frontend regressions, type-check and lint passed; the complete frontend run with local socket permission passed 164 files / 1,347 tests, and production build passed. Independent confirmation resolved the four original issues and found the nonpositive/log-scale interaction subsequently fixed in `95b0728c`.

- Fifth integrated Rust workspace run at `0a86a3a8`: 4,436 passed, five failed and three existing ignored across 65 targets. All failures were old Search/Spore fixtures that directly created live collection aggregates without the daily capacity history the writer also produces. `23f59e31` adds matching mint/melt histories and aggregate capacities in those fixtures, plus one related inferred-cluster fixture. It changes no product code and preserves every original assertion. The two targets then passed 15/15 and 27/27, and API all-targets Clippy passed. Logs: `/tmp/ckbadger-exactness-workspace-final-0a86.log`, `/tmp/ckbadger-runtime-api-{search,spore}-fixture-green.log`.

## Independent-review corrections

| Finding                                            | Final correction                                                                                                                      | Regression evidence                                                                                                                                                                        |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Collection history source and legal net-zero state | `5fca54dd`: shared source selection for detail/chart/range baseline, with exact zero only for legitimate empty live state             | API assets 60/60, library 625/625                                                                                                                                                          |
| Signed Knowledge flow and capacity geometry        | `33390610`: exact signed values/tooltips and raw-amount allocation widths; zero segments omitted                                      | Real Knowledge page rendering and allocation tests; final frontend suite                                                                                                                   |
| Log-scale domain                                   | `95b0728c`: log available only for strictly positive visible values, including data/zoom changes; fractional line/bar values retained | LineChart 12/12 and final frontend suite; positive fractional bar regression failed before correction                                                                                      |
| Truncated HTTP and missing body measurements       | `ba1a446b`, `0a86a3a8`: typed failures and one global/endpoint classification; unknown bytes remain null/n/a                          | Real truncated 200 and 429 regressions failed before correction; complete 429 and empty body retain their legitimate semantics; bench 60/60                                                |
| Partial heartbeat and disk sampling                | `ba1a446b`: preserve missing observations through complete-window aggregation                                                         | Disk mean runtime regression first failed; heartbeat test first required a new Option type, so its initial failure was compile-time, not a claimed runtime red; performance tests 60/60    |
| Cycles identity and completeness                   | `ba1a446b`: canonical transaction identities and atomic domain read-check-write; complete checked block sum                           | Stale slot overwrite failed before correction; gate/replacement test; tx 11/11, block 11/11, append-only 12/12, worker 6/6                                                                 |
| Orphan-only address first appearance               | `95b0728c`: remove fully reversed orphan-born addresses while retaining historical zero-balance addresses                             | Store first-seen regression, real cross-month fresh/replay comparison of address/cohort/tx-index bytes, and unchanged append-only cell payloads; store 701/701 and reorg integration 18/18 |

The cycles gate is per store handle. It covers direct put/delete and normal, synced and no-WAL batches. Conditional cycles writes hold the same gate through the final write and do not await or recursively acquire it. The repository has no separate SST ingest mutation entry point. Only domain TX_INDEX/BLOCK_HEADERS are changed by cycles; no generic append-only mutation API was introduced.

## Performance evidence

DAO refresh computes tip and rolling baseline through one exact lifecycle scan (`1aaa1ad9`), retaining a single per-entry calculation path. The controlled 8,192-entry mixed-lifecycle fixture measured median refresh time of 162.53 ms before the optimization and 101.30 ms after it (37.7% lower); every run asserted identical capacity and compensation outputs. The original homepage baseline was also compared with the final candidate. These debug-build synthetic measurements do not establish full-chain sync throughput. [DAO performance acceptance](2026-10-25-dao-performance-acceptance.md) records environment, all samples, lifecycle mix, phase measurements and reproduction commands.

Exact block fee statistics preserve the arithmetic mean of each transaction rate (not a byte-weighted rate). A synthetic debug-build fixture contains 1,000 transactions with 500 distinct serialized sizes. Ten iterations took approximately 194–319 µs with the old inexact float algorithm, 2,708,694 µs with the first exact rational implementation, 1,483,908 µs after incremental LCM reduction, and 13,040 µs after grouping by size and using bounded integer GCD. The final exact implementation is about 1.30 ms per 1,000 transactions in this fixture. These measurements describe the test build and synthetic input, not production or release-mode latency.

A small write-gate ABBA measurement used a fresh temporary domain RocksDB and 200 single-key CF_SYNC_META commits per segment. Direct/gated/gated/direct timings were **5,793 / 2,303 / 2,334 / 2,411 µs**. The first direct segment includes cold-start effects; this fixture measures only lock-inclusive commit cost, not concurrent contention or production throughput, and establishes no performance threshold. The temporary test harness was removed after measurement. Its actual stdout is transcribed in `/tmp/ckbadger-runtime-domain-write-gate-abba.log`.

## Regression evidence and review scope

Functional failing-before/passing-after evidence includes the backward-midnight live DAO underflow, negative prior DAO balance being silently healed by a positive delta, UDT hourly transfers crossing a backward timestamp, exact block fee statistics above 2^53, and unavailable network timing in the frontend. Some early helper tests failed to compile before their new helper existed; those are recorded as test-first implementation checks, not as proof that the old behavior executed and failed. The independent mainnet integer-versus-old-float comparison supplies separate numerical regression evidence.

The final guard inventory includes all baseline search hits and a separate review of matching newly added lines. Review continues beyond the initial backlog: raw-time UDT hour buckets and script-chart completeness were found by the full date-call sweep; chart tooltips that reused floating-point geometry values now format the original exact values. Optional monitoring observations must remain unavailable when missing; a partial sample cannot become a complete zero-valued measurement.

Independent review found the additional defects listed above; Sol fixed them, and independent confirmation closed them. The classification table was reconciled with those corrections. Raw red/green observations for runtime fixes are recorded in `/tmp/ckbadger-runtime-red-evidence.log` and `/tmp/ckbadger-runtime-truncated429-{red,green}.log`; the first file explicitly identifies transcribed interactive evidence and the heartbeat type-level failure. Post-resync RPC/API/verify checks require newly generated data and remain separate from synthetic regression validation.

## Result

- Status: implementation, integration, comprehensive triage, independent confirmation and final Rust/frontend validation are complete. Testnet and post-resync acceptance remain explicitly pending below.
- Behavior: one persisted monotonic day/hour definition; exact daily work and integer-formatted read values; one DAO snapshot reduction path; real rolling 24-hour DAO deltas/address deduplication; required values fail with context and unavailable observations remain explicit.
- Re-sync required: **yes**. CachedBlockHeader, DailyBlockStats, TxIndexEntry and other changed serialized chain data require one rebuild from genesis after adopting this branch. No chain database was deleted or rebuilt during this task. If the optional crawler has an existing network store, recreate it by crawling for the optional GeoIP schema; it is independent of chain replay.
- Next: user executes or authorizes the single chain re-sync, then verifies exact RPC work sums against persisted daily/API values, `verify --depth sampling` (including S31), X3/X4 comparisons, and actual homepage DAO/capacity/hash-rate data. Repeat for testnet when its RPC is available; the testnet full-header audit remains explicitly pending.
- Versioned documentation changed only where the described behavior or schema changed: POSTMORTEM, STORE_SCHEMA, TESTING, this execution/triage record, the mainnet audit and controlled DAO performance evidence. No principle wording was changed in README/CLAUDE.
