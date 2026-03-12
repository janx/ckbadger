# Terminal Search Bar Design

## Goal

- Refine the compact navbar search bar into a more compact, simpler terminal-style control.
- Keep the unified placeholder copy and existing search behavior intact.

## Context

- `frontend/components/search-bar.tsx` currently uses the compact variant for all header search instances.
- The compact variant is visually stronger than before, but it still reads as a conventional rounded input rather than a terminal command line.
- Search behavior, routing, keyboard navigation, and result dropdown behavior are already covered by tests and should remain unchanged.

## Requirements

- Compact search should feel tighter and more terminal-like.
- Visual language should use a prompt-style structure, darker surface, harder edges, and restrained glow.
- No scanning animations, shortcut hints, or decorative labels.
- The placeholder remains `Search block / tx / address / cell ...`.
- Dropdown behavior and search logic must not change.

## Approach

- Keep `SearchBar` as the single owner of variant styling.
- Add a prompt element for the compact variant so the field reads as `> query`.
- Reduce vertical height and padding for the compact input.
- Shift compact styling from soft rounded-card form language toward terminal-panel language.
- Tighten the dropdown border/shadow treatment so it visually matches the compact input.

## Testing

- Add compact regression assertions for:
  - terminal prompt presence
  - compact height and edge treatment
  - restrained terminal-style shadow/border treatment
- Preserve existing behavior tests for search results, routing, and feedback.

## Risks

- Style assertions can be too brittle if they target too many Tailwind classes; tests should focus only on the classes that define the intended terminal regression boundary.
- Compact-only prompt markup must not interfere with input focus or result dropdown positioning.
