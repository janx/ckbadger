## Goal

- Finish the round-one frontend integration states: pending DOB composition, genesis transaction evidence, and complete block transaction columns at the approved viewport sizes.

## Principle Alignment

- CKB Native: `decode_pending` stays explicitly unclassified until the indexer supplies execution evidence. Genesis transactions show their actual no-proposal origin and zero execution state.
- Local First: pending-only HTTP polling waits for indexer-owned DOB results. The frontend neither calculates storage classifications nor writes any store.
- Agent Friendly: one shared tier label/tooltip/style mapping is reused by object and mNFT pages. Exact bigint amount formatting remains a single calculation path; narrow tables change only presentation.

## Result

- Pending composition reads “Decode pending” with a neutral style and an explanation that DOB execution has not completed. Inventory table/cards, object details, and collection cards consume the same state.
- The decoded-DOB query polls only while its returned status is pending. Decoded and failed results refresh the matching object detail once so persisted media classification becomes visible; terminal results and request errors stop polling.
- Block tables accept non-cellbase genesis transactions without requiring or inventing a proposal. Transaction detail prevents genesis from enqueueing a canonical lazy replay.
- Block transaction tables use an 898px column budget and stacked size/cycle bars. All resource counts remain exact integers. CKB values retain all eight decimals and may wrap before the decimal or unit; global table column widths and inline resource presentation remain unchanged.
- Committed and pool tables now use the shared single-color Hash component. Hash links preserve the existing visible truncation, expose the full hash in their title, and have no nested copy affordance. Raw byte viewers remain separate.
- Re-sync required: **no** for these frontend changes.
- Next: merge this follow-up; parent owns the final integrated full-suite/build gate.

### Validation

- Regression tests cover pending label/style/tooltip propagation, pending-to-decoded refresh, failed/error polling termination, genesis proposal/replay behavior, exact wrapping, stacked resource evidence, and neutral navigable table/pool hashes.
- After the indexer lane released its quiet window, the pre-refactor mNFT, amount, and resource baseline passed 44 existing tests. The old implementation reproduced 17 targeted failures; a separate run with the label fixes but old query lifecycle reproduced all three DOB lifecycle failures.
- The fixed implementation passed 188 tests across 15 files, including new BlockPosition/DAO coverage from the preceding quiet-window commit. `pnpm type-check`, `pnpm lint`, and `git diff --check` passed. No test/build process was started during the performance window.
- Fixture-backed Chromium at 1440px with the actual 928px table budget: table clientWidth/scrollWidth both 928, every cell has equal client/scroll widths, and the cycles column ends at the table boundary. The fixture includes `184,467,440,737.09551615 CKB`, `1,234,567.89012345 CKB`, and `3,500,000,000` cycles.
- At 390px, document scrollWidth is 390. Both viewports have zero page errors. Screenshots are local ignored artifacts under `docs/superpowers/plans/2026-10-26-site-redesign-acceptance/block-transactions-complete-{1440,390}.png`.
