# Site redesign execution — 2026-10-26

## Goal

Implement **round 1** selected in the approved
`docs/superpowers/plans/2026-10-26-site-redesign-implementation-plan.md` decisions:
all persisted changes, Phase A foundations, B2 correctness fixes, and D1/D2 pages.
Address, inventory/items, scripts/cells, DAO/charts, network topics and final error-page artwork
remain the later read-time/frontend rounds in that plan.

## Principle Alignment

- Design starts from `docs/prompts/`, with the approved local boards and snapshots as the page reference.
- The indexer owns every changed persistent chain write, all in the **domain** store. Lazy group cycles
  use domain sync metadata and are removed with their transaction on rollback. The append-only store
  and network store receive no new mutation paths. Storage tests cover append-only rejection and
  preservation of immutable cell payloads.
- Exact numbers use canonical chain evidence and shared classification/calculation paths. Missing
  node RocksDB evidence, malformed lineage and inconsistent indexes fail with entity context.
  Pending DOB decoding and lazy cycles have explicit states.
- Existing stores and services remain untouched. Performance experiments use isolated scratch
  stores under `.worktrees/site-redesign-perf-results`; the user performs the final re-sync.
- No script transaction index, approximate aggregates, backfills, or inline bulk VM decoding.

## Execution

Starting revision: `2c292b73`. The checkout was clean and `fix/dao-phase1-stuck` was already
an ancestor. Integration branch: `integration/site-redesign-round1`. Three agents worked in
independent `.worktrees/site-redesign-*` worktrees, with root owning integration and block evidence.

| Owner            | Delivered scope                                                                            | Detail                                                                                                                                                                                                                                                                       |
| ---------------- | ------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Frontend agent   | Phase A, Transaction/Activities, transaction filters/pool, block relationships             | [Foundations](reports/2026-10-26-frontend-foundations.md), [Transaction/Activities](reports/2026-10-26-transaction-activities.md), [Final controls](reports/2026-10-26-transactions-final-controls.md), [Block relationships](reports/2026-10-26-block-position-evidence.md) |
| Indexer agent    | All round-1 writer changes, native lazy VM, paired scratch performance                     | [Indexer report](site-redesign-indexer.md)                                                                                                                                                                                                                                   |
| API agent        | B2 fixes, transaction/activities/pool read contracts, exact group joins                    | [API report](implementation/2026-10-26-site-redesign-api.md)                                                                                                                                                                                                                 |
| Root integration | Homepage exact counts, block/list evidence, shared transaction table, gates and acceptance | This report                                                                                                                                                                                                                                                                  |

### Persisted scope

- Fix 6 B: explicit DOB decode-pending provenance; transfers preserve the decoded profile.
- Fix 7 / gap 31: one bulk/live identity-action classification, stored in existing history rows.
- Fix 8: Type ID ELF lineage and registered-family validation with reorg undo, including retirement,
  reactivation and burn without losing historical versions.
- Fix 11: separate DAO phase-0 deposits and phase-1 requests.
- Fix 15: observed fork switchbacks linked in both directions after canonical replay.
- CKB moved / gap 9: sum positive participant deltas, excluding cellbase; the helper feeds API Value
  and the existing daily/hourly aggregates.
- Gap 22: existing mNFT mint/transfer/burn aggregation verified; no additional writer needed.
- Gap 58: exact unique current depositor locks; rollback now matches bulk/live semantics.
- Gaps 6/24: on-demand in-process CKB verifier, exact historical VM context and group results.
  No added bulk rows or computations.

### Page and read changes

- Self-hosted fonts, canonical tokens, neutral hashes, reusable flat tables/activity rows/cell
  diagrams and a shared 72 px developer-tier boundary. Redirects and distinct crash handling work.
- Homepage 24-hour activities count each non-cellbase transaction once. Per-kind occurrences stay
  separate because categories overlap. Daily activity joins exact complete-day transaction counts
  with same-day cellbase counts; it does not sum classification counters.
- Blocks list/detail use node molecule bytes including extensions, exact transaction sizes and
  fees, complete/pending/failed cycles, consensus limits, cellbase messages, uncles/proposals,
  reward components and payment height. Counts are checked against the canonical index.
- Transaction detail displays cell flow, exact occupied capacity, executed versus attached script
  groups, witness ranges and expanded deps with hover links. Transactions and pool views use
  canonical Value, activity headlines, state filters, stable paging and full-snapshot pool summaries.
  Cell tiles expose exact token base units, item/file identity, immutable names and verified media
  composition, with separate object and CKBFS continuity. Historical inputs read their actual
  outpoint bytes and stored `hash_type`; imported script labels cannot override that evidence.
  Valid binary CKBFS filenames retain their file ID without invented text. Historical mNFT class
  names/renderers are not borrowed from mutable current metadata. See the
  [API evidence](reports/2026-10-27-transaction-cell-evidence.md) and
  [transaction visual acceptance](reports/2026-10-27-transaction-cell-semantics.md).
- Observed native Type ID references use their canonical mapping for current executable resolution.
  Retired or burned code remains queryable by historical binary hash, and family endpoints preserve
  registered-reference history without promoting non-ELF payloads into executable versions.
  Shared family provenance keeps still-live registered-reference users and their full capacity
  history without assigning them to a historical binary. Unregistered references retain their own
  capacity history, without inventing a permanent family after retirement. Family/version metadata
  joins use the request's pinned store view.
  `/scripts/lookup` still omits references with no resolvable executable. Correct retirement now
  sends unregistered non-ELF references into that existing branch; lookup does not expose their own
  current usage. Their explicit reference-history endpoint remains available. D2 transaction lookup
  supplies `txHash` and uses actual deps first; full script-page presentation remains in later D5.
- Registered reference forms remain distinct from arbitrary Type ID deployment observations.
  Multiple registrations for one binary are retained. Mainnet USDI's node-proven xUDT deployment
  is registered under xUDT; its token identity remains USDI. The independent testnet USDI binary
  remains unchanged.
- Block transaction rows show actual proposal origin and output capacity. Global Activity totals
  exclude cellbase; per-block evidence follows the approved block composition.

### Verification

Final integrated backend and frontend gates passed, including the D2 cell-evidence correction
and the committed `.cell` fixture's canonical identity rows. No production fallback was added
to accommodate an incomplete fixture.

- Root regressions reproduced category-sum overcounting, daily activity overcounting, nullable cycle
  rendering failures and mismatched block proposal counts before their fixes. Targeted checks pass:
  21 block API tests, exact rolling-window API/cache checks, 44 combined frontend checks and the
  expanded 9-test homepage count suite.
- Native VM historical fixtures execute VM0/VM1/VM2 and DAO request/completion with exact expected
  cycles. The indexer report records owner suite counts and append-only/reorg checks.
- Final integrated frontend gate at `66a7314a` passed: **1,456 tests in 176 files**, typecheck,
  lint, SPA and agent-renderer build. The preceding 1,445-test gate and 11-state browser acceptance
  remain recorded; see the
  [integrated frontend report](reports/2026-10-26-integrated-frontend-acceptance.md).
  The D2 follow-up lane passed **1,456 tests in 176 files**, typecheck, lint and build, plus eight
  1440/390 visual states for cell identity, file/object hover/focus and pending media. Root also
  inspected its desktop/mobile captures. These eight states complement the earlier eleven.
- Final integrated Rust gate at `f5f64aba`: **4,532 tests passed across 66 suites, zero failures**,
  workspace check,
  strict all-target workspace Clippy and formatting. Three existing opt-in native-VM tests were
  skipped by the default command and explicitly passed in the dedicated historical run. This
  includes Type ID lifecycle, family provenance, lazy-store invariants and the identity-owner
  allocation refactor.
  The standalone CLI build also passed; SHA-256 is
  `c42d60ee47d4d05b05c61547cf8bf3ac574b006383266db679d44250f9616acd`.
  HEAD remained unchanged throughout the run; the final documentation commit changes no code.
  Gate metadata and logs are retained under
  `.worktrees/site-redesign-perf-results/root-round1-acceptance-integrated-gates.json` and its listed
  log paths. Earlier gates and first failures remain preserved: test-only unnecessary-copy Clippy
  failure at `66a7314a`, and the incomplete `.cell` fixture at `1eb5a929`. The latter now seeds the
  identity rows both canonical writers create, preserves all witness assertions, checks both cell
  display descriptors, and separately rejects a missing row with transaction/output/item context.
  The final API unit suite has 648 tests, transaction suites 46, and indexer suite 1,803.
- Family regressions reproduced disappearing nonzero capacity history, stale version counters
  after same-binary reactivation, and a false missing-family error from stale warmup metadata.
  They now verify retained live users/history, no duplicate counting or invented binary attribution,
  and actionable errors for contradictory persisted totals. The writer and API use shared checked
  membership and summation operators; API reads do not repair persistent state.
- Registration regressions distinguish declared anchors from arbitrary copied deployments, retain
  multiple anchors and the first declared display preference, remove revoked registrations on
  reimport, and reject conflicting metadata before writes. A committed fixture replays all **471**
  native Type ID ELF deployments in 0–12M through the owning rollup, including the real unregistered
  reference that switched between `.bit` families. The source audit covered 1,042,797 native Type ID
  outputs in 1,042,764 transactions and found no remaining registration conflict after the metadata
  correction. API tests retain all-anchor checks and the separate executable-retirement contract.
- The integrated CLI at `230fec15` also read the completed 12M scratch store through a separate read-only
  API. Six fast checks passed. The command correctly exited 1 because `sync_complete` failed:
  this bounded run stops at 12,000,000 while the node was at 20,840,590. This is not a full-tip
  verification pass. The temporary API did not run an indexer or crawler.
  The real Type ID retired at 11,993,376 and reactivated at 11,993,505; at the 12M snapshot it
  correctly resolves to the newer ELF, while the old consumed ELF and its own reference-capacity
  history remain queryable. Code-cell lookup alone does not establish binary-version membership;
  non-ELF exclusion is asserted by the writer regression and deployment fixture.
- 1440/390 browser checks: homepage, block, blocks list, transactions and pool fixture previews
  have no document overflow or runtime errors. The final block-table layout also preserves all
  columns within 928 px and retains every decimal of a max-u64 capacity. Final post-merge captures
  passed and are stored under the ignored `docs/superpowers/plans/2026-10-26-site-redesign-acceptance/`
  directory. Transaction/Activities and hover checks are recorded in the frontend lane reports.
  These are deterministic fixture previews, not live rebuilt-database verification. The root block
  fixture uses the approved #20,827,932 numeric evidence with synthetic hashes and activity examples;
  the homepage uses its October 24 fixtures and an independent pipeline fixture. Any live historical
  comparison still belongs after the user re-syncs.
- Paired scratch performance is complete with the bounded disposition below. The first candidate
  12M run exposed a legitimate
  ELF-to-non-ELF Type ID transition at block **11,993,376**; the new lineage assertion rejected
  it. The shared writer retirement/reactivation and rollback logic is now fixed and covered
  by regression tests. Its next candidate was superseded by the family-attribution correction;
  both raw runs are retained and are not final acceptance evidence. A subsequent candidate reached
  12M but failed final aggregation because arbitrary Type ID deployments had been promoted to
  permanent family registration. The correction separates imported registration anchors from
  observed deployments and preserves every registered reference for a binary. The final candidate
  and fresh baseline both completed 0–12M, and the inverse-order 0–2M pair also completed. Mixed
  phase results prompted an additional unchanged-candidate 12M repeat, which completed but again
  showed higher build/CPU totals. Those results prompted controlled owner profiling; they remain
  part of the evidence. The existing `address_reduce_ms` timer includes the complete parallel owner join,
  so it cannot attribute a difference to the address reducer alone. All runs remain in the
  evidence, including slower results. Unrelated background work is observed and is not stopped.
- Controlled owner microbenchmarks isolated repeated full identity-record clones and duplicate
  history-key encoding. The shared bulk/live classifier now copies only the exact prior owner,
  expiry and error context, and bulk emits each final reverse-history row once. All 1,799 indexer
  tests and the integrated gate above pass. This correction changes no persisted encoding and does
  not by itself close the performance gate. The completed three-way microbenchmark and actual-chain
  paired diagnostic provide the additional evidence below.
- The integrated same-binary diagnostic replayed the exact same **12,000,001 blocks and 27,359,903
  transactions** into baseline/current ScriptOwner and ObjectOwner, in alternating order on the
  same worker CPU. Common state matched. Full Script CPU was **82.797 → 74.940 s (−9.49%)**;
  Object CPU was **22.390 → 22.258 s (−0.59%)**. In the exact dense interval Object CPU rose
  **0.184 s (+1.33%)**, within the observed order/core variation. The 438-batch diagnostic's total
  **8,464.356 CPU seconds** includes **105.186 seconds** of extra baseline-owner work; it is not a
  production throughput number, and no subtraction is used to manufacture one.
- Performance disposition: **no measured, attributable critical-path regression beyond observed
  run/order/core variation after the refactor**. This is a bounded acceptance, not proof that
  end-to-end throughput cannot regress. Shared-host activity, adaptive batching, cache state,
  unpaired prepare/commit work, lineage memory accounting and final rollup costs remain explicit
  limits. This historical workload contains no Spore, DID, BitCell or `.cell` protocol operations;
  those paths are covered by tests and synthetic profiles, not the 12M chain pass. Independent
  API/indexer hot-path reviews found no additional general per-cell/transaction allocations in
  facts/history/preparation. See the complete [performance report](reports/2026-10-26-site-redesign-bulk-performance.md)
  for every retained slow/failed run, source hashes, exact windows, order/core strata and scope.
- A read-only API also opened the completed integrated diagnostic snapshot using the preserved
  `9c52d494` CLI. The same six fast checks passed. Overall verification correctly failed only
  `sync_complete` against real node height **20,841,165** because this scratch store ends at 12M.
  Both temporary verification APIs and all timed processes were stopped; no user service was changed.

## Result

- Round 1 implementation and integrated validation are complete on
  `integration/site-redesign-round1`. Three agent lanes delivered the writer, API and frontend
  changes; root integrated and verified them. D3–D8 remain later read-time/frontend rounds as
  decided in the approved plan. Live post-re-sync acceptance remains the user-run next step.
- Re-sync required: **yes**. Do not use these schema changes against an old store.
- What to do next: the user performs one final mainnet/testnet purge and genesis re-sync,
  then runs `ckbadger verify --depth sampling` and reviews the pages at the historical fixture
  heights. No running service or user store was restarted, purged or rebuilt by this task.

## 2026-10-27 startup import correction

The first full mainnet run on `e23fee85` completed bulk finalization at block 20,844,328, then
entered an indexer restart loop during the required bulk-to-live handoff. Startup label import
cleared the JoyID and M-NFT Issuer families learned from mainnet Type ID history because bundled
metadata listed those bytecodes only under testnet. Both deployments were independently verified
against local node block data. See [PROTO-013](POSTMORTEM.md#proto-013-startup-label-cleanup-erased-families-learned-from-mainnet-type-id-history)
for the incident and ownership error.

Three agent lanes delivered the importer fix, independent persisted-startup regressions and a
read-only review. Root integrated the regression as `18e924cc` and the fix as `7f489cf8`.
The importer now preserves locally observed binary attribution, revokes imported registrations
separately, and validates persisted and planned membership before label writes. Explicit metadata
can still label the exact previously unknown binary it declares. The strict membership check is
unchanged; invalid historical attribution is not reconstructed during import. All changed
production writes target the indexer-owned **domain** store; append-only and network writes do
not change.

- Both real lifecycle regressions failed before the fix and passed afterward. They apply the 471
  recorded historical deployment observations plus the verified JoyID observation, persist and
  close the stores, then perform two reopen/import/refresh cycles. Version and index assertions
  precede refresh; six relevant script domain CFs and an immutable cell payload are compared
  afterward. These are persisted startup-operation tests, not a launched supervisor test.
- Label-import unit tests passed **51/51** (43 existing and 8 new); the independent lifecycle
  rerun passed **2/2**, and the full writer-addresses suite passed **21/21**. The independent final
  review found no remaining blocker, including for revoked anchors, copied Type IDs, active
  registrations in other metadata entries, stable family IDs and already-damaged stores.
- Root's integrated gate at `7f489cf8` passed formatting, workspace check, strict all-target
  workspace Clippy, **4,542 tests across 66 suites with zero failures**, and a debug CLI build.
  The default command left three existing opt-in native-VM tests unrun; this importer-only fix
  does not change those paths. The indexer suite passed **1,813/1,813**. HEAD stayed unchanged
  throughout the gate. Metadata and exact commands are recorded by
  `root-startup-lineage-fix-gates.py` and `root-startup-lineage-fix-integrated-gates.json` in the
  evidence directory below. Debug CLI SHA-256:
  `fd49f0cb09d34b0cc0cdaf2d8a8e9af84ecc03bdc393054d9b76ef651cc9e87b`.
- Evidence is retained under `.worktrees/site-redesign-startup-fix-results/`: the incident log
  snapshot, independently hashed deployment evidence, RED/GREEN lifecycle logs, and integrated
  gate metadata. The compact verified deployment fixture is committed with the tests.

Re-sync required: **yes for the mainnet store already damaged by the old importer**. Healthy
stores need no schema migration or re-sync for this fix alone. Stop the old supervisor before
replacing its binary, build the corrected revision, then rebuild the affected chain stores from
genesis and run the normal verification. The separate network store does not require rebuilding.
This task leaves the user's services, `work/` data and running release binary unchanged.

## 2026-10-27 full mainnet verification closure

After the asset-contract correction and the accepted layered calendar verifier,
run `20261027T084559Z-08e068e3` passed **all 64 checks**, with
`scopeComplete: true`, no skipped/inconclusive/error/failed checks, in 428.16 s.
This is a complete run, distinct from the earlier 63-pass run and the subsequent
selected entity-check run. The indexed calendar remains an explicitly declared
dependency; passing does not claim an independent proof of every historical date.

CLI and API both used the isolated release artifact for `625e0476`, SHA-256
`2f7f6b159c6207e7b2f1ca219cd748bd5443ec71099940abc6947d7e64b97821`.
The API used port 18102 and separate RocksDB secondary directories, then was
stopped. Existing services, binaries and primary stores were not replaced or
restarted. Re-sync required: **no** for this verification closure.

- Report: `work/mainnet/perf/verify/20261027T084559Z-08e068e3/report.json`.
- Reproducer and process/command logs: `.worktrees/site-redesign-round2-results/verify-round1-full.py`,
  `round1-full-verify.json`, `round1-full-verify.log`, and `round1-full-api.log`.
- Next implementation round: [D3 Address v3](SITE_REDESIGN_ROUND2.md).

## 2026-10-27 D3 completion

The three round-2 lanes completed the Address v3 read contracts, page and
independent acceptance on `integration/site-redesign-round2`. At fixed code
revision `1e05934c`, integrated Rust gates passed **4,589 tests across 69 suites**
with zero failures; three existing opt-in native-VM tests were not run by the
default command. Frontend gates passed **1,476 tests in 177 files**, type checks,
lint and both production builds. Final browser acceptance passed 22 states at
1440/390 px, and a separate read-only mainnet API probe checked the real address's
40 live cells and every class's cursor pages, exact capacity and occupied totals.
Its seven fast checks also passed. Temporary APIs and previews were stopped.

Re-sync required: **no**. No logical store write path changed in D3. The
[round-2 report](SITE_REDESIGN_ROUND2.md) separates complete round-1 verification,
D3 unit/integration tests, deterministic browser fixtures and actual mainnet
evidence. D4–D8 remain the next authorized read-time/frontend rounds; their
[gap audit](reports/2026-10-27-site-redesign-round2-acceptance-and-remaining-gaps.md)
defines concrete contracts, page work and acceptance gates.
