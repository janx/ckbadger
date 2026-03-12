# DAO Overview Desktop Baseline Alignment Design

## Goal

- Align the homepage `Nervos DAO` card with the left-column cards on desktop so the top bar-chart baseline matches `Knowledge Bytes` and the lower stats baseline matches `Activity`.

## Context

- The homepage row is composed in `frontend/components/home-content.tsx`.
- The left column stacks `KnowledgeSizeTrend` and `ActivityBarChartCard`, both using a `h-14` chart area.
- The right-column `DaoOverview` currently uses a shorter `h-10` chart area and an auto-height stats grid, so its internal horizontal breakpoints land above the left column baselines.

## Requirements

- Desktop only: preserve current mobile/stacked behavior.
- Keep the existing data flow and query structure.
- Fix alignment by changing layout rhythm, not by adding placeholder spacing or duplicate content paths.
- Maintain the existing DAO stat content and hover behavior.

## Approach

- Keep the homepage row structure unchanged.
- Adjust `DaoOverview` only:
  - use the same desktop chart height rhythm as `KnowledgeSizeTrend`
  - let the stats grid expand to fill the remaining desktop height
  - vertically center each stat block within its quadrant
- Add a frontend regression test that asserts the desktop-only layout classes responsible for this alignment.

## Testing

- Add a targeted regression test in `frontend/__tests__/components/dao-overview.test.tsx`.
- Verify the test fails before the layout change and passes after it.

## Risks

- Tailwind layout changes that rely on `lg:` classes can regress silently without a DOM-level assertion.
- Over-constraining mobile layout would be a regression, so desktop-only classes must be additive.
