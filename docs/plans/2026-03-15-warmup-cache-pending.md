# Warmup Cache Pending Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make warmup cache unavailability explicit so affected pages can show a warmup message and automatically refetch until data becomes ready.

**Architecture:** The API will emit a dedicated `warmup_pending` error instead of a generic internal error for cache-miss-on-warmup cases. The frontend will parse structured API errors and use a shared warmup-aware query helper plus a shared info state component so all warmup cache pages behave consistently without browser-level reloads.

**Tech Stack:** Rust, Axum, React 19, TanStack Query v5, Vitest

---

### Task 1: Document the design

**Files:**
- Create: `docs/plans/2026-03-15-warmup-cache-pending-design.md`
- Create: `docs/plans/2026-03-15-warmup-cache-pending.md`

**Step 1: Write the design and implementation plan**

- Capture the warmup contract, frontend behavior, and test strategy.

**Step 2: Verify the docs exist**

Run: `ls docs/plans | rg 'warmup-cache-pending'`
Expected: both plan files are listed

### Task 2: Add failing API tests

**Files:**
- Modify: `crates/api/tests/api_integration.rs`

**Step 1: Write the failing test**

- Add a regression test that starts an API instance without the scripts warmup cache populated.
- Assert `/api/v1/scripts` returns `503` with `error = "warmup_pending"`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-api warmup_pending -- --nocapture`
Expected: FAIL because the route still returns `internal_error` / `500`

### Task 3: Add failing frontend tests

**Files:**
- Modify: `frontend/__tests__/lib/api.test.ts`
- Modify: `frontend/__tests__/pages/scripts.test.tsx`
- Create or modify shared frontend helpers as needed

**Step 1: Write the failing tests**

- Add an API parsing test proving `warmup_pending` becomes a structured frontend error.
- Add a page test proving `/scripts` shows a warmup message and retries until successful data arrives.

**Step 2: Run tests to verify they fail**

Run: `cd frontend && npx vitest run __tests__/lib/api.test.ts __tests__/pages/scripts.test.tsx`
Expected: FAIL because warmup-aware parsing/retry behavior does not exist yet

### Task 4: Implement API warmup_pending contract

**Files:**
- Modify: `crates/api/src/response.rs`
- Modify: warmup-cache-backed route modules under `crates/api/src/routes/`

**Step 1: Add warmup-specific API error helper**

- Introduce `ApiError::warmup_pending(...)` returning `503`.

**Step 2: Replace cache-unavailable route errors**

- Update all warmup cache miss call sites to use the new helper.

**Step 3: Run targeted API tests**

Run: `cargo test -p ckbadger-api warmup_pending -- --nocapture`
Expected: PASS

### Task 5: Implement frontend warmup-aware handling

**Files:**
- Modify: `frontend/lib/api.ts`
- Create or modify shared hooks/components under `frontend/lib/` and `frontend/components/ui/`
- Update affected pages under `frontend/app/`

**Step 1: Add structured API error parsing**

- Preserve HTTP status, API error code, and message.

**Step 2: Add warmup-aware shared query behavior**

- Centralize retry interval / detection so pages do not duplicate logic.

**Step 3: Update warmup cache pages**

- Show the shared warmup state during `warmup_pending`.
- Keep existing empty and non-warmup error states unchanged.

**Step 4: Run targeted frontend tests**

Run: `cd frontend && npx vitest run __tests__/lib/api.test.ts __tests__/pages/scripts.test.tsx`
Expected: PASS

### Task 6: Verify regressions

**Files:**
- No additional file changes expected

**Step 1: Run targeted Rust and frontend suites**

Run: `cargo test -p ckbadger-api warmup_pending -- --nocapture`
Expected: PASS

Run: `cd frontend && npx vitest run __tests__/lib/api.test.ts __tests__/pages/scripts.test.tsx`
Expected: PASS

**Step 2: Run broader guardrails**

Run: `cargo test -p ckbadger-api test_script_ -- --nocapture`
Expected: PASS

Run: `cd frontend && npx vitest run __tests__/pages/scripts.test.tsx`
Expected: PASS
