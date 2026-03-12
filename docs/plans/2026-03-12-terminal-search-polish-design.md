# Terminal Search Polish Design

## Goal

- Polish the compact navbar search so it reads more like a focused terminal command line.
- Right-align the stats line as a narrow block under the navbar instead of a full-width left-anchored strip.

## Context

- `frontend/components/search-bar.tsx` already renders compact search as an underlined terminal input with overlay text and cursor positioning.
- `frontend/components/stats-bar.tsx` no longer renders a visible leading prompt.
- `frontend/components/layout/header.tsx` still places the stats row in a full-width container without right-edge anchoring.

## Requirements

- The compact search must remain `> _placeholder` when empty and `> query_` when populated.
- The prompt should sit closer to the cursor/text so the line reads as one terminal command, not two regions.
- The underline should stay box-free, but become a bit cleaner and more intentional.
- The compact dropdown must stay opaque and use a more restrained terminal panel style.
- The stats line should become a content-width block aligned to the container’s right edge.
- Search behavior, routing, data loading, and keyboard behavior must remain unchanged.

## Approach

- Tighten the compact prompt slot width and move the visible command text closer to it.
- Slightly reduce the compact input height and tracking, while keeping the underline-only treatment.
- Soften the default underline and brighten it on focus instead of restoring any outer border.
- Tweak the compact dropdown border, background, and shadow so it feels like a terminal output panel instead of a generic popover.
- In the header, right-align the stats row by letting the container justify its child to the end rather than stretching the stats bar across the row.

## Testing

- Update `frontend/__tests__/components/search-bar.test.tsx` to assert:
  - tighter compact prompt width
  - command-line overlay starts closer to the prompt
  - compact input keeps underline-only styling with the refined class contract
  - compact dropdown uses the polished opaque panel classes
- Update `frontend/__tests__/components/header.test.tsx` to assert:
  - stats row container uses right alignment instead of the old desktop left-offset contract
- Keep `frontend/__tests__/components/stats-bar.test.tsx` ensuring no leading prompt or blinking cursor reappears.

## Risks

- Tightening the prompt spacing too far can make the line feel cramped; keep enough room for the cursor.
- Right-aligning the stats block changes header balance, so the container alignment must be explicit and regression-tested.
