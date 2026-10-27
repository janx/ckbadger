## Goal

Implement D3 Address v3 from the approved `Address.dc.html` / `AddressTxs.dc.html` board, including its four holdings classes, capacity partitions, three history tabs, and lock evidence. D4–D8 remain outside this delivery. The approved Address board has no capacity-history chart.

## Principle Alignment

- The balance and occupied / ordinary-free / DAO-free partition consume one pinned holdings response. The page never combines separately cached balances into a fabricated total.
- Physical live-cell ownership stays separate from protocol-named `.cell` rights. Named-only addresses retain their names and activity even when physical balance and cell count are zero.
- Amounts keep exact decimal precision. Pending cycles, nullable item metadata, unresolved composition, and failed refreshes remain explicit. Script references retain their actual hash type.
- All changes are frontend/read-contract consumers. There are no chain-store, append-only-store, or network-store writes, service restarts, or schema changes.

## Result

- Added the exact balance partition, four holdings sections, six-class occupied partition with hover linking, and unframed Activities / Transactions / Live cells tabs. Title / hero / metadata sizes follow the approved 26 / 18 / 11 px scale.
- Holdings counts cover the whole owner. Server-side live-cell filtering happens before cursor pagination. Token kinds count positive balances; zero-token cells remain part of the physical capacity partition.
- DAO rows now describe currently owned cells, including received withdrawal requests. Their block/time links use the current phase's evidence. Identity “held since” consumes the API's exact continuous-ownership field; it does not substitute the current cell's creation height.
- Participant activity labels describe sent/received positions without inferring global mint/burn from a single participant. Pool rows preserve nullable block/time and interpretation state. Shared activity and capacity components retain their existing callers.
- Holdings refresh every 60 seconds while the tab is active, with focus refresh and explicit retained-data error notices. The existing pool polling remains separate.
- New `AddressHoldings` / `AddressCell` contracts match the real Rust router response. `address-producer.ts` contains captured producer JSON from API lane `9618fbb7`, including a transferred DAO request, nullable metadata, exact identity tenure, and a `data1` type-script reference. No legacy `commonKnowledgeSize` field is fabricated.
- Updated the LLM discovery files for the new owner holdings and filtered cells endpoints.
- Re-sync required: **no**.

### Validation

- Before refactoring: existing address tests **31/31**, shared foundation/activity tests **59/59**.
- Regression-first checks covered nullable pool evidence, retained-data refresh errors, participant sent/received wording, standard labels, duplicate balance presentation, and withdrawal-request block/time. RED logs are retained under `.worktrees/site-redesign-round2-results/`.
- Full frontend gate passed: type-check, lint, **1,472 tests in 176 files**, and the SPA plus agent-renderer production build. After adding four independent component tests, the final focused run passed **185 tests in five files**, and type-check/build passed again. The new component tests cover both new component files rather than relying solely on page tests.
- The 44 page tests cover exact producer JSON, all three tabs, filtered pagination/reset, more than one holdings page, named-only rights, nullable fields, stale errors, current DAO ownership, request block links, and identity tenure. API method tests assert class/cursor serialization.
- An isolated fixture preview was exercised at 1440 and 390 px. Independent acceptance reported 22 WIP states passing, including partition hover, tile/table/document widths, named-only/empty/error states, transferred DAO and retained-data refresh errors. Immutable-commit replay is performed by the separate acceptance lane.
- These browser states are deterministic frontend fixtures, including captured API producer responses; they are **not** evidence from the user's running database. Integrated API/frontend gates and live read-only smoke checks are coordinated by the root agent.

### Evidence and reproduction

- Gate logs: `.worktrees/site-redesign-round2-results/{test-final,focused-final,type-check-final,lint-final,build-final}.log`. Full-suite non-failing React `act(...)` notices occur in existing pipeline animation tests; the focused address run has no diagnostics.
- RED logs: `activity-pool-red.log`, `stale-red.log`, `final-fidelity-red.log`, and `dao-phase-red.log` in the same directory.
- Task-only preview: `frontend/.address-preview.html` and `.address-preview.tsx`, served on port 3135. These temporary entries are excluded from the commit. Scenarios are `board`, `named-only`, `empty`, `error`, `transferred-dao`, and `refetch-error`; the last exposes `window.__addressPreviewRefetch()` to exercise retained data after failure.
- Independent browser screenshots, assertions, and rerun recipe are retained by the acceptance lane under the ignored `docs/superpowers/plans/2026-10-27-address-acceptance/` directory. The tracked acceptance report records its final replay.

Next: merge the frontend and API commits, complete the independent immutable browser replay, and run the integrated gate. No frontend contract request remains unresolved.
