# Terminal Keycaps and Logo Tune Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move shortcut hints into bordered keycaps, slightly strengthen the compact placeholder, and reduce/move the logo per the approved tuning.

**Architecture:** Keep all behavior local to `SearchBar` and `Logo`. Remove shortcut-hint text from the shared placeholder, render keycaps explicitly for home and compact variants, and keep the rest of the terminal search behavior unchanged.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library

---

### Task 1: Add failing regressions

**Files:**

- Modify: `frontend/__tests__/components/search-bar.test.tsx`
- Modify: `frontend/__tests__/components/logo.test.tsx`
- Reference: `frontend/components/search-bar.tsx`
- Reference: `frontend/components/layout/logo.tsx`

**Step 1: Write the failing test**

Add assertions that:

- the shared placeholder string no longer includes `[/?]`
- compact search renders shortcut keycaps
- compact placeholder uses the new slightly stronger class
- logo width classes and top offsets match the tuned values

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test search-bar.test.tsx logo.test.tsx`

Expected: FAIL because the current placeholder still contains `[/?]`, compact has no keycaps, and the logo still uses the previous size/offset classes.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test search-bar.test.tsx logo.test.tsx`

Expected: FAIL with placeholder, keycap, and logo-class assertions.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Implement the SearchBar and Logo refinement

**Files:**

- Modify: `frontend/components/search-bar.tsx`
- Modify: `frontend/components/layout/logo.tsx`

**Step 1: Re-run the failing tests**

Run: `cd frontend && pnpm test search-bar.test.tsx logo.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- remove `[/?]` from the shared placeholder
- render bordered shortcut keycaps for home and compact variants
- reserve compact right-side space for the keycaps
- raise the compact placeholder opacity slightly
- reduce logo size to 0.95x and move it up by 2px

**Step 3: Run the tests to verify they pass**

Run: `cd frontend && pnpm test search-bar.test.tsx logo.test.tsx`

Expected: PASS

**Step 4: Keep implementation minimal**

Do not change search routing, command-line cursor behavior, underline styling, dropdown behavior, or header alignment.

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

Keep any fixes scoped to shortcut hints, placeholder visibility, and logo tuning.

**Step 4: Commit**

If the user wants a commit after verification:

```bash
git add docs/plans/2026-03-12-terminal-keycaps-logo-tune-design.md docs/plans/2026-03-12-terminal-keycaps-logo-tune.md frontend/components/search-bar.tsx frontend/components/layout/logo.tsx frontend/__tests__/components/search-bar.test.tsx frontend/__tests__/components/logo.test.tsx
git commit -m "style: tune terminal keycaps and logo"
```
