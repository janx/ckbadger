## Goal

Verify the final integrated frontend after the API serializer, pool width, pending composition, genesis, and block hash fixes.

## Principle Alignment

Exact chain-derived values remain complete in the UI. Validation uses isolated fixtures without restarting services, writing chain stores, or changing production source.

## Result

- Initial integrated gate input: root HEAD `9f32c2a73acdc4a3c5c29d8853e3c6b9e5d84d97`, plus the `frontend/app/blocks/page.tsx` Hash `text-xs` change later committed as `533eda7e`. HEAD stayed unchanged through the run.
- `pnpm type-check`, `pnpm lint`, `pnpm test`, and `pnpm build` all passed. The full suite passed **1,445 tests in 175 files**; build produced both the SPA and agent renderer.
- Final Chromium acceptance covered Block Activities, Block Transactions, Blocks, Transactions, and Pool at 1440 and 390px, plus a constrained 928px block transaction table. All 11 states had zero page errors and document scrollWidth equal to viewport width.
- The natural block table is 968px with scrollWidth968. At the requested constrained928 budget, clientWidth/scrollWidth are both928; the cycles edge is inside the container. Pool/Blocks/Transactions tables each have clientWidth/scrollWidth1248. Every inspected table cell fits its width.
- Every rendered transaction hash is neutral, without nested copy affordances. All eight Blocks hashes occupy exactly one line at each viewport. Final screenshots were opened and visually inspected.
- Re-sync required: **no** for this validation and the frontend fixes.
- The integrated backend gate also passed. Next: the user performs the final re-sync and live historical acceptance. See the [consolidated execution report](../SITE_REDESIGN_EXECUTION.md) for the final checks and scoped performance conclusion.

### Final D2 cell-evidence integration

- Root source `66a7314a` passed the complete frontend gate: type-check, lint, **1,456 tests in 176 files**, and the production SPA/agent-renderer build. HEAD remained unchanged throughout this run. Subsequent Rust test-fixture corrections do not change the frontend sources.
- The [transaction-cell follow-up](2026-10-27-transaction-cell-semantics.md) adds exact token quantities, object/identity/file metadata, verified or pending media badges, and CKBFS continuity. Its eight additional browser states cover normal, file hover, object keyboard focus and pending media at 1440/390 px. Root inspected the desktop/mobile captures; measured document and cell widths fit, with no page errors.
- These eight states complement the earlier eleven; both groups are deterministic API-shape fixtures, not acceptance against a re-synced live database. Their artifacts and reproduction recipe remain separate.
- Final root metadata: `.worktrees/site-redesign-perf-results/root-d2-frontend-gates.json`; logs: `frontend-d2-integrated-{type-check,lint,test,build}.log` in the same directory. The prior gate and browser evidence below are retained.

### Evidence

- Gate metadata: `frontend-final-gates.json`; logs: `frontend-final-{type-check,lint,test,build}.log` under `.worktrees/site-redesign-perf-results/`.
- Geometry/hashes/errors (same artifact directory): `frontend-final-visual.json`; run output: `frontend-final-visual.log`; reproducer: `visual-final.cjs`.
- The preview is stopped and its temporary frontend entries were removed. To reproduce, copy
  `docs/superpowers/plans/2026-10-26-site-redesign-acceptance/root-preview.html` and
  `root-preview.tsx` into `frontend/.root-preview.html` and `frontend/.root-preview.tsx`, start
  the frontend Vite server on `127.0.0.1:3133`, then run `visual-final.cjs`. Stop that preview
  and remove only those temporary entries afterward.
- Screenshots: `docs/superpowers/plans/2026-10-26-site-redesign-acceptance/{block,block-txs,blocks,transactions,pool}-{1440,390}.png`, plus `block-txs-constrained-928-1440.png`.
- Non-failing notices: three existing React SceneSvg `act(...)` notices in pipeline animation tests; Vite CJS Node API deprecation; the non-module runtime-config script cannot be bundled; Browserslist data is ten months old. Type-check and lint emitted no diagnostics.
