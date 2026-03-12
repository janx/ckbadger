# Homepage Chart Navigation Design

## Goal

- Add precise click navigation for the homepage `Activity Types` and `Script Usage` pie sections.

## Context

- The homepage currently renders both pie sections through `frontend/components/activity-card.tsx`.
- Existing behavior supports hover highlighting only.
- Destination routes already exist:
  - `/charts/activity-type-breakdown`
  - `/charts/most-utilized-scripts`
  - script detail routes via `getScriptDetailHref()`

## Requirements

- `Activity Types`
  - Clicking a pie slice navigates to `/charts/activity-type-breakdown`
  - Clicking non-slice section chrome also navigates to `/charts/activity-type-breakdown`
- `Script Usage`
  - Clicking a pie slice navigates to the matching script detail page
  - Clicking a legend item navigates to the matching script detail page
  - Clicking other section chrome navigates to `/charts/most-utilized-scripts`

## Approach

- Keep `ActivityCard` as the composition root.
- Extend `PieSection` with two navigation concepts:
  - section-level destination for non-item clicks
  - item-level destination resolver for slice and legend clicks
- Extend `PieChart` so individual slices can invoke item navigation directly instead of relying on container bubbling.
- Reuse `getScriptDetailHref()` to preserve existing script route rules.

## Event Boundaries

- Slice clicks must not trigger the section-level navigation.
- Legend item clicks must not trigger the section-level navigation.
- Empty area clicks inside the section should route to the section-level chart page.

## Testing

- Add frontend regression tests for:
  - `Activity Types` slice click
  - `Activity Types` section click
  - `Script Usage` slice click
  - `Script Usage` legend click
  - `Script Usage` section click

## Risks

- SVG slice click handling can accidentally collide with hover state or section click handlers.
- Script labels may be human-readable names or fallback code-hash snippets; navigation must use raw script metadata, not rendered labels.
