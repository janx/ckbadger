# Footer Broadcast Strip Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current footer plaque/card treatment with a segmented broadcast strip while keeping the attribution text, build version, shortcut hint, and quick links intact.

**Architecture:** Keep `SiteFooter` as the single footer component and restructure its markup into four explicit channel slots: credits, build, shortcut, and links. Drive the change with a failing component test first, then make the smallest markup and class changes required to swap the current plaque layout for the approved strip layout.

**Tech Stack:** React, TypeScript, Tailwind CSS, Vitest, Testing Library

---

### Task 1: Update the footer test to describe the broadcast-strip contract

**Files:**

- Modify: `frontend/__tests__/components/site-footer.test.tsx`

**Step 1: Write the failing test**

Update the existing footer test so it asserts:

```ts
expect(screen.getByText('Credits')).toBeInTheDocument();
expect(screen.getByText('Build')).toBeInTheDocument();
expect(screen.getByText('Shortcut')).toBeInTheDocument();
expect(screen.getByText('Links')).toBeInTheDocument();
expect(screen.getByText(/coded by Claude and Codex\./i)).toBeInTheDocument();
```

Keep assertions for:

- `@busyforking`
- `0.1.0+feature/foo@abcdef123456`
- `Press ? for shortcuts`
- `Hardforks`
- `Github`

**Step 2: Run test to verify it fails**

Run:

```bash
cd frontend && pnpm test -- --run __tests__/components/site-footer.test.tsx
```

Expected: FAIL because the current footer still renders the plaque layout and does not expose the broadcast-strip channel labels.

**Step 3: Keep the test focused**

- Remove assertions that overfit the plaque layout.
- Do not assert exact Tailwind class strings except broad layout anchors already used by the suite.

**Step 4: Run test to verify it still fails for the right reason**

Run the same command from Step 2.

Expected: FAIL on missing channel labels or old structure.

**Step 5: Commit**

```bash
git add frontend/__tests__/components/site-footer.test.tsx
git commit -m "test(frontend): cover footer broadcast strip structure"
```

### Task 2: Implement the broadcast-strip footer layout

**Files:**

- Modify: `frontend/components/layout/site-footer.tsx`

**Step 1: Write the minimal implementation**

Restructure `SiteFooter` into four slots:

- `Credits`
- `Build`
- `Shortcut`
- `Links`

Representative target shape:

```tsx
<div className="...">
  <section>
    <span>Credits</span>
    <p>
      Designed by <a ...>@busyforking</a>, coded by Claude and Codex. ❤️
    </p>
  </section>
</div>
```

Requirements:

- remove the current plaque treatment
- keep the footer as one continuous strip
- make slot separators stronger than the current card styling
- keep version, shortcut, and links in dedicated channels

**Step 2: Run test to verify it passes**

Run:

```bash
cd frontend && pnpm test -- --run __tests__/components/site-footer.test.tsx
```

Expected: PASS

**Step 3: Refactor if needed**

- Tighten duplicated slot markup only if readability clearly improves.
- Do not introduce shared abstractions unless the four-slot structure becomes noisy.

**Step 4: Re-run test to stay green**

Run the same command from Step 2.

Expected: PASS

**Step 5: Commit**

```bash
git add frontend/components/layout/site-footer.tsx frontend/__tests__/components/site-footer.test.tsx
git commit -m "feat(frontend): turn footer into broadcast strip"
```

### Task 3: Verify the touched frontend surface

**Files:**

- Modify: none

**Step 1: Run targeted component test**

Run:

```bash
cd frontend && pnpm test -- --run __tests__/components/site-footer.test.tsx
```

Expected: PASS

**Step 2: Run type-check**

Run:

```bash
cd frontend && pnpm type-check
```

Expected: PASS

**Step 3: Inspect the final diff**

Run:

```bash
git diff -- docs/plans/2026-03-08-footer-design.md docs/plans/2026-03-08-footer-refresh.md frontend/components/layout/site-footer.tsx frontend/__tests__/components/site-footer.test.tsx
```

Expected: Only the approved footer redesign and updated plan documents appear.

**Step 4: Commit**

```bash
git add docs/plans/2026-03-08-footer-design.md docs/plans/2026-03-08-footer-refresh.md frontend/components/layout/site-footer.tsx frontend/__tests__/components/site-footer.test.tsx
git commit -m "docs: update footer redesign plan"
```
