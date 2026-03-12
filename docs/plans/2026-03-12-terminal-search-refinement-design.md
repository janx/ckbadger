# Terminal Search Refinement Design

## Goal

- Refine the compact navbar search bar so it behaves like a terminal command line.
- Place the blinking cursor before the placeholder when the query is empty, and after the query when text exists.
- Remove the search bar border and rely on terminal-style structure instead.
- Make the search dropdown fully opaque.
- Align the stats-line `>` prompt with the search-bar `>` prompt.

## Context

- `frontend/components/search-bar.tsx` already renders a compact prompt and command-text overlay.
- The blinking cursor currently always appears after the visible text.
- The compact input still carries visible border semantics.
- The dropdown background still uses a translucent terminal panel style.
- `frontend/components/stats-bar.tsx` retains a leading `>` but does not yet share the same prompt-width semantics as the search bar.

## Requirements

- Empty compact search state must render:
  - `> _Search block / tx / address / cell ...`
- Typed compact search state must render:
  - `> 0xabc123_`
- Compact search shell should have no visible outer border.
- Compact dropdown should use an opaque background.
- Stats-line `>` must align horizontally with the search-bar `>` by sharing the same prompt width.
- Search logic, keyboard behavior, routing, and result data must remain unchanged.

## Approach

- Keep the real input for semantics and interaction, but continue using the compact command-line overlay for visible text.
- Render the compact command line in two states:
  - empty: cursor node first, placeholder text second
  - non-empty: query text first, cursor node second
- Hide compact input border with transparent-border styling and rely on prompt separator, dark panel fill, and glow for terminal structure.
- Switch the compact dropdown from translucent to fully opaque background styling.
- Give the stats prompt a fixed width matching the compact search prompt and test that alignment contract directly.

## Testing

- Update `frontend/__tests__/components/search-bar.test.tsx` to assert:
  - empty-state cursor appears before placeholder
  - typed-state cursor appears after query
  - compact input border is transparent
  - dropdown background is opaque
- Update `frontend/__tests__/components/stats-bar.test.tsx` to assert:
  - no trailing blinking cursor
  - leading stats prompt uses the shared fixed-width alignment treatment

## Risks

- Overlay ordering must remain readable while not interfering with keyboard input.
- Removing visible border should not reduce search affordance too far; prompt separator and background contrast need to carry enough structure.
- Prompt alignment should be enforced with a simple width contract instead of brittle pixel assumptions.
