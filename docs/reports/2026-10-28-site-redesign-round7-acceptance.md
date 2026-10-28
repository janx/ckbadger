# D8 not-found and crash-page acceptance

## Goal

Complete the final D8 page group from the approved site redesign plan: entity
not-found, unknown route and crash pages. Three `gpt-6-sol` agents implemented
the evidence, presentation and vessels slices in isolated worktrees. Root
integrated them on `integration/site-redesign-round7` and owns independent
acceptance.

## Principle Alignment

`WORLD_VIEW.md` and `INFORMATION_DESIGN.md` guide the implementation. A 404
describes the owning request's result, not proof of exhaustive chain absence.
Only exact, scoped `/search` evidence supplies an alternative block/transaction
link. Search failure remains separate from the primary 404. Transport and
internal errors do not become missing entities; secondary-widget failures do
not erase an already loaded entity.

The new `/vessels/snapshot` endpoint reads the domain and append-only stores
under a coherent read view. An owned pin also covers its blocking work after
HTTP cancellation. There are **no production writes** to domain, append-only or
network stores, and no schema or indexer changes.

The artwork examines at most **12 canonical blocks × 6 transactions × 4
outputs**, displays at most **64 live cells**, and checks at most **64 tracked
outpoints**. The newest eligible live outputs come first. Exact decimal capacity
and occupied bytes remain separate from illustrative canvas geometry. The API
reports the canonical anchor and selection bounds. Proven consumption differs
from a still-live cell leaving the sample or an outpoint leaving the canonical
view. New-block/reorg events refresh this bounded view; it never loops a replay
or claims to show every intervening block event.

## Result

**D8 is complete.** Final implementation `15d92d55` passed the frontend gates,
the unchanged Rust source passed the complete workspace gates, and the frozen
executable passed independent mainnet and browser acceptance. All D1–D8 page
groups are now implemented. Next: the user's whole-site review and final
verification on their updated instance.

### Design decisions

- Entity and route 404s and crashes share the EVA-style serif title card,
  bundled Noto Serif Display and canonical capacity/lock/type/data palette.
  The random Game-of-Life component has been removed.
- The board's historical heights and recorded replay are replaced by the
  current bounded canonical sample. `/tokens` already redirects to Inventory;
  an arbitrary unknown route is not described as a known moved page.
- The mockup's unconditional claims that the chain/data are healthy and that
  reload cannot help are not evidence supplied by a rendering failure. The
  crash page instead reports the error, full route, network, build and an
  explicitly unchecked chain tip, with copy and recovery controls.
- Reduced motion, hidden pages, disconnection, source failure and network
  switching are explicit states. An outer provider failure renders a static,
  provider-independent recovery page rather than re-entering failed providers.

### Review and integration corrections

Review corrected exact checked-endpoint provenance, lower-height reorg and
same-height hash replacement, old-network canvas pixels, and sample eviction
being confused with consumption. Endpoint regressions also cover invalid source
sizes, incomplete type evidence, missing/conflicting canonical markers and the
64-cell/288-output bounds. Shared WebSocket subscribers reuse a connecting
socket; failed handshakes now exhaust the existing ten-retry budget.

The first combined frontend run exposed five obsolete/ambiguous page assertions
and missing strict MSW WebSocket/snapshot fixtures. These are integration test
failures, retained in the ignored acceptance artifacts. Assertions now name the
checked-endpoint field, distinguish 404 from 500, and use a local WebSocket plus
the Rust-produced vessels fixture. No tests were skipped and the strict
unhandled-request policy remains enabled. An earlier
foundation run's localhost-listener failures were sandbox restrictions; the
affected Vite suite passed when run with local listening allowed.

Independent mobile acceptance then found that a full API endpoint in the
finding paragraph extended to x=712 in a 390 px viewport. The shell's hidden
overflow concealed this from the document-width assertion. Final acceptance
checks individual visible text ranges in addition to page width. The first
browser run also had four clipboard waits fail because its case-sensitive
selector expected `Details copied`, while the product correctly reported
`Error details copied.`; the harness now matches the actual message. A later
spent-cell probe initially asserted its consuming-transaction link before the
separate transaction request had completed; it now waits for that evidence.
Both harness failures are retained in preflight artifacts. The final run has
zero failed cases.

### Workspace verification

At `0f752d6247da72aa147409a4cb06f674688e38b1`, the complete Rust workspace
passed **4,769 tests in 75 suites**, with zero failures. Three existing opt-in
native-VM tests remain unrun by the default command. Formatting, workspace
checking and strict all-target Clippy (`-D warnings`) passed. All **26**
regenerated API producer fixtures match their committed frontend consumers.

The same revision passed **1,720 frontend tests in 217 files**, type checking,
lint and production build. The final mobile fix at
`15d92d55c3175a4d957cad126a9014bc5bdf46d2` passed the complete frontend suite
again: **1,721 tests in 217 files**, type checking, lint and production build.
Rust sources and manifests are unchanged from the complete workspace run.

### Frozen acceptance artifact

- Branch: `integration/site-redesign-round7`.
- Implementation revision: `15d92d55c3175a4d957cad126a9014bc5bdf46d2`.
- Executable: `.worktrees/site-redesign-round7-results/d8-errors-vessels-15d92d55c317-ckbadger`.
- SHA-256: `99206009f342543a49b95cbfe7419690e170aadad905301402d697ad61789b98`.
- Built with `cargo build -p ckbadger --offline --locked -j 2` after the final
  frontend build; frozen at 2026-10-28 02:17:43 UTC. The acceptance runner checks
  this executable's hash before starting its own secondary API.
- Ignored working evidence is retained under
  `.worktrees/site-redesign-round7-results/`: `accepted-build.json`,
  `full-gates-0f752d62/`, `integrated-frontend-gates.json`, `accepted-results.json`,
  `oracle-results.json` and `browser/`. `preflight-browser-0f752d62/` preserves
  the earlier browser run and clipping evidence rather than replacing it with
  a passing report.

### Independent mainnet and browser acceptance

Final acceptance ran **2026-10-28 02:20:26–02:22:01 UTC** against the frozen
executable and final frontend bundle. Seven HTTP probes confirmed canonical
block/transaction/cell reads, correct primary 404s, and scoped alternative
identifier searches.

The independent oracle passed **305 assertions**, using only **12 raw-block
RPCs** for the exact artwork window ending at **20,854,604**, hash
`0x32c2868a5d2705bd470fd14776988222bc7f6b34f029408503b9e611de599921`.
It reconstructs births and spends from raw transactions, checks parent/hash
continuity, and independently applies the selection rule. All **35 displayed
cells** match in order, exact shannons, occupied capacity, four anatomy byte
counts and creation height. The window contains **4 spent cells**; a separate
snapshot checks **35 tracked outpoints**, including exact consumer hashes and
heights. This is an exact proof of the bounded displayed sample, not a sampled
estimate of whole-chain totals. No complete header scan is involved.

Chromium passed **26 cases**: thirteen states at both **1440 and 390 px**.
Real requests cover unknown routes, block-as-transaction and
transaction-as-block alternatives, absent cells, an independently proven spent
cell and actual new-block refresh. Controlled fixtures cover search/API/source
failure, rendering crashes, a very long crash route and WebSocket disconnection;
reduced motion is also exercised. The spent cell remains a normal detail page
and links to its raw-block-proven consuming transaction.

The two live-feed cases received actual `new_block` WebSocket messages and
advanced the HTTP snapshot from **20,854,609 → 20,854,610** and
**20,854,616 → 20,854,617**, respectively. Reorg, same-height replacement,
duplicate-event and network-switch behavior are covered by automated tests;
the live session does not claim to have induced a mainnet reorg.

All browser cases check document width and visible text ranges, one footer,
local font resources, finite SVG geometry and correct error classification.
Alternative links and diagnostic copying work with keyboard Enter; copied
reports preserve the full long route and network. Desktop/mobile route, entity
and crash captures were visually reviewed against the approved boards. Current
chain anchors and verified findings replace the boards' historical examples,
and long evidence wraps without truncation.

### Cost and operating boundary

The previous random artwork performed **zero cell-snapshot reads**. The new
artwork performs one bounded initial snapshot and refreshes on chain changes;
an indexed view behind the received event polls every two seconds until it
catches up. Hidden artwork releases its realtime subscription. Concrete payload
size in the final 35-cell snapshot is **10,145 bytes** as compact JSON. Five
warm localhost snapshot reads took **4.153–7.212 ms**, median **5.676 ms**.
These are local observations, not a throughput guarantee or a speedup claim
against the old zero-read artwork. Bulk-sync write cost is unchanged: no writer
or persisted aggregate changed.

Re-sync required: **no**. The user's existing services and primary stores are
not restarted, purged or rebuilt. Acceptance uses task-owned read-only API
secondaries and preview processes. Both final child processes stopped; their
PIDs are absent and ports **18108/3151** are released, recorded in
`cleanup-verification.json`. Agent-owned visual-check servers and browsers were
also stopped. This D8 sample proof is not a replacement claim for another full
64-check chain run.
