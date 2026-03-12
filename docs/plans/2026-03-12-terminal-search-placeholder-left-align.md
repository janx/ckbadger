# Terminal Search Placeholder Left-Align Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Dim the compact terminal placeholder slightly and restore the navbar stats row to the shared left alignment under the search prompt.

**Architecture:** Keep the change limited to presentational classes in `SearchBar` and layout classes in `Header`. Preserve all search behavior, stats rendering, and the existing compact terminal structure.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library, TanStack Query

---

### Task 1: Add failing regressions

**Files:**

- Modify: `frontend/__tests__/components/search-bar.test.tsx`
- Modify: `frontend/__tests__/components/header.test.tsx`
- Reference: `frontend/components/search-bar.tsx`
- Reference: `frontend/components/layout/header.tsx`

**Step 1: Write the failing test**

Add assertions that:

- the compact empty-state placeholder uses a dimmer class
- the stats row container uses the shared desktop left offset
- the stats row container no longer uses `justify-end`

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test header.test.tsx search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL because the placeholder is still at the previous opacity and the header still right-aligns the stats row.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test header.test.tsx search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL with placeholder-class and header-alignment assertions.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the minimal refinement

**Files:**

- Modify: `frontend/components/search-bar.tsx`
- Modify: `frontend/components/layout/header.tsx`

**Step 1: Re-run the failing tests**

Run: `cd frontend && pnpm test header.test.tsx search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- dim the compact placeholder text class one step further
- restore the stats row container to the shared desktop left offset

**Step 3: Run the tests to verify they pass**

Run: `cd frontend && pnpm test header.test.tsx search-bar.test.tsx stats-bar.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Do not change prompt width, underline styling, dropdown styling, search routing, or stats content.

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

Keep any fixes scoped to placeholder opacity and stats-row alignment.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-terminal-search-placeholder-left-align-design.md docs/plans/2026-03-12-terminal-search-placeholder-left-align.md frontend/components/layout/header.tsx frontend/components/search-bar.tsx frontend/__tests__/components/header.test.tsx frontend/__tests__/components/search-bar.test.tsx
git commit -m "style: dim terminal placeholder and realign stats"
```
