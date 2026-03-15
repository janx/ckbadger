# Global Pagination Default Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make paginated explorer pages default to 50 records per page across frontend requests, pagination UI, and API shared defaults.

**Architecture:** Add a single frontend default page size constant, replace page-level hardcoded pagination limits with that constant, and update the API shared `default_limit()` helper to `50` so unspecified route limits match the same policy.

**Tech Stack:** React 19, TanStack Query v5, Vitest, Rust, Axum

---

### Task 1: Lock the new default with failing tests

**Files:**

- Modify: `frontend/__tests__/pages/blocks-page.test.tsx`
- Modify: `frontend/__tests__/pages/transactions-page.test.tsx`
- Modify: `frontend/__tests__/pages/forks-page.test.tsx`
- Modify: `crates/api/src/response.rs`

**Step 1: Write the failing tests**

- Change the frontend page tests to expect `limit: 50`.
- Change the Rust `test_default_limit` assertion to expect `50`.

**Step 2: Run the targeted tests to verify failure**

Run:

```bash
cd frontend && pnpm test -- --run blocks-page.test.tsx transactions-page.test.tsx forks-page.test.tsx
cargo test -p ckbadger-api response::tests::test_default_limit -- --nocapture
```

Expected: fail because the implementation still uses `20` or `25`.

### Task 2: Introduce a single frontend page-size source

**Files:**

- Create: `frontend/lib/pagination.ts`
- Modify: paginated frontend pages and components that currently hardcode `20` or `25`

**Step 1: Add the shared constant**

Create:

```ts
export const DEFAULT_PAGE_SIZE = 50;
```

**Step 2: Replace hardcoded pagination values**

- Update paginated page queries to use `DEFAULT_PAGE_SIZE`.
- Update matching `CursorPagination` `pageSize` props to use `DEFAULT_PAGE_SIZE`.
- Update any local `PAGE_SIZE` constants for paginated explorer components to use the shared constant.

### Task 3: Align API shared defaults and fix tests

**Files:**

- Modify: `crates/api/src/response.rs`
- Modify: affected frontend tests

**Step 1: Change the API shared default limit**

- Update `default_limit()` from `20` to `50`.

**Step 2: Update affected frontend tests**

- Replace test fixtures and expectations that model default paginated responses as `20` or `25` when they are asserting default page behavior.

### Task 4: Verify

**Files:**

- No new files

**Step 1: Run focused tests**

Run:

```bash
cd frontend && pnpm test -- --run blocks-page.test.tsx transactions-page.test.tsx forks-page.test.tsx
cargo test -p ckbadger-api response::tests::test_default_limit -- --nocapture
```

**Step 2: Run broader pagination-focused frontend tests**

Run:

```bash
cd frontend && pnpm test -- --run CursorPagination.test.tsx activities-stream-explorer.test.tsx
```

**Step 3: Run type-level safety checks if the touched test set passes**

Run:

```bash
cd frontend && pnpm type-check
cargo check -p ckbadger-api
```
