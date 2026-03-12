# Homepage Activity Card Equal-Height Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Bound homepage activity pie growth, rebalance the legend, and make the homepage row 5 cards strictly equal height on desktop without internal scrolling.

**Architecture:** Keep the fix local to the homepage activity components. Add a bounded full-width sizing hook to `PieChart`, then consume it from `ActivityCard` by replacing the 50/50 split with a capped chart rail and flexible legend rail while matching the left card's desktop height contract.

**Tech Stack:** React 19, Tailwind CSS, Vitest, Testing Library

---

### Task 1: Add failing layout regressions for `PieChart`

**Files:**

- Modify: `frontend/__tests__/components/pie-chart.test.tsx`
- Reference: `frontend/components/ui/pie-chart.tsx`

**Step 1: Write the failing test**

Add a regression that renders:

```tsx
render(
  <PieChart
    data={[
      { label: 'A', value: 60 },
      { label: 'B', value: 40 },
    ]}
    fullWidth
    showLegend={false}
    chartClassName="max-w-[15rem]"
    testIdPrefix="bounded"
  />
);

expect(screen.getByTestId('bounded-chart-shell')).toHaveClass('max-w-[15rem]');
```

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test pie-chart.test.tsx`

Expected: FAIL because `PieChart` does not yet expose a dedicated chart-shell class/test id.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to confirm the failure is for the intended reason**

Run: `cd frontend && pnpm test pie-chart.test.tsx`

Expected: FAIL with a missing test id or class assertion for the bounded chart shell.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 2: Add failing layout regressions for `ActivityCard`

**Files:**

- Modify: `frontend/__tests__/components/home-layer2.test.tsx`
- Reference: `frontend/components/activity-card.tsx`

**Step 1: Write the failing test**

Extend the existing `ActivityCard` coverage with assertions like:

```tsx
const { container } = renderActivityCard();

await screen.findByText('Activity Types');

expect(container.querySelector('.lg\\:h-\\[44rem\\]')).toBeTruthy();
expect(screen.getByTestId('activity-types-chart-rail')).not.toHaveClass('w-1/2');
expect(screen.getByTestId('activity-types-chart-shell')).toHaveClass('max-w-[15rem]');
```

**Step 2: Run test to verify it fails**

Run: `cd frontend && pnpm test home-layer2.test.tsx`

Expected: FAIL because the current component has no desktop height contract and still uses the symmetric half-width chart rail.

**Step 3: Write minimal implementation**

No production code in this task.

**Step 4: Run test to confirm the failure is for the intended reason**

Run: `cd frontend && pnpm test home-layer2.test.tsx`

Expected: FAIL with missing `lg:h-[44rem]` or missing bounded chart-rail test id/class assertions.

**Step 5: Commit**

Do not commit yet. Implementation is incomplete.

### Task 3: Implement bounded full-width sizing in `PieChart`

**Files:**

- Modify: `frontend/components/ui/pie-chart.tsx`
- Test: `frontend/__tests__/components/pie-chart.test.tsx`

**Step 1: Re-run the focused failing test**

Run: `cd frontend && pnpm test pie-chart.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- add a `chartClassName` prop for the chart shell
- attach it to the full-width chart wrapper
- add a stable `data-testid` for the chart shell when `testIdPrefix` is provided
- keep slice rendering, colors, hover, and click behavior unchanged

Example target shape:

```tsx
<div
  data-testid={testIdPrefix ? `${testIdPrefix}-chart-shell` : undefined}
  className={cn(
    fullWidth ? 'relative aspect-square w-full' : 'relative',
    chartClassName
  )}
>
```

**Step 3: Run the test to verify it passes**

Run: `cd frontend && pnpm test pie-chart.test.tsx`

Expected: PASS

**Step 4: Keep the API minimal**

Do not add resize observers, inline width calculation, or chart-specific JS measurement logic.

**Step 5: Commit**

Do not commit yet. `ActivityCard` still depends on this API.

### Task 4: Implement the equal-height `ActivityCard` layout

**Files:**

- Modify: `frontend/components/activity-card.tsx`
- Test: `frontend/__tests__/components/home-layer2.test.tsx`

**Step 1: Re-run the focused failing test**

Run: `cd frontend && pnpm test home-layer2.test.tsx`

Expected: FAIL before implementation.

**Step 2: Write minimal implementation**

- add `lg:h-[44rem]` and flex-column layout to the outer `TerminalPanel`
- give `TerminalPanelContent` and the inner content stack classes that preserve the fixed outer height without using scrolling
- replace the `w-1/2` chart column with a bounded chart rail such as:

```tsx
<div
  data-testid={`${testIdPrefix}-chart-rail`}
  className="flex basis-[clamp(11.5rem,38%,15rem)] justify-center shrink-0"
>
  <PieChart
    ...
    fullWidth
    chartClassName="w-full max-w-[15rem]"
    testIdPrefix={testIdPrefix}
  />
</div>
```

- change the legend side to `min-w-0 flex-1`
- modestly increase legend marker/text sizing at desktop widths
- tighten vertical gaps so the full card stays within `44rem`

**Step 3: Run the test to verify it passes**

Run: `cd frontend && pnpm test home-layer2.test.tsx`

Expected: PASS

**Step 4: Preserve existing behavior**

Do not change:

- section navigation
- slice click behavior
- legend click behavior
- query keys or API data usage

**Step 5: Commit**

Do not commit yet. Full verification still pending.

### Task 5: Run focused verification

**Files:**

- No code changes required unless failures appear

**Step 1: Run focused frontend tests**

Run: `cd frontend && pnpm test pie-chart.test.tsx home-layer2.test.tsx latest-activities.test.tsx`

Expected: PASS

**Step 2: Run frontend type-check**

Run: `cd frontend && pnpm type-check`

Expected: PASS

**Step 3: Fix any regression and re-run**

Keep any follow-up changes scoped to:

- bounded pie sizing
- `ActivityCard` desktop height contract
- legend sizing and spacing

**Step 4: Commit**

If the user wants a commit after implementation and verification:

```bash
git add docs/plans/2026-03-12-home-activity-card-equal-height-design.md docs/plans/2026-03-12-home-activity-card-equal-height.md frontend/components/ui/pie-chart.tsx frontend/components/activity-card.tsx frontend/__tests__/components/pie-chart.test.tsx frontend/__tests__/components/home-layer2.test.tsx
git commit -m "fix: align homepage activity card layout"
```
