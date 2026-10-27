# Site redesign round 6 — Fiber, Forks, Network and Hardforks

## Goal

Complete D7 of `2026-10-26-site-redesign-implementation-plan.md`, following
D6 DAO and Charts acceptance. The user explicitly requested implementation by
Sol agents and continuation through D7. `FINAL_REVIEW.md` overrides older
board typography and spacing; historical board numbers are examples, not
current chain evidence.

## Principle Alignment

- Start with `docs/prompts/WORLD_VIEW.md` and `INFORMATION_DESIGN.md`;
  fork work also follows `REORG_HANDLING.md` and relevant postmortems.
- Read existing chain and network stores. No production store writes, schema
  changes, backfills, new crawls or primary-service restarts belong to this round.
  Domain and append-only state remains indexer-owned; network state remains
  crawler-owned. APIs use isolated read-only secondaries for acceptance.
- Preserve exact capacities, token quantities, epoch fractions and timestamps.
  Distinguish unavailable evidence from a real zero or empty result.
- Fiber represents what this chain reveals about channels, not off-chain
  routing or payment volume. A party attribution requires evidence.
- A node's observed chain switches differ from canonical uncle inclusion.
  Historical branch membership does not imply a block is still noncanonical.
- Network observations are scoped, retained crawler evidence. Advertisements
  are directed claims about peers, not established connections.

## Execution

Three `gpt-6-sol` agents implement isolated slices. Root coordinates contracts,
reviews integration, runs independent acceptance and records the result.

| Lane        | Ownership                                                                                   | Worktree                                  |
| ----------- | ------------------------------------------------------------------------------------------- | ----------------------------------------- |
| Fiber Sol   | Network and channel API/read models, pages, fixtures and tests                              | `.worktrees/site-redesign-round6-fiber`   |
| Forks Sol   | Switch history, branch and transaction fate, uncle history, pages and tests                 | `.worktrees/site-redesign-round6-forks`   |
| Network Sol | Retained advertisements and daily coverage, Network and Hardforks pages, evidence and tests | `.worktrees/site-redesign-round6-network` |

Each lane owns only its corresponding sections of `frontend/lib/api.ts` and
resource tests. Real serialized API fixtures connect producers to consumers.

| Slice         | Required result                                                                                                              | Acceptance                                                                                          |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Fiber network | Exact pair aggregation, channel states, lifetime and unsettled force-close scope                                             | Complete scope before pagination; explicit unknown parties; no off-chain claims                     |
| Fiber channel | Three on-chain stages, attributable opening contributions and settlement payouts, commitment arguments, exact ready epoch    | Open, settled and waiting states; fractional boundaries; actual transaction, cell and address links |
| Forks         | Node switch history, two branches, reversal links, current canonical status, uncle inclusion and transaction re-entry        | Reversed events and canonical re-entry; exact timestamp differences; distinct daily uncle counts    |
| Network       | “Where the nodes are”, software/time views, directed retained advertisement evidence and explicit daily observation coverage | Latest completed versus active round; missing days remain gaps; disabled crawler is explicit        |
| Hardforks     | VM/version and hash-type timeline, network-specific activation and consensus evidence                                        | Mainnet/testnet boundaries and network mismatch; no artificial developer-tier gap                   |
| Presentation  | Approved hierarchy and drill-downs                                                                                           | 1440/390 px, keyboard/readouts, loading/error/empty, no page overflow                               |

### Network coverage boundary

The current `HistoryPoint` stores a scalar and distribution buckets. The crawler
overwrites each hourly/daily bucket after a completed round; it does not retain
a round count or round ID in that history. Exact historical rounds per day are
therefore unavailable from this store. D7 reports retained observation coverage
and explicit missing dates, with the count's unavailability stated. It does not
invent zero counts or add a crawler write path to reproduce board literals.

Advertisement evidence retains first/last observation rounds, not membership in
every intervening round. A graph must identify the observed endpoint round and
retention limits; it must not fill intermediate memberships or imply a complete
historical topology.

## Result

D7 is complete at implementation revision `53ab417b`, integrated on
`integration/site-redesign-round6`. All three Sol lanes delivered their slices,
with root integration and independent acceptance. The final board review also
closed the Hardfork deployment markers, Network palette, Forks topic title and
mobile prose wrapping.

- The combined workspace gate passed **4,762 Rust tests**. After the final
  API/presentation changes, **1,381 API tests** and **1,679 frontend tests** passed,
  with type checking, lint, builds, formatting and strict workspace Clippy.
- **25 API producer fixtures** match frontend consumers. Selected mainnet
  evidence passed **1,879 assertions**, plus **20 fork headers / 60 comparisons**.
  **22 real page/width combinations** and **70 controlled states** passed.
- Re-sync required: **no**. No production write path or schema changed. All
  task-owned API/preview processes stopped; existing services were not restarted.
- The [D7 acceptance report](reports/2026-10-28-site-redesign-round6-acceptance.md)
  records exact revision/binary provenance, test boundaries, fixed failures,
  calendar and observation limits, screenshots and read-performance costs.
- What to do next: **D8 not-found/crash pages**. The requested finish line through
  D7 is complete.
