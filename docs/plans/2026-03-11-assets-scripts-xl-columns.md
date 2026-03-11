# Assets & Scripts xl-Width Column Enhancement — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add HMul, Circulation, and script cell count columns visible at xl breakpoint on the assets and scripts list pages.

**Architecture:** API adds `h_multiplier` field to `AssetResponse` (computed from existing capacity data) and `live_cells_count`/`cells_count` fields to `ScriptResponse` (from existing `ScriptInfo`). Frontend adds xl-only columns to both pages. No new DB schema or store changes.

**Tech Stack:** Rust (Axum API), TypeScript/React (frontend), Vitest (frontend tests)

**Design doc:** `docs/plans/2026-03-11-assets-scripts-xl-columns-design.md`

---

### Task 1: Add `h_multiplier` to AssetResponse (API)

**Files:**

- Modify: `crates/api/src/routes/assets.rs:217-241` (AssetResponse struct)
- Modify: `crates/api/src/warmup.rs:126-153` (CachedAssetEntry::to_asset_response)

**Step 1: Add field to AssetResponse**

In `crates/api/src/routes/assets.rs`, add after `fully_onchain_count`:

```rust
pub h_multiplier: Option<f64>,
```

**Step 2: Compute h_multiplier in to_asset_response**

In `crates/api/src/warmup.rs`, in `to_asset_response()`, replace the static field mapping with computed value:

```rust
h_multiplier: {
    match (&self.live_capacity, &self.live_occupied_capacity) {
        (Some(cap_str), Some(occ_str)) => {
            let cap: f64 = cap_str.parse().unwrap_or(0.0);
            let occ: f64 = occ_str.parse().unwrap_or(0.0);
            if occ > 0.0 {
                Some(((cap / occ) * 100.0).round() / 100.0)
            } else {
                None
            }
        }
        _ => None,
    }
},
```

**Step 3: Add `h_multiplier` sort key**

In `crates/api/src/routes/assets.rs`, add `HMultiplier` variant to `AssetSortKey` enum and add the sort comparison in `compare_asset_entries`:

```rust
// In AssetSortKey enum:
HMultiplier,

// In compare_asset_entries match:
AssetSortKey::HMultiplier => {
    let left_hm = compute_h_multiplier(left);
    let right_hm = compute_h_multiplier(right);
    apply_direction(left_hm.partial_cmp(&right_hm).unwrap_or(Ordering::Equal), direction)
}
```

Add helper:

```rust
fn compute_h_multiplier(entry: &CachedAssetEntry) -> f64 {
    match (&entry.live_capacity, &entry.live_occupied_capacity) {
        (Some(cap_str), Some(occ_str)) => {
            let cap: f64 = cap_str.parse().unwrap_or(0.0);
            let occ: f64 = occ_str.parse().unwrap_or(0.0);
            if occ > 0.0 { cap / occ } else { 0.0 }
        }
        _ => 0.0,
    }
}
```

**Step 4: Run `cargo check`**

Run: `cargo check -p ckbadger-api`
Expected: PASS (no compilation errors)

**Step 5: Commit**

```
feat(api): add h_multiplier field to asset response
```

---

### Task 2: Add `live_cells_count` and `cells_count` to ScriptResponse (API)

**Files:**

- Modify: `crates/api/src/routes/scripts.rs:100-123` (ScriptResponse struct)
- Modify: `crates/api/src/routes/scripts.rs:425-450` (script_info_to_response)

**Step 1: Add fields to ScriptResponse**

In `ScriptResponse`, add after `live_occupied_capacity_sum`:

```rust
pub live_cells_count: i64,
pub cells_count: i64,
```

**Step 2: Populate fields in script_info_to_response**

In `script_info_to_response`, add to the `Ok(ScriptResponse { ... })` block:

```rust
live_cells_count: info.lock_live_cells_count + info.type_live_cells_count,
cells_count: info.lock_cells_count + info.type_cells_count,
```

**Step 3: Add sort keys for cell counts**

Add `LiveCells` and `Cells` variants to `ScriptSortKey`:

```rust
LiveCells,
Cells,
```

Add cases in `compare_script_entries`:

```rust
ScriptSortKey::LiveCells => apply_direction(
    (left.1.lock_live_cells_count + left.1.type_live_cells_count)
        .cmp(&(right.1.lock_live_cells_count + right.1.type_live_cells_count)),
    direction,
),
ScriptSortKey::Cells => apply_direction(
    (left.1.lock_cells_count + left.1.type_cells_count)
        .cmp(&(right.1.lock_cells_count + right.1.type_cells_count)),
    direction,
),
```

**Step 4: Run `cargo check`**

Run: `cargo check -p ckbadger-api`
Expected: PASS

**Step 5: Commit**

```
feat(api): add live_cells_count and cells_count to script response
```

---

### Task 3: Add `hMultiplier` to frontend Asset type and display HM column

**Files:**

- Modify: `frontend/lib/api.ts:623-652` (Asset interface)
- Modify: `frontend/app/assets/assets-page-client.tsx` (AssetTable component)

**Step 1: Add `hMultiplier` to Asset interface**

In `frontend/lib/api.ts`, add to `Asset` interface after `fullyOnchainCount`:

```typescript
hMultiplier: number | null;
```

**Step 2: Add `hMultiplier` sort key**

In `frontend/app/assets/assets-page-client.tsx`, add `'hMultiplier'` to `AssetSortKey` type:

```typescript
type AssetSortKey =
  | 'name'
  | 'type'
  | 'supply'
  | 'transfers24h'
  | 'holders'
  | 'transfers'
  | 'occupied'
  | 'capacity'
  | 'onchainRatio'
  | 'hMultiplier';
```

**Step 3: Add HM column header in table header row**

In the table header `div` (line ~289), inside the `hidden xl:contents` block, add after the Occupied header:

```tsx
{
  renderSortHeader('hMultiplier', 'HM', capacityColumnClass, 'right');
}
```

**Step 4: Add HM column cell in table row**

In the table row `div` (line ~304), inside the `hidden xl:block` wrapper, add after the Occupied cell:

```tsx
<div className={`${capacityColumnClass} text-text-secondary font-mono tabular-nums`}>
  {asset.hMultiplier != null ? (
    <span title={`H-Multiplier: capacity / occupied = ×${asset.hMultiplier.toFixed(2)}`}>
      ×{asset.hMultiplier.toFixed(2)}
    </span>
  ) : (
    <span className="text-text-muted">-</span>
  )}
</div>
```

**Step 5: Run `cd frontend && npx vitest run`**

Expected: Existing tests pass (mock data doesn't include hMultiplier, but it's optional/null)

**Step 6: Commit**

```
feat(frontend): add HM column to assets table at xl width
```

---

### Task 4: Add Circulation column for Tokens tab

**Files:**

- Modify: `frontend/app/assets/assets-page-client.tsx` (AssetTable component)

**Step 1: Add a formatTokenAmount helper**

Add at top of file (after imports), reusing the same logic from `frontend/app/tokens/[typeHash]/client-page.tsx`:

```typescript
function formatTokenSupply(totalSupply: string | null, decimals: number | null): string | null {
  if (!totalSupply) return null;
  if (decimals == null || decimals === 0) {
    return new Intl.NumberFormat().format(BigInt(totalSupply));
  }
  const num = BigInt(totalSupply);
  const divisor = BigInt(10 ** decimals);
  const integer = (num / divisor).toString();
  const remainder = num % divisor;
  const formatted = new Intl.NumberFormat().format(BigInt(integer));
  if (remainder === 0n) return formatted;
  const decimal = remainder.toString().padStart(decimals, '0').replace(/0+$/, '');
  return `${formatted}.${decimal}`;
}
```

**Step 2: Add Circulation column header**

In the table header row, add Circulation header only when `assetType === 'token'`, inside the `hidden xl:contents` block, before the Occupied header:

```tsx
{
  assetType === 'token' && (
    <div className="hidden xl:contents">
      {renderSortHeader('supply', 'Circulation', capacityColumnClass, 'right')}
    </div>
  );
}
```

**Step 3: Add Circulation column cell**

In the table row, add after Standard column, only when token tab:

```tsx
{
  assetType === 'token' && (
    <div className="hidden xl:block">
      <div className={`${capacityColumnClass} text-text-primary font-mono tabular-nums`}>
        {(() => {
          const formatted = formatTokenSupply(asset.totalSupply, asset.decimals);
          return formatted ? (
            <span title={`Total Circulation: ${formatted}`}>{formatted}</span>
          ) : (
            <span className="text-text-muted">-</span>
          );
        })()}
      </div>
    </div>
  );
}
```

**Step 4: Run `cd frontend && npx vitest run`**

Expected: PASS

**Step 5: Commit**

```
feat(frontend): add Circulation column to tokens tab at xl width
```

---

### Task 5: Add cell count columns and deployed_at to scripts page

**Files:**

- Modify: `frontend/lib/api.ts:1035-1056` (KnownScript interface)
- Modify: `frontend/app/scripts/page.tsx` (ScriptsPage component)

**Step 1: Add new fields to KnownScript interface**

In `frontend/lib/api.ts`, add to `KnownScript` after `liveOccupiedCapacitySum`:

```typescript
liveCellsCount?: number;
cellsCount?: number;
```

**Step 2: Add sort keys to ScriptSortKey type**

In `frontend/app/scripts/page.tsx`, update:

```typescript
type ScriptSortKey =
  | 'name'
  | 'kind'
  | 'description'
  | 'occupied'
  | 'capacity'
  | 'liveCells'
  | 'cells'
  | 'deployed';
```

**Step 3: Add xl-only columns to the header row**

In the table header `div` (line ~202), wrap existing headers and add new xl-only ones. The new columns should appear between Description and Occupied:

```tsx
<div className="hidden xl:contents">
  {renderSortHeader('liveCells', 'Live Cells', 'w-24 shrink-0', 'right')}
  {renderSortHeader('cells', 'Total Cells', 'w-24 shrink-0', 'right')}
  {renderSortHeader('deployed', 'Deployed', 'w-24 shrink-0', 'right')}
</div>
```

**Step 4: Add xl-only columns to the data rows**

In the table row `div` (line ~212), between the Description cell and Occupied cell:

```tsx
<div className="hidden xl:contents">
  <div className="text-text-secondary w-24 shrink-0 text-right font-mono tabular-nums">
    {script.liveCellsCount != null ? new Intl.NumberFormat().format(script.liveCellsCount) : '-'}
  </div>
  <div className="text-text-muted w-24 shrink-0 text-right font-mono tabular-nums">
    {script.cellsCount != null ? new Intl.NumberFormat().format(script.cellsCount) : '-'}
  </div>
  <div className="text-text-muted w-24 shrink-0 text-right font-mono tabular-nums">
    {script.deployedAt != null ? (
      <AppLink
        href={`/blocks/${script.deployedAt}`}
        className="hover:text-emphasis hover:underline"
      >
        #{new Intl.NumberFormat().format(script.deployedAt)}
      </AppLink>
    ) : (
      '-'
    )}
  </div>
</div>
```

**Step 5: Run `cd frontend && npx vitest run`**

Expected: PASS

**Step 6: Commit**

```
feat(frontend): add cell count and deployed columns to scripts page at xl width
```

---

### Task 6: Update tests for new columns

**Files:**

- Modify: `frontend/__tests__/pages/assets.test.tsx`
- Modify: `frontend/__tests__/pages/scripts.test.tsx`

**Step 1: Add `hMultiplier` to mock data in assets test**

Add `hMultiplier: 2.0` to `mockTokenAssets.data[0]` and appropriate values to other mocks (or `null`).

**Step 2: Add test for HM column rendering**

```typescript
it('renders HM column at xl width with formatted multiplier', async () => {
  vi.mocked(api.getAssets).mockResolvedValue({
    ...mockTokenAssets,
    data: [{ ...mockTokenAssets.data[0], hMultiplier: 2.0 }],
  });

  render(<AssetsPage />);

  await waitFor(() => {
    expect(screen.getByRole('button', { name: 'Sort by HM' })).toBeInTheDocument();
    expect(screen.getByText('×2.00')).toBeInTheDocument();
  });
});
```

**Step 3: Add test for Circulation column**

```typescript
it('renders Circulation column for tokens tab at xl width', async () => {
  vi.mocked(api.getAssets).mockResolvedValue({
    ...mockTokenAssets,
    data: [{ ...mockTokenAssets.data[0], hMultiplier: 2.0 }],
  });

  render(<AssetsPage />);

  await waitFor(() => {
    expect(screen.getByRole('button', { name: 'Sort by Circulation' })).toBeInTheDocument();
  });
});
```

**Step 4: Add `liveCellsCount` and `cellsCount` to mock data in scripts test**

Add to `mockScriptsResponse.data` entries:

```typescript
liveCellsCount: 5000,
cellsCount: 12000,
```

**Step 5: Add test for script cell count columns**

```typescript
it('renders cell count columns at xl width', async () => {
  vi.mocked(api.getScripts).mockResolvedValue({
    ...mockScriptsResponse,
    data: mockScriptsResponse.data.map((s) => ({
      ...s,
      liveCellsCount: 5000,
      cellsCount: 12000,
    })),
  });

  render(<ScriptsPage />);

  await waitFor(() => {
    expect(screen.getByRole('button', { name: 'Sort by Live Cells' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sort by Total Cells' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sort by Deployed' })).toBeInTheDocument();
  });
});
```

**Step 6: Run all tests**

Run: `cd frontend && npx vitest run`
Expected: ALL PASS

**Step 7: Run Rust tests**

Run: `cargo test -p ckbadger-api`
Expected: PASS (existing tests + any compile fixes for new fields in test data)

**Step 8: Commit**

```
test: add tests for xl-width asset HM/Circulation and script cell count columns
```

---

### Task 7: Add skeleton loaders for new columns

**Files:**

- Modify: `frontend/app/assets/assets-page-client.tsx` (skeleton section)
- Modify: `frontend/app/scripts/page.tsx` (skeleton section)

**Step 1: Add skeleton placeholders for HM and Circulation in assets loading state**

In the assets skeleton (line ~238), inside the `hidden xl:contents` equivalent area, add skeleton divs matching the new column widths.

**Step 2: Add skeleton placeholders for cell count columns in scripts loading state**

In the scripts skeleton (line ~159), add xl-only skeleton divs for Live Cells, Total Cells, and Deployed columns.

**Step 3: Run `cd frontend && npx vitest run`**

Expected: PASS

**Step 4: Commit**

```
feat(frontend): add skeleton loaders for new xl-width columns
```

---

### Task 8: Final verification

**Step 1: Run full pre-commit checks**

Run: `cargo check && cargo clippy`
Run: `cd frontend && pnpm type-check && pnpm lint`

**Step 2: Fix any issues**

**Step 3: Run all tests**

Run: `cargo test -p ckbadger-api && cd frontend && npx vitest run`
Expected: ALL PASS

**Step 4: Final commit (if fixes needed)**

---

## Design notes

- **Flags column (System/Deprecated) skipped**: `deprecated` scripts are excluded during label import. `is_system` has no data source — hardcoded `false`. Both would render as empty for all rows. Can be added later when data becomes available.
- **HMul sort**: Sorted as f64; entries with no capacity data sort to bottom (0.0).
- **Circulation column**: Only visible in Tokens tab. Reuses `supply` sort key (already mapped to `totalSupply` on backend).
- **`deployed` sort key**: New backend sort key needed — add `Deployed` to `ScriptSortKey` enum in `scripts.rs` and sort by `deployed_at` field.
