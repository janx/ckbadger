# Command Line Search Cursor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Render compact navbar search as a true command line with a text-end blinking cursor, and remove the trailing cursor from the stats line.

**Architecture:** Keep `SearchBar` responsible for compact visual rendering while preserving the native input for semantics and behavior. Add a small dedicated `GlobalStatsBar` test so cursor removal is verified independently from header layout tests.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library, TanStack Query

---

### Task 1: Add failing regression tests

**Files:**

- Modify: `frontend/__tests__/components/search-bar.test.tsx`
- Create: `frontend/__tests__/components/stats-bar.test.tsx`
- Reference: `frontend/components/search-bar.tsx`
- Reference: `frontend/components/stats-bar.tsx`

**Step 1: Write the failing test**

Add tests that assert:

- compact search renders a command text layer with a blinking cursor
- typing into compact search replaces the placeholder command text with the query and keeps the cursor present
- `GlobalStatsBar` no longer renders the trailing blinking cursor

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL because compact search still relies on native input text rendering and stats line still renders the cursor.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL with missing overlay/cursor assertions and the existing stats-line cursor.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement command-line compact search and stats cursor removal

**Files:**

- Modify: `frontend/components/search-bar.tsx`
- Modify: `frontend/components/stats-bar.tsx`

**Step 1: Re-run the failing tests**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- add compact command-text overlay markup
- render either the placeholder or live query in that overlay
- render the blinking cursor inside the overlay so it always follows the visible text
- make the compact input text/caret visually transparent while keeping it interactive
- remove the trailing blinking cursor from `GlobalStatsBar`

**Step 3: Run the tests to verify they pass**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Do not change query logic, navigation behavior, dropdown result logic, or stats data content.

**Step 5: Commit**

Do not commit yet. Full verification still pending.

### Task 3: Run verification

**Files:**

- No code changes required unless failures appear

**Step 1: Run targeted tests**

Run: `cd frontend && pnpm test search-bar.test.tsx header.test.tsx stats-bar.test.tsx`

Expected: PASS

**Step 2: Run frontend type-check**

Run: `cd frontend && pnpm type-check`

Expected: PASS

**Step 3: Fix any failure and re-run**

Keep fixes local to compact search rendering and stats-bar cursor removal.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-command-line-search-cursor-design.md docs/plans/2026-03-12-command-line-search-cursor.md frontend/components/search-bar.tsx frontend/components/stats-bar.tsx frontend/__tests__/components/search-bar.test.tsx frontend/__tests__/components/stats-bar.test.tsx
git commit -m "style: make navbar search a command line"
```
