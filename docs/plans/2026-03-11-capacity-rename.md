# Capacity Terminology Rename Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rename capacity terminology from "Occupied/Unoccupied/Occupation" to "Used/Unused/Statistics" across the full stack, and replace Capacity Utilization with H-Multiplier on Script/Asset pages.

**Architecture:** Mechanical rename across 4 layers (store types → indexer writers → API responses → frontend), plus a new H-Multiplier display component for Script/Asset pages. Cell/Address pages keep "Capacity Utilization" concept with updated sub-labels.

**Tech Stack:** Rust (serde structs, API routes), TypeScript/React (components, pages, tests)

---

## Terminology Mapping

| Old                                    | New                               | Scope                    |
| -------------------------------------- | --------------------------------- | ------------------------ |
| Occupied Capacity                      | Used Capacity                     | All display text         |
| Unoccupied                             | Unused                            | All display text         |
| Capacity & Occupation                  | Capacity Statistics               | Section headers          |
| Occupation History                     | Capacity History                  | Section headers          |
| Total Capacity (label)                 | Cells Capacity                    | Script/Asset pages only  |
| Capacity Utilization bar               | H-Multiplier display              | Script/Asset pages only  |
| `occupiedCapacity`                     | `usedCapacity`                    | All variable/field names |
| `liveOccupiedCapacity`                 | `liveUsedCapacity`                | All variable/field names |
| `occupiedCapacitySum`                  | `usedCapacitySum`                 | All variable/field names |
| `liveOccupiedCapacitySum`              | `liveUsedCapacitySum`             | All variable/field names |
| `totalOccupiedCapacity`                | `totalUsedCapacity`               | All variable/field names |
| `OccupationRangeKey`                   | `CapacityRangeKey`                | Type names               |
| `CapacityOccupationSection`            | `CapacityStatisticsSection`       | Component names          |
| `OccupationRangeSelector`              | `CapacityRangeSelector`           | Component names          |
| `*OccupationChart*` (API methods)      | `*CapacityChart*`                 | API method names         |
| `occupation-range.ts`                  | `capacity-range.ts`               | File names               |
| `capacity-occupation-section.tsx`      | `capacity-statistics-section.tsx` | File names               |
| `occupation-range-selector.tsx`        | `capacity-range-selector.tsx`     | File names               |
| Chart series `"occupied"/"unoccupied"` | `"used"/"unused"`                 | API + frontend           |
| URL `charts/occupation`                | `charts/capacity-history`         | API routes               |
| Route `cell-age-vs-occupied-capacity`  | `cell-age-vs-used-capacity`       | Frontend route           |

## H-Multiplier (HODL Multiplier)

- Replaces CapacityUtilization bar on Script/Asset pages (tokens, clusters, spores/objects, scripts)
- Formula: `HMul = Cells Capacity / Used Capacity` (ratio, e.g. "1.23x")
- Abbreviated as "HMul" in display
- Interpretation: HMul close to 1.0x means cells are tightly packed; higher means more spare capacity

## Pages & Their Behavior

| Page                            | Capacity Utilization Bar                   | H-Multiplier | Section Header        |
| ------------------------------- | ------------------------------------------ | ------------ | --------------------- |
| Cell `/cell/[outpoint]`         | Keep (inline, labeled "Utilization Ratio") | No           | N/A (inline)          |
| Address `/address/[addr]`       | Keep (labeled "Capacity Utilization")      | No           | N/A (inline)          |
| Token `/tokens/[typeHash]`      | Remove                                     | Yes          | "Capacity Statistics" |
| Script `/scripts/[name]`        | Remove                                     | Yes          | "Capacity Statistics" |
| Script `/script/[codeHash]`     | Remove                                     | Yes          | N/A (inline in panel) |
| Cluster `/clusters/[clusterId]` | Remove                                     | Yes          | "Capacity Statistics" |
| Object `/objects/[sporeId]`     | Remove                                     | Yes          | "Capacity Statistics" |
| Object Collection stat cards    | Rename labels only                         | No           | N/A                   |

---

## Task 1: Backend Store Types Rename

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs`
- Modify: `crates/ckbadger-store/src/cell_ops.rs`

**Renames in `types.rs`:**

- `ScriptInfo.lock_occupied_capacity_sum` → `lock_used_capacity_sum`
- `ScriptInfo.lock_live_occupied_capacity_sum` → `lock_live_used_capacity_sum`
- `ScriptInfo.type_occupied_capacity_sum` → `type_used_capacity_sum`
- `ScriptInfo.type_live_occupied_capacity_sum` → `type_live_used_capacity_sum`
- `ScriptDailyDelta.live_occupied_capacity_delta` → `live_used_capacity_delta`
- `TokenDailyDelta.live_occupied_capacity_delta` → `live_used_capacity_delta`
- `ClusterDailyDelta.live_occupied_capacity_delta` → `live_used_capacity_delta`
- `SporeDailyDelta.live_occupied_capacity_delta` → `live_used_capacity_delta`
- `ObjectDailyDelta.live_occupied_capacity_delta` → `live_used_capacity_delta`

**Renames in `cell_ops.rs`:**

- `TokenCellStats.total_occupied_capacity` → `total_used_capacity`
- All references to `total_occupied_capacity` in the file

**Important:** These are serde-serialized to RocksDB. Rename requires DB rebuild (acceptable per project policy).

**Commit:** `refactor(store): rename occupied_capacity fields to used_capacity`

---

## Task 2: Backend Indexer Writer Rename

**Files:** All files under `crates/indexer/src/` that reference the renamed store fields.

Search for `occupied_capacity` across all indexer source files and rename to `used_capacity`. Key files:

- `crates/indexer/src/db/writer/*.rs` (14 modules)
- `crates/indexer/src/parser/*.rs`

Also rename any local variables named `occupied` to `used` where they refer to capacity.

**Commit:** `refactor(indexer): rename occupied_capacity references to used_capacity`

---

## Task 3: Backend API Response Types + Chart Builders

**Files:**

- Modify: `crates/api/src/routes/scripts.rs`
- Modify: `crates/api/src/routes/tokens.rs`
- Modify: `crates/api/src/routes/assets.rs`
- Modify: `crates/api/src/routes/spore.rs`
- Modify: `crates/api/src/routes/search.rs`
- Modify: `crates/api/src/routes/charts.rs` (if exists)

**Response type field renames** (serde camelCase → JSON):

- `live_occupied_capacity` → `live_used_capacity` (AssetResponse, ClusterResponse, SporeResponse, NftCollectionDetailResponse)
- `occupied_capacity_sum` → `used_capacity_sum` (ScriptUsageResponse, DeploymentUsage)
- `live_occupied_capacity_sum` → `live_used_capacity_sum` (ScriptResponse, ScriptUsageResponse, DeploymentUsage, CodeCellResponse)
- `total_occupied_capacity` → `total_used_capacity` (TokenResponse)

**Chart series renames:**

- `"occupied"` → `"used"` (in HashMap keys)
- `"unoccupied"` → `"unused"` (in HashMap keys)
- `"Occupied"` → `"Used"` (series labels)
- `"Unoccupied"` → `"Unused"` (series labels)

**Function renames:**

- `build_capacity_occupation_chart*` → `build_capacity_history_chart*` (in assets.rs, spore.rs)
- `build_script_occupation_chart` → `build_script_capacity_history_chart`
- `get_token_occupation_chart` → `get_token_capacity_chart`
- `get_*_occupation_chart` → `get_*_capacity_chart` (all variants)

**URL route renames:**

- `/tokens/{type_hash}/charts/occupation` → `/tokens/{type_hash}/charts/capacity-history`
- `/scripts/{name}/charts/occupation` → `/scripts/{name}/charts/capacity-history`
- `/scripts/charts/occupation` → `/scripts/charts/capacity-history`
- `/spore/clusters/{cluster_id}/charts/occupation` → `/spore/clusters/{cluster_id}/charts/capacity-history`
- `/spore/objects/{spore_id}/charts/occupation` → `/spore/objects/{spore_id}/charts/capacity-history`
- `/assets/objects/{collection_id}/charts/occupation` → `/assets/objects/{collection_id}/charts/capacity-history`

**Local variable renames** in chart builder functions:

- `cumulative_occupied` → `cumulative_used`
- `initial_occupied` → `initial_used`
- `occupied_delta` → `used_delta`
- `base_occupied` → `base_used`
- `unoccupied` → `unused`

**Commit:** `refactor(api): rename occupied/occupation to used/capacity-history`

---

## Task 4: Backend Integration Tests

**Files:**

- Modify: `crates/api/tests/api_integration.rs`

Rename all `test_*_occupation_chart_*` test functions and update:

- URL paths from `charts/occupation` to `charts/capacity-history`
- JSON field assertions from `occupied`/`unoccupied` to `used`/`unused`
- Response field assertions from `liveOccupiedCapacity*` to `liveUsedCapacity*` etc.

**Commit:** `test(api): update integration tests for capacity rename`

---

## Task 5: Frontend Library Renames

**Files:**

- Rename: `frontend/lib/occupation-range.ts` → `frontend/lib/capacity-range.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/ai/markdown-renderer.ts`
- Modify: `frontend/lib/ai/markdown-route.ts`
- Modify: `frontend/components/charts/chart-calculation-descriptions.ts`

**In `capacity-range.ts`** (formerly `occupation-range.ts`):

- `OccupationRangeKey` → `CapacityRangeKey`
- `OCCUPATION_RANGE_OPTIONS` → `CAPACITY_RANGE_OPTIONS`
- `getOccupationRangeParams` → `getCapacityRangeParams`

**In `api.ts`:**

- All `occupiedCapacity` fields → `usedCapacity`
- All `liveOccupiedCapacity` fields → `liveUsedCapacity`
- All `occupiedCapacitySum` fields → `usedCapacitySum`
- All `liveOccupiedCapacitySum` fields → `liveUsedCapacitySum`
- All `totalOccupiedCapacity` fields → `totalUsedCapacity`
- `OccupiedCapacityBreakdown` → `UsedCapacityBreakdown`
- `virtualOccupiedCapacity` → `virtualUsedCapacity`
- API method renames: `getTokenOccupationChart` → `getTokenCapacityChart`, etc.
- API URL paths: `charts/occupation` → `charts/capacity-history`

**In `markdown-renderer.ts`:**

- `occupiedCapacity` → `usedCapacity`
- `liveOccupiedCapacity` → `liveUsedCapacity`
- `liveOccupiedCapacitySum` → `liveUsedCapacitySum`
- `occupiedCapacitySum` → `usedCapacitySum`
- Display text: "occupied capacity" → "used capacity"
- `getCellAgeVsOccupiedCapacityChart` → `getCellAgeVsUsedCapacityChart`

**In `markdown-route.ts`:**

- `cell-age-vs-occupied-capacity` → `cell-age-vs-used-capacity`

**In `chart-calculation-descriptions.ts`:**

- All "occupied capacity" → "used capacity"
- All "occupied/unoccupied" → "used/unused"
- Chart key `chart-cell-age-vs-occupied-capacity` → `chart-cell-age-vs-used-capacity`
- "utilization" → update context-appropriately

**Commit:** `refactor(frontend): rename occupation/occupied in lib files`

---

## Task 6: Frontend Component Renames

**Files:**

- Rename: `frontend/components/ui/capacity-occupation-section.tsx` → `capacity-statistics-section.tsx`
- Rename: `frontend/components/ui/occupation-range-selector.tsx` → `capacity-range-selector.tsx`
- Modify: `frontend/components/ui/capacity-utilization.tsx`
- Modify: `frontend/components/object/object-collection-stat-cards.tsx`

**In `capacity-statistics-section.tsx`** (formerly `capacity-occupation-section.tsx`):

- Component: `CapacityOccupationSection` → `CapacityStatisticsSection`
- Props: `CapacityOccupationSectionProps` → `CapacityStatisticsSectionProps`
- `occupationRange` → `capacityRange`
- `onOccupationRangeChange` → `onCapacityRangeChange`
- `occupationChart` → `capacityChart`
- `isOccupationChartLoading` → `isCapacityChartLoading`
- `occupiedCapacity` → `usedCapacity`
- Header text: `"Capacity & Occupation"` → `"Capacity Statistics"`
- Loading text: `"Loading occupation history..."` → `"Loading capacity history..."`
- Empty text: `"No occupation history yet"` → `"No capacity history yet"`
- Replace `<CapacityUtilization>` with new `<HMultiplier>` component
- Pass `totalLabel="Cells Capacity"` as default

**In `capacity-range-selector.tsx`** (formerly `occupation-range-selector.tsx`):

- Component: `OccupationRangeSelector` → `CapacityRangeSelector`
- Props: `OccupationRangeSelectorProps` → `CapacityRangeSelectorProps`
- Import path updates

**In `capacity-utilization.tsx`:**

- Props: `occupiedCapacity` → `usedCapacity`
- Display text: `"Occupied:"` → `"Used:"`
- Display text: `"Unoccupied:"` → `"Unused:"`
- Display text: `"occupied"` in ratio → `"used"`
- Variable: `occupiedRaw` → `usedRaw`, `occupied` → `used`, `unoccupied` → `unused`
- Keep `totalLabel` defaulting to `"Total Capacity"` (this component is used on Cell/Address pages)

**In `object-collection-stat-cards.tsx`:**

- `liveOccupiedCapacity` → `liveUsedCapacity`
- `occupied` → `used` (local var)
- `occupationPercent` → `usedPercent`
- Display: `"Occupied Capacity"` → `"Used Capacity"`
- Display: `"Occupied Ratio"` → `"Used Ratio"`

**Commit:** `refactor(frontend): rename capacity components and update labels`

---

## Task 7: New H-Multiplier Component

**Files:**

- Create: `frontend/components/ui/h-multiplier.tsx`

**Component:** `HMultiplier` — displays HODL Multiplier (Cells Capacity / Used Capacity)

```tsx
interface HMultiplierProps {
  totalCapacity: string;
  usedCapacity: string;
  totalLabel?: string;
  className?: string;
}

export function HMultiplier({
  totalCapacity,
  usedCapacity,
  totalLabel = 'Cells Capacity',
  className,
}: HMultiplierProps) {
  // Parse BigInt values
  // Calculate HMul = total / used (as float ratio)
  // Display: totalLabel value, Used value, HMul badge "1.23x"
  // Show bar visualization (used portion in gold, unused in gold/30)
}
```

Display layout:

- Top row: label (e.g. "Cells Capacity") + value (e.g. "1.23M CKB")
- Bar: used portion (gold) + unused portion (gold/30)
- Bottom row: "Used: X CKB" on left, "HMul: 1.23x" on right

**Commit:** `feat(frontend): add HMultiplier component for script/asset pages`

---

## Task 8: Frontend Page Updates

**Files:**

- Modify: `frontend/app/tokens/[typeHash]/client-page.tsx`
- Modify: `frontend/app/scripts/[name]/client-page.tsx`
- Modify: `frontend/app/script/[codeHash]/client-page.tsx`
- Modify: `frontend/app/clusters/[clusterId]/client-page.tsx`
- Modify: `frontend/app/objects/[sporeId]/client-page.tsx`
- Modify: `frontend/app/address/[addr]/client-page.tsx`
- Modify: `frontend/app/cell/[outpoint]/client-page.tsx`
- Modify: `frontend/app/charts/cell-age-vs-occupied-capacity/page.tsx` → rename directory
- Modify: `frontend/app/charts/page.tsx`
- Modify: `frontend/src/routes/router.tsx`

**For each Script/Asset page** (tokens, scripts, script, clusters, objects):

- Update imports: `CapacityOccupationSection` → `CapacityStatisticsSection`
- Update imports: `OccupationRangeSelector` → `CapacityRangeSelector`
- Update imports: occupation-range → capacity-range
- Rename state: `occupationRange` → `capacityRange`
- Rename variables: `occupationRangeParams` → `capacityRangeParams`
- Rename queries: `occupationChart` → `capacityChart`
- Update query keys: `*-occupation-chart` → `*-capacity-chart`
- Update API calls: `getTokenOccupationChart` → `getTokenCapacityChart` etc.
- Update description text: "occupation" → "capacity"
- For script/[codeHash]: replace `<CapacityUtilization>` with `<HMultiplier>`; rename "Occupation History" → "Capacity History"

**For Address page:**

- Keep "Capacity Utilization" label
- Change "Occupied:" → "Used:", "Unoccupied:" → "Unused:"
- Change variable `occupiedBig` → `usedBig`
- Change `occupiedCapacity` → `usedCapacity` in data access

**For Cell page:**

- Change "Occupied Capacity" label → "Used Capacity"
- Keep "Utilization Ratio" label
- Change variable names: `occupied` → `used`, `occupiedBytes` → `usedBytes`, `occupiedRatioPercent` → `usedRatioPercent`
- Change `occupiedCapacity` → `usedCapacity` in data access
- Change `occupiedCapacityBreakdown` → `usedCapacityBreakdown`
- "Virtual Occupied Capacity" → "Virtual Used Capacity"

**For Charts page + route:**

- Rename directory: `cell-age-vs-occupied-capacity/` → `cell-age-vs-used-capacity/`
- Update router.tsx path
- Update chart page component name
- Update charts/page.tsx query keys and href

**Commit:** `refactor(frontend): update all pages for capacity rename + HMultiplier`

---

## Task 9: Frontend Test Updates

**Files:**

- Rename: `frontend/__tests__/lib/occupation-range.test.ts` → `capacity-range.test.ts`
- Rename: `frontend/__tests__/pages/cell-age-vs-occupied-capacity.test.tsx` → `cell-age-vs-used-capacity.test.tsx`
- Modify: `frontend/__tests__/pages/object-detail.test.tsx`
- Modify: `frontend/__tests__/pages/token-detail.test.tsx`
- Modify: `frontend/__tests__/pages/script-code-hash.test.tsx`
- Modify: `frontend/__tests__/pages/script-detail.test.tsx`
- Modify: `frontend/__tests__/pages/cluster.test.tsx`
- Modify: `frontend/__tests__/pages/address.test.tsx`
- Modify: `frontend/__tests__/pages/common-knowledge-composition.test.tsx`
- Modify: `frontend/__tests__/components/stacked-area-chart.test.tsx`
- Modify: `frontend/__tests__/routes/detail-route-inputs.test.tsx`
- Modify: `frontend/__tests__/lib/tooling-config.test.ts`
- Modify: `frontend/__tests__/pages/assets.test.tsx`
- Modify: `frontend/__tests__/pages/identity-collection.test.tsx`
- Modify: `frontend/__tests__/pages/mnft-item-detail.test.tsx`
- Modify: `frontend/__tests__/lib/api.test.ts`
- Modify: `frontend/__tests__/lib/markdown-renderer.test.ts`
- Modify: `frontend/__tests__/lib/markdown-route.test.ts`
- Modify: `frontend/__tests__/msw/handlers.ts` (if mock API responses reference occupied)

Update all test assertions, mock data, and descriptions to match new terminology.

**Commit:** `test(frontend): update tests for capacity terminology rename`

---

## Task 10: LLM Discovery Docs

**Files:**

- Modify: `frontend/public/llms.txt`
- Modify: `frontend/public/llms-full.txt`

Update references to `cell-age-vs-occupied-capacity` → `cell-age-vs-used-capacity` and any "occupied" terminology.

**Commit:** `docs: update LLM discovery for capacity rename`

---

## Execution Notes

- **DB rebuild required** after Task 1+2 (store field renames)
- Tasks 1-4 (backend) must be done before Tasks 5-9 (frontend) since frontend depends on API field names
- Tasks 5, 6, 7 can be parallelized within frontend
- Task 8 depends on Tasks 5, 6, 7
- Task 9 depends on Task 8
- Run `cargo check && cargo clippy` after Tasks 1-4
- Run `pnpm type-check && pnpm lint && pnpm test` after Tasks 5-9
