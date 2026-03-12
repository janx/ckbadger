# Terminal Search Polish Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Polish the compact terminal search styling and right-align the navbar stats block without changing search or stats behavior.

**Architecture:** Keep the behavioral logic unchanged in `SearchBar` and `GlobalStatsBar`. Restrict the change to presentational classes in `SearchBar` and layout classes in `Header`, with regression coverage pinning the new prompt spacing, underline styling, dropdown panel styling, and stats alignment.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library, TanStack Query

---

### Task 1: Write failing regressions

**Files:**

- Modify: `frontend/__tests__/components/header.test.tsx`
- Modify: `frontend/__tests__/components/search-bar.test.tsx`
- Reference: `frontend/components/layout/header.tsx`
- Reference: `frontend/components/search-bar.tsx`

**Step 1: Write the failing test**

Add assertions that:

- the stats row container is right-aligned
- the compact prompt width is tighter
- the compact command line starts closer to the prompt
- the compact input keeps underline-only styling with the polished class contract
- the compact dropdown uses the polished opaque panel styling

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test header.test.tsx search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL because the current header still uses the old stats-row container contract and the compact search still uses the pre-polish classes.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test header.test.tsx search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL with header alignment and compact search class assertion output.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the polish

**Files:**

- Modify: `frontend/components/layout/header.tsx`
- Modify: `frontend/components/search-bar.tsx`

**Step 1: Re-run the failing tests**

Run: `cd frontend && pnpm test header.test.tsx search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- right-align the stats row container
- tighten the compact prompt spacing
- refine the compact underline classes
- polish the compact dropdown panel classes

**Step 3: Run the tests to verify they pass**

Run: `cd frontend && pnpm test header.test.tsx search-bar.test.tsx stats-bar.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Do not alter search query behavior, result ordering, stats data rendering, or mobile-menu structure.

**Step 5: Commit**

Do not commit yet. Full verification still pending.

### Task 3: Verify the refinement

**Files:**

- No code changes required unless failures appear

**Step 1: Run targeted regressions**

Run: `cd frontend && pnpm test search-bar.test.tsx header.test.tsx stats-bar.test.tsx`

Expected: PASS

**Step 2: Run frontend type-check**

Run: `cd frontend && pnpm type-check`

Expected: PASS

**Step 3: Fix any failure and re-run**

Keep any fixes scoped to terminal search presentation and header stats alignment.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-terminal-search-polish-design.md docs/plans/2026-03-12-terminal-search-polish.md frontend/components/layout/header.tsx frontend/components/search-bar.tsx frontend/__tests__/components/header.test.tsx frontend/__tests__/components/search-bar.test.tsx
git commit -m "style: polish terminal navbar search"
```
