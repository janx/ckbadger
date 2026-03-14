# Code Cells Display Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Show all code cells (live + consumed) for script deployments instead of a single outpoint, so users can see how many code cells exist and which are usable as `cell_dep`.

**Architecture:** New store method `list_all_cells_by_data_hash` returns all matching cells (live + consumed). New API endpoint `GET /scripts/code-cells` returns the full list. Existing lookup/list endpoints gain count fields. Frontend replaces single code cell display with expandable list on both script pages.

**Tech Stack:** Rust (store + API), TypeScript/React (frontend), RocksDB prefix scan, TanStack Query

**Design doc:** `docs/plans/2026-03-14-code-cells-display-design.md`

---

### Task 1: Store — `list_all_cells_by_data_hash`

**Files:**

- Modify: `crates/ckbadger-store/src/cell_ops.rs` (after `find_any_cell_by_data_hash` ~line 532)

**Step 1: Add method**

Add after the existing `find_any_cell_by_data_hash` method (~line 532):

```rust
/// List all cells (live and consumed) matching a data hash.
///
/// Returns cells sorted by creation block (ascending, matching index key order).
/// Each result includes a `bool` indicating whether the cell is live (`true`) or consumed (`false`).
/// Used by the code-cells endpoint to show all deployment cells for a script.
pub fn list_all_cells_by_data_hash(
    &self,
    data_hash: &[u8],
    cells_store: &CkbadgerStore,
) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo, bool)>> {
    let mut results = Vec::new();

    let iter = self.iterator_cf(
        self.cf_cell_by_data_hash(),
        rocksdb::IteratorMode::From(data_hash, rocksdb::Direction::Forward),
    );

    for item in iter {
        let (key, _) = item.map_err(|e| {
            anyhow::anyhow!(
                "failed to iterate cell index in list_all_cells_by_data_hash: {}",
                e
            )
        })?;
        if !key.starts_with(data_hash) {
            break;
        }
        if key.len() >= 74 {
            let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
            if let Some(cell) = self.get_cell(&tx_hash, output_index, cells_store)? {
                results.push((tx_hash, output_index, cell, true));
            } else if let Some(cell) =
                self.get_consumed_cell(&tx_hash, output_index, cells_store)?
            {
                results.push((tx_hash, output_index, cell, false));
            }
        }
    }

    Ok(results)
}
```

**Step 2: Run check**

```bash
cargo check -p ckbadger-store
```

**Step 3: Add test**

Add a test in `crates/ckbadger-store/src/cell_ops.rs` in the existing `#[cfg(test)] mod tests` block. Find it by searching for `mod tests` in that file.

```rust
#[test]
fn test_list_all_cells_by_data_hash_returns_live_and_consumed() {
    let dir = tempfile::tempdir().unwrap();
    let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

    let data_hash = vec![0xAA; 32];
    let tx1 = vec![0x01; 32];
    let tx2 = vec![0x02; 32];
    let cell = LiveCellInfo {
        capacity: 100_00000000,
        lock_script_hash: vec![0x11; 32],
        lock_code_hash: vec![0x22; 32],
        lock_hash_type: 1,
        lock_args: vec![0x33; 20],
        type_script_hash: None,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 50,
        occupied_capacity: 61_00000000,
        udt_amount: None,
        data_hash: None,
    };

    // Cell 1: live at block 5
    let mut batch = StoreBatch::new(&store);
    batch.put_live_cell_marker(&tx1, 0, 5);
    batch.put_cell_payload(&tx1, 0, &cell);
    batch.put_cell_by_data_hash(&data_hash, 5, &tx1, 0);
    batch.commit().unwrap();

    // Cell 2: consumed (created block 10, consumed block 20)
    let mut batch = StoreBatch::new(&store);
    batch.put_cell_payload(&tx2, 0, &cell);
    batch.put_consumed_cell(&tx2, 0, &cell, 10, 20);
    batch.put_cell_by_data_hash(&data_hash, 10, &tx2, 0);
    batch.commit().unwrap();

    let results = store.list_all_cells_by_data_hash(&data_hash, &store).unwrap();
    assert_eq!(results.len(), 2);
    // Sorted by block: block 5 first, block 10 second
    assert_eq!(results[0].0, tx1);
    assert!(results[0].3, "first cell should be live");
    assert_eq!(results[1].0, tx2);
    assert!(!results[1].3, "second cell should be consumed");
}
```

**Step 4: Run test**

```bash
cargo test -p ckbadger-store list_all_cells_by_data_hash
```

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/cell_ops.rs
git commit -m "feat(store): add list_all_cells_by_data_hash for code cells display"
```

---

### Task 2: Store — `list_all_cells_by_type`

**Files:**

- Modify: `crates/ckbadger-store/src/cell_ops.rs` (after `list_all_cells_by_data_hash`)

**Step 1: Add method**

Same pattern as Task 1 but uses `cf_cell_by_type()`:

```rust
/// List all cells (live and consumed) matching a type script hash.
///
/// For type_id scripts this typically returns 0-1 results.
/// Each result includes a `bool` indicating whether the cell is live (`true`) or consumed (`false`).
pub fn list_all_cells_by_type(
    &self,
    type_hash: &[u8],
    cells_store: &CkbadgerStore,
) -> anyhow::Result<Vec<(Vec<u8>, i16, PositionedCellInfo, bool)>> {
    let mut results = Vec::new();

    let iter = self.iterator_cf(
        self.cf_cell_by_type(),
        rocksdb::IteratorMode::From(type_hash, rocksdb::Direction::Forward),
    );

    for item in iter {
        let (key, _) = item.map_err(|e| {
            anyhow::anyhow!(
                "failed to iterate cell index in list_all_cells_by_type: {}",
                e
            )
        })?;
        if !key.starts_with(type_hash) {
            break;
        }
        if key.len() >= 74 {
            let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
            if let Some(cell) = self.get_cell(&tx_hash, output_index, cells_store)? {
                results.push((tx_hash, output_index, cell, true));
            } else if let Some(cell) =
                self.get_consumed_cell(&tx_hash, output_index, cells_store)?
            {
                results.push((tx_hash, output_index, cell, false));
            }
        }
    }

    Ok(results)
}
```

**Step 2: Run check and test**

```bash
cargo check -p ckbadger-store && cargo test -p ckbadger-store list_all_cells_by
```

**Step 3: Commit**

```bash
git add crates/ckbadger-store/src/cell_ops.rs
git commit -m "feat(store): add list_all_cells_by_type for code cells display"
```

---

### Task 3: API — New `GET /scripts/code-cells` endpoint

**Files:**

- Modify: `crates/api/src/routes/scripts.rs`

**Step 1: Add response types and query struct**

Add after the existing `CodeCellResponse` struct (~line 604):

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellEntry {
    pub tx_hash: String,
    pub output_index: i32,
    pub status: &'static str,
    pub created_at_block: i64,
    pub capacity: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellsResponse {
    pub code_cells: Vec<CodeCellEntry>,
    pub live_count: i64,
    pub total_count: i64,
}
```

**Step 2: Add handler**

Add after the existing `get_code_cell` handler:

```rust
async fn get_code_cells(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CodeCellQuery>,
) -> ApiResult<CodeCellsResponse> {
    let code_hash_bytes = hex::decode(
        params
            .code_hash
            .strip_prefix("0x")
            .unwrap_or(&params.code_hash),
    )
    .map_err(|_| ApiError::bad_request("Invalid code_hash hex"))?;

    let all_script_infos: Vec<ckbadger_store::ScriptInfo> = load_script_infos_cached(&state)?
        .into_iter()
        .map(|(_, info)| info)
        .collect();

    let hash_type = match params.hash_type.as_str() {
        "data" => 0,
        "type" => 1,
        "data1" => 2,
        "data2" => 4,
        _ => 0,
    };
    let script_info = merge_script_info_for_reference(&all_script_infos, &code_hash_bytes)
        .unwrap_or_else(|| ckbadger_store::ScriptInfo {
            code_hash: code_hash_bytes.clone(),
            hash_type,
            ..Default::default()
        });

    let (type_ref, data_ref) = deployment_reference_hashes(&script_info);

    let mut all_cells: Vec<(Vec<u8>, i16, ckbadger_store::PositionedCellInfo, bool)> = Vec::new();

    // Collect from type-ref index
    if let Some(type_hash) = type_ref.as_deref() {
        let cells = state
            .store
            .list_all_cells_by_type(type_hash, &state.append_only_store)
            .map_err(|e| ApiError::internal(format!("code cells type lookup failed: {}", e)))?;
        all_cells.extend(cells);
    }

    // Collect from data-ref index (deduplicate against type-ref results)
    if let Some(data_hash) = data_ref.as_deref() {
        let cells = state
            .store
            .list_all_cells_by_data_hash(data_hash, &state.append_only_store)
            .map_err(|e| ApiError::internal(format!("code cells data lookup failed: {}", e)))?;
        for cell in cells {
            if !all_cells.iter().any(|(h, i, _, _)| h == &cell.0 && *i == cell.1) {
                all_cells.push(cell);
            }
        }
    }

    // Also include the imported outpoint fallback if not already present
    if let (Some(tx_hash), Some(idx)) = (
        &script_info.code_cell_tx_hash,
        script_info.code_cell_output_index,
    ) {
        if !tx_hash.is_empty() {
            let idx_i16 = idx as i16;
            if !all_cells.iter().any(|(h, i, _, _)| h == tx_hash && *i == idx_i16) {
                // Try to load this cell (live or consumed)
                if let Some(cell) = state
                    .store
                    .get_cell(tx_hash, idx_i16, &state.append_only_store)
                    .unwrap_or(None)
                {
                    all_cells.push((tx_hash.clone(), idx_i16, cell, true));
                } else if let Some(cell) = state
                    .store
                    .get_consumed_cell(tx_hash, idx_i16, &state.append_only_store)
                    .unwrap_or(None)
                {
                    all_cells.push((tx_hash.clone(), idx_i16, cell, false));
                }
            }
        }
    }

    // Sort: live first, then by created_at_block ascending
    all_cells.sort_by(|a, b| {
        b.3.cmp(&a.3)
            .then_with(|| a.2.created_at_block.cmp(&b.2.created_at_block))
    });

    let live_count = all_cells.iter().filter(|(_, _, _, live)| *live).count() as i64;
    let total_count = all_cells.len() as i64;

    let code_cells = all_cells
        .into_iter()
        .map(|(tx_hash, output_index, cell, is_live)| CodeCellEntry {
            tx_hash: format!("0x{}", hex::encode(&tx_hash)),
            output_index: i32::from(output_index),
            status: if is_live { "live" } else { "consumed" },
            created_at_block: cell.created_at_block,
            capacity: cell.cell.capacity.to_string(),
        })
        .collect();

    ok(CodeCellsResponse {
        code_cells,
        live_count,
        total_count,
    })
}
```

**Step 3: Register route**

In `routes()` function (~line 35), add a new route. Add it BEFORE the `/scripts/{name}` route to avoid path conflicts:

```rust
.route("/scripts/code-cells", get(get_code_cells))
```

**Step 4: Run check**

```bash
cargo check -p ckbadger-api
```

**Step 5: Add test**

Add in the `#[cfg(test)] mod tests` block at the bottom of scripts.rs:

```rust
#[test]
fn get_code_cells_returns_live_and_consumed() {
    use ckbadger_store::{CkbadgerStore, LiveCellInfo, StoreBatch};
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());

    let data_hash = vec![0xCC; 32];
    let tx1 = vec![0x01; 32];
    let tx2 = vec![0x02; 32];
    let cell = LiveCellInfo {
        capacity: 200_00000000,
        lock_script_hash: vec![0x11; 32],
        lock_code_hash: vec![0x22; 32],
        lock_hash_type: 1,
        lock_args: vec![0x33; 20],
        type_script_hash: None,
        type_code_hash: None,
        type_hash_type: None,
        type_args: None,
        data_size: 100,
        occupied_capacity: 100_00000000,
        udt_amount: None,
        data_hash: None,
    };

    // Live cell
    let key1 = ckbadger_store::keys::encode_outpoint(&tx1, 0);
    let marker1 = ckbadger_store::types::encode_live_cell_marker(5);
    let payload = bincode::serialize(&cell).unwrap();
    store.put_cf(store.cf_live_cells(), &key1, &marker1).unwrap();
    store.put_cf(store.cf_cells(), &key1, &payload).unwrap();
    let mut batch = StoreBatch::new(&store);
    batch.put_cell_by_data_hash(&data_hash, 5, &tx1, 0);
    batch.commit().unwrap();

    // Consumed cell
    store
        .put_cf(
            store.cf_cells(),
            &ckbadger_store::keys::encode_outpoint(&tx2, 0),
            &payload,
        )
        .unwrap();
    let mut batch = StoreBatch::new(&store);
    batch.put_consumed_cell(&tx2, 0, &cell, 10, 20);
    batch.put_cell_by_data_hash(&data_hash, 10, &tx2, 0);
    batch.commit().unwrap();

    let results = store.list_all_cells_by_data_hash(&data_hash, &store).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results[0].3, "first should be live");
    assert!(!results[1].3, "second should be consumed");
}
```

**Step 6: Run tests**

```bash
cargo test -p ckbadger-api -- code_cells
```

**Step 7: Commit**

```bash
git add crates/api/src/routes/scripts.rs
git commit -m "feat(api): add GET /scripts/code-cells endpoint returning all code cells"
```

---

### Task 4: API — Add count fields to `ScriptLookupInfo` and `ScriptResponse`

**Files:**

- Modify: `crates/api/src/routes/scripts.rs`

**Step 1: Add fields to `ScriptLookupInfo` (~line 170)**

Add before the closing brace:

```rust
    pub code_cells_live_count: i64,
    pub code_cells_total: i64,
```

**Step 2: Add fields to `ScriptResponse` (~line 120)**

Add before the closing brace:

```rust
    pub code_cells_live_count: i64,
    pub code_cells_total: i64,
```

**Step 3: Create a shared count helper**

Add after `resolve_deployed_at` (~line 267):

```rust
fn count_code_cells(
    info: &ckbadger_store::ScriptInfo,
    store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
) -> Result<(i64, i64), ApiRouteError> {
    let (type_ref, data_ref) = deployment_reference_hashes(info);

    let mut seen = std::collections::HashSet::new();
    let mut live_count: i64 = 0;
    let mut total_count: i64 = 0;

    if let Some(type_hash) = type_ref.as_deref() {
        let cells = store
            .list_all_cells_by_type(type_hash, cells_store)
            .map_err(|e| ApiError::internal(format!("count_code_cells type failed: {}", e)))?;
        for (tx_hash, idx, _, is_live) in cells {
            if seen.insert((tx_hash, idx)) {
                total_count += 1;
                if is_live {
                    live_count += 1;
                }
            }
        }
    }

    if let Some(data_hash) = data_ref.as_deref() {
        let cells = store
            .list_all_cells_by_data_hash(data_hash, cells_store)
            .map_err(|e| ApiError::internal(format!("count_code_cells data failed: {}", e)))?;
        for (tx_hash, idx, _, is_live) in cells {
            if seen.insert((tx_hash, idx)) {
                total_count += 1;
                if is_live {
                    live_count += 1;
                }
            }
        }
    }

    Ok((live_count, total_count))
}
```

**Step 4: Populate in `lookup_scripts` handler**

In the `lookup_scripts` handler (~line 546), add the count call and fields. After the `resolve_code_cell` call, add:

```rust
let (code_cells_live_count, code_cells_total) =
    count_code_cells(&info, &state.store, &state.append_only_store)?;
```

And add to the `ScriptLookupInfo` construction:

```rust
code_cells_live_count,
code_cells_total,
```

**Step 5: Populate in `script_info_to_response`**

In `script_info_to_response` (~line 446), add the count call after `resolve_code_cell`:

```rust
let (code_cells_live_count, code_cells_total) =
    count_code_cells(info, &state.store, &state.append_only_store)?;
```

And add to the `ScriptResponse` construction:

```rust
code_cells_live_count,
code_cells_total,
```

**Step 6: Run check and tests**

```bash
cargo check -p ckbadger-api && cargo test -p ckbadger-api --lib
```

**Step 7: Commit**

```bash
git add crates/api/src/routes/scripts.rs
git commit -m "feat(api): add codeCellsLiveCount/codeCellsTotal to lookup and script responses"
```

---

### Task 5: Frontend — API types and method

**Files:**

- Modify: `frontend/lib/api.ts`

**Step 1: Add `CodeCellEntry` interface**

Add after the `CodeCellScript` interface (~line 271):

```typescript
interface CodeCellEntry {
  txHash: string;
  outputIndex: number;
  status: 'live' | 'consumed';
  createdAtBlock: number;
  capacity: string;
}

interface CodeCellsResponse {
  codeCells: CodeCellEntry[];
  liveCount: number;
  totalCount: number;
}
```

**Step 2: Add count fields to `ScriptLookupInfo`**

In the `ScriptLookupInfo` interface (~line 1182), add before closing brace:

```typescript
codeCellsLiveCount: number;
codeCellsTotal: number;
```

**Step 3: Add count fields to `KnownScript`**

In the `KnownScript` interface (~line 1122), add before closing brace:

```typescript
  codeCellsLiveCount?: number;
  codeCellsTotal?: number;
```

**Step 4: Add `getCodeCells` method**

Add after the existing `getCodeCell` method (~line 2241):

```typescript
getCodeCells: (
  codeHash: string,
  hashType: ScriptRefHashType
): Promise<CodeCellsResponse> => {
  const query = new URLSearchParams();
  query.set('code_hash', codeHash);
  query.set('hash_type', hashType);
  return fetchApi(`/scripts/code-cells?${query}`);
},
```

**Step 5: Export new types**

Add `CodeCellEntry` and `CodeCellsResponse` to the exports if there's an explicit export block. If types are used directly via `api.` namespace, no export needed.

**Step 6: Run type check**

```bash
cd frontend && pnpm type-check
```

**Step 7: Commit**

```bash
git add frontend/lib/api.ts
git commit -m "feat(frontend): add CodeCells API types and getCodeCells method"
```

---

### Task 6: Frontend — Code cells sub-section component

**Files:**

- Create: `frontend/components/ui/code-cells-list.tsx`

**Step 1: Create the component**

```typescript
'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api } from '@/lib/api';
import { HexDisplay } from '@/components/ui/hex-display';
import { Capacity } from '@/components/ui/capacity';
import { Badge } from '@/components/ui/page-header';
import { TerminalRow } from '@/components/ui/terminal-panel';
import type { ScriptRefHashType } from '@/lib/script-ref';

interface CodeCellsListProps {
  codeHash: string;
  hashType: ScriptRefHashType;
}

export function CodeCellsList({ codeHash, hashType }: CodeCellsListProps) {
  const { data, isLoading } = useQuery({
    queryKey: ['code-cells', codeHash, hashType],
    queryFn: () => api.getCodeCells(codeHash, hashType),
    staleTime: Infinity,
  });

  if (isLoading) {
    return <div className="text-text-dim px-4 py-3 text-xs">Loading code cells...</div>;
  }

  if (!data || data.codeCells.length === 0) {
    return <div className="text-text-dim px-4 py-3 text-xs">No code cells found</div>;
  }

  return (
    <div>
      <div className="border-base-border bg-base-surface/50 text-text-dim flex items-center gap-x-4 border-b px-4 py-2 font-mono text-xs uppercase tracking-wider">
        <div className="w-48">Outpoint</div>
        <div className="w-20">Status</div>
        <div className="w-28 text-right">Created At</div>
        <div className="flex-1 text-right">Capacity</div>
      </div>
      {data.codeCells.map((cell) => (
        <TerminalRow key={`${cell.txHash}-${cell.outputIndex}`}>
          <div className="flex items-center gap-x-4">
            <div className="w-48">
              <Link
                href={`/cell/${cell.txHash}-${cell.outputIndex}`}
                className="hover:underline"
              >
                <HexDisplay
                  value={`${cell.txHash}:${cell.outputIndex}`}
                  size="sm"
                  startChars={8}
                  endChars={8}
                />
              </Link>
            </div>
            <div className="w-20">
              <Badge variant={cell.status === 'live' ? 'green' : 'gray'}>
                {cell.status === 'live' ? 'Live' : 'Consumed'}
              </Badge>
            </div>
            <div className="w-28 text-right">
              <Link
                href={`/blocks/${cell.createdAtBlock}`}
                className="text-emphasis font-mono text-xs hover:underline"
              >
                #{cell.createdAtBlock.toLocaleString()}
              </Link>
            </div>
            <div className="flex-1 text-right">
              <Capacity value={cell.capacity} className="text-sm" />
            </div>
          </div>
        </TerminalRow>
      ))}
    </div>
  );
}

export function CodeCellsSummary({
  liveCount,
  totalCount,
}: {
  liveCount: number;
  totalCount: number;
}) {
  const consumedCount = totalCount - liveCount;
  if (totalCount === 0) return <span className="text-text-dim">-</span>;
  return (
    <span className="text-text-dim text-xs">
      {liveCount > 0 && (
        <span className="text-positive">{liveCount} live</span>
      )}
      {liveCount > 0 && consumedCount > 0 && ', '}
      {consumedCount > 0 && <span>{consumedCount} consumed</span>}
    </span>
  );
}
```

**Step 2: Run lint and type check**

```bash
cd frontend && pnpm type-check && pnpm lint
```

**Step 3: Commit**

```bash
git add frontend/components/ui/code-cells-list.tsx
git commit -m "feat(frontend): add CodeCellsList and CodeCellsSummary components"
```

---

### Task 7: Frontend — Update `/script/:codeHash` page

**Files:**

- Modify: `frontend/app/script/[codeHash]/client-page.tsx`

**Step 1: Import new component**

Add to imports:

```typescript
import { CodeCellsList, CodeCellsSummary } from '@/components/ui/code-cells-list';
```

**Step 2: Replace single code cell display**

In the Deployment panel, replace the single code cell column and row (~lines 190-238) with:

1. Keep the deployment row but replace the "Code Cell" column content with a summary badge using `CodeCellsSummary` from the lookup data.
2. Add an expandable `CodeCellsList` below the deployment row that uses the `codeHash` and `hashType`.

The deployment row's "Code Cell" column (the `<div className="w-40 shrink-0">` block, ~lines 199-214) should show a summary instead of a single outpoint:

```typescript
<div className="w-40 shrink-0">
  {knownScript ? (
    <CodeCellsSummary
      liveCount={knownScript.codeCellsLiveCount ?? 0}
      totalCount={knownScript.codeCellsTotal ?? 0}
    />
  ) : codeCellTxHash ? (
    <Link
      href={`/cell/${codeCellTxHash}-${codeCellOutputIndex}`}
      className="hover:underline"
    >
      <HexDisplay
        value={`${codeCellTxHash}:${codeCellOutputIndex}`}
        size="sm"
        startChars={8}
        endChars={8}
      />
    </Link>
  ) : (
    <span className="text-text-dim">-</span>
  )}
</div>
```

Then add the code cells list section after the deployment row (after the closing `</TerminalRow>`, before the "Same Deployment References" section at ~line 240):

```typescript
<div className="border-base-border border-t">
  <div className="text-text-dim px-4 py-2 text-[11px] uppercase tracking-wider">
    Code Cells
  </div>
  <CodeCellsList codeHash={codeHash} hashType={hashType} />
</div>
```

**Step 3: Run type check and lint**

```bash
cd frontend && pnpm type-check && pnpm lint
```

**Step 4: Commit**

```bash
git add frontend/app/script/[codeHash]/client-page.tsx
git commit -m "feat(frontend): show all code cells in script-by-code-hash page"
```

---

### Task 8: Frontend — Update `/scripts/:name` page

**Files:**

- Modify: `frontend/app/scripts/[name]/client-page.tsx`

**Step 1: Import components**

Add to imports:

```typescript
import { CodeCellsList, CodeCellsSummary } from '@/components/ui/code-cells-list';
```

**Step 2: Add expand/collapse state**

Add state for tracking which deployment is expanded (showing code cells):

```typescript
const [expandedCodeCells, setExpandedCodeCells] = useState<string | null>(null);
```

**Step 3: Replace code cell column in deployment row**

In each deployment row, replace the single code cell link (~lines 420-434) with `CodeCellsSummary`:

```typescript
{lookupInfo ? (
  <button
    onClick={(e) => {
      e.stopPropagation();
      setExpandedCodeCells(
        expandedCodeCells === deployment.codeHash ? null : deployment.codeHash
      );
    }}
    className="text-left hover:underline"
  >
    <CodeCellsSummary
      liveCount={lookupInfo.codeCellsLiveCount}
      totalCount={lookupInfo.codeCellsTotal}
    />
  </button>
) : deployment.codeCellTxHash && deployment.codeCellOutputIndex !== null ? (
  <Link
    href={`/cell/${deployment.codeCellTxHash}-${deployment.codeCellOutputIndex}`}
    onClick={(e) => e.stopPropagation()}
    className="text-emphasis text-xs hover:underline"
  >
    <HexDisplay
      value={`${deployment.codeCellTxHash}:${deployment.codeCellOutputIndex}`}
      startChars={8}
      endChars={8}
    />
  </Link>
) : (
  <span className="text-text-dim">-</span>
)}
```

Where `lookupInfo` is resolved from the existing `deploymentLookup`:

```typescript
const lookupInfo = deploymentLookup?.[deployment.codeHash];
```

**Step 4: Add expandable code cells section**

After each deployment `TerminalRow`, add the expandable section:

```typescript
{expandedCodeCells === deployment.codeHash && (
  <div className="border-base-border bg-base-bg/50 border-b">
    <CodeCellsList
      codeHash={deployment.codeHash}
      hashType={
        (deployment.hashType as ScriptRefHashType) ?? 'data'
      }
    />
  </div>
)}
```

**Step 5: Update column header**

Change the "Deployment" column header text (~line 397) from referencing single code cell to:

```
Code Cells
```

**Step 6: Run type check, lint, format**

```bash
cd frontend && pnpm type-check && pnpm lint && pnpm format
```

**Step 7: Commit**

```bash
git add frontend/app/scripts/[name]/client-page.tsx
git commit -m "feat(frontend): show expandable code cells per deployment in named script page"
```

---

### Task 9: Cleanup and verify

**Step 1: Run all Rust tests**

```bash
cargo test --lib
```

**Step 2: Run all frontend checks**

```bash
cd frontend && pnpm type-check && pnpm lint && npx vitest run
```

**Step 3: Run clippy**

```bash
cargo clippy
```

**Step 4: Format**

```bash
pnpm format
```

**Step 5: Final commit if any formatting changes**

```bash
git add -A && git commit -m "chore: format"
```
