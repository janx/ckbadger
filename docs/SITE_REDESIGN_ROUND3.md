# Site redesign round 3 — Inventory and asset pages

## Goal

Implement D4 of the approved `2026-10-26-site-redesign-implementation-plan.md`
after the accepted Address v3 round. This round covers Inventory's three views,
Token, Object, Collection, m-NFT and the four identity item pages. Existing
collection routes must retain their usable listing and detail behavior.

Starting revision: `cce36584`. Integration branch:
`integration/site-redesign-round3`. D5–D8 remain subsequent rounds.

The original boards and snapshots live in the ignored
`docs/superpowers/plans/2026-10-25-site-ui-review/` directory. Its
`FINAL_REVIEW.md` overrides older board typography, Bitcoin colour, terminology
and tier spacing. The previous round's
[remaining-gap audit](reports/2026-10-27-site-redesign-round2-acceptance-and-remaining-gaps.md)
is an initial source map, not proof that every listed contract is absent today.

## Principle Alignment

- Start with `docs/prompts/WORLD_VIEW.md`, `INFORMATION_DESIGN.md` and
  `ACTIVITY_DESIGN.md`. Items lead to their current and historical cell evidence;
  physical lock ownership remains distinct from protocol-named rights.
- Read-time API and frontend work only: no new domain, append-only or network
  writes, schema migration or backfill. Existing services and primary stores are
  not restarted, purged or rebuilt. An isolated read-only API may be used for
  live evidence, with separate secondary directories.
- Exact amounts and full scopes precede pagination. Missing fields and decoding
  failures stay explicit; no approximations, fallback calculation chains or
  historical metadata borrowed from today's mutable state.
- Capacity history stays in tier 1. Reuse the accepted indexed calendar and
  existing canonical history; do not independently fetch every chain header.
- Activities and Transactions appear together. Shared feed joins retain all
  participants and use one transaction calculation path. No new transaction
  index or write-time work is introduced.
- Bitcoin media stays link-out; decode-pending is not pure CKB. All numerical
  provenance and fixture/live differences must be recorded during acceptance.

## Execution

| Lane      | Ownership                                                                              | Worktree                                   |
| --------- | -------------------------------------------------------------------------------------- | ------------------------------------------ |
| Token API | Inventory, token history/supply/holders, shared entity feed contracts                  | `.worktrees/site-redesign-round3-token`    |
| Item API  | Objects, collections, DOB, m-NFT and identity evidence; shared feed integration        | `.worktrees/site-redesign-round3-assets`   |
| Frontend  | All D4 boards, shared components, producer contract fixtures, page/component tests     | `.worktrees/site-redesign-round3-frontend` |
| Root      | Independent requirements and source review, browser/live acceptance, integration gates | Main checkout                              |

The lanes start from the same revision, use the root's exact ignored
`Cargo.lock`, and have separate Rust targets. Read contracts are agreed before
consumers depend on them. Shared Rust helpers belong to one owner; other lanes
consume the helper rather than creating competing calculation paths.

### Required slices and evidence

| Slice       | Required behavior                                                                                                                               | Acceptance                                                                                                                   |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| Inventory   | Tokens / Objects / Identities, category/standard/composition/search filters, complete totals and cursor paging, exact capacity/occupied columns | Three views at 1440/390; multi-page/filter reset; missing metadata and decode-pending                                        |
| Token       | Exact supply event sequence, capacity/supply/weekly history, full holder distribution, current cells, shared activity/transaction views         | Mint/transfer/burn and duplicate transfer rows; unknown decimals; values above JS safe integer; totals close over full scope |
| Object      | Current owner and outpoint, canonical cell versions and capacity, composition, DOB DNA and mapped genes, life history                           | Mint/move/burn; consumed versus current cell; DNA/trait hover; pending/malformed evidence; btcfs link-out                    |
| Collection  | Exact collection totals/history, decoded blueprint, allele frequencies, trait-filtered item plates and shared feeds                             | Whole-collection denominators, multi-page filtering, pending items, holder counts and both widths                            |
| m-NFT       | Protocol-defined configure/state/lineage, current ownership and cell history, class/issuer evidence                                             | Verified bit meanings and unknown bits; live/burned items; actual version bytes and exact capacities                         |
| `.bit`      | Account versions, action attribution, owner/manager/next/expiry and witness records                                                             | Update/renew/transfer/recycle, target versus neighbour action, witness provenance, chronological links                       |
| `.bit Cell` | Physical owner and history, expiry with explicit time basis, proven linked account, raw undecoded fields                                        | Expired but live cell; live/dead linked account; no invented field meaning                                                   |
| did:ckb     | Exact DID, current/historical documents, handles/PDS/keys, live outpoint and PDS grouping                                                       | 20/32-byte identifiers, handle changes, missing service, full grouping scope                                                 |
| `.cell`     | Named owner/manager and their history, current/version records, next/expiry and coin-type labels                                                | Unresolved prefixes, physical owner separation, named-only participants, witness provenance                                  |

Every entity uses the final 26 px title, 18 px hero and 11 px metadata scale;
index titles are 22 px. Raw evidence follows the shared 72 px tier boundary.
No document horizontal overflow is accepted at 390 px. Amounts retain their
precision, hover/focus links identify real entities, and background errors remain
visible even when previous data is retained.

Existing affected tests run before refactoring. Bug fixes reproduce failures
before changes. New routes, components and utilities have meaningful tests.
Each slice supplies exact source commits and captured producer JSON; final
acceptance replays immutable combined code. Root runs integrated Rust/frontend
checks and records fixture and real-chain evidence separately.

## Result

Implementation is in progress. No D4 page is declared accepted solely because
its previous route or a partial read contract already exists.

- Re-sync required: **no** for the planned read-time/frontend scope.
- Next: complete the slices and their gates, then proceed to D5 Scripts/Cells.
