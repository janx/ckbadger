# Homepage Latest Activities Locked Height Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Lock the homepage `Latest Activities` card height, render exactly 5 activity groups, and clip overflow without showing a scrollbar.

**Architecture:** Keep the existing transaction-grouped activity card and adjust only its local rendering constraints. Raise the group cap from 4 to 5, then add a fixed-height container with `overflow-hidden` so the card remains visually stable without introducing scroll behavior.

**Tech Stack:** React 19, TypeScript, Vitest, Testing Library, Tailwind CSS

---

### Task 1: Add failing component regression tests for the new card limit and clipping contract

**Files:**

- Modify: `frontend/__tests__/components/latest-activities.test.tsx`
- Reference: `frontend/components/latest-activities.tsx`

**Step 1: Write the failing test**

Add tests that assert:

- the card renders 5 grouped transactions when 5+ groups are available
- a 6th grouped transaction is not rendered
- the card content container applies `overflow-hidden`

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test latest-activities.test.tsx`

Expected: FAIL because the component currently renders only 4 groups and does not expose the locked-height clipping container.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test latest-activities.test.tsx`

Expected: FAIL with assertions tied to the 5-group limit and fixed-height container.

**Step 5: Commit**

Do not commit yet. The tests are intentionally failing.

### Task 2: Implement the fixed-height 5-group card behavior

**Files:**

- Modify: `frontend/components/latest-activities.tsx`
- Test: `frontend/__tests__/components/latest-activities.test.tsx`

**Step 1: Write the failing test**

Use the tests from Task 1.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test latest-activities.test.tsx`

Expected: FAIL before implementation.

**Step 3: Write minimal implementation**

- increase the rendered group cap from 4 to 5
- add a local constant for the group cap
- apply a fixed-height class to the `LatestActivities` panel
- add `overflow-hidden` to the content region so no scrollbar can appear
- keep the existing grouped transaction rendering unchanged otherwise

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm test latest-activities.test.tsx`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/components/latest-activities.tsx frontend/__tests__/components/latest-activities.test.tsx
git commit -m "fix: lock homepage latest activities height"
```

### Task 3: Run targeted verification

**Files:**

- No code changes required unless failures appear

**Step 1: Run the updated component tests**

Run: `cd frontend && pnpm test latest-activities.test.tsx`

Expected: PASS

**Step 2: Run adjacent regression tests**

Run: `cd frontend && pnpm test latest-activity-groups.test.ts`

Expected: PASS

**Step 3: Run type-check**

Run: `cd frontend && pnpm type-check`

Expected: PASS

**Step 4: Run lint**

Run: `cd frontend && pnpm lint`

Expected: PASS

**Step 5: Commit**

If everything passes and the user wants a final consolidation commit, create one with:

```bash
git add docs/plans/2026-03-12-homepage-latest-activities-locked-height-design.md docs/plans/2026-03-12-homepage-latest-activities-locked-height.md frontend/components/latest-activities.tsx frontend/__tests__/components/latest-activities.test.tsx
git commit -m "fix: lock homepage latest activities height"
```
