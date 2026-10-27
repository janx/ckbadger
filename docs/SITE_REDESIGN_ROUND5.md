# Site redesign round 5 — Nervos DAO and Charts

## Goal

Implement D6 of `2026-10-26-site-redesign-implementation-plan.md`, starting from
accepted D5 revision `eb0a803e` on `integration/site-redesign-round5`. The approved
DAO, DAO Transactions, Charts and Chart Detail boards live under the ignored
`docs/superpowers/plans/2026-10-25-site-ui-review/` directory. `FINAL_REVIEW.md`
overrides older board typography and terminology. Historical board snapshots
illustrate presentation; they are not an arithmetic specification or current
chain evidence.

## Principle Alignment

- Start from `docs/prompts/WORLD_VIEW.md`, `INFORMATION_DESIGN.md`,
  `docs/DAO_CALCULATIONS.md` and relevant `docs/POSTMORTEM.md` entries. Chain
  facts remain the source of truth; calculations have one exact path.
- This is a read-time API and frontend round. No domain, append-only or network
  store writes, schema changes, new indexes or backfills are planned. Existing
  primary stores and services remain untouched.
- DAO phase 0 is distinct from phase 1: compensation freezes at the withdrawal
  request, while the DAO epoch lock can continue. Compare epoch fractions
  exactly. A satisfied epoch lock does not establish every transaction-validity
  condition. Future epoch bins are facts about lock periods, not predicted dates.
- Separate actual AR growth from estimated APC. Historical date anchors use
  canonical indexed `chain_time_ms`, with that calendar dependency disclosed.
  Runtime acceptance does not enumerate every historical header or claim to
  independently prove the entire calendar.
- Use exact network genesis baselines, actual occupied capacity, and the existing
  shared secondary-issuance split. Preserve exact integers through API and UI;
  convert only bounded drawing coordinates to floating point. Use compensation
  terminology; burnt secondary issuance is final.
- Reuse shared chart primitives and consistent physical units. Missing data,
  empty data and request failures remain distinguishable. Daily charts exclude
  the incomplete current day and retain legitimate blockless dates.

## Execution

The user explicitly requested Sol for implementation. Two existing
`gpt-6-sol` agents own complete feature slices in isolated worktrees, with
independent Cargo targets seeded from the accepted build. Root owns design
review, integration, independent acceptance and reports.

| Lane       | Ownership                                                                                      | Worktree                                 |
| ---------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------- |
| DAO Sol    | DAO read contracts, exact epoch/AR/concentration evidence, DAO page and tests                  | `.worktrees/site-redesign-round5-dao`    |
| Charts Sol | Typed chart catalogue, overview, all detail variants, chart read contracts if needed and tests | `.worktrees/site-redesign-round5-charts` |

DAO owns `dao.rs` and DAO API types; Charts owns `statistics.rs` and shared chart
components. Shared frontend API sections follow those ownership boundaries.
Producers publish real serialized JSON fixtures for frontend consumers when a
contract changes.

| Slice                   | Required behavior                                                                                        | Acceptance evidence                                                                                      |
| ----------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| DAO overview            | Phase 0/1 totals, current versus ever depositors, rolling 24-hour change, completed-day history          | Exact full-denominator counts and capacities; legitimate empty and young-chain states                    |
| DAO return and issuance | Actual AR growth, separate estimated APC, miner/compensation/burnt split, paid versus owed               | Header AR/C/S/U, genesis baseline and conservation identities                                            |
| DAO withdrawal windows  | Phase-0 cycle boundaries and phase-1 minimum epoch locks                                                 | Fractional epochs, different epoch lengths, exact equality, malformed evidence                           |
| DAO owners              | Concentration across all phase-0 owners, top owners and actual lock kinds                                | Independent live-cell enumeration and exact address/script hashes                                        |
| DAO evidence            | Activities and Transactions, raw header fields and DAO script links                                      | Correct filters, pagination, meaningful links and developer tier                                         |
| Charts overview         | 28 approved previews with exact last values, units and meaningful deltas                                 | Catalogue coverage and real API responses                                                                |
| Chart details           | All 29 existing detail routes, shared shell plus line/bar/stack/ranked/distribution/HODL/cohort variants | Preserve the supplemental circulation-ratio route, range/log controls, source/evidence and related links |
| Presentation            | Approved hierarchy, accessible exact readouts and explicit states                                        | 1440/390 px, no document horizontal overflow, loading/error/empty, large values and interactions         |

Run existing affected tests before refactors and failing regression tests before
bug fixes. Final gates include the production frontend build before final Rust
checks, complete relevant suites, producer/consumer fixture comparison and
read-only mainnet acceptance. Compare read performance with the frozen accepted
D5 binary. Task APIs use separate read-only secondary directories and unused
ports; all task children are stopped after acceptance.

Independent DAO acceptance uses a bounded live-cell enumeration, relevant
canonical headers and exact integer/fraction arithmetic. If the node advances
during enumeration, account for intervening DAO transactions or report the
anchor mismatch explicitly. This is selected D6 evidence, separate from the
previous full 64-check verification.

## Result

D6 is complete. DAO and all 29 chart detail routes are implemented, together
with the 28-preview overview and exact read contracts. Independent acceptance
checked 22,357 live DAO cells, 99 DAO assertions and 2,538 completed chart dates.
All four selected CLI checks passed; this is separate from the earlier full
64-check chain verification.

The final mobile miner layout and overview readouts were corrected with D7.
The combined Rust workspace run at `1781213e` passed 4,762 tests with zero
failures; frontend and browser evidence, fixture agreement and the declared
calendar dependency are recorded in the
[D6 acceptance report](reports/2026-10-28-site-redesign-round5-acceptance.md).

- Re-sync required: **no** for D6 read-time changes.
- What to do next: [D7](SITE_REDESIGN_ROUND6.md) is also complete; D8 not-found/crash
  pages are next.
