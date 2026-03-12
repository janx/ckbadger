# Terminal Search Placeholder Left-Align Design

## Goal

- Make the compact terminal search placeholder text a bit dimmer.
- Move the navbar stats line back to left alignment so the `block` label starts under the search prompt `>`.

## Context

- `frontend/components/search-bar.tsx` already renders compact search with a terminal-style overlay and underline-only shell.
- The compact placeholder is currently readable but still a little too bright for a terminal hint.
- `frontend/components/layout/header.tsx` currently right-aligns the stats row as a narrow block.
- `frontend/components/stats-bar.tsx` no longer renders its own leading `>`, so the first visible character is `b` from `block`.

## Requirements

- Only the compact empty-state placeholder should get dimmer.
- Typed compact query text, cursor brightness, prompt width, and underline treatment should remain unchanged.
- The stats row must move back to the left-aligned desktop baseline shared with the compact search component.
- The visible `b` in `block` should sit under the same vertical line as the search prompt `>`.
- Search behavior, header structure, and stats content must remain unchanged.

## Approach

- Lower the opacity of the compact placeholder text class only in the empty-state overlay branch.
- Restore the header stats container to the shared desktop offset constant instead of right alignment.
- Keep `GlobalStatsBar` prompt-free so the `block` label becomes the aligned first visible character.

## Testing

- Update `frontend/__tests__/components/search-bar.test.tsx` to assert the dimmer compact placeholder class.
- Update `frontend/__tests__/components/header.test.tsx` to assert the stats row uses `md:pl-[96px]` and no longer uses `justify-end`.

## Risks

- If the placeholder is dimmed too far, readability may suffer on lower-contrast displays.
- Reverting stats alignment must not accidentally reintroduce the old stats prompt or full-row layout drift.
