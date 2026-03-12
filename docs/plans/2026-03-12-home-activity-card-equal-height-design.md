# Homepage Activity Card Equal-Height Design

## Goal

- Fix homepage row 5 at large desktop widths where the `ActivityCard` pies scale too large.
- Keep the right-side pie charts visually balanced with their legends at maximum width.
- Enforce strict equal height between `LatestActivities` and `ActivityCard` on desktop.
- Preserve the no-scroll-card requirement: neither card may rely on internal scrolling.

## Principle Alignment

- CKB Native: No domain or data behavior changes. This is a presentation-only fix for existing activity views.
- Local First: No new client-side measurement loop, resize observer churn, or runtime layout bookkeeping. The layout should stay CSS-driven and cheap.
- Agent Friendly: Make the height contract and pie-size cap explicit in component props/classes so later changes do not silently reintroduce unbounded growth.

## Context

- `frontend/components/latest-activities.tsx` already defines the left card as `h-[44rem]`.
- `frontend/components/activity-card.tsx` does not define a matching desktop height, so its height is currently content-driven.
- `frontend/components/activity-card.tsx` renders each pie section as a 50/50 split between chart and legend.
- `frontend/components/ui/pie-chart.tsx` uses `fullWidth + aspect-square + w-full`, so the pie expands with the full width of its column.
- At maximum width, the chart column becomes very large, but the legend remains fixed at `text-[10px]` with tiny markers. The pie dominates visually while the legend stays undersized.
- Because both pie sections grow with width, the right card's natural height exceeds the left card's fixed `44rem`, creating a visibly taller card.

## Requirements

- On `lg` and above, row 5 must show `LatestActivities` and `ActivityCard` at the same height.
- Neither card may use internal scrolling to satisfy the equal-height requirement.
- The pie chart diameter must stop growing after a defined desktop maximum.
- The legend must remain readable and proportionate when the chart reaches its size cap.
- Mobile and tablet stacked layouts may keep natural height behavior.
- Existing hover behavior, slice highlighting, and navigation behavior must remain unchanged.
- No API, query, or activity data changes.
- The change must include regression tests for the bounded pie layout and desktop height contract.

## Recommended Approach

- Keep `LatestActivities` as the source height contract at `44rem` on desktop.
- Give `ActivityCard` the same `lg:h-[44rem]` height and convert it into a flex column so the panel has a stable outer box.
- Change `PieSection` from a symmetric `w-1/2` split to a bounded chart rail plus flexible legend rail:
  - chart rail gets a fixed desktop budget
  - legend rail consumes the remaining width
- Keep the pie responsive within that rail, but cap its rendered size on desktop so `fullWidth` no longer implies unbounded scaling.
- Slightly strengthen the legend's visual presence with modest responsive sizing for text, gap, and marker size. The legend should scale a little, not linearly with the pie.
- Tighten section spacing enough to fit two pie sections plus the stats row inside the `44rem` card without clipping or scrolling.

## Size Budget

- Desktop card height contract: `44rem` (`704px`)
- Right card budget should fit:
  - terminal header
  - content padding
  - top stat strip
  - two pie sections
  - gaps between these blocks
- Use a desktop pie cap of roughly `15rem` (`240px`) for each section.
- The chart rail should not exceed about `38%` of the section width once the card is wide enough.
- The legend must keep enough remaining width to show labels and percentages without becoming visually tiny relative to the chart.

This gives a stable layout budget:

- two pie sections at about `240px` each
- one compact stat strip
- reduced section gaps
- total comfortably below the `44rem` outer card height

## Component Changes

### `frontend/components/ui/pie-chart.tsx`

- Add an explicit way for `fullWidth` pies to respect a maximum visual size.
- Keep the current pie math and interaction model unchanged.
- The change should stay generic so other callers can opt into bounded full-width behavior later.

### `frontend/components/activity-card.tsx`

- Make the outer `TerminalPanel` explicitly match the left card height on desktop.
- Refactor `PieSection` so the chart column is bounded and the legend column flexes.
- Reduce the section spacing and keep the current click-through behavior for slices, legend items, and section chrome.
- Give legend rows slightly larger desktop typography and markers so the legend remains proportionate after the chart is capped.

### `frontend/components/latest-activities.tsx`

- No functional change expected.
- Keep `h-[44rem]` as the source contract unless the whole homepage row is intentionally redesigned later.

## Testing

- Update `frontend/__tests__/components/pie-chart.test.tsx`:
  - assert that the new bounded full-width API/class is rendered when requested
  - preserve existing palette-color assertions
- Update `frontend/__tests__/components/home-layer2.test.tsx`:
  - assert that `ActivityCard` applies the desktop height contract
  - assert that the pie section uses the bounded chart rail rather than an equal `w-1/2` split
  - preserve existing interaction tests for slice click and legend click behavior
- Manual verification after implementation:
  - homepage at maximum width
  - homepage around `lg` breakpoint
  - stacked/mobile layout
  - script-usage and activity-type section hover/click behavior

## Risks

- If the chart rail cap is set too low, the pie can feel cramped next to large legends.
- If the legend typography is increased too aggressively, the card can again exceed the `44rem` budget.
- If the bounded size is implemented with clipping instead of true sizing, hover states and SVG content could be cut off.

## Non-Goals

- No redesign of homepage row ordering.
- No change to `LatestActivities` content density.
- No runtime JS measurement logic.
- No hidden overflow strategy that masks content loss.
