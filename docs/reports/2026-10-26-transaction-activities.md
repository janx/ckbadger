## Goal

- Implement round 1 D2 Transaction (Main v5) and Activities on the shared Phase A foundations, with exact chain evidence and responsive interaction.

## Principle Alignment

- CKB Native: whole cells flow through a capacity pool, script groups distinguish execution from output attachments, and witness bytes and read-only deps stay visible as evidence.
- Local First: no database writes, service restarts, purges, or live-store rebuilds. The page reads the backend's canonical classifications, witness fields, output liveness, and dep resolution.
- Agent Friendly: one shared activity classifier/headline accepts an `ActivityBody`, including an uncommitted pool body without fabricated block/time fields. One shared row renders the homepage and Activities.
- Fail early: committed cell capacities and occupied bytes are required; byte ranges, integers, selected witness/group identifiers, and continuity endpoints retain explicit validation. No estimated per-group cycles or protocol counts are introduced.

## Result

- Transaction now leads with the activity and participant balance changes, then consumed/created cell tiles, a measured capacity pool/fee diagram, and exact object/identity type-instance continuity. Facts follow the domain view. Developer evidence starts after the shared 72 px tier boundary.
- Script groups, cells, witnesses, and expanded dep-group members link on hover/focus. Executed input locks and same-hash attached output locks remain separate. Witness selection persists in the URL and uses backend field ranges; unknown bytes remain opaque. Raw cells/scripts/JSON and the lazily loaded graph remain available.
- Pending, proposed, and committed-awaiting-index states preserve polling and transient unresolved-input behavior. The page does not invent committed timestamps, confirmations, or derived totals.
- Activities has completed-day statistics (corrected to UTC+8 and exact unique totals in the final-controls follow-up), class filter counts, the shared flat activity renderer, day dividers, polling/head buffering, infinite paging, and an explicit Older control. Missing completed-day data stays explicit. Protocol is unnumbered because the daily contract does not supply an exact protocol count.
- Headlines now distinguish mint/burn/transfer using the exact net delta of the same item across all participants. Participant-only rendering does not turn a transfer into a mint. RGB++ actions have plain headlines, lock-transition descriptions, and exact Bitcoin tx/output references in the approved copper colour.
- Re-sync required: **no** for this frontend lane. Backend/indexer re-sync requirements belong to their respective reports.
- Next: integrate with the API contract and backend lanes, then run the integration gates. Historical-height comparison against rebuilt services belongs to the user's later re-sync acceptance; screenshots here are deterministic fixtures, not a claim of live historical-data validation.

### Validation

- Existing full suite before the refactor: 1,381 tests passed at Phase A completion.
- Full D2 frontend suite: **170 files, 1,397 tests passed**. Later pool-body typing and context-only changes have focused classifier/headline/protocol/transaction coverage.
- `pnpm lint` and production `pnpm build`: passed.
- `pnpm type-check`: passed with the parent lane's committed API/Blocks/Transactions contracts and fixture changes temporarily overlaid, unstaged. Those copies are not included in this commit.
- Regression-first checks caught the mint/transfer distinction and the same-hash input/output-lock hover ambiguity. Existing pending/retry/error/graph/.cell/witness selection tests remain, adapted to the approved layout and server-owned witness decoding.
- Chromium at 1,440 px and 390 px: transaction and activities pages have no browser errors and document scroll width equals viewport width. Selecting witness #0 linked the four exact fixture cell members; byte 20 hover selected its lock field.
- Screenshot artifacts: `docs/superpowers/plans/2026-10-26-site-redesign-acceptance/tx-{1440,390}.png` and `activities-{1440,390}.png` (local ignored acceptance artifacts).

### Contract limits kept explicit

- Exact per-group cycles appear when supplied; null says unavailable, never a share of transaction cycles. Output locks are marked as attached and not executed.
- Tile content uses canonical cell classification and registered script names. The transaction contract does not supply per-cell token/object display metadata; item changes remain available in the balance table without inventing per-cell amounts.
- Live historical snapshot validation awaits the separately authorized user rebuild/re-sync; this lane changes no running service.
