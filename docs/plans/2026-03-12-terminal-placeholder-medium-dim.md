# Terminal Placeholder Medium Dim Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Dim the compact terminal placeholder to a medium level without changing any other search-bar styling or behavior.

**Architecture:** Keep the change isolated to the compact empty-state overlay text in `SearchBar`. Use a single regression test to pin the new placeholder class, then adjust the implementation with the smallest possible diff.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library

---

### Task 1: Add the failing regression

**Files:**

- Modify: `frontend/__tests__/components/search-bar.test.tsx`
- Reference: `frontend/components/search-bar.tsx`

**Step 1: Write the failing test**

Change the compact empty-state placeholder assertion to the new medium-dim class target.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: FAIL because the current placeholder class is still at the previous opacity tier.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: FAIL with the compact placeholder class assertion.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the medium dimming

**Files:**

- Modify: `frontend/components/search-bar.tsx`

**Step 1: Re-run the failing test**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- lower the compact empty-state placeholder text class to the approved medium-dim value

**Step 3: Run the test to verify it passes**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Do not change prompt width, cursor styling, typed text styling, underline styling, dropdown styling, or search behavior.

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
git add docs/plans/2026-03-12-terminal-placeholder-medium-dim-design.md docs/plans/2026-03-12-terminal-placeholder-medium-dim.md frontend/components/search-bar.tsx frontend/__tests__/components/search-bar.test.tsx
git commit -m "style: dim terminal placeholder"
```
