# Terminal Search Refinement Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refine the compact navbar search into a borderless terminal command line with cursor positioning that depends on empty vs typed state, while making the dropdown opaque and aligning the stats prompt with the search prompt.

**Architecture:** Keep all behavior local to `SearchBar` and `GlobalStatsBar`. Preserve the native input for semantics and interaction, but render compact visible text through a command-line overlay whose child order changes between empty and typed states.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library, TanStack Query

---

### Task 1: Add failing regression tests

**Files:**

- Modify: `frontend/__tests__/components/search-bar.test.tsx`
- Modify: `frontend/__tests__/components/stats-bar.test.tsx`
- Reference: `frontend/components/search-bar.tsx`
- Reference: `frontend/components/stats-bar.tsx`

**Step 1: Write the failing test**

Add tests that assert:

- compact command line renders cursor before placeholder when empty
- compact command line renders cursor after query when typed
- compact input uses transparent border styling
- compact dropdown container uses an opaque background class
- global stats prompt uses fixed-width prompt alignment and no trailing cursor

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL because cursor order and prompt-alignment contracts do not exist yet and the dropdown remains translucent.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL with cursor-order, dropdown, and prompt-class assertion output.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the refined terminal command line

**Files:**

- Modify: `frontend/components/search-bar.tsx`
- Modify: `frontend/components/stats-bar.tsx`

**Step 1: Re-run the failing tests**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- render compact command-line children in state-dependent order
- remove visible compact border via transparent border styling
- make compact dropdown background fully opaque
- give the stats prompt the same fixed prompt width as the search prompt

**Step 3: Run the tests to verify they pass**

Run: `cd frontend && pnpm test search-bar.test.tsx stats-bar.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Do not change query logic, routing logic, dropdown data flow, or header layout structure.

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

Keep fixes local to terminal search rendering and stats prompt alignment.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-terminal-search-refinement-design.md docs/plans/2026-03-12-terminal-search-refinement.md frontend/components/search-bar.tsx frontend/components/stats-bar.tsx frontend/__tests__/components/search-bar.test.tsx frontend/__tests__/components/stats-bar.test.tsx
git commit -m "style: refine terminal navbar search"
```
