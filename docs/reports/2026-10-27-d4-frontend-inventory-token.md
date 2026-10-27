# D4 frontend — Inventory and Token slice

## Goal

Implement the approved Inventory and Token boards, and the shared asset detail, capacity-history,
cell-version and feed components used by the remaining D4 pages. This report covers this slice,
not completion of Object, Collection, mNFT or identities.

## Principle Alignment

- CKB Native: occupied/free capacity remains an exact partition; token quantities remain decimal
  integer strings until display. Nullable decimals are raw amounts, and nullable cycles remain
  pending/failed instead of zero.
- Local First: consume read-only API projections. No store, schema, writer or service changes.
- Agent Friendly: real API router JSON is retained in a TypeScript `satisfies` fixture, with page
  regressions exercising the producer responses rather than only hand-written response mocks.
- Fail Fast: same-response distribution totals, capacity partitions, cell-version anatomy and
  complete-day/week bounds reject inconsistent values with context. Independent request snapshots
  are not checked against each other as if they shared an anchor.

## Result

- Inventory has three class tabs, exact global counts, standard/composition controls, full decimal
  amounts, live-versus-minted distinctions and explicit DOB decoding-pending state. Filters reset
  cursor pagination. Failed background reads retain rows and show the refresh error.
- Token has same-symbol collisions, a proven-or-unknown issuer, synchronized capacity/supply/weekly
  charts, complete supply-change evidence, concentration and balance buckets, top/all holders,
  full Activities and Transactions, and the raw type script after the 72 px tier boundary.
- Holder-row percentages use the total supply in the same holder-page response. Token history
  uses complete indexed UTC+8 days; supply-event dates explicitly use raw block timestamps in
  UTC+8. Every displayed integer and fractional amount is preserved.
- Shared cell-version views consume the complete oldest-first history, exact occupied-byte
  anatomy, canonical consumption and original lock/type script evidence.
- Re-sync required: no.
- Next: integrate the remaining D4 asset/identity pages and independently accept all final routes
  at 1440 and 390 px. The root-owned acceptance report records the combined result.

### Evidence

Logs are under `.worktrees/site-redesign-round3-results/` (ignored artifacts):

- `d4-baseline.log`: 111 existing D4 tests in 13 files passed before the refactor.
- `chart-baseline.log`: 22 shared foundation tests passed before the controlled-axis extension.
- `inventory-red.log` and `inventory-standard-red.log`: incorrect object activity/count semantics,
  error-versus-empty behavior and the xUDT label reproduced before correction.
- `token-event-date-red.log`: an event after UTC+8 midnight incorrectly used the previous UTC date.
- `token-slice-tests.log`: 145 focused tests passed at the first slice gate.
- `token-review-green.log`: 64 tests passed after snapshot, metadata scale, date and lifecycle review.
- `producer-green.log`: 38 Inventory/Token/producer tests passed; `producer-types.log` passed.
- `slice-typecheck.log`, `slice-lint.log`, `slice-build.log`: passed, including the SPA and agent renderer.
- `slice-full-test-escalated.log`: all 183 files / 1,506 tests passed before the browser follow-up.
- `early-browser-red.log`: three root browser findings reproduced in regressions: capacity semantic colors, long H-multiplier wrapping, and latest-activity / duplicate-order text.

The initial full-suite attempt (`slice-full-test.log`) hit sandbox `listen EPERM` in the two Vite
proxy socket tests, plus one existing Address pagination wait timeout under concurrent load.
The listener-enabled rerun is retained as `slice-full-test-escalated.log`; no assertions were relaxed.
`early-browser-green.log` then passed all 46 affected tests (including all shared Chart foundations), and `early-browser-typecheck.log` passed.
Build warnings are the existing runtime-config non-module script notice and old Browserslist data.

### Explicit evidence boundaries

- `asset-producer.ts` contains unchanged real router JSON from synthetic persisted API test
  fixtures. It proves wire compatibility, not live mainnet correctness.
- The task-owned preview has `scenario=producer` and a distinct `scenario=board` with synthetic
  demonstration values. Browser evidence is fixture-backed, not a rebuilt live-store observation.
- Inventory totals are read at their returned canonical anchor; the independently refreshed
  cached list can lag. The UI states this difference instead of claiming an atomic combined view.
- Stored token metadata does not retain per-field provenance. The UI explicitly says the source
  is not recorded. Recovering provenance requires a later owning writer/schema change; this
  read-only slice does not infer it from a current label or an arbitrary compatible script.
