# D6 Nervos DAO and Charts — implementation and acceptance

## Goal

Complete the approved D6 DAO and Charts slice after accepted D5 `eb0a803e`.
Implementation used `gpt-6-sol` agents; root coordinated contracts, integration,
independent node evidence and browser acceptance. The last presentation fixes
are integrated with the following D7 round.

## Principle Alignment

- Design follows `docs/prompts/`, the approved boards and `DAO_CALCULATIONS.md`.
  Historical board values are examples, not arithmetic inputs.
- No production write path changes target the **domain**, **append-only** or
  **network** store. No schema, backfill, crawler run or primary-service restart
  is introduced. Acceptance APIs use separate read-only secondaries.
- Amounts remain exact integers or decimal CKB strings. Phase 0 and phase 1,
  actual AR growth and estimated APC, current and ever depositors remain distinct.
- Completed-day histories use the declared indexed running-max chain calendar.
  Runtime verification does not fetch every historical header or independently
  re-prove date attribution. Missing block-bearing days fail; certified blockless
  dates preserve the cumulative series through one shared calculation path.

## Result

### Delivered behavior

- DAO exposes phase totals, exact epoch-lock readiness, thirty six-epoch bins,
  full-denominator owner concentration, top-owner address/lock evidence, raw
  C/AR/S/U and actual AR anchors. Its history uses current depositor counts.
  The page includes Activities and Transactions plus transaction/script links.
- Charts has 28 overview previews and all 29 existing detail routes, including
  the supplemental circulation-ratio route. Shared models support exact
  readouts, range controls, positive-line log scale, keyboard interaction,
  hardfork markers, source/evidence and related links. Utilization retains both
  occupied-capacity and owned-capacity histories and their exact rankings.
- Supply and secondary issuance retain sub-CKB precision and exclude the
  incomplete indexed day. Secondary categories form a disjoint partition;
  frozen phase-1 compensation is separate subset evidence.
- Four verification checks now use exact integers and the chain-derived genesis
  burnt baseline. `secondary_issuance_partition_conservation` establishes the
  partition identity, not independent category allocation or calendar validity.

### Corrections found during integration

The first frontend integration exposed obsolete page expectations and two real
regressions: signed knowledge flow and miner identity links. Sol restored exact
signed flows, including values above JavaScript's safe integer range, and kept
full miner address/lock-hash destinations. A failed background refresh now keeps
one header and its previous plot with an explicit stale-data alert and Retry.

The full Rust attempt at `663238f7` completed 73 suites with 4,735 passes and four
failures. One DAO test still asserted cumulative depositors; its replacement
proves current counts can decrease while cumulative counts increase. Three
Script fixtures omitted canonical intermediate heights required by completed-day
chart lookup. The fixtures now contain a contiguous chain and completed/incomplete
day boundaries, preserving their Type ID lineage and utilization assertions.
No production validation was relaxed.

At `041d5884`, the complete frontend passed **1,651 tests in 203 files**, type
checking, lint and production build. Formatting, workspace check and strict
all-target Clippy passed. The entire affected API library passed **681/681** and
Script API suite **48/48**, closing all four failures; the other Rust suites were
unchanged. Eight exported API fixtures matched their committed frontend JSON.
The final combined D7 workspace gate is recorded in the D7 acceptance report.

Browser review then found narrow overview readouts splitting values/units, and a
long miner address expanding a 390 px page to 695 px. Sol stacked card readouts,
kept value/unit groups intact, and made shared detail fact labels wrap and stack
on mobile. Complete addresses and all links remain available. These final visual
changes are covered by the combined D7 gate and browser replay.

### Independent mainnet evidence

The frozen D6 debug CLI at `041d5884088ff3a28af13caa9c9e5b6df5497091` has SHA-256
`59c4d48e87f1831e38cbc74c97283683b9b8eef35fc6a3d2ac5c7b7a6c8d28d9`.
Its production frontend build preceded the Rust checks and CLI build.

At canonical block **20,850,936**, the independent oracle enumerated **22,357 live
DAO cells** and used relevant deposit/request headers. Its exact integer/fraction,
Molecule hashing and address calculations passed **99 assertions**: phase counts
and capacities, readiness, all thirty bins, concentration, top owners, raw header
fields, AR anchors and the independently anchored statistics partition. The
retained cache contains 23,874 relevant headers; the final run needed only 22 new
header RPCs, four indexer-tip calls and 46 live-cell pages. This is not an
all-header scan. The anchor was checked for canonicality.

An independent block-0 output sum verified the exact genesis burnt amount.
Across **2,538 completed dates** (2019-11-16 through 2026-10-27), supply and
secondary identities and date continuity passed, retaining **15,228 fractional
CKB values**. Both utilization histories also have consecutive indexed dates.
These are exact identity/calendar-consistency checks, not an independent proof
of every category's attribution or the entire calendar.

Selected CLI run `20261027T175351Z-78c2323d` passed all four requested checks:
`genesis_baseline_burnt_invariant`, `chart_total_supply_monotonic`,
`secondary_issuance_partition_conservation`, and `burnt_supply_genesis_invariant`.
It intentionally reports `scopeComplete=false`: this was not another 64-check run.

All 30 real API probes returned 200. The initial browser pass exercised DAO,
Charts overview and all 29 details at 1440/390 px: **61 of 62 passed**, with the
miner mobile overflow described above. At `1781213e`, all four corrected
overview/miner page-width combinations and all 40 controlled D6
loading/error/empty cases passed. The combined workspace run passed **4,762
tests in 74 suites**, zero failures, with three existing opt-in native-VM tests
unrun by the default command. Final D7 acceptance retains this evidence and
replayed the changed presentation at final revision `53ab417b`; all four
corrected page/width combinations and all 40 controlled D6 cases passed again.

### Read performance and artifacts

Single warmed debug-build probes against the same local stores are shown below.
The node and filesystem cache can advance between runs; these are regression
observations, not a controlled speedup benchmark.

| Endpoint              | Accepted D5 baseline |       D6 |
| --------------------- | -------------------: | -------: |
| Most-utilized Scripts |             1,511 ms | 1,666 ms |
| Most-utilized Assets  |             2,038 ms |   286 ms |
| Total supply          |                61 ms |   113 ms |
| Secondary issuance    |                90 ms |   154 ms |
| DAO statistics        |              2.62 ms |  2.78 ms |

The new full DAO evidence endpoint took 5.06 s for the live-cell/owner aggregate.
Cold utilization warmup retains an explicit 503 until its read cache is ready;
it is not represented as empty history.

Ignored reproducible evidence lives in `.worktrees/site-redesign-round5-results/`:
`integrated-all-gates.json`, `corrected-all-gates.json`, `accepted-build.json`,
`producer-consumer-results.json`, `oracle-results.json`, `chart-oracle-results.json`,
`selected-verify.json`, `live-acceptance.json`, and `live-browser/` captures.
The initial browser failure is retained rather than relabelled as a pass.
Final D6 presentation/state replay is under `.worktrees/site-redesign-round6-results/`.
All task-owned D6 API/preview processes were stopped and reaped.

- Re-sync required: **no**.
- Acceptance status: **complete**; the combined D7 report records the final
  presentation/state replay and executable provenance.
- What to do next: D7 is also complete; D8 not-found/crash pages are next.
