# Frontend SPA Migration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current `Next.js static export` frontend shell with a `Vite + React SPA` served by the existing Rust `frontend` process, while preserving the current API contract and most page/business components.

**Architecture:** Keep the three-process runtime (`indexer`, `api`, `frontend`) and keep the existing React/TanStack Query component tree as the business layer. Replace Next-specific routing/build concerns with a Vite SPA shell plus a Rust static server that serves `frontend/dist/` and falls back to `index.html` for non-file paths.

**Tech Stack:** Vite, React 19, React Router, TanStack Query, Vitest, Axum, tower-http, rust-embed

**Status (2026-03-06):** SPA shell migration and Next runtime removal are complete. `frontend` now builds and runs without `next` or `eslint-config-next`; remaining follow-up is bundle/route-structure cleanup rather than shell migration.

---

### Task 1: Stand up the Vite SPA shell without changing page business logic

**Files:**

- Create: `frontend/index.html`
- Create: `frontend/vite.config.ts`
- Create: `frontend/src/main.tsx`
- Create: `frontend/src/app/root.tsx`
- Create: `frontend/src/app/providers.tsx`
- Create: `frontend/src/routes/router.tsx`
- Create: `frontend/__tests__/routes/router-shell.test.tsx`
- Modify: `frontend/package.json`
- Modify: `frontend/tsconfig.json`
- Modify: `frontend/vitest.config.mts`
- Modify: `frontend/__tests__/setup.ts`
- Modify: `frontend/app/providers.tsx`

**Step 1: Write the failing test**

Add a route-shell test that proves a React Router app can boot with the shared providers and render the home route.

```tsx
import { render, screen } from '@testing-library/react';
import { RouterProvider, createMemoryRouter } from 'react-router-dom';
import { createAppRouter } from '@/src/routes/router';

it('renders the SPA shell on the home route', async () => {
  const router = createMemoryRouter(createAppRouter(), {
    initialEntries: ['/'],
  });

  render(<RouterProvider router={router} />);

  expect(await screen.findByText(/ckbadger/i)).toBeInTheDocument();
});
```

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/router-shell.test.tsx`

Expected: FAIL because `createAppRouter`, `src/main.tsx`, and the Vite SPA shell do not exist yet.

**Step 3: Write minimal implementation**

- Add `frontend/index.html` and `frontend/vite.config.ts` for a standard Vite entry.
- Create `frontend/src/main.tsx` that mounts the app with React 19.
- Move provider composition into `frontend/src/app/providers.tsx`.
- Keep `frontend/app/providers.tsx` as a thin compatibility wrapper that re-exports the shared provider component so existing Next page code keeps working during migration.
- Create `frontend/src/app/root.tsx` and `frontend/src/routes/router.tsx` with a minimal `/` route that renders the existing home page entry.
- Update `frontend/package.json` scripts so `dev`/`build` point to Vite and keep `test`/`type-check` working.
- Update `frontend/tsconfig.json` and `frontend/vitest.config.mts` so `@/src/*` and existing `@/*` imports both resolve cleanly during the transition.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/router-shell.test.tsx`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/index.html frontend/vite.config.ts frontend/src/main.tsx frontend/src/app/root.tsx frontend/src/app/providers.tsx frontend/src/routes/router.tsx frontend/__tests__/routes/router-shell.test.tsx frontend/package.json frontend/tsconfig.json frontend/vitest.config.mts frontend/__tests__/setup.ts frontend/app/providers.tsx
git commit -m "feat: add vite spa shell"
```

### Task 2: Remove Next-only route-param ownership from detail pages

**Files:**

- Create: `frontend/__tests__/routes/detail-route-inputs.test.tsx`
- Modify: `frontend/app/script/[codeHash]/client-page.tsx`
- Modify: `frontend/app/scripts/[name]/client-page.tsx`
- Modify: `frontend/app/tokens/[typeHash]/client-page.tsx`
- Modify: `frontend/app/clusters/[clusterId]/client-page.tsx`
- Modify: `frontend/app/nfts/[sporeId]/client-page.tsx`
- Modify: `frontend/app/nfts/mnft/[nftId]/client-page.tsx`
- Modify: `frontend/app/nfts/dotbit/[nftId]/client-page.tsx`
- Modify: `frontend/app/nfts/did/[nftId]/client-page.tsx`
- Modify: `frontend/components/nft/identity-nft-item-detail.tsx`
- Modify: `frontend/app/script/[codeHash]/page.tsx`
- Modify: `frontend/app/scripts/[name]/page.tsx`
- Modify: `frontend/app/tokens/[typeHash]/page.tsx`
- Modify: `frontend/app/clusters/[clusterId]/page.tsx`
- Modify: `frontend/app/nfts/[sporeId]/page.tsx`
- Modify: `frontend/app/nfts/mnft/[nftId]/page.tsx`
- Modify: `frontend/app/nfts/dotbit/[nftId]/page.tsx`
- Modify: `frontend/app/nfts/did/[nftId]/page.tsx`

**Step 1: Write the failing test**

Add a route-input regression test that proves detail page components can render from explicit props instead of reading `next/navigation` directly.

```tsx
import { render, screen } from '@testing-library/react';
import { ScriptCodeHashPageClient } from '@/app/script/[codeHash]/client-page';

it('renders a script detail page from explicit route props', async () => {
  render(<ScriptCodeHashPageClient codeHash="0x1234" />);
  expect(await screen.findByText(/script/i)).toBeInTheDocument();
});
```

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/detail-route-inputs.test.tsx`

Expected: FAIL because the detail client pages still own route lookup through Next hooks instead of explicit props.

**Step 3: Write minimal implementation**

- Refactor each detail `client-page.tsx` to accept the required identifier as an explicit prop (`codeHash`, `name`, `typeHash`, `clusterId`, `sporeId`, `nftId`).
- Keep the Next `page.tsx` wrappers temporarily, but make them the only layer that reads Next route params and passes them into the shared page component.
- Apply the same pattern to NFT item detail components that currently derive identifiers internally.
- Do not add fallback chains; the prop becomes the single source of truth for route input.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/detail-route-inputs.test.tsx`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/__tests__/routes/detail-route-inputs.test.tsx frontend/app/script/[codeHash]/client-page.tsx frontend/app/scripts/[name]/client-page.tsx frontend/app/tokens/[typeHash]/client-page.tsx frontend/app/clusters/[clusterId]/client-page.tsx frontend/app/nfts/[sporeId]/client-page.tsx frontend/app/nfts/mnft/[nftId]/client-page.tsx frontend/app/nfts/dotbit/[nftId]/client-page.tsx frontend/app/nfts/did/[nftId]/client-page.tsx frontend/components/nft/identity-nft-item-detail.tsx frontend/app/script/[codeHash]/page.tsx frontend/app/scripts/[name]/page.tsx frontend/app/tokens/[typeHash]/page.tsx frontend/app/clusters/[clusterId]/page.tsx frontend/app/nfts/[sporeId]/page.tsx frontend/app/nfts/mnft/[nftId]/page.tsx frontend/app/nfts/dotbit/[nftId]/page.tsx frontend/app/nfts/did/[nftId]/page.tsx
git commit -m "refactor: pass explicit route params to detail pages"
```

### Task 3: Build the SPA detail routes and restore canonical detail navigation

**Files:**

- Create: `frontend/__tests__/routes/detail-navigation.test.tsx`
- Modify: `frontend/src/routes/router.tsx`
- Modify: `frontend/lib/detail-routes.ts`
- Modify: `frontend/lib/search-routing.ts`
- Modify: `frontend/components/search-bar.tsx`
- Modify: `frontend/app/assets/assets-page-client.tsx`
- Modify: `frontend/app/scripts/page.tsx`
- Modify: `frontend/app/tx/[hash]/client-page.tsx`
- Modify: `frontend/components/ui/script-view.tsx`
- Modify: `frontend/app/address/[addr]/client-page.tsx`
- Modify: `frontend/app/dao/page.tsx`
- Modify: `frontend/app/cell/[outpoint]/client-page.tsx`

**Step 1: Write the failing test**

Add a regression test that reproduces the original bug in the new router layer: clicking an asset or script link must land on the matching detail route instead of the home route.

```tsx
import userEvent from '@testing-library/user-event';
import { render, screen } from '@testing-library/react';
import { RouterProvider, createMemoryRouter } from 'react-router-dom';
import { createAppRouter } from '@/src/routes/router';

it('navigates to script and token detail routes from explorer links', async () => {
  const user = userEvent.setup();
  const router = createMemoryRouter(createAppRouter(), {
    initialEntries: ['/assets'],
  });

  render(<RouterProvider router={router} />);

  await user.click(await screen.findByRole('link', { name: /script/i }));
  expect(router.state.location.pathname).toMatch(/^\/script\//);
});
```

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/detail-navigation.test.tsx`

Expected: FAIL because the SPA router does not yet own the detail paths and some links still point at the old static-export workaround paths.

**Step 3: Write minimal implementation**

- Add detail routes to `frontend/src/routes/router.tsx` for:
  - `/script/:codeHash`
  - `/scripts/:name`
  - `/tokens/:typeHash`
  - `/clusters/:clusterId`
  - `/nfts/:sporeId`
  - `/nfts/mnft/:nftId`
  - `/nfts/dotbit/:nftId`
  - `/nfts/did/:nftId`
- Keep `frontend/lib/detail-routes.ts` as the single canonical path builder.
- Update search/navigation/link callers to use those canonical semantic paths, not static-export workaround URLs or query-string detours.
- Reuse the refactored detail page components from Task 2 as the route elements.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/detail-navigation.test.tsx __tests__/pages/assets.test.tsx __tests__/pages/scripts.test.tsx __tests__/components/search-bar.test.tsx __tests__/lib/search-routing.test.ts`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/__tests__/routes/detail-navigation.test.tsx frontend/src/routes/router.tsx frontend/lib/detail-routes.ts frontend/lib/search-routing.ts frontend/components/search-bar.tsx frontend/app/assets/assets-page-client.tsx frontend/app/scripts/page.tsx frontend/app/tx/[hash]/client-page.tsx frontend/components/ui/script-view.tsx frontend/app/address/[addr]/client-page.tsx frontend/app/dao/page.tsx frontend/app/cell/[outpoint]/client-page.tsx frontend/__tests__/pages/assets.test.tsx frontend/__tests__/pages/scripts.test.tsx frontend/__tests__/components/search-bar.test.tsx frontend/__tests__/lib/search-routing.test.ts
git commit -m "fix: route asset and script links through spa detail pages"
```

### Task 4: Migrate the remaining explorer routes into the SPA router

**Files:**

- Create: `frontend/__tests__/routes/explorer-routes.test.tsx`
- Modify: `frontend/src/routes/router.tsx`
- Modify: `frontend/app/page.tsx`
- Modify: `frontend/app/blocks/page.tsx`
- Modify: `frontend/app/blocks/[id]/client-page.tsx`
- Modify: `frontend/app/transactions/page.tsx`
- Modify: `frontend/app/tx/[hash]/client-page.tsx`
- Modify: `frontend/app/address/[addr]/client-page.tsx`
- Modify: `frontend/app/assets/page.tsx`
- Modify: `frontend/app/charts/page.tsx`
- Modify: `frontend/app/charts/address-cohort-retention/page.tsx`
- Modify: `frontend/app/charts/capacity-turnover-ratio/page.tsx`
- Modify: `frontend/app/charts/cell-age-vs-occupied-capacity/page.tsx`
- Modify: `frontend/app/charts/cell-size-distribution/page.tsx`
- Modify: `frontend/app/charts/common-knowledge-composition/page.tsx`
- Modify: `frontend/app/charts/knowledge-size/page.tsx`
- Modify: `frontend/app/charts/most-utilized-assets/page.tsx`
- Modify: `frontend/app/charts/most-utilized-scripts/page.tsx`
- Modify: `frontend/app/charts/secondary-issuance/page.tsx`
- Modify: `frontend/app/charts/total-supply/page.tsx`
- Modify: `frontend/app/charts/hodl-wave/page.tsx`
- Modify: `frontend/app/charts/epoch-time-length/page.tsx`
- Modify: `frontend/app/dao/page.tsx`
- Modify: `frontend/app/dao/charts/page.tsx`
- Modify: `frontend/app/forks/page.tsx`
- Modify: `frontend/app/forks/[id]/client-page.tsx`
- Modify: `frontend/app/hardforks/page.tsx`
- Modify: `frontend/app/not-found.tsx`
- Modify: `frontend/app/[...slug]/page.tsx`

**Step 1: Write the failing test**

Add a router coverage test for the main explorer paths and one deep-link refresh path.

```tsx
import { matchRoutes } from 'react-router-dom';
import { createAppRouter } from '@/src/routes/router';

it('matches the core explorer routes in the SPA router', () => {
  const routes = createAppRouter();

  expect(matchRoutes(routes, '/blocks')).toBeTruthy();
  expect(matchRoutes(routes, '/transactions')).toBeTruthy();
  expect(matchRoutes(routes, '/charts/most-utilized-scripts')).toBeTruthy();
  expect(matchRoutes(routes, '/tx/0x1234')).toBeTruthy();
});
```

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/explorer-routes.test.tsx`

Expected: FAIL because the SPA router does not yet cover the remaining explorer pages.

**Step 3: Write minimal implementation**

- Expand `frontend/src/routes/router.tsx` so all current explorer pages are reachable through React Router.
- Where a page currently mixes Next route lookup with business rendering, extract the route input and keep the page body unchanged.
- Keep `frontend/app/[...slug]/page.tsx` and other Next entry files only as temporary compatibility wrappers until the SPA fully owns all paths.
- Route not-found behavior through the shared `frontend/components/not-found-page.tsx` instead of relying on Next catch-all behavior.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run __tests__/routes/explorer-routes.test.tsx __tests__/pages/block-detail.test.tsx __tests__/pages/transactions-page.test.tsx __tests__/pages/blocks-page.test.tsx __tests__/pages/forks-page.test.tsx __tests__/pages/charts.test.tsx`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/__tests__/routes/explorer-routes.test.tsx frontend/src/routes/router.tsx frontend/app/page.tsx frontend/app/blocks/page.tsx frontend/app/blocks/[id]/client-page.tsx frontend/app/transactions/page.tsx frontend/app/tx/[hash]/client-page.tsx frontend/app/address/[addr]/client-page.tsx frontend/app/assets/page.tsx frontend/app/charts/page.tsx frontend/app/charts/address-cohort-retention/page.tsx frontend/app/charts/capacity-turnover-ratio/page.tsx frontend/app/charts/cell-age-vs-occupied-capacity/page.tsx frontend/app/charts/cell-size-distribution/page.tsx frontend/app/charts/common-knowledge-composition/page.tsx frontend/app/charts/knowledge-size/page.tsx frontend/app/charts/most-utilized-assets/page.tsx frontend/app/charts/most-utilized-scripts/page.tsx frontend/app/charts/secondary-issuance/page.tsx frontend/app/charts/total-supply/page.tsx frontend/app/charts/hodl-wave/page.tsx frontend/app/charts/epoch-time-length/page.tsx frontend/app/dao/page.tsx frontend/app/dao/charts/page.tsx frontend/app/forks/page.tsx frontend/app/forks/[id]/client-page.tsx frontend/app/hardforks/page.tsx frontend/app/not-found.tsx frontend/app/[...slug]/page.tsx
git commit -m "feat: migrate explorer routes into spa router"
```

### Task 5: Switch the Rust frontend server and CLI to `frontend/dist/` with SPA fallback

**Files:**

- Create: `crates/api/tests/frontend_server_spa.rs`
- Modify: `crates/api/src/entry.rs`
- Modify: `crates/api/src/embedded_frontend.rs`
- Modify: `crates/cli/src/main.rs`

**Step 1: Write the failing test**

Add a focused Rust integration test that proves the standalone frontend server serves files when present and falls back to `index.html` for deep links.

```rust
#[tokio::test]
async fn frontend_server_falls_back_to_index_html_for_spa_route() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html>spa</html>").unwrap();
    std::fs::create_dir_all(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets").join("app.js"), "console.log('ok');").unwrap();

    // start frontend server against dir.path()
    // GET /script/0x1234 -> assert 200 and body contains "spa"
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-api frontend_server_falls_back_to_index_html_for_spa_route -- --nocapture`

Expected: FAIL because the frontend server and embedded handler still assume the Next `out/` shape and CLI still resolves `frontend/` instead of `frontend/dist/`.

**Step 3: Write minimal implementation**

- Update `crates/api/src/entry.rs` so filesystem serving is explicitly designed for a Vite-style `dist/` directory with single-file fallback to `index.html`.
- Update `crates/api/src/embedded_frontend.rs` to embed from `frontend/dist/` and treat hashed `assets/` files as immutable cacheable assets.
- Update `crates/cli/src/main.rs` so `resolve_frontend_dir()` prefers `workdir/frontend/dist/`, then `workdir/frontend/`, then `share/frontend/dist/`, then `share/frontend/`.
- Keep the serving rule simple and fail-fast:
  - existing file path => serve file
  - non-file path => serve `index.html`
  - missing `index.html` => return startup error

**Step 4: Run test to verify it passes**

Run: `cargo test -p ckbadger-api frontend_server_falls_back_to_index_html_for_spa_route -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/api/tests/frontend_server_spa.rs crates/api/src/entry.rs crates/api/src/embedded_frontend.rs crates/cli/src/main.rs
git commit -m "feat: serve vite frontend dist with spa fallback"
```

### Task 6: Remove Next-specific build/dependency/docs assumptions

**Files:**

- Modify: `frontend/package.json`
- Modify: `frontend/next.config.ts`
- Modify: `frontend/app/layout.tsx`
- Modify: `frontend/app/globals.css`
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `docs/ARCHITECTURE_MAP.md`

**Step 1: Write the failing test**

Add a lightweight tooling regression test that proves the frontend build no longer emits Next-only output expectations.

```tsx
import pkg from '@/package.json';

it('uses vite rather than next for dev and build scripts', () => {
  expect(pkg.scripts.dev).toMatch(/^vite/);
  expect(pkg.scripts.build).toMatch(/^vite build/);
});
```

Place this in `frontend/__tests__/lib/tooling-config.test.ts`.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/lib/tooling-config.test.ts`

Expected: FAIL because package scripts and docs still describe Next static export.

**Step 3: Write minimal implementation**

- Remove `next`, `eslint-config-next`, and other no-longer-needed Next-specific runtime/build dependencies from `frontend/package.json`.
- Either delete `frontend/next.config.ts` or replace it with a migration note file if a clean removal must wait for one final follow-up commit.
- Make sure global styles remain rooted in the Vite app entry instead of the old Next layout entry.
- Update `README.md`, `CLAUDE.md`, and `docs/ARCHITECTURE_MAP.md` so they describe:
  - `Vite + React SPA`
  - `frontend/dist/`
  - the Rust `frontend` process as a SPA static server
- Update command snippets from “static export to out/” to the new Vite build behavior.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run __tests__/lib/tooling-config.test.ts`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/package.json frontend/next.config.ts frontend/app/layout.tsx frontend/app/globals.css frontend/__tests__/lib/tooling-config.test.ts README.md CLAUDE.md docs/ARCHITECTURE_MAP.md
git commit -m "chore: remove next static export assumptions"
```

### Task 7: Run the full migration verification and package checks

**Files:**

- Modify: `docs/plans/2026-03-06-frontend-spa-migration.md`
- Test: `frontend/__tests__/routes/router-shell.test.tsx`
- Test: `frontend/__tests__/routes/detail-route-inputs.test.tsx`
- Test: `frontend/__tests__/routes/detail-navigation.test.tsx`
- Test: `frontend/__tests__/routes/explorer-routes.test.tsx`
- Test: `frontend/__tests__/lib/tooling-config.test.ts`
- Test: `crates/api/tests/frontend_server_spa.rs`

**Step 1: Run targeted frontend tests**

Run:

```bash
cd frontend && pnpm exec vitest run __tests__/routes/router-shell.test.tsx __tests__/routes/detail-route-inputs.test.tsx __tests__/routes/detail-navigation.test.tsx __tests__/routes/explorer-routes.test.tsx __tests__/lib/tooling-config.test.ts
```

Expected: PASS

**Step 2: Run the broader frontend regression suite**

Run:

```bash
cd frontend && pnpm exec vitest run __tests__/pages/assets.test.tsx __tests__/pages/scripts.test.tsx __tests__/pages/script-detail.test.tsx __tests__/pages/script-code-hash.test.tsx __tests__/pages/token-detail.test.tsx __tests__/pages/cluster.test.tsx __tests__/pages/nft-detail.test.tsx __tests__/pages/mnft-item-detail.test.tsx __tests__/pages/dotbit-item-detail.test.tsx __tests__/pages/did-item-detail.test.tsx __tests__/pages/block-detail.test.tsx __tests__/pages/transactions-page.test.tsx __tests__/pages/blocks-page.test.tsx __tests__/pages/forks-page.test.tsx __tests__/pages/charts.test.tsx __tests__/components/search-bar.test.tsx __tests__/lib/search-routing.test.ts
```

Expected: PASS

**Step 3: Run frontend build and type-check**

Run:

```bash
cd frontend && pnpm type-check
cd frontend && pnpm build
```

Expected:

- TypeScript passes with no Next-only module errors.
- Vite emits `frontend/dist/`.

**Step 4: Run Rust verification**

Run:

```bash
cargo test -p ckbadger-api frontend_server_falls_back_to_index_html_for_spa_route -- --nocapture
cargo test -p ckbadger test_resolve_frontend_dir -- --nocapture
```

Expected: PASS

**Step 5: Smoke-check the packaged runtime shape**

Run:

```bash
rg -n "frontend/dist|Vite|React Router|SPA fallback" README.md CLAUDE.md docs/ARCHITECTURE_MAP.md crates/api/src/entry.rs crates/api/src/embedded_frontend.rs crates/cli/src/main.rs
```

Expected:

- Docs and runtime code agree on `frontend/dist/`.
- No remaining documentation claims that production uses Next static export.

**Step 6: Commit**

```bash
git add docs/plans/2026-03-06-frontend-spa-migration.md
git commit -m "test: verify frontend spa migration"
```

Plan complete and saved to `docs/plans/2026-03-06-frontend-spa-migration.md`.

Two execution options:

1. Subagent-Driven (this session) - I dispatch fresh subagent per task, review between tasks, fast iteration
2. Parallel Session (separate) - Open new session with executing-plans, batch execution with checkpoints

If no preference is given, use option 1 in this session.
