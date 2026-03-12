# DAO Overview Desktop Baseline Alignment Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Align the homepage DAO overview card with the left-column chart and stats baselines on desktop only.

**Architecture:** Keep the homepage grid unchanged and fix alignment inside `DaoOverview`. Match the top chart rhythm to the left column and make the lower stats grid consume the remaining desktop height so both horizontal baselines line up without affecting mobile layout.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library

---

### Task 1: Add the failing desktop layout regression test

**Files:**

- Modify: `frontend/__tests__/components/dao-overview.test.tsx`
- Reference: `frontend/components/dao-overview.tsx`

**Step 1: Write the failing test**

Add a test that asserts the DAO card renders desktop-only layout classes for:

- the top chart wrapper using `lg:h-14`
- the stats grid using `lg:flex-1`
- each stat cell centering its contents vertically on desktop

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test dao-overview.test.tsx`

Expected: FAIL because the current DAO overview does not expose those desktop alignment classes.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test dao-overview.test.tsx`

Expected: FAIL with missing desktop alignment class assertions.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the desktop-only layout fix

**Files:**

- Modify: `frontend/components/dao-overview.tsx`
- Reference: `frontend/components/home-layer2.tsx`
- Reference: `frontend/components/activity-card.tsx`

**Step 1: Write the failing test**

Use the regression test from Task 1.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test dao-overview.test.tsx`

Expected: FAIL before implementation.

**Step 3: Write minimal implementation**

- keep mobile classes unchanged
- change the DAO chart wrapper to `lg:h-14`
- make the stats grid fill remaining desktop height
- center stat content within each cell on desktop

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm test dao-overview.test.tsx`

Expected: PASS

**Step 5: Commit**

Do not commit yet. Full verification still pending.

### Task 3: Verify targeted behavior

**Files:**

- No code changes required unless failures appear

**Step 1: Run the targeted test**

Run: `cd frontend && pnpm test dao-overview.test.tsx`

Expected: PASS

**Step 2: Run adjacent homepage component tests**

Run: `cd frontend && pnpm test home-layer2.test.tsx`

Expected: PASS

**Step 3: Fix any failure, then re-run**

Keep fixes scoped to the DAO overview alignment change.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-dao-overview-desktop-baseline-alignment-design.md docs/plans/2026-03-12-dao-overview-desktop-baseline-alignment.md frontend/components/dao-overview.tsx frontend/__tests__/components/dao-overview.test.tsx
git commit -m "fix: align homepage dao overview baselines"
```
