# Terminal Search Bar Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the compact navbar search bar tighter and more terminal-like while preserving placeholder copy and all search behavior.

**Architecture:** Keep the change local to `SearchBar`. Update only compact-variant markup and styling, then verify with focused `SearchBar` tests so no routing or dropdown behavior regresses.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library

---

### Task 1: Add failing compact terminal-style regression tests

**Files:**

- Modify: `frontend/__tests__/components/search-bar.test.tsx`
- Reference: `frontend/components/search-bar.tsx`

**Step 1: Write the failing test**

Add a test that asserts the compact variant:

- renders a visible terminal prompt element
- uses a tighter height
- uses harder edge treatment and terminal-style border/shadow classes

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: FAIL because the current compact variant has no prompt element and still uses the older rounded-panel styling.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: FAIL with missing prompt and styling assertions.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the compact terminal styling

**Files:**

- Modify: `frontend/components/search-bar.tsx`
- Reference: `frontend/components/layout/header.tsx`

**Step 1: Re-run the failing test**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- add compact-only prompt markup
- reduce compact height and padding
- tighten the compact border, radius, and shadow treatment
- lightly align the dropdown container with the same terminal visual language

**Step 3: Run the test to verify it passes**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Do not alter routing logic, query logic, dropdown behavior, or placeholder copy.

**Step 5: Commit**

Do not commit yet. Full verification still pending.

### Task 3: Run verification

**Files:**

- No code changes required unless failures appear

**Step 1: Run targeted tests**

Run: `cd frontend && pnpm test search-bar.test.tsx header.test.tsx`

Expected: PASS

**Step 2: Run frontend type-check**

Run: `cd frontend && pnpm type-check`

Expected: PASS

**Step 3: Fix any failure and re-run**

Keep fixes local to the compact terminal search styling.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-terminal-search-bar-design.md docs/plans/2026-03-12-terminal-search-bar.md frontend/components/search-bar.tsx frontend/__tests__/components/search-bar.test.tsx
git commit -m "style: tighten terminal search bar"
```
