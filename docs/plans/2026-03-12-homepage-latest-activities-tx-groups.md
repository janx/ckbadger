# Homepage Latest Activities Tx Groups Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Group homepage latest activities by transaction, show one transaction block per group, and default-expand up to three owner activities inside each group.

**Architecture:** Keep the backend feed unchanged and derive a frontend-only grouped view model from the existing `GlobalActivity[]` response. Put grouping, sorting, and summary generation in a pure helper module so the homepage component stays focused on rendering and realtime highlight behavior.

**Tech Stack:** React 19, TanStack Query, TypeScript, Vitest, Testing Library

---

### Task 1: Add failing pure-function tests for grouping and summaries

**Files:**

- Create: `frontend/__tests__/lib/latest-activity-groups.test.ts`
- Reference: `frontend/lib/api.ts`
- Reference: `frontend/components/latest-activities.tsx`

**Step 1: Write the failing test**

Add tests that assert:

- multiple `GlobalActivity` items with the same `txHash` become one group
- participants are sorted with negative deltas first, then positive, then zero
- DAO summaries outrank structural fallback text
- object and identity summaries outrank structural fallback text
- fallback summaries render as `X sent · Y received` and append `· Z asset events` when appropriate

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test latest-activity-groups.test.ts`

Expected: FAIL because the helper module does not exist yet.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test latest-activity-groups.test.ts`

Expected: FAIL with missing module or missing exports for the grouping helpers.

**Step 5: Commit**

Do not commit yet. The tests are intentionally failing.

### Task 2: Implement the grouping helper module

**Files:**

- Create: `frontend/lib/latest-activity-groups.ts`
- Modify: `frontend/lib/api.ts`
- Test: `frontend/__tests__/lib/latest-activity-groups.test.ts`

**Step 1: Write the failing test**

Use the tests from Task 1.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test latest-activity-groups.test.ts`

Expected: FAIL before implementation.

**Step 3: Write minimal implementation**

Implement and export:

- a frontend group type for the homepage card
- a `groupLatestActivitiesByTx()` helper
- a `buildLatestActivityGroupSummary()` helper
- a participant sort function based on delta bucket, absolute delta, asset-change count, and stable feed order

Keep the logic deterministic and free of React state.

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm test latest-activity-groups.test.ts`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/lib/latest-activity-groups.ts frontend/lib/api.ts frontend/__tests__/lib/latest-activity-groups.test.ts
git commit -m "feat: add homepage latest activity grouping helpers"
```

### Task 3: Add failing component regression tests for grouped rendering

**Files:**

- Create: `frontend/__tests__/components/latest-activities.test.tsx`
- Reference: `frontend/components/latest-activities.tsx`
- Reference: `frontend/__tests__/utils/test-utils.tsx`

**Step 1: Write the failing test**

Add tests that assert:

- the component renders one transaction block for multiple activities sharing a `txHash`
- only the first 3 participant rows render by default
- the component shows `+N more` when a group has hidden participants
- transaction metadata (`txHash` / block / relative time) renders once per group, not per participant row
- a DAO or object/identity transaction shows the stronger summary text instead of structural fallback

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test latest-activities.test.tsx`

Expected: FAIL because the current component still renders flat activity rows.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to verify it still fails for the intended reason**

Run: `cd frontend && pnpm test latest-activities.test.tsx`

Expected: FAIL with grouped-layout assertions not yet satisfied.

**Step 5: Commit**

Do not commit yet. The component behavior is still incomplete.

### Task 4: Implement grouped homepage rendering and group-level highlight

**Files:**

- Modify: `frontend/components/latest-activities.tsx`
- Modify: `frontend/lib/api.ts`
- Reference: `frontend/lib/latest-activity-groups.ts`
- Test: `frontend/__tests__/components/latest-activities.test.tsx`

**Step 1: Write the failing test**

Use the tests from Task 3.

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test latest-activities.test.tsx`

Expected: FAIL before implementation.

**Step 3: Write minimal implementation**

- request enough raw latest activities to reliably fill 4 grouped transactions
- transform the fetched `GlobalActivity[]` into grouped transaction blocks
- render at most 4 groups
- render at most 3 participant rows per group
- show `+N more` for hidden participants
- move the realtime highlight key from per-activity rows to per-transaction groups
- keep transaction links and address links intact
- preserve existing terminal-panel styling and loading skeleton behavior as closely as practical

**Step 4: Run test to verify it passes**

Run: `cd frontend && pnpm test latest-activities.test.tsx`

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/components/latest-activities.tsx frontend/__tests__/components/latest-activities.test.tsx
git commit -m "feat: group homepage latest activities by transaction"
```

### Task 5: Run targeted verification and adjacent regression checks

**Files:**

- No code changes required unless failures appear

**Step 1: Run helper tests**

Run: `cd frontend && pnpm test latest-activity-groups.test.ts`

Expected: PASS

**Step 2: Run component tests**

Run: `cd frontend && pnpm test latest-activities.test.tsx`

Expected: PASS

**Step 3: Run adjacent homepage regressions**

Run: `cd frontend && pnpm test latest-transactions.test.tsx`

Expected: PASS

**Step 4: Run type-check**

Run: `cd frontend && pnpm type-check`

Expected: PASS

**Step 5: Commit**

If all verification passes and the user wants a final consolidation commit, create one with:

```bash
git add docs/plans/2026-03-12-homepage-latest-activities-tx-groups-design.md docs/plans/2026-03-12-homepage-latest-activities-tx-groups.md frontend/lib/latest-activity-groups.ts frontend/lib/api.ts frontend/components/latest-activities.tsx frontend/__tests__/lib/latest-activity-groups.test.ts frontend/__tests__/components/latest-activities.test.tsx
git commit -m "feat: group homepage latest activities by transaction"
```
