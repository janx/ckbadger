# Homepage Chart Navigation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add homepage pie-chart click navigation with separate destinations for slices, legends, and section chrome.

**Architecture:** Keep the homepage card as the owner of routing decisions. Add item-aware click hooks to the shared pie section/chart components, then wire `Activity Types` and `Script Usage` with their respective chart-page and detail-page destinations.

**Tech Stack:** React 19, React Router, Vitest, Testing Library

---

### Task 1: Add failing navigation regression tests

**Files:**

- Modify: `frontend/__tests__/components/home-layer2.test.tsx`
- Reference: `frontend/components/activity-card.tsx`

**Step 1: Write the failing test**

Add tests that assert:

- clicking an `Activity Types` slice routes to `/charts/activity-type-breakdown`
- clicking the `Activity Types` section chrome routes to `/charts/activity-type-breakdown`
- clicking a `Script Usage` slice routes to the script detail page
- clicking a `Script Usage` legend item routes to the script detail page
- clicking `Script Usage` section chrome routes to `/charts/most-utilized-scripts`

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test home-layer2.test.tsx`

Expected: FAIL because the current pie sections do not navigate on click.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test home-layer2.test.tsx`

Expected: FAIL with missing click navigation assertions.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement item-aware pie navigation

**Files:**

- Modify: `frontend/components/activity-card.tsx`
- Modify: `frontend/components/ui/pie-chart.tsx`
- Modify: `frontend/lib/api.ts` if exported types need adjustment
- Reference: `frontend/lib/detail-routes.ts`

**Step 1: Write the failing test**

Use the tests from Task 1.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test home-layer2.test.tsx`

Expected: FAIL before implementation.

**Step 3: Write minimal implementation**

- Add optional slice click callbacks to `PieChart`
- Add optional section click and item click behavior to `PieSection`
- Build script pie entries with raw script metadata
- Resolve script detail links with `getScriptDetailHref()`
- Stop event propagation on slice and legend item clicks so section routing does not fire

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm test home-layer2.test.tsx`

Expected: PASS

**Step 5: Commit**

Do not commit yet. Full verification still pending.

### Task 3: Run targeted and broader verification

**Files:**

- No code changes required unless failures appear

**Step 1: Run targeted frontend tests**

Run: `cd frontend && pnpm test home-layer2.test.tsx`

Expected: PASS

**Step 2: Run adjacent regression tests**

Run: `cd frontend && pnpm test activity-trend.test.tsx`

Expected: PASS

**Step 3: Run lint or type-check if touched types or event props require it**

Run: `cd frontend && pnpm type-check`

Expected: PASS

**Step 4: Fix any failure, then re-run**

Keep changes minimal and local to the new navigation behavior.

**Step 5: Commit**

If everything passes and the user wants a commit, create one with:

```bash
git add docs/plans/2026-03-12-homepage-chart-navigation-design.md docs/plans/2026-03-12-homepage-chart-navigation.md frontend/components/activity-card.tsx frontend/components/ui/pie-chart.tsx frontend/__tests__/components/home-layer2.test.tsx
git commit -m "feat: add homepage chart navigation"
```
