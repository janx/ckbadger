# Command Line Search Cursor Design

## Goal

- Make the compact navbar search bar look like a real terminal command line.
- Move the blinking cursor from the stats line into the visible command text inside the search bar.
- Keep the cursor at the end of the currently visible text even when the user has typed content.

## Context

- `frontend/components/search-bar.tsx` already gives the compact variant a terminal-like shell with a prompt.
- The visible text still comes from the native input rendering, so the field does not yet read as an actual command line.
- `frontend/components/stats-bar.tsx` still renders the blinking cursor at the end of the stats line.

## Requirements

- Compact search should visually read as a terminal command line.
- The compact variant should show:
  - a fixed prompt
  - visible text content rendered in command-line style
  - a blinking cursor at the end of the visible text
- Empty state:
  - show placeholder text as the visible command content
  - cursor appears at the end of the placeholder
- Typed state:
  - show the actual query as the visible command content
  - cursor appears at the end of the query
- Real input behavior, keyboard navigation, routing, and dropdown results must remain unchanged.
- The blinking cursor must be removed from the stats line.

## Approach

- Keep the real `input` for semantics, focus, keyboard input, and form submission.
- For compact mode, add an overlay “command text” layer that renders:
  - the current query when non-empty
  - otherwise the placeholder
  - plus the blinking cursor span as part of the same visible line
- Make the real input text transparent in compact mode while keeping the caret hidden, so only the command overlay is visible.
- Keep the overlay non-interactive so focus and selection continue to belong to the input.
- Remove the trailing cursor span from `GlobalStatsBar`.

## Testing

- Extend `frontend/__tests__/components/search-bar.test.tsx` to assert:
  - compact mode renders the command text overlay and cursor
  - typing updates the visible command text and keeps the cursor at its end
- Add `frontend/__tests__/components/stats-bar.test.tsx` to assert:
  - `GlobalStatsBar` no longer renders the trailing blinking cursor

## Risks

- The overlay text must stay visually aligned with the underlying input padding.
- Making the input text transparent must not hide placeholder behavior for non-compact variants.
- Cursor removal from the stats bar should not affect its data rendering or link layout.
