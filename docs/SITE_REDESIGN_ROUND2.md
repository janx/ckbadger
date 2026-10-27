# Site redesign round 2 — Address v3

## Goal

Implement D3 of the approved `2026-10-26-site-redesign-implementation-plan.md`:
the address page, its four holding classes, exact live-cell partitions, and the
Activities / Transactions / Live cells views. The approved `Address.dc.html` and
`AddressTxs.dc.html` boards, with the final review overrides, are the reference.

Starting revision: `d3b770f5`. Integration branch:
`integration/site-redesign-round2`.

## Principle Alignment

- Design starts from `docs/prompts/WORLD_VIEW.md`: an address identifies an
  ownership rule, and its holdings and history derive from chain evidence.
- All changes in this round are read-time API or frontend changes. No new
  domain, append-only, or network write path is planned. No re-sync is required.
- Counts and capacities cover the full matching set, independently of display
  pagination. Missing or inconsistent evidence must produce an actionable error.
- Existing services and stores are not restarted, rebuilt, or purged. Live
  checks use a separate API process with independent secondary directories.
- The address board specifies a balance partition and occupied-capacity class
  partition; it does not specify a new address capacity-history chart.

## Execution

| Lane        | Ownership                                                                   | Worktree                                     |
| ----------- | --------------------------------------------------------------------------- | -------------------------------------------- |
| API         | Gaps 7/8, read contracts, API regression tests                              | `.worktrees/site-redesign-round2-api`        |
| Frontend    | Address v3, client types, page/component tests                              | `.worktrees/site-redesign-round2-frontend`   |
| Acceptance  | Independent requirements review, 1440/390 acceptance, later-round gap audit | `.worktrees/site-redesign-round2-acceptance` |
| Integration | Review and merge, complete gates, round-1 full verification closure         | Main checkout                                |

The gates require the original tests before changes, meaningful regression and
contract coverage, final Rust/frontend checks, and browser checks against the
approved board at both widths. Fixture acceptance and live verification are
recorded separately.

### Integration baseline

Before D3 changes, the complete mainnet verification passed **64/64**,
`scopeComplete: true`, in 428.16 s. Run:
`20261027T084559Z-08e068e3`; CLI and temporary read-only API both used the accepted
`625e0476` artifact. The temporary API on port 18102 was stopped afterward.
This validates the previous round and its fixes, not the forthcoming D3 endpoints.

### Implemented behavior

- API source `9618fbb7` adds the current physical owner's holdings summary and
  class-filtered cell pages. The same classifier supplies all six classes;
  filtering precedes pagination, and totals cover the full owner scope. Each
  response uses the existing pinned store view. Independent page requests do
  not constitute a shared snapshot.
- Current DAO rows include received withdrawal requests; the original
  depositor's completed history is not counted as present holdings. Identity
  tenure follows canonical consume/recreate links. Named `.cell` rights stay
  separate from physical cells and capacity. Clusterless Spore groupings are
  not presented as on-chain collections.
- Frontend source `4d8ccdcf` implements the exact balance partition, four holding
  sections, six occupied-capacity classes, and Activities / Transactions / Live
  cells tabs. Class filters use the new API. Pending cycles and metadata remain
  explicit; stale data after a failed refresh carries a visible error notice.
- Holdings refresh every 60 seconds in the active tab. The balance and its
  partition consume one response; pool polling remains separate. Current-phase
  DAO block/time links and script hash types are preserved.
- The two implementation lanes are integrated at `1e05934c`. Detailed reports:
  [API](implementation/2026-10-27-site-redesign-round2-api.md) and
  [frontend](reports/2026-10-27-address-v3-frontend.md).

### Integrated validation

The root gate holds `1e05934c` unchanged while checking formatting, workspace
type checks, strict all-target Clippy, the full Rust suite, frontend type checks,
lint, the full frontend suite, and SPA/agent-renderer production builds.
Frontend validation passed **1,476 tests in 177 files**, type checks, lint, and
both builds. Rust formatting, workspace check and strict all-target Clippy
passed; the full suite passed **4,589 tests across 69 suites, zero failures**.
Three existing opt-in native-VM tests requiring the local mainnet node were not
run by the default command. Their execution paths did not change in D3, and no
test was newly ignored. Both gate records confirm HEAD remained unchanged.

Commands and per-command results are retained in
`.worktrees/site-redesign-round2-results/run-integrated-gates.py`,
`integrated-backend-gates.json`, `integrated-frontend-gates.json`, and their
listed logs. Before integration, the API lane passed 83 affected endpoint tests;
the frontend lane ran the existing tests before refactoring and preserved its
regression-first failure logs. Captured router JSON supplies the frontend's
producer contract fixture.

Independent browser acceptance replayed API `9618fbb7` and frontend `4d8ccdcf`
at acceptance revision `e046eabb`: **22/22 states passed**, with no assertion
failures or page errors, at 1440 and 390 px. These cover all three tabs, partition
hover, named-only ownership, empty/error states, received DAO requests and
retained-data refresh errors. Root also inspected five final desktop/mobile
captures. The acceptance lane independently passed 185 focused frontend tests
and compared both captured producer JSON fixtures field-for-field. See the
[acceptance report](reports/2026-10-27-site-redesign-round2-acceptance-and-remaining-gaps.md)
for commands, screenshot paths and the distinction between fixture rendering
and the live probe below. Task-owned previews on 3135 and 3136 were stopped and
their temporary entry points cleaned up.

### Real mainnet read-only probe

The new API was run separately on port 18103 using the API lane debug CLI,
source `9618fbb7`, SHA-256
`7db7a50db0e5a3b22c4f0d029b6f4c07a0f5ab86f64de9b07a524d0f66a34be2`.
It used independent secondary directories over the existing stores and was
stopped after the probe. No user service or release binary was replaced.

For the board's real address (lock hash
`0x3ec9b125118c048e7149eac1a6b6940e401c84fdd32e392b1dc74690bb585496`),
the probe followed every cursor with a limit of seven, independently summed
each class's rows, and checked unique outpoints, complete counts, capacities,
and occupied capacities against the holdings response. All six classes passed;
a second summary confirmed the address's class totals remained stable during
the probe. An empty address also returned a genuine zero inventory.

| Quantity                                 |      Exact live result |
| ---------------------------------------- | ---------------------: |
| Physical live cells                      |                     40 |
| DAO / token kinds / objects / identities |         5 / 9 / 14 / 1 |
| Balance                                  | 9,651,178.35338057 CKB |
| Occupied                                 |     7,127.00000000 CKB |
| Ordinary free                            |    54,571.35338057 CKB |
| DAO free                                 | 9,589,480.00000000 CKB |

The balance partition closes exactly. Current object composition is 10 Bitcoin

- CKB, three centralized mixtures, and one decode-pending item. The approved
  board fixture's one pure-CKB item is not evidence that the current pending item
  has decoded; the UI must retain the observed pending state. Fixture script/plain
  held-capacity splits also differ from current chain values, although occupied
  capacity agrees. Full live values are in `d3-live-address.json`.

The debug summary took 81.5 ms on its first request and 12.5 ms on the second;
class pages took 10.3–39.6 ms. These are single-run local observations, not a
throughput benchmark or a large-owner bound. The API lane separately exercised
a sparse 2,059-cell fixture with complete totals.

The same CLI/API passed **7/7 fast checks**, `scopeComplete: true`, run
`20261027T092459Z-35db3a67`. This validates the new endpoint smoke scope alongside
its direct assertions; the complete 64-check run above remains the previous
round's baseline, not a new full-depth run on D3.

- Probe: `.worktrees/site-redesign-round2-results/probe-live-address.py`.
- Responses/timings: `.worktrees/site-redesign-round2-results/d3-live-address.json`.
- Fast report: `work/mainnet/perf/verify/20261027T092459Z-35db3a67/report.json`.

## Result

Three agent lanes completed D3, integrated on `integration/site-redesign-round2`.
Implementation, full integrated gates, immutable browser acceptance and the
real-mainnet read-only probe passed. No domain, append-only, or network write
path changed. User services and primary data were not restarted or rebuilt.

- Re-sync required: **no**.
- Next: D4 Inventory and asset/identity pages, followed by D5–D8 in the approved
  order. The [remaining-gap audit](reports/2026-10-27-site-redesign-round2-acceptance-and-remaining-gaps.md)
  records source entry points, existing contracts, missing read-time fields, and
  acceptance requirements; these pages are not claimed complete by D3.
