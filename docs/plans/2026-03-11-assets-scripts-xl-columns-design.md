# Assets & Scripts Page xl-Width Column Enhancement

## Goal

Progressively show more useful information on the assets and scripts list pages at xl (1280px) width, using existing API data and one new computed field.

## Principle Alignment

- **CKB Native**: H-Multiplier is a CKB-native metric showing cell capacity utilization efficiency
- **Local First**: No new external data dependencies
- **Agent Friendly**: Clear, sortable columns with machine-readable values

## Assets Page

### New columns at xl

| Column                | Tab visibility  | Data source                         | Format                 |
| --------------------- | --------------- | ----------------------------------- | ---------------------- |
| **HM** (H-Multiplier) | All tabs        | New API field `hMultiplier`         | `×1.23`                |
| **Circulation**       | Tokens tab only | Existing `totalSupply` + `decimals` | Formatted token amount |

### Column order

**Tokens tab (xl):** Name / Standard / Circulation / 24h Txns / Holders / Occupied / HM / Capacity

**Objects & Identities tab (xl):** Name / Standard / Items / 24h Txns / Holders / Occupied / HM / Capacity

### API change

Add `hMultiplier: Option<f64>` to `AssetResponse`:

- Computed as `live_capacity / live_occupied_capacity`
- Returns `None` when `live_occupied_capacity` is zero or absent
- Rounded to 2 decimal places

### HM semantics

- `1.00` = cell stores exactly the minimum required capacity
- Higher values = more excess CKB locked in cells
- Useful for understanding capital efficiency of asset collections

## Scripts Page

### New xl breakpoint

Currently scripts page has no xl-specific layout (xl = md). Add xl breakpoint with additional columns.

| Priority | Column          | Data source              | Notes                               |
| -------- | --------------- | ------------------------ | ----------------------------------- |
| 1        | **Live Cells**  | Existing API data        | Active cell count                   |
| 2        | **Total Cells** | Existing API data        | Historical cell count               |
| 3        | **Flags**       | `isSystem`, `deprecated` | Badge display, shown only when true |
| 4        | **Deployed**    | `deployedAt`             | Block number, links to block page   |

### Column order (xl)

Script Name / Kind / Flags / Description / Live Cells / Total Cells / Deployed / Occupied / Capacity

## Out of scope

- No new breakpoints (2xl/3xl)
- No backend API changes for scripts (all data already returned)
- No changes to mobile card layouts
- No changes to md/lg breakpoint layouts
