# Header Start Column and Logo Position Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give desktop search and stats a shared structural start column and enlarge/reposition the desktop logo slightly.

**Architecture:** Keep the logo visually absolute, but introduce a shared desktop spacer column in `Header` so search and stats align from the same layout start. Keep the change presentation-only by updating `Header` and `Logo`, with regression tests pinning the shared spacer contract and responsive logo classes.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library, Zustand

---

### Task 1: Add failing regressions

**Files:**

- Modify: `frontend/__tests__/components/header.test.tsx`
- Create: `frontend/__tests__/components/logo.test.tsx`
- Reference: `frontend/components/layout/header.tsx`
- Reference: `frontend/components/layout/logo.tsx`

**Step 1: Write the failing test**

Add assertions that:

- desktop search no longer relies on `md:pl-[96px]`
- header search and stats each render the same shared desktop start-column spacer
- logo link uses the new desktop position classes
- logo image uses the larger desktop width classes

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test header.test.tsx logo.test.tsx`

Expected: FAIL because the old padding offset is still present and the logo still uses the previous size/position classes.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test header.test.tsx logo.test.tsx`

Expected: FAIL with header spacer and logo class assertions.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement shared start column and logo adjustment

**Files:**

- Modify: `frontend/components/layout/header.tsx`
- Modify: `frontend/components/layout/logo.tsx`

**Step 1: Re-run the failing tests**

Run: `cd frontend && pnpm test header.test.tsx logo.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- replace the old desktop padding offset with a shared desktop start-column spacer
- update the desktop logo size and offsets

**Step 3: Run the tests to verify they pass**

Run: `cd frontend && pnpm test header.test.tsx logo.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Do not change search logic, stats content, nav labels, or mobile-menu behavior.

**Step 5: Commit**

Do not commit yet. Full verification still pending.

### Task 3: Verify the refinement

**Files:**

- No code changes required unless failures appear

**Step 1: Run targeted regressions**

Run: `cd frontend && pnpm test header.test.tsx logo.test.tsx search-bar.test.tsx stats-bar.test.tsx`

Expected: PASS

**Step 2: Run frontend type-check**

Run: `cd frontend && pnpm type-check`

Expected: PASS

**Step 3: Fix any failure and re-run**

Keep any fixes scoped to header layout and logo presentation.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-header-start-column-logo-design.md docs/plans/2026-03-12-header-start-column-logo.md frontend/components/layout/header.tsx frontend/components/layout/logo.tsx frontend/__tests__/components/header.test.tsx frontend/__tests__/components/logo.test.tsx
git commit -m "style: align header start column and logo"
```
