# Terminal Search Underline Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove the stats-line prompt and restyle the compact navbar search into a single underlined terminal command line without a vertical separator after `>`.

**Architecture:** Keep search behavior in `SearchBar` and stats rendering in `GlobalStatsBar`. Preserve the compact overlay model for visible command text, but swap the prompt separator and box styling for a unified underline-only shell.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library, TanStack Query

---

### Task 1: Write failing UI regressions

**Files:**

- Modify: `frontend/__tests__/components/search-bar.test.tsx`
- Modify: `frontend/__tests__/components/stats-bar.test.tsx`
- Reference: `frontend/components/search-bar.tsx`
- Reference: `frontend/components/stats-bar.tsx`

**Step 1: Write the failing test**

Add assertions that:

- compact prompt no longer carries a separator border
- compact input uses underline-only styling instead of a boxed border
- global stats bar no longer renders a visible leading prompt

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL because the current prompt still uses a separator and the stats bar still renders `>`.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL with prompt-border, underline-style, and stats-prompt assertions.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the underline-only terminal search

**Files:**

- Modify: `frontend/components/search-bar.tsx`
- Modify: `frontend/components/stats-bar.tsx`

**Step 1: Re-run the failing tests**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- remove the compact prompt separator
- restyle compact search to keep only a shared underline
- remove the visible global stats prompt

**Step 3: Run the tests to verify they pass**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Do not alter search routing, query timing, result ordering, or header structure beyond the approved UI changes.

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

Keep any fixes scoped to compact search styling and stats-line prompt removal.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-terminal-search-underline-design.md docs/plans/2026-03-12-terminal-search-underline.md frontend/components/search-bar.tsx frontend/components/stats-bar.tsx frontend/__tests__/components/search-bar.test.tsx frontend/__tests__/components/stats-bar.test.tsx
git commit -m "style: refine terminal search underline"
```
