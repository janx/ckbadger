# Navbar Search Unification Design

## Goal

- Use the compact navbar search style on the homepage as well as other pages.
- Unify all `SearchBar` placeholder copy to `Search block / tx / address / cell ...`.
- Make the compact navbar search more visually prominent.
- Keep the stats line left-aligned with the navbar search baseline.

## Context

- `frontend/components/layout/header.tsx` currently uses `variant="home"` on `/` and `variant="compact"` elsewhere.
- `frontend/components/search-bar.tsx` gives `home`, `compact`, and `default` different placeholder copy and different visual treatments.
- The compact variant is currently much smaller and less visually prominent than the homepage version.
- The stats row already has a dedicated desktop offset in `header.tsx`; this needs to remain tied to the active navbar search baseline.

## Requirements

- Header search on desktop and mobile should always use the compact variant.
- Placeholder text should be identical across all `SearchBar` variants.
- Compact styling should remain compact relative to the default variant, but become more visible in the navbar.
- Search behavior, query routing, result dropdowns, and keyboard handling must remain unchanged.
- Stats row alignment must continue to follow the same left desktop offset as the search bar.

## Approach

- Keep `SearchBar` as the single owner of placeholder and variant-specific styling.
- Introduce a shared placeholder constant so all variants use the same copy.
- Restyle the compact variant with a stronger border, taller control, and subtle persistent shadow.
- Update `Header` so both desktop and mobile menu search instances always request the compact variant.
- Preserve the existing shared desktop offset constant in `header.tsx` so the stats row stays locked to the search baseline.

## Testing

- Add regression coverage in `frontend/__tests__/components/search-bar.test.tsx` for:
  - shared placeholder text across variants
  - the more prominent compact styling
- Update `frontend/__tests__/components/header.test.tsx` to assert:
  - homepage header now uses the compact variant
  - stats row still uses the shared desktop search offset

## Risks

- Styling assertions can become brittle if they overfit exact Tailwind class strings; tests should focus on the few classes that define the intended regression boundary.
- The home variant still exists after this change, so placeholder unification must not depend on current callers.
