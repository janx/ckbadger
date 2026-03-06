# Frontend Bundle Optimization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reduce the frontend main entry bundle by lazily loading heavy visualization code and other clearly non-essential-at-startup UI.

**Architecture:** Start with explicit lazy boundaries around the heaviest visualization components so the bundle change is visible in app code and testable. Measure after each step rather than hiding the split behind bundler-only chunk heuristics. If component-level splits are insufficient, leave room for a later route-level phase.

**Tech Stack:** Vite 5, React 19, React Router, Suspense/lazy, Vitest

---

### Task 1: Capture the current bundle baseline and add a regression check

**Files:**

- Modify: `frontend/__tests__/lib/tooling-config.test.ts`
- Create: `frontend/docs/bundle-baseline.md` (optional, only if a local artifact note is useful)

**Step 1: Write the failing test**

Add a tooling-level regression test that confirms the project still uses the local dynamic client helper for heavy graph loading paths rather than direct eager imports.

Do not assert specific byte sizes in Vitest. Instead, assert the structural condition that enables splitting.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/lib/tooling-config.test.ts`

Expected: FAIL if the heavy graph entry points still do not import through the local lazy boundary you intend to enforce.

**Step 3: Write minimal implementation**

Update the regression test to inspect the graph wrapper modules and confirm they are wired through the intended lazy-loading boundary.

Record the current `pnpm build` output size in your notes before changing behavior.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run __tests__/lib/tooling-config.test.ts`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/__tests__/lib/tooling-config.test.ts
git commit -m "test: add bundle split regression coverage"
```

### Task 2: Lazy-load the heaviest graph dependency behind explicit wrappers

**Files:**

- Modify: `frontend/components/cell-graph.tsx`
- Modify: `frontend/components/proposal-graph.tsx`
- Modify: `frontend/lib/dynamic-client.tsx`
- Test: `frontend/__tests__/lib/tooling-config.test.ts`
- Test: any existing graph-adjacent page tests indirectly covering these components

**Step 1: Write the failing test**

Extend the tooling regression test so it proves the graph components import `react-force-graph-2d` only through the local dynamic/lazy helper path and not through any eager top-level path.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/lib/tooling-config.test.ts`

Expected: FAIL until the graph wrapper modules are structured the new way.

**Step 3: Write minimal implementation**

Make the graph entry points explicitly lazy-loaded through the local helper with stable loading placeholders.

Keep the heavy dependency out of the main render path. The loading fallback must preserve basic layout and not collapse the surrounding UI.

**Step 4: Run test to verify it passes**

Run:

```bash
cd frontend && pnpm exec vitest run __tests__/lib/tooling-config.test.ts
cd frontend && pnpm build
```

Expected: PASS, and the build output should show a reduced main entry chunk relative to baseline.

**Step 5: Commit**

```bash
git add frontend/components/cell-graph.tsx frontend/components/proposal-graph.tsx frontend/lib/dynamic-client.tsx frontend/__tests__/lib/tooling-config.test.ts
git commit -m "perf: lazy load graph components"
```

### Task 3: Lazy-load heavy graph sections at the route boundary where appropriate

**Files:**

- Modify: `frontend/app/tx/[hash]/client-page.tsx`
- Modify: `frontend/app/cell/[outpoint]/client-page.tsx`
- Modify: any other route that mounts graph components immediately
- Test: `frontend/__tests__/pages/tx-detail.test.tsx`
- Test: `frontend/__tests__/pages/cell.test.tsx`

**Step 1: Write the failing test**

Add or update a page test that proves the route renders a loading placeholder first when the graph section is deferred, while preserving the rest of the detail page.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm exec vitest run __tests__/pages/tx-detail.test.tsx __tests__/pages/cell.test.tsx`

Expected: FAIL until the route-level lazy section exists.

**Step 3: Write minimal implementation**

Wrap the graph sections in route-local lazy boundaries if they are still mounted eagerly at initial route render.

Do not delay the whole page. Only defer the heavy graph block.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm exec vitest run __tests__/pages/tx-detail.test.tsx __tests__/pages/cell.test.tsx`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/app/tx/[hash]/client-page.tsx frontend/app/cell/[outpoint]/client-page.tsx frontend/__tests__/pages/tx-detail.test.tsx frontend/__tests__/pages/cell.test.tsx
git commit -m "perf: defer graph sections on detail pages"
```

### Task 4: Evaluate additional heavy visualization targets

**Files:**

- Modify: any route-local heavy visualization component discovered during measurement
- Test: matching page/component tests

**Step 1: Identify the next-heaviest candidate**

Use the build output and module inspection to choose one additional heavy candidate only if the main entry chunk is still too large after Task 2 and Task 3.

**Step 2: Write the failing test**

Add or update a focused regression test proving the chosen component/section can render behind a fallback without breaking the page.

**Step 3: Run test to verify it fails**

Run the smallest relevant Vitest command for that target.

Expected: FAIL

**Step 4: Write minimal implementation**

Add a local lazy boundary only for that target.

Do not broaden scope without evidence from the build output.

**Step 5: Run test to verify it passes**

Run the focused test plus `cd frontend && pnpm build`.

Expected: PASS

**Step 6: Commit**

```bash
git add <exact files touched>
git commit -m "perf: lazy load additional heavy visualization"
```

### Task 5: Final measurement and validation

**Files:**

- Modify: `docs/plans/2026-03-06-frontend-bundle-optimization-design.md` (optional status note)
- Modify: `docs/plans/2026-03-06-frontend-bundle-optimization.md` (optional status note)

**Step 1: Run final verification**

Run:

```bash
cd frontend && pnpm lint
cd frontend && pnpm type-check
cd frontend && pnpm build
cd frontend && pnpm exec vitest run \
  __tests__/lib/tooling-config.test.ts \
  __tests__/pages/tx-detail.test.tsx \
  __tests__/pages/cell.test.tsx \
  __tests__/pages/assets.test.tsx \
  __tests__/pages/nft-detail.test.tsx
```

Expected: PASS

**Step 2: Compare bundle output**

Compare the final main entry chunk against the baseline captured before Task 2.

Document whether:

- the main entry chunk is materially smaller
- the Vite warning remains or disappears
- another round of route-level splitting is warranted

**Step 3: Commit**

```bash
git add docs/plans/2026-03-06-frontend-bundle-optimization-design.md docs/plans/2026-03-06-frontend-bundle-optimization.md
git commit -m "docs: record bundle optimization results"
```
