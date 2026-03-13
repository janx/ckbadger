# Address Activities Script Columns Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Split the address detail desktop activity table so script calls no longer render inside the assets column.

**Architecture:** Keep the current API contract and badge components, but change the address activity table layout so desktop rows have independent `Assets` and `Scripts` columns. Mobile rendering stays as-is because it already models the separation correctly.

**Tech Stack:** React 19, Vite, Vitest, Testing Library, Tailwind utility classes, `frontend/app/address/[addr]/client-page.tsx`

---

### Task 1: Add regression test

**Files:**

- Modify: `frontend/__tests__/pages/address.test.tsx`

**Step 1: Write the failing test**

- Add a regression test covering an activity row that contains both one token asset change and one script call.
- Assert the table renders separate `Assets` and `Scripts` headers for the desktop activity layout.
- Assert the script call still links to `/scripts/RGB%2B%2B%20Lock`.

**Step 2: Run test to verify it fails**

Run: `pnpm test -- --run frontend/__tests__/pages/address.test.tsx`

Expected: FAIL because the desktop table still has only one data column for both activity dimensions.

### Task 2: Implement the layout split

**Files:**

- Modify: `frontend/app/address/[addr]/client-page.tsx`

**Step 1: Write minimal implementation**

- Update the desktop activity header to include independent `Assets` and `Scripts` columns.
- Update each desktop row so asset badges render only in the assets cell and script-call badges render only in the scripts cell.
- Keep mobile rendering unchanged.

**Step 2: Run test to verify it passes**

Run: `pnpm test -- --run frontend/__tests__/pages/address.test.tsx`

Expected: PASS

### Task 3: Run focused verification

**Files:**

- Modify: none

**Step 1: Run nearby tests**

Run: `pnpm test -- --run frontend/__tests__/pages/address.test.tsx frontend/__tests__/components/latest-activities.test.tsx`

Expected: PASS
