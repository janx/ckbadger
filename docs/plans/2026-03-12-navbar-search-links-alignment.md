# Navbar Search And Stats Alignment Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reposition the navbar search bar and links while aligning the stats line to the new search baseline on desktop and right-aligning mobile menu links.

**Architecture:** Keep `Header` as the single layout owner. Update only the flex structure and spacing classes in `frontend/components/layout/header.tsx`, leaving `SearchBar`, `Logo`, and `GlobalStatsBar` behavior intact.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library

---

### Task 1: Add failing header layout regression tests

**Files:**

- Modify: `frontend/__tests__/components/header.test.tsx`
- Reference: `frontend/components/layout/header.tsx`

**Step 1: Write the failing test**

Add assertions that cover:

- desktop search is rendered in the left layout group beside the logo
- desktop nav is right-aligned and no longer uses the old left padding offset
- stats row container uses the same left padding value as the desktop search baseline
- mobile expanded menu right-aligns nav links

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test header.test.tsx`

Expected: FAIL because the current header layout still uses left-padded desktop nav and left-aligned mobile links.

**Step 3: Write minimal implementation**

No production changes in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test header.test.tsx`

Expected: FAIL with assertions tied to the old layout structure.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the new header alignment

**Files:**

- Modify: `frontend/components/layout/header.tsx`
- Reference: `frontend/components/search-bar.tsx`
- Reference: `frontend/components/stats-bar.tsx`

**Step 1: Re-run the failing test**

Run: `cd frontend && pnpm test header.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- Reorder the desktop flex layout to place the search bar in the left group.
- Right-align desktop nav with `ml-auto`.
- Replace the stats row padding with the same left offset used by the desktop search baseline.
- Right-align the mobile menu links without changing their order.

**Step 3: Run test to verify it passes**

Run: `cd frontend && pnpm test header.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Avoid new abstractions unless the spacing would otherwise be duplicated unclearly.

**Step 5: Commit**

Do not commit yet. Full verification still pending.

### Task 3: Run verification

**Files:**

- No code changes required unless failures appear

**Step 1: Run targeted tests**

Run: `cd frontend && pnpm test header.test.tsx`

Expected: PASS

**Step 2: Run adjacent search regression tests**

Run: `cd frontend && pnpm test search-bar.test.tsx`

Expected: PASS

**Step 3: Run frontend type-check if layout edits affect component typing**

Run: `cd frontend && pnpm type-check`

Expected: PASS

**Step 4: Fix and re-run if needed**

Keep fixes local to the header layout.

**Step 5: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-navbar-search-links-alignment-design.md docs/plans/2026-03-12-navbar-search-links-alignment.md frontend/components/layout/header.tsx frontend/__tests__/components/header.test.tsx
git commit -m "fix: align navbar search and stats baseline"
```
