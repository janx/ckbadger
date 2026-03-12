# Terminal Keycaps and Logo Tune Design

## Goal

- Slightly strengthen the compact terminal placeholder from the current dim state.
- Replace the placeholder tail shortcut text with bordered `/` and `?` keycaps.
- Reduce the logo to 0.95x of its current size and move it up by 2px.

## Context

- `frontend/components/search-bar.tsx` currently uses a shared placeholder string ending in `[/?]`.
- The home variant already renders bordered `/` and `?` hints on the right side.
- The compact variant has no separate keycaps yet, so the shortcut hint currently lives inside the placeholder string.
- `frontend/components/layout/logo.tsx` has the latest enlarged logo classes from the previous header pass.

## Requirements

- All search bar variants should use the same placeholder copy:
  - `Search block / tx / address / cell ...`
- The compact placeholder should become a little more visible than its current state.
- Shortcut hints should move out of the placeholder and into bordered `/` and `?` keycaps.
- Both the home and compact variants should render the bordered keycaps.
- The logo should scale down to roughly 0.95x of its current width and move upward by 2px.
- Search behavior, cursor behavior, typed text styling, underline, dropdown, and header alignment must remain unchanged.

## Approach

- Remove `[/?]` from `SEARCH_PLACEHOLDER`.
- Add a small shared shortcut-hint renderer for the home and compact variants.
- For compact mode, leave space on the right side of the terminal line so the keycaps do not overlap the overlay text.
- Raise the compact placeholder from the current dim level to a slightly stronger opacity tier.
- Update the logo width classes and top offsets, leaving the left offsets unchanged.

## Testing

- Update `frontend/__tests__/components/search-bar.test.tsx` to assert:
  - the shared placeholder string no longer contains `[/?]`
  - home variant still renders bordered shortcut keycaps
  - compact variant now renders shortcut keycaps
  - compact empty-state placeholder uses the slightly stronger class
- Update `frontend/__tests__/components/logo.test.tsx` to assert:
  - new responsive width classes
  - top offsets moved up by 2px

## Risks

- Compact keycaps can overlap the terminal line if the overlay does not reserve space on the right.
- Removing `[/?]` from the placeholder must stay synchronized across all variants and tests.
