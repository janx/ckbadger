# Terminal Search Underline Design

## Goal

- Remove the visible `>` prompt from the global stats line.
- Refine the compact navbar search so it reads like a single terminal input line.
- Remove the vertical separator between `>` and the blinking cursor/text.
- Replace the boxed compact search shell with an underline-only treatment.

## Context

- `frontend/components/search-bar.tsx` already renders compact search with a prompt slot and command-text overlay.
- The current compact prompt uses a right border to separate `>` from the cursor/text.
- The compact input still reads as a box instead of a command line because it carries panel/border semantics.
- `frontend/components/stats-bar.tsx` still renders a visible leading `>` prompt.

## Requirements

- `GlobalStatsBar` must not render a visible leading `>` anymore.
- Compact search must render `> _placeholder` when empty and `> query_` when typed.
- There must be no vertical rule between `>` and the blinking cursor/text.
- The compact search should not have a visible box border; only an underline should define the command line.
- The underline should span the whole compact command line, including the prompt area.
- Search behavior, routing, result loading, and dropdown behavior must remain unchanged.

## Approach

- Keep the real `<input>` for semantics, focus, keyboard handling, and form submission.
- Continue using the compact command overlay for visible terminal text and cursor placement.
- Change the compact prompt from a bordered slot to a simple inline prompt with fixed width and no separator line.
- Restyle compact input to remove outer borders and keep only a bottom border as the terminal underline.
- Remove the visible prompt node from `GlobalStatsBar` so the stats row starts directly with data text.

## Testing

- Update `frontend/__tests__/components/search-bar.test.tsx` to assert:
  - compact prompt has no separator border
  - compact input uses underline-only styling
  - cursor order remains correct in empty and typed states
- Update `frontend/__tests__/components/stats-bar.test.tsx` to assert:
  - no visible stats prompt is rendered
  - stats data still renders
  - no blinking cursor exists in the stats line

## Risks

- Removing the boxed shell can reduce affordance if the underline contrast is too weak.
- Removing the stats prompt changes left-edge rhythm; the header layout must still feel intentional.
