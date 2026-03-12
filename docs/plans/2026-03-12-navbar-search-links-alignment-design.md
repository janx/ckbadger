# Navbar Search And Stats Alignment Design

## Goal

- Reposition the global search bar to the left side of the navbar.
- Move desktop navigation links to the right edge.
- Align the stats line left edge with the search bar left edge.
- Apply the same left-search/right-links intent to the mobile menu layout.

## Context

- `frontend/components/layout/header.tsx` currently renders `Logo`, then desktop links, then the desktop search bar.
- The stats line uses a separate container row with `md:pl-[112px]`, so its left start point follows the logo offset instead of the desktop search bar.
- Mobile menu content renders the search bar first and the links as a left-aligned stacked list.

## Requirements

- Desktop:
  - `Logo` remains at the far left.
  - `SearchBar` moves to the left group immediately after the logo.
  - Desktop links move to the right side and stay right-aligned.
  - Stats line shares the same left baseline as the desktop search bar.
- Mobile:
  - Search remains above the links.
  - Links become right-aligned in the expanded menu.

## Approach

- Keep all routing and search behavior unchanged.
- Reorder the desktop header flex layout to `Logo + search + nav`.
- Use shared spacing values in `header.tsx` so the stats row and desktop search row start from the same horizontal offset.
- Limit test scope to layout structure and class regressions in `frontend/__tests__/components/header.test.tsx`.

## Testing

- Add regression assertions for:
  - desktop search container living in the left-aligned flex group
  - desktop nav using right alignment instead of fixed left padding
  - stats row using the same left padding as the search baseline
  - mobile menu links using right alignment

## Risks

- Header tests currently assert the old desktop structure and will need to be updated before implementation.
- Layout changes must not affect search variant selection or link active-state styling.
