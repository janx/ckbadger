# Site redesign round 7 — Not found and crash pages

## Goal

Complete D8 of `2026-10-26-site-redesign-implementation-plan.md` after D7
acceptance. The user requested the next step with code implementation assigned
to Sol agents. The approved entity/route not-found and crash boards share an
EVA-style title card and chain-backed cell vessels. `FINAL_REVIEW.md` governs
typography and spacing; board data and replay events are illustrative only.

## Principle Alignment

- Begin with `docs/prompts/WORLD_VIEW.md` and `INFORMATION_DESIGN.md`.
- Distinguish an absent entity, an unknown route and an unsuccessful request.
  Alternative identifiers require actual search evidence. Describe the scope
  of the checks performed rather than implying exhaustive chain/pool coverage.
- Use real cell payloads and exact capacity/occupied-byte arithmetic. Bound the
  artwork's working set and disclose its selection and anchor. New-block events
  advance the display; historical replay must not be represented as live.
- Keep all production stores read-only in this round. No schema changes,
  re-sync, primary-service restarts, crawler writes or full-header scans.
- Preserve the canonical cell anatomy colors and bundled Noto Serif Display.
  Recovery controls must work with long errors, reduced motion and unavailable
  auxiliary data at both desktop and mobile widths.

## Execution

Three existing `gpt-6-sol` agents implement isolated slices from `d5da6e4c`.
Root coordinates contracts, reviews integration and runs independent acceptance.

| Lane         | Ownership                                                                                | Worktree                                   |
| ------------ | ---------------------------------------------------------------------------------------- | ------------------------------------------ |
| Vessels Sol  | Chain-backed bounded artwork, feed lifecycle, read API if needed and tests               | `.worktrees/site-redesign-round7-vessels`  |
| Evidence Sol | Identifier classification, search evidence, individual entity error branches and tests   | `.worktrees/site-redesign-round7-evidence` |
| Shell Sol    | Shared EVA template, route/crash boundaries, recovery, responsive presentation and tests | `.worktrees/site-redesign-round7-shell`    |

Acceptance covers true 404 versus transport/internal failure, typed alternative
links, exact real-cell anatomy, duplicate/reorg/network-switch handling,
disconnected and reduced-motion states, keyboard recovery, local fonts and
1440/390 px geometry. Live observations and controlled failure fixtures are
reported separately. Temporary acceptance services use isolated read-only
secondaries and are stopped afterward.

## Result

The shared template now covers entity/route 404s and rendering failures. Exact
typed search evidence supplies alternative identifiers; unavailable requests
remain errors. Real canonical cells replace random artwork, with bounded reads,
explicit source captions and new-block/reorg refreshes. Recovery preserves full
diagnostic details and supports keyboard operation, local fonts and reduced
motion. An independent mobile check found and corrected long evidence text
clipping.

The complete Rust workspace passed **4,769 tests in 75 suites** and all **26**
producer/consumer fixture comparisons. Final frontend gates passed **1,721
tests in 217 files**, type checking, lint and build. The frozen implementation
at `15d92d55` passed **305 independent mainnet assertions** using only its
12-block window and **26 browser cases** at 1440/390 px, including actual
new-block updates. The [detailed report](reports/2026-10-28-site-redesign-round7-acceptance.md)
records revisions, real-chain proof, controlled failures and operating costs.

Re-sync required: **no**. No schema, indexer or production store write path
changed. The user's primary services and stores remain untouched; all task-owned
acceptance services have stopped. D1–D8 page implementation is complete; the next step is the user's
whole-site review and the plan's final verification on their updated instance.
