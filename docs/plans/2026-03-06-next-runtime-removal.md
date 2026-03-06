# Next Runtime Removal Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove production runtime dependence on `next/*` from the frontend so the shipped UI is a pure `Vite + React Router` SPA served by the Rust frontend process.

**Architecture:** Replace all runtime `next/*` imports with local frontend abstractions rooted in the SPA shell. Keep `frontend/app/**` as temporary page wrappers where that lowers churn, but move runtime semantics into local modules/components. Remove Vite aliasing and package dependencies that only exist to preserve the old Next runtime boundary.

**Tech Stack:** Vite 5, React 19, React Router, TanStack Query, Vitest, ESLint, Rust Axum frontend server

---

### Task 1: Add canonical local navigation and link entry points

**Files:**

- Create: `frontend/src/navigation.ts`
- Create: `frontend/components/ui/link.tsx`
- Modify: `frontend/components/ui/app-link.tsx`
- Test: `frontend/__tests__/routes/detail-navigation.test.tsx`
- Test: `frontend/__tests__/lib/tooling-config.test.ts`

**Step 1: Write the failing test**

Extend the tooling/config regression test so it imports the new canonical local modules and proves runtime code no longer needs `next/link` as the primary navigation boundary.

Add or update a route navigation test that exercises the canonical link component inside router context.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/detail-navigation.test.tsx __tests__/lib/tooling-config.test.ts`

Expected: FAIL because the canonical local navigation/link entry points do not exist yet.

**Step 3: Write minimal implementation**

Create `frontend/src/navigation.ts` and re-export the local SPA navigation helpers currently living in `frontend/src/next-compat/navigation.tsx`, but under the permanent non-Next path.

Create `frontend/components/ui/link.tsx` as the canonical app link component. It can delegate to the existing `AppLink` implementation first, but the permanent import path must be local and framework-neutral.

Keep `frontend/components/ui/app-link.tsx` as a thin compatibility re-export for now so the migration can proceed incrementally.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/detail-navigation.test.tsx __tests__/lib/tooling-config.test.ts`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/src/navigation.ts frontend/components/ui/link.tsx frontend/components/ui/app-link.tsx frontend/__tests__/routes/detail-navigation.test.tsx frontend/__tests__/lib/tooling-config.test.ts
git commit -m "refactor: add local frontend navigation boundary"
```

### Task 2: Replace runtime `next/navigation` imports with the local navigation module

**Files:**

- Modify: `frontend/components/search-bar.tsx`
- Modify: `frontend/components/command-palette.tsx`
- Modify: `frontend/components/nft/identity-nft-item-detail.tsx`
- Modify: `frontend/components/layout/header.tsx`
- Modify: `frontend/app/[...slug]/page.tsx`
- Modify: `frontend/app/tokens/page.tsx`
- Modify: `frontend/app/nfts/page.tsx`
- Modify: `frontend/app/dao/charts/page.tsx`
- Modify: `frontend/app/forks/[id]/client-page.tsx`
- Modify: `frontend/app/blocks/[id]/client-page.tsx`
- Modify: `frontend/app/address/[addr]/client-page.tsx`
- Modify: `frontend/app/tx/[hash]/client-page.tsx`
- Modify: `frontend/app/cell/[outpoint]/client-page.tsx`
- Modify: `frontend/app/scripts/[name]/client-page.tsx`
- Modify: `frontend/app/script/[codeHash]/client-page.tsx`
- Modify: `frontend/app/assets/assets-page-client.tsx`
- Modify: `frontend/app/nfts/[sporeId]/client-page.tsx`
- Modify: `frontend/app/nfts/mnft/[nftId]/client-page.tsx`
- Modify: `frontend/app/clusters/[clusterId]/client-page.tsx`
- Test: `frontend/__tests__/setup.ts`
- Test: `frontend/__tests__/components/search-bar.test.tsx`
- Test: `frontend/__tests__/components/command-palette.test.tsx`
- Test: `frontend/__tests__/components/header.test.tsx`
- Test: `frontend/__tests__/components/identity-nft-item-detail.test.tsx`
- Test: `frontend/__tests__/pages/address.test.tsx`
- Test: `frontend/__tests__/pages/block-detail.test.tsx`
- Test: `frontend/__tests__/pages/cell.test.tsx`
- Test: `frontend/__tests__/pages/cluster.test.tsx`
- Test: `frontend/__tests__/pages/did-item-detail.test.tsx`
- Test: `frontend/__tests__/pages/dotbit-item-detail.test.tsx`
- Test: `frontend/__tests__/pages/fork-detail.test.tsx`
- Test: `frontend/__tests__/pages/mnft-item-detail.test.tsx`
- Test: `frontend/__tests__/pages/nft-detail.test.tsx`
- Test: `frontend/__tests__/pages/nfts-page.test.ts`
- Test: `frontend/__tests__/pages/script-code-hash.test.tsx`
- Test: `frontend/__tests__/pages/script-detail.test.tsx`
- Test: `frontend/__tests__/pages/token-detail.test.tsx`
- Test: `frontend/__tests__/pages/tx-detail.test.tsx`

**Step 1: Write the failing test**

Update one representative test first, such as `frontend/__tests__/components/search-bar.test.tsx`, to mock the new local navigation module instead of `next/navigation`.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/components/search-bar.test.tsx`

Expected: FAIL because the runtime code still imports `next/navigation`.

**Step 3: Write minimal implementation**

Replace runtime `next/navigation` imports with `@/src/navigation` (or the chosen alias path) across all production components and page clients.

Then update the related tests and the shared test setup to mock the local navigation module instead of `next/navigation`.

Do not keep dual imports in runtime files.

**Step 4: Run test to verify it passes**

Run:

```bash
cd frontend && pnpm exec vitest run \
  __tests__/components/search-bar.test.tsx \
  __tests__/components/command-palette.test.tsx \
  __tests__/components/header.test.tsx \
  __tests__/components/identity-nft-item-detail.test.tsx \
  __tests__/pages/address.test.tsx \
  __tests__/pages/block-detail.test.tsx \
  __tests__/pages/cell.test.tsx \
  __tests__/pages/cluster.test.tsx \
  __tests__/pages/did-item-detail.test.tsx \
  __tests__/pages/dotbit-item-detail.test.tsx \
  __tests__/pages/fork-detail.test.tsx \
  __tests__/pages/mnft-item-detail.test.tsx \
  __tests__/pages/nft-detail.test.tsx \
  __tests__/pages/nfts-page.test.ts \
  __tests__/pages/script-code-hash.test.tsx \
  __tests__/pages/script-detail.test.tsx \
  __tests__/pages/token-detail.test.tsx \
  __tests__/pages/tx-detail.test.tsx
```

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/components frontend/app frontend/__tests__/setup.ts frontend/__tests__/components frontend/__tests__/pages
git commit -m "refactor: replace next navigation imports"
```

### Task 3: Replace runtime `next/link` imports with the local link component

**Files:**

- Modify: `frontend/components/not-found-page.tsx`
- Modify: `frontend/components/nft/identity-nft-item-detail.tsx`
- Modify: `frontend/components/nft/nft-activity-card.tsx`
- Modify: `frontend/components/nft/nft-collection-stat-cards.tsx`
- Modify: `frontend/components/home-charts.tsx`
- Modify: `frontend/components/mempool-blocks.tsx`
- Modify: `frontend/components/latest-transactions.tsx`
- Modify: `frontend/components/latest-blocks.tsx`
- Modify: `frontend/components/deep-fork-alert.tsx`
- Modify: `frontend/components/ui/page-header.tsx`
- Modify: `frontend/components/ui/address.tsx`
- Modify: `frontend/components/ui/chart-card.tsx`
- Modify: `frontend/components/chain-wave/packed-container.tsx`
- Modify: `frontend/components/chain-wave/index.tsx`
- Modify: `frontend/components/layout/site-footer.tsx`
- Modify: `frontend/components/layout/logo.tsx`
- Modify: `frontend/components/layout/header.tsx`
- Modify: `frontend/components/charts/chart-page.tsx`
- Modify: `frontend/app/transactions/page.tsx`
- Modify: `frontend/app/forks/page.tsx`
- Modify: `frontend/app/blocks/page.tsx`
- Modify: `frontend/app/forks/[id]/client-page.tsx`
- Modify: `frontend/app/blocks/[id]/client-page.tsx`
- Modify: `frontend/app/address/[addr]/client-page.tsx`
- Modify: `frontend/app/tx/[hash]/client-page.tsx`
- Modify: `frontend/app/cell/[outpoint]/client-page.tsx`
- Modify: `frontend/app/tokens/[typeHash]/client-page.tsx`
- Modify: `frontend/app/scripts/[name]/client-page.tsx`
- Modify: `frontend/app/nfts/[sporeId]/client-page.tsx`
- Modify: `frontend/app/dao/page.tsx`
- Modify: `frontend/app/nfts/mnft/[nftId]/client-page.tsx`
- Modify: `frontend/app/clusters/[clusterId]/client-page.tsx`
- Modify: `frontend/app/script/[codeHash]/client-page.tsx`
- Modify: `frontend/app/charts/cell-count/page.tsx`
- Modify: `frontend/app/charts/knowledge-size/page.tsx`
- Modify: `frontend/app/charts/total-supply/page.tsx`
- Modify: `frontend/app/hardforks/page.tsx`
- Modify: `frontend/app/charts/miner-address-distribution/page.tsx`
- Modify: `frontend/app/charts/hodl-wave/page.tsx`
- Modify: `frontend/app/charts/common-knowledge-composition/page.tsx`
- Modify: `frontend/app/charts/cell-age-vs-occupied-capacity/page.tsx`
- Modify: `frontend/app/charts/secondary-issuance/page.tsx`
- Modify: `frontend/app/charts/most-utilized-assets/page.tsx`
- Modify: `frontend/app/charts/most-utilized-scripts/page.tsx`
- Test: `frontend/__tests__/routes/detail-navigation.test.tsx`
- Test: `frontend/__tests__/routes/explorer-routes.test.tsx`
- Test: `frontend/__tests__/pages/assets.test.tsx`
- Test: `frontend/__tests__/pages/scripts.test.tsx`

**Step 1: Write the failing test**

Update `frontend/__tests__/routes/detail-navigation.test.tsx` so it imports and uses the canonical local link component path instead of the old compat path.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/detail-navigation.test.tsx __tests__/routes/explorer-routes.test.tsx __tests__/pages/assets.test.tsx __tests__/pages/scripts.test.tsx`

Expected: FAIL until runtime files stop importing `next/link`.

**Step 3: Write minimal implementation**

Replace runtime `next/link` imports with the canonical local link component.

Where plain anchors are enough, keep the local abstraction so navigation behavior remains centralized.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/detail-navigation.test.tsx __tests__/routes/explorer-routes.test.tsx __tests__/pages/assets.test.tsx __tests__/pages/scripts.test.tsx`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/components frontend/app frontend/__tests__/routes frontend/__tests__/pages/assets.test.tsx frontend/__tests__/pages/scripts.test.tsx
git commit -m "refactor: replace next link imports"
```

### Task 4: Replace `next/image` and `next/dynamic` in runtime code

**Files:**

- Create: `frontend/components/ui/image.tsx`
- Create: `frontend/lib/dynamic-client.tsx`
- Modify: `frontend/components/layout/logo.tsx`
- Modify: `frontend/app/address/[addr]/client-page.tsx`
- Modify: `frontend/app/nfts/[sporeId]/client-page.tsx`
- Modify: `frontend/app/assets/assets-page-client.tsx`
- Modify: `frontend/components/proposal-graph.tsx`
- Modify: `frontend/components/cell-graph.tsx`
- Test: `frontend/__tests__/pages/address.test.tsx`
- Test: `frontend/__tests__/pages/assets.test.tsx`
- Test: `frontend/__tests__/pages/nft-detail.test.tsx`

**Step 1: Write the failing test**

Add or update a focused test around one image-using page and one graph component import path so the code no longer relies on `next/image` or `next/dynamic`.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/pages/address.test.tsx __tests__/pages/assets.test.tsx __tests__/pages/nft-detail.test.tsx`

Expected: FAIL until runtime imports are switched.

**Step 3: Write minimal implementation**

Create a local `Image` component with the limited prop surface actually needed by the app.

Create a local dynamic-client helper for client-only graph loading, or inline `React.lazy` where it is clearer. Remove runtime `next/dynamic` usage completely.

Replace runtime `next/image` imports with the local image component.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run __tests__/pages/address.test.tsx __tests__/pages/assets.test.tsx __tests__/pages/nft-detail.test.tsx`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/components/ui/image.tsx frontend/lib/dynamic-client.tsx frontend/components/layout/logo.tsx frontend/app/address/[addr]/client-page.tsx frontend/app/nfts/[sporeId]/client-page.tsx frontend/app/assets/assets-page-client.tsx frontend/components/proposal-graph.tsx frontend/components/cell-graph.tsx frontend/__tests__/pages/address.test.tsx frontend/__tests__/pages/assets.test.tsx frontend/__tests__/pages/nft-detail.test.tsx
git commit -m "refactor: remove next image and dynamic imports"
```

### Task 5: Remove Vite aliasing and Next package assumptions

**Files:**

- Modify: `frontend/vite.config.ts`
- Modify: `frontend/package.json`
- Modify: `frontend/eslint.config.mjs`
- Modify: `frontend/tsconfig.json`
- Test: `frontend/__tests__/lib/tooling-config.test.ts`

**Step 1: Write the failing test**

Extend `frontend/__tests__/lib/tooling-config.test.ts` to assert:

- `package.json` has no `next` dependency
- `package.json` has no `eslint-config-next` dependency
- `vite.config.ts` no longer contains `next/*` aliases

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/lib/tooling-config.test.ts`

Expected: FAIL because the package/config still references Next runtime assumptions.

**Step 3: Write minimal implementation**

Remove:

- `next`
- `eslint-config-next`
- any remaining Next-specific lint/config assumptions
- `next/*` alias entries from `frontend/vite.config.ts`

Ensure lint and type-check still work with the framework-neutral toolchain.

**Step 4: Run test to verify it passes**

Run:

```bash
cd frontend && pnpm install
cd frontend && pnpm exec vitest run __tests__/lib/tooling-config.test.ts
cd frontend && pnpm lint
cd frontend && pnpm type-check
```

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/vite.config.ts frontend/package.json frontend/eslint.config.mjs frontend/tsconfig.json frontend/__tests__/lib/tooling-config.test.ts pnpm-lock.yaml
git commit -m "chore: drop next runtime dependencies"
```

### Task 6: Tighten the transition boundary and verify no runtime `next/*` imports remain

**Files:**

- Modify: `frontend/app/layout.tsx`
- Modify: `frontend/app/providers.tsx`
- Modify: `frontend/src/app/providers.tsx`
- Modify: `frontend/src/app/root.tsx`
- Modify: any remaining runtime files reported by ripgrep
- Test: `frontend/__tests__/routes/router-shell.test.tsx`
- Test: `frontend/__tests__/routes/explorer-routes.test.tsx`
- Test: `frontend/__tests__/routes/detail-route-inputs.test.tsx`
- Test: `frontend/__tests__/routes/detail-navigation.test.tsx`
- Test: `crates/api/tests/frontend_server_spa.rs`

**Step 1: Write the failing test**

Add a tooling regression assertion that scans the runtime frontend tree for `next/*` imports and fails if any remain outside explicitly allowed test files.

**Step 2: Run test to verify it fails**

Run:

```bash
cd frontend && pnpm exec vitest run \
  __tests__/routes/router-shell.test.tsx \
  __tests__/routes/explorer-routes.test.tsx \
  __tests__/routes/detail-route-inputs.test.tsx \
  __tests__/routes/detail-navigation.test.tsx \
  __tests__/lib/tooling-config.test.ts
```

Expected: FAIL until the remaining runtime import sites are removed or relocated.

**Step 3: Write minimal implementation**

Clean up the remaining transition boundary:

- move any still-runtime logic out of `frontend/app/layout.tsx` if it blocks package cleanup
- make `frontend/app/providers.tsx` a thin wrapper only, or remove it if no longer needed
- eliminate any last runtime `next/*` imports

Keep test-only mocks if they are still useful, but runtime code must be clean.

**Step 4: Run test to verify it passes**

Run:

```bash
cd frontend && pnpm exec vitest run \
  __tests__/routes/router-shell.test.tsx \
  __tests__/routes/explorer-routes.test.tsx \
  __tests__/routes/detail-route-inputs.test.tsx \
  __tests__/routes/detail-navigation.test.tsx \
  __tests__/lib/tooling-config.test.ts
cd frontend && pnpm lint
cd frontend && pnpm type-check
cd frontend && pnpm build
cargo test -p ckbadger-api frontend_server_falls_back_to_index_html_for_spa_route -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/app frontend/src frontend/__tests__/routes frontend/__tests__/lib/tooling-config.test.ts crates/api/tests/frontend_server_spa.rs
git commit -m "refactor: remove remaining next runtime imports"
```

### Task 7: Final validation and docs sync

**Files:**

- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `docs/ARCHITECTURE_MAP.md`
- Modify: `docs/plans/2026-03-06-frontend-spa-migration.md`

**Step 1: Write the failing test**

No new automated test is required here. The validation step is a full command checklist plus doc consistency review.

**Step 2: Run validation checklist before doc edits**

Run:

```bash
rg -n "from 'next/|from \"next/" frontend -g '!frontend/__tests__/**'
```

Expected: either no matches or only intentionally retained non-runtime migration files with clear justification.

**Step 3: Write minimal implementation**

Update project docs so they no longer describe the frontend as depending on Next runtime behavior.

Sync the migration plan status note if this phase completes the runtime-removal objective.

**Step 4: Run final verification**

Run:

```bash
cd frontend && pnpm lint
cd frontend && pnpm type-check
cd frontend && pnpm build
cd frontend && pnpm exec vitest run \
  __tests__/routes/router-shell.test.tsx \
  __tests__/routes/detail-route-inputs.test.tsx \
  __tests__/routes/detail-navigation.test.tsx \
  __tests__/routes/explorer-routes.test.tsx \
  __tests__/components/identity-nft-item-detail.test.tsx \
  __tests__/pages/did-item-detail.test.tsx \
  __tests__/pages/dotbit-item-detail.test.tsx \
  __tests__/lib/tooling-config.test.ts
cargo test -p ckbadger-api frontend_server_falls_back_to_index_html_for_spa_route -- --nocapture
cargo test -p ckbadger test_resolve_frontend_dir -- --nocapture
```

Expected: PASS

**Step 5: Commit**

```bash
git add README.md CLAUDE.md docs/ARCHITECTURE_MAP.md docs/plans/2026-03-06-frontend-spa-migration.md
git commit -m "docs: finalize next runtime removal migration"
```
