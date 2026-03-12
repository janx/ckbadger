# Header Start Column and Logo Position Design

## Goal

- Fix the remaining desktop misalignment between the search prompt `>` and the first stats label.
- Enlarge the logo slightly and move it a bit left and down.

## Context

- `frontend/components/layout/header.tsx` currently aligns desktop search and stats using ad-hoc left padding.
- The stats row no longer has its own `>` prompt, so the first visible character in the row is the `b` in `block`.
- The desktop logo is absolutely positioned, which means it does not reserve layout width for the search and stats rows.
- `frontend/components/layout/logo.tsx` currently uses a 100px-wide image and earlier top/left offsets.

## Requirements

- Desktop search and stats must share the same structural start column.
- The `b` in `block` should sit under the same vertical line as the search prompt `>`.
- The fix should not rely on another magic padding number tied to the current visual state.
- The desktop logo should be a little larger and shifted slightly left and down.
- Search behavior, stats content, nav layout, and mobile menu behavior must remain unchanged.

## Approach

- Introduce an explicit desktop start-column spacer in the header layout so desktop search and stats both start after the same reserved logo column.
- Remove the old desktop left-padding offset from search and stats rows.
- Keep the actual logo absolutely positioned for the visual effect, but reserve layout width separately through the shared desktop start column.
- Update the logo’s responsive classes so desktop gets a larger image and slightly adjusted offsets while mobile stays close to the current feel.

## Testing

- Update `frontend/__tests__/components/header.test.tsx` to assert:
  - the desktop search wrapper no longer uses the old padding offset
  - the search and stats rows each render the shared desktop start-column spacer
  - the stats row no longer uses the old padding offset contract
- Add `frontend/__tests__/components/logo.test.tsx` to assert:
  - the logo link uses the new responsive desktop positioning classes
  - the logo image uses the larger responsive width classes

## Risks

- Because the real logo is absolute while tests mock it in normal flow, the layout contract must be tested through the shared spacer rather than by DOM geometry.
- The desktop logo slot width must be large enough for the enlarged logo without unnecessarily shrinking the search area.
