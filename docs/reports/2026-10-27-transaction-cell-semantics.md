## Goal

- Complete the approved Main v5 transaction cell tiles and CKBFS continuity in D2. The previous tiles exposed generic classes or script names and omitted the board's file connection.

## Principle Alignment

- CKB Native: tiles consume exact per-cell API descriptors. File continuity requires a one-input/one-output type group and matching file identifiers; filenames never establish identity.
- Local First: this frontend change adds no indexer work, persistence, or additional metadata requests. It consumes the API lane's read-time descriptor.
- Agent Friendly: the typed descriptor, reusable content component, explicit missing metadata, and regression fixtures preserve a single exact amount-formatting path.

## Result

- Input/output tiles show the standard, canonical item identifier, available item/collection name, and detail link. CKBFS tiles show the decoded filename and link to the actual cell. mNFT items use their existing item route.
- Token tiles format API base-unit strings with the existing BigInt formatter. Unknown decimals are explicitly raw; absent amounts are unavailable, never zero.
- Verified media composition appears as a compact badge. BTC+CKB uses the approved copper/jade treatment. Missing classification does not invent a badge; `decode_pending` is neutral and says it awaits DOB execution.
- File continuity uses a neutral dashed line; object/identity continuity retains its class color. Legend hover/focus now also highlights the corresponding script evidence. Ambiguous groups and different identifiers do not receive a file connection.
- Re-sync required: **no** for this frontend/API-read correction. The separate round-one indexer changes retain their own re-sync requirements.
- Integrated with the API lane's `display` response. These checks do not establish that a running or rebuilt store already returns the new data. D3 and later page redesigns remain outside this correction; the user's live acceptance follows the one final re-sync.

### Validation

- Existing transaction/foundation baseline: **40 tests passed** before edits. The missing file connection and all four metadata cases reproduced failures before their implementation.
- Focused final coverage: **62 tests across 4 files**. Includes object/media/file/token/identity cells, unknown metadata, exact large token quantities, invalid continuity, script/witness linkage, and existing raw rendering.
- Final frontend gate: `pnpm type-check`, `pnpm lint`, `pnpm test`, `pnpm build`, and `git diff --check` passed. **176 files / 1,456 tests passed**. The first full-suite attempt hit two sandbox loopback-listener failures in the existing Vite-config tests; the full suite then passed outside the sandbox.
- The same complete gate also passed on integrated root source `66a7314a`: **176 files / 1,456 tests**, type-check, lint and both production builds. Root metadata is `.worktrees/site-redesign-perf-results/root-d2-frontend-gates.json`; `frontend-d2-integrated-*` logs retain its output.
- Existing warnings remain: three pipeline `act(...)` warnings; Vite CJS deprecation, the existing runtime-config non-module script warning, and stale Browserslist data. No new page errors occurred.
- API-shape fixture browser checks at **1440 and 390 px** cover normal cell content, file hover, object keyboard focus, witness selection, and decode-pending media. Document widths equal viewport widths; every cell's scroll width fits its client width. File selection highlights input/output 0 and its type group; object and witness selection highlight input/output 1.
- Eight screenshots, the fixture entrypoints, browser recipe, and measured `results.json` are preserved separately under the ignored `docs/superpowers/plans/2026-10-26-site-redesign-acceptance/tx-cell-followup/`. They are fixture evidence, not live-data acceptance. Full test/build logs are in `.worktrees/site-redesign-perf-results/tx-cell-final-{test,build}.log`.
- Task-owned Chromium runs exited and Vite 3132 was stopped before the next performance quiet window. Commit hooks need not repeat the already completed gate during that window.

### Reproduce the browser evidence

From the repository root, copy `tx-cell-preview.html` and `tx-cell-preview.tsx` from the artifact folder into the target frontend as `.tx-cell-preview.html` and `.tx-cell-preview.tsx`. Use an existing installed Playwright/Chromium and set `TMPDIR` to a created directory under `.worktrees/site-redesign-perf-results/`. Start that frontend's Vite on `127.0.0.1:3132` with `--strictPort`, then run the preserved `visual.cjs`. Its output paths stay in the artifact folder. Stop only this task-owned preview afterward.
