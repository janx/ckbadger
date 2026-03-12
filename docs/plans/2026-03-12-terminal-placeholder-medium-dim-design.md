# Terminal Placeholder Medium Dim Design

## Goal

- Make the compact terminal search placeholder visibly weaker without changing the rest of the command-line styling.

## Context

- `frontend/components/search-bar.tsx` renders the compact empty state through a visible overlay text node.
- The placeholder is already dimmed relative to typed text, but still reads a bit too prominently.
- Prompt, cursor, typed text, underline, and dropdown styling are already where they should be.

## Requirements

- Only the compact empty-state placeholder should get dimmer.
- The new look should be a medium fade, not barely noticeable and not near-invisible.
- Prompt brightness, typed query brightness, blinking cursor, underline, dropdown, and behavior must remain unchanged.

## Approach

- Lower the compact empty-state placeholder class from the current opacity tier to a medium-dim tier.
- Leave the typed-state overlay branch untouched.
- Keep this as a one-line style refinement inside `SearchBar`.

## Testing

- Update `frontend/__tests__/components/search-bar.test.tsx` to assert the new compact empty-state placeholder class.

## Risks

- If the placeholder is pushed too low it may become hard to read on some displays; use a middle step instead of an aggressive fade.
