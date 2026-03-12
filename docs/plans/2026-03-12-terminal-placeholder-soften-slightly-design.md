# Terminal Placeholder Soften Slightly Design

## Goal

- Soften the compact terminal placeholder a little from its current strength without making it look weak.

## Context

- `frontend/components/search-bar.tsx` currently renders the compact empty-state placeholder at `text-text-dim/48`.
- The latest tuning made the placeholder slightly stronger and moved shortcut hints into bordered `/` and `?` keycaps.
- Prompt, cursor, typed text, underline, keycaps, dropdown, and logo are all already in the right place.

## Requirements

- Only the compact empty-state placeholder should get slightly weaker.
- The new value should remain comfortably readable and should not cross into the “too weak” range.
- Keycaps, prompt, cursor, typed text, underline, dropdown, and behavior must remain unchanged.

## Approach

- Lower the compact empty-state placeholder class one small step, from `text-text-dim/48` to `text-text-dim/44`.
- Keep the change isolated to the empty-state overlay branch in `SearchBar`.

## Testing

- Update `frontend/__tests__/components/search-bar.test.tsx` to assert the new compact placeholder class.

## Risks

- If the placeholder keeps drifting downward across multiple small changes it can become underpowered; this step should stay moderate.
