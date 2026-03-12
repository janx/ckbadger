# Navbar Search Unification Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Unify header search usage on the compact variant, share one placeholder string across all search bars, and make the compact navbar search more prominent while preserving stats alignment.

**Architecture:** Keep `SearchBar` responsible for placeholder copy and variant styling, and keep `Header` responsible only for choosing the navbar variant and maintaining the shared desktop alignment offset. Do not change any search logic, routing, or dropdown behavior.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library

---

### Task 1: Add failing regression tests

**Files:**

- Modify: `frontend/__tests__/components/search-bar.test.tsx`
- Modify: `frontend/__tests__/components/header.test.tsx`
- Reference: `frontend/components/search-bar.tsx`
- Reference: `frontend/components/layout/header.tsx`

**Step 1: Write the failing test**

Add tests that assert:

- `home`, `compact`, and default `SearchBar` variants all use `Search block / tx / address / cell ...`
- the compact input has the updated more prominent classes
- the homepage header now requests `compact` instead of `home`
- the stats row still uses the shared desktop search offset

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test search-bar.test.tsx header.test.tsx`

Expected: FAIL because placeholders differ today and homepage header still requests the home variant.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test search-bar.test.tsx header.test.tsx`

Expected: FAIL with assertion output tied to the old variant wiring and placeholder/style differences.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the unified compact navbar search

**Files:**

- Modify: `frontend/components/search-bar.tsx`
- Modify: `frontend/components/layout/header.tsx`

**Step 1: Re-run the failing tests**

Run: `cd frontend && pnpm test search-bar.test.tsx header.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- Introduce a shared placeholder constant in `SearchBar`
- update the compact classes to be more prominent
- switch both header search usages to `variant="compact"`
- keep the shared desktop offset constant so stats alignment remains bound to the search baseline

**Step 3: Run tests to verify they pass**

Run: `cd frontend && pnpm test search-bar.test.tsx header.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Avoid removing the `home` variant unless a failing test requires it. This task is about unifying navbar usage, not deleting component capabilities.

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

Keep fixes local to the navbar search and stats alignment work.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-navbar-search-unification-design.md docs/plans/2026-03-12-navbar-search-unification.md frontend/components/search-bar.tsx frontend/components/layout/header.tsx frontend/__tests__/components/search-bar.test.tsx frontend/__tests__/components/header.test.tsx
git commit -m "fix: unify navbar search styling"
```
