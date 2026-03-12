# Terminal Placeholder Soften Slightly Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Soften the compact terminal placeholder slightly from its current strength while leaving all other styling and behavior untouched.

**Architecture:** Keep the change isolated to one compact empty-state text class inside `SearchBar`. Use a single regression assertion to pin the new `/44` target, then verify it alongside the existing header/logo/stats contracts.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library

---

### Task 1: Add the failing regression

**Files:**

- Modify: `frontend/__tests__/components/search-bar.test.tsx`
- Reference: `frontend/components/search-bar.tsx`

**Step 1: Write the failing test**

Change the compact empty-state placeholder assertion from `text-text-dim/48` to `text-text-dim/44`.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: FAIL because the current placeholder class is still `text-text-dim/48`.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: FAIL with the compact placeholder class assertion.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the slight softening

**Files:**

- Modify: `frontend/components/search-bar.tsx`

**Step 1: Re-run the failing test**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- lower the compact empty-state placeholder text class from `text-text-dim/48` to `text-text-dim/44`

**Step 3: Run the test to verify it passes**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Do not change placeholder copy, keycaps, prompt, cursor, typed text, underline, dropdown, header alignment, or logo.

**Step 5: Commit**

Do not commit yet. Full verification still pending.

### Task 3: Verify the refinement

**Files:**

- No code changes required unless failures appear

**Step 1: Run focused regressions**

Run: `cd frontend && pnpm test search-bar.test.tsx header.test.tsx logo.test.tsx stats-bar.test.tsx`

Expected: PASS

**Step 2: Run frontend type-check**

Run: `cd frontend && pnpm type-check`

Expected: PASS

**Step 3: Fix any failure and re-run**

Keep any fixes scoped to the placeholder opacity change.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-terminal-placeholder-soften-slightly-design.md docs/plans/2026-03-12-terminal-placeholder-soften-slightly.md frontend/components/search-bar.tsx frontend/__tests__/components/search-bar.test.tsx
git commit -m "style: soften terminal placeholder slightly"
```
