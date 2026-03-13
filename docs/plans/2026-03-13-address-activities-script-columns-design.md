# Address Activities Script Columns Design

**Goal:** Make script calls visibly independent from asset changes on the address detail activity table.

## Problem

The address detail page already receives `assetChanges` and `scriptCalls` as separate API fields, but the desktop activity table still renders both inside a single `Assets` column. That collapses two different activity dimensions into one presentation slot and makes script calls look like a subtype of asset changes.

## Design

### Desktop activity layout

- Split the current desktop `Assets` column into two columns: `Assets` and `Scripts`.
- Render `activity.assetChanges` only in the `Assets` column.
- Render `activity.scriptCalls` only in the `Scripts` column.
- Leave a column visually empty when that activity has no entries for that dimension.

### Mobile activity layout

- Keep the existing stacked mobile layout.
- Continue rendering `Assets` and `Scripts` as separate sections because it already matches the intended semantics.

### Scope boundary

- Frontend only.
- No API response changes.
- No store/indexer changes.
- No activity classification changes.

## Testing

- Add a regression test for the address page that covers an activity containing both a token asset change and a script call.
- Verify the desktop table exposes independent `Assets` and `Scripts` headers.
- Verify the script call link still resolves to the script detail route.

## Result

The address detail page will present asset changes and script calls as two independent activity dimensions, matching the existing API model and the project activity semantics.
