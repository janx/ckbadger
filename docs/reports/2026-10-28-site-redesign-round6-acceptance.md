# D7 Fiber, Forks, Network and Hardforks — implementation and acceptance

## Goal

Complete D7 of the approved redesign, as requested, with implementation delegated
to three **gpt-6-sol** agents. Integration is on `integration/site-redesign-round6`.
Root coordinated ownership, cross-reviews, immutable builds and independent
acceptance. This round also closes the final D6 presentation defects described
in the [D6 report](2026-10-28-site-redesign-round5-acceptance.md).

## Principle Alignment

- `docs/prompts/`, the approved boards and `FINAL_REVIEW.md` govern the design.
  Exact chain facts remain distinct from this node's observations and from
  unavailable evidence.
- No production write path targets **domain**, **append-only** or **network**
  stores. No schema changes, backfills or new crawler observations are introduced.
  Task APIs use their own read-only secondary directories and unused ports.
- Amounts, transaction positions, epoch fractions and time differences remain
  exact. Runtime checks retain the accepted indexed-calendar dependency rather
  than scanning all historical headers.

## Result

### Delivered behavior and evidence boundaries

**Fiber** now presents complete pre-pagination channel/state totals, all observed
locks and exact pair aggregates. Pair lines represent only channels with exactly
two distinct observed locks; represented and omitted counts are explicit.
Observed locks can appear on funding inputs or outputs and are not all claimed
to be signers. Unknown addresses stay unknown. Funding net input includes change,
fees and other transaction activity; it is not an invented off-chain allocation.

Channel detail joins canonical funding/current commitment cells, opening lock
flows, close and final settlement outputs, exact argument bytes and transaction
structure. Partial settlement that replaces the tracked commitment appears as
its own transaction stage. Modern 57-byte commitments distinguish the initial
delay from a later settlement output; the delay is not reapplied to a subsequent
commitment. Legacy 36-byte args retain an explicit unverified-rule state instead
of assuming the modern flag semantics. Epoch-lock satisfaction does not promise
that every other transaction-validity condition is satisfied. The source rule is
documented by the [Fiber commitment-lock contract](https://github.com/nervosnetwork/fiber-scripts/tree/main/contracts/commitment-lock).

**Forks** separates canonical daily uncle inclusion from this indexer's observed
chain switches. Their clocks are labelled: indexed chain time versus detection
wall time, both displayed as UTC+8. Detail preserves the old branch, the branch
adopted at that event, and today's canonical branch as three distinct facts.
It shows reversal links, current canonical re-entry, positive uncle inclusion,
signed competing-block timestamp differences and subsequent canonical transaction
locations. A deep-fork detection pauses before rollback and is labelled as a
detection, without claiming a dropped or adopted branch. Canonical header/uncle
evidence is scanned once per event and shared across its old blocks.

**Network** shows retained verified peers with approximate GeoIP coordinates,
country/software/ASN evidence, accessible selection and disclosed marker
displacement. Local observed sessions and advertised peer relationships remain
different concepts. Directed advertisements include retained observations whose
last observed round is the latest completed round; active-round observations do
not leak into that view. Distinct peer pairs and alias observations have separate
counts and all retained rows remain reachable.

Daily network history exposes retained observation coverage, leading/trailing
gaps and missing dates. Existing history rows do not contain round IDs/counts,
so exact historical rounds per day remain `null` with an explanation. A missing
observation is not a zero-round day. The server's observation clock determines
the incomplete UTC day; query `to` is an inclusive range bound and cannot expose
today's incomplete bucket. First/last advertisement rounds do not establish
membership in every intermediate round.

**Hardforks** presents the VM0/VM1/VM2 and hash-type timeline, edition boundaries,
activation blocks and the actual node's consensus features. A query for another
network is rejected instead of mixing its constants with this store. Mainnet and
testnet activation boundaries and missing/malformed consensus responses have
regression coverage. This developer page has no artificial second-tier gap.

The final board audit added named code-version deployment markers next
to the VM timeline. Their source is each registered version's earliest indexed
canonical code-cell creation, ordered by block, transaction index and output
index. Code bytes, creation time and epoch remain separate from the hash-type
rules that select a VM. This does not assert that a historical code cell is still
live or that a particular transaction executes it.

### Review corrections and integration

Cross-review found and closed deep-event naming/rollback claims, repeated uncle
scans, ambiguous event timezone, output-only locks described as signers, omitted
pair denominators, a hidden updated-commitment transaction, stale rows after a
state-filter change, and the Network history upper-bound defect. Successful
node-backed Fiber evidence now has an API producer fixture consumed by the UI.
Network, Hardfork and Fork fixtures likewise come from serialized API responses.

The D6 follow-up fixes overview numeric/unit wrapping and a long miner address
that expanded a 390 px page to 695 px. Shared detail fact labels now wrap and
stack on mobile while retaining full links. An integration-only Epoch chart mock
was also updated to include D7's required consensus features.

The first real Fiber evidence requests exposed an upstream read-validation bug:
empty cell data was checked with the protocol's zero sentinel, although the
indexer's `CellData.data_hash` always stores a Blake2b digest. The validator now
calls that same indexer calculation for all byte sequences. A producer fixture
first reproduced the 500 using the writer's digest; a regression also rejects
the incorrect zero sentinel. All four selected mainnet channel states then
returned 200. No stored payload was changed or accepted under a second rule.

The complete Rust attempt at `ef7b9d82` passed 4,759 tests and failed two old
Forks statistics tests. Those fixtures recorded switch events without retaining
their old block evidence. They now seed coherent old/adopted branches, while the
deep-fork fixture records zero orphaned blocks because detection stops before
rollback. Original ordering and detection-time assertions remain. This was a
fixture correction; no production evidence requirement was relaxed.

The same board audit corrects Forks to its approved 26 px topic title and removes
Network's use of gold for observation states. The shared identity line wraps
normal prose at word boundaries while allowing long hashes to fit a narrow
viewport. Network retains “Where the nodes are”, as named in Phase D7 of the plan.

Sharing the strict deployment selector with Script detail exposed ten fixture
or error-expectation failures in `api_scripts`. The fixtures now contain complete
canonical creation/consumption locations and correct per-block transaction counts;
negative cases still reject missing indexes and invalid output bounds. The full
48-test suite passes with the shared strict selector. Same-block deployments
have a separate regression in which transaction hashes sort in the opposite
order to canonical transaction indexes; both the API and UI preserve chain order.

### Integrated validation

At `1781213e`, the complete Rust workspace passed **4,762 tests in 74 suites**,
zero failures. Three existing opt-in native-VM tests remain unrun by the default
command. All 24 producer fixtures then matched their committed consumers.

The final implementation revision is
`53ab417b7c11a012e4e988b3672223f4fd785e3d`. Its complete frontend suite passed
**1,679 tests in 210 files**, with type checking, lint and production build.
The complete final API regression passed **1,381 tests in 35 suites**, zero
failures or ignored tests, covering the shared Scripts selector and all D7 read
paths. All **25** exported API fixtures match their committed frontend consumers.
Formatting, workspace check and strict all-target workspace Clippy also passed.
The full 4,762-test workspace gate predates only the final API/presentation
changes; those received the complete final API and frontend gates above.

### Independent evidence and provenance

The frozen debug CLI is
`.worktrees/site-redesign-round6-results/d7-fiber-forks-network-53ab417b7c11-ckbadger`,
SHA-256 `d7b74bdc523becb490cb7119ee8aad55c1f0ff892ba84c63215bd83e12c61c0e`.
The production frontend build preceded the final Rust checks and executable
build. Acceptance uses that immutable binary, its own ports 18107/3150 and
separate API secondary directories; no supervisor/indexer/crawler is launched.

The selected Fiber oracle independently hashes node scripts and checks funding
inputs/outputs, net flows, modern/legacy commitment fields, transaction stages
and final outputs for open, cooperatively closed, force-close-waiting and settled
channels. Full-list channel totals and pair/party aggregates are also compared
across API views; those identities are not an independent store enumeration.

For the five selected fork events, the node RPC does not expose every old branch
through `get_header`. A bounded raw-node reader instead retrieves exactly the
selected fork points and branch headers by hash from the local CKB RocksDB
secondary. It performs no chain scan. Node RPC independently checks canonical
fork points, positive uncle inclusion and subsequent transaction locations.
Negative uncle claims and reversal/deep-event edge cases have fixture coverage.
The node's consensus response and the original code-cell bytes, transaction
index, height, timestamp and epoch independently check the Hardfork markers;
earliest-version selection completeness is covered by deterministic fixtures.

Indexed daily calendars remain a declared dependency. This acceptance neither
fetches every historical header nor repeats the earlier full 64-check run.

The final selected evidence run passed **1,879 assertions**, using 726 bounded
RPC calls plus **20 direct fork headers / 60 field-and-link comparisons**.
Its canonical anchor is block **20,851,303**,
`0xee45a48b8a035705c3a38820cd4a9c9facce0f74a89cd51fcc103a9e9a810065`.
The five current-run fork captures are isolated from archived earlier captures.
All **249 returned code-version markers** match original node bytes, transaction
order, height, raw timestamp and epoch.

The complete Fiber list contained 347 channels: 38 open, 301 cooperatively
closed, four force-close-waiting and four settled. Open funding capacity was
43,889 CKB, with 25 observed locks and 35 exact-two-lock pairs. At completed
crawler round 38 the map contained 64 located verified retained peers; 6,383
retained advertisement observations represented 3,676 distinct directed pairs.
Completed UTC-day history was empty because the retained crawler observations
were from the current incomplete UTC day. It was not reported as zero rounds.
These live snapshots have later heights/rounds than the historical design boards.

All **22 real page/width combinations** passed at 1440/390 px: the D7 surfaces
and the two corrected D6 pages. They checked successful HTTP evidence, one
correctly sized title, no page overflow, finite SVG coordinates, links, Fiber
filters and keyboard selection, Network peer selection, and all 249 deployment
marks with keyboard-selected cell/block evidence. Root also inspected final
desktop/mobile screenshots, including the partial-commitment and legacy Fiber
states, full miner addresses, Hardfork readouts and Network prose wrapping.
All **30 controlled D7** and **40 controlled D6** loading/error/empty/disabled
states also passed. The final probe set returned 31 successful responses and
the one expected 400 for a mainnet/testnet mismatch. Both task-owned API and
preview processes were stopped and reaped; the user's services and primary
stores were not restarted, replaced or purged.

### Read performance and artifacts

Single debug-build observations against the same advancing local stores follow.
Baseline D7 routes came from the frozen DAO-only `33176b59` binary; their code
was unchanged from D5. Concurrent compilation, cache state and later chain data
mean these are regression observations, not a controlled benchmark.

| Endpoint         | Baseline |                                      D7 |
| ---------------- | -------: | --------------------------------------: |
| Fiber statistics |   8.6 ms |                                  194 ms |
| Fork list        |   2.1 ms |                                  138 ms |
| Network summary  |  56.8 ms |                                   75 ms |
| Hardforks        |   3.4 ms | 163–219 ms warmed; 1,956 ms first probe |

Fiber statistics now contains the full state/party summary and Forks includes
retained branch previews. Hardforks grows from a 1.2 KB response to 84.7 KB with
249 validated deployment markers; its additional read cost is material and is
recorded rather than described as a speedup. New Fiber network, map and
advertisement probes took 43, 88 and 178 ms respectively. Four Fiber evidence
probes took 48–84 ms. No indexer write or bulk-sync path changed.

Reproducible ignored evidence lives in `.worktrees/site-redesign-round6-results/`:
`final-all-gates.json`, `accepted-build.json`, `live-acceptance.json`,
`oracle-results.json`, `oracle-node-evidence.json`,
`oracle-fork-local-db-headers.json`, the bounded reader sources, and
`live-browser/`, `browser-states/`, `d6-browser-states/` captures. The complete
workspace run is archived in `preflight-178/integrated-backend-gates.json`.
`preflight-ef7/` retains the actual Fiber failure and two old fixture failures;
`preflight-178-rpc-orphan/` retains the initial RPC-only oracle limitation.

### Scope and next step

- Re-sync required: **no**.
- Acceptance status: **complete through D7**, including the final D6 presentation
  replay. No production domain, append-only or network write path changed.
- What to do next: **D8 not-found and crash pages**. D8 was not implemented in
  this round.
