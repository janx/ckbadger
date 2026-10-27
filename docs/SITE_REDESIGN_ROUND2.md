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

## Result

Implementation and acceptance are in progress. D4–D8 remain subsequent rounds
in the approved page order. This document will record exact validation evidence
and the final remaining work after integration.

- Re-sync required: **no**.
