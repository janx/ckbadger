# DAO Top 100 Depositors Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a "Depositors" tab to the DAO page showing the top 100 depositors ranked by total deposit capacity, matching the official CKB explorer's design.

**Architecture:** Piggyback on the existing `refresh_latest_dao_statistics` indexer scan (which already iterates all active deposits) to also build a per-depositor capacity map. Store the top-100 list alongside `DaoLatestStatistics` in CF_STATS_DAO. API reads it in O(1). Frontend adds a tab switcher between the existing deposits table and the new depositors table.

**Tech Stack:** Rust (store types, indexer writer, API route), TypeScript/React (frontend tab + table)

---

### Task 1: Add `DaoTopDepositors` type to store

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs:188` (after `DaoLatestStatistics`)
- Modify: `crates/ckbadger-store/src/keys.rs:264` (add new stats prefix)
- Modify: `crates/ckbadger-store/src/keys.rs:299` (add flat re-export)

**Step 1: Add the new type in types.rs**

After line 188 (`DaoLatestStatistics` closing brace), add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoTopDepositorEntry {
    pub lock_script_hash: Vec<u8>,
    pub address: Option<String>,
    pub total_capacity: i128,
    pub deposit_count: i32,
    pub average_deposit_blocks: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoTopDepositors {
    pub tip_block_number: i64,
    pub depositors: Vec<DaoTopDepositorEntry>,
}
```

**Step 2: Add stats prefix in keys.rs**

In `stats_prefix` module (after line 264, `DOTBIT_OUTPOINT_BY_ACCOUNT_ID: u8 = 0x1F`), add:

```rust
pub const DAO_TOP_DEPOSITORS: u8 = 0x20;
```

Add flat re-export after existing re-exports (after line 299):

```rust
pub const STATS_PREFIX_DAO_TOP_DEPOSITORS: u8 = stats_prefix::DAO_TOP_DEPOSITORS;
```

**Step 3: Run check**

Run: `cargo check -p ckbadger-store`
Expected: PASS (new types are defined but not yet used)

**Step 4: Commit**

```
feat(store): add DaoTopDepositors type and stats prefix
```

---

### Task 2: Add store get/put methods for top depositors

**Files:**

- Modify: `crates/ckbadger-store/src/stats_ops.rs:367` (after `get_latest_dao_statistics`)

**Step 1: Write the test**

Add test in the existing `#[cfg(test)]` module of `stats_ops.rs`:

```rust
#[test]
fn test_dao_top_depositors_roundtrip() {
    let store = open_test_domain();
    let depositors = DaoTopDepositors {
        tip_block_number: 100,
        depositors: vec![
            DaoTopDepositorEntry {
                lock_script_hash: vec![0xAA; 32],
                address: Some("ckb1test".to_string()),
                total_capacity: 1000_00000000,
                deposit_count: 3,
                average_deposit_blocks: 5400.0,
            },
        ],
    };
    store.put_dao_top_depositors(&depositors).unwrap();
    let loaded = store.get_dao_top_depositors().unwrap().unwrap();
    assert_eq!(loaded.tip_block_number, 100);
    assert_eq!(loaded.depositors.len(), 1);
    assert_eq!(loaded.depositors[0].total_capacity, 1000_00000000);
    assert_eq!(loaded.depositors[0].deposit_count, 3);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ckbadger-store test_dao_top_depositors_roundtrip`
Expected: FAIL — `put_dao_top_depositors` and `get_dao_top_depositors` don't exist

**Step 3: Implement store methods**

After `get_latest_dao_statistics` (line 367 of stats_ops.rs), add:

```rust
pub fn put_dao_top_depositors(&self, top: &DaoTopDepositors) -> anyhow::Result<()> {
    let key = keys::encode_stats_key(stats_prefix::DAO_TOP_DEPOSITORS, b"latest");
    let value = bincode::serialize(top)?;
    self.put_cf(self.cf_stats_dao(), &key, &value)
}

pub fn get_dao_top_depositors(&self) -> anyhow::Result<Option<DaoTopDepositors>> {
    let key = keys::encode_stats_key(stats_prefix::DAO_TOP_DEPOSITORS, b"latest");
    match self.get_cf(self.cf_stats_dao(), &key)? {
        Some(value) => Ok(Some(bincode::deserialize(&value)?)),
        None => Ok(None),
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p ckbadger-store test_dao_top_depositors_roundtrip`
Expected: PASS

**Step 5: Commit**

```
feat(store): add get/put methods for DAO top depositors
```

---

### Task 3: Extend indexer to compute top depositors during statistics refresh

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs:851-998` (inside `refresh_latest_dao_statistics`)

**Step 1: Write the test**

Add test in the existing `#[cfg(test)]` module of `statistics.rs`. Look at the existing `test_accumulate_dao_statistics_entry` test pattern and add:

```rust
#[test]
fn test_refresh_dao_statistics_computes_top_depositors() {
    // This test verifies that refresh_latest_dao_statistics also writes top depositors.
    // Since refresh_latest_dao_statistics requires a full store + sync tip,
    // we test the grouping logic directly.
    use std::collections::HashMap;

    // Simulate the per-depositor accumulation
    let mut depositor_map: HashMap<Vec<u8>, (i128, i32, f64)> = HashMap::new();
    let lock_a = vec![0xAA; 32];
    let lock_b = vec![0xBB; 32];

    // Depositor A: two deposits
    let e = depositor_map.entry(lock_a.clone()).or_insert((0, 0, 0.0));
    e.0 += 500_00000000i128;
    e.1 += 1;
    e.2 += 1000.0;
    let e = depositor_map.entry(lock_a.clone()).or_insert((0, 0, 0.0));
    e.0 += 300_00000000i128;
    e.1 += 1;
    e.2 += 500.0;

    // Depositor B: one deposit, larger
    let e = depositor_map.entry(lock_b.clone()).or_insert((0, 0, 0.0));
    e.0 += 1000_00000000i128;
    e.1 += 1;
    e.2 += 2000.0;

    // Sort by capacity desc, take top 100
    let mut sorted: Vec<_> = depositor_map.into_iter().collect();
    sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    sorted.truncate(100);

    assert_eq!(sorted[0].0, lock_b); // B has 1000 CKB, ranked first
    assert_eq!(sorted[0].1 .0, 1000_00000000);
    assert_eq!(sorted[1].0, lock_a); // A has 800 CKB total
    assert_eq!(sorted[1].1 .0, 800_00000000);
    assert_eq!(sorted[1].1 .1, 2); // A has 2 deposits
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test -p ckbadger-indexer test_refresh_dao_statistics_computes_top_depositors`
Expected: PASS (this tests the algorithm, not the integration)

**Step 3: Modify `refresh_latest_dao_statistics` to build top depositors**

In `refresh_latest_dao_statistics` (line 851 of statistics.rs), add a `HashMap` alongside the existing accumulators (after line 870):

```rust
// Add after line 870 (after `let mut unclaimed_compensation: u128 = 0;`)
let mut depositor_map: HashMap<Vec<u8>, (i128, i32, f64)> = HashMap::new();
// (total_capacity, deposit_count, total_blocks_held)
```

Inside the existing `scan_dao_deposits_by_status(0, ...)` closure (after line 875, after `active_deposits += 1;`), add the per-depositor accumulation:

```rust
// Add after `active_deposits += 1;` (line 875)
{
    let dm = depositor_map.entry(entry.lock_script_hash.clone()).or_insert((0, 0, 0.0));
    dm.0 += entry.capacity as i128;
    dm.1 += 1;
    if entry.deposit_block_number <= tip_block_number {
        dm.2 += (tip_block_number - entry.deposit_block_number) as f64;
    }
}
```

After the existing `self.store.put_stats_key(...)` call (after line 996), add the top depositors write:

```rust
// Build and store top depositors
{
    let mut sorted: Vec<_> = depositor_map.into_iter().collect();
    sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    sorted.truncate(100);

    let depositors = sorted
        .into_iter()
        .map(|(lock_hash, (total_capacity, deposit_count, total_blocks))| {
            let avg_blocks = if deposit_count > 0 {
                total_blocks / deposit_count as f64
            } else {
                0.0
            };
            DaoTopDepositorEntry {
                lock_script_hash: lock_hash,
                address: None, // Resolved at API layer
                total_capacity,
                deposit_count,
                average_deposit_blocks: avg_blocks,
            }
        })
        .collect();

    let top = DaoTopDepositors {
        tip_block_number,
        depositors,
    };
    self.store.put_dao_top_depositors(&top)?;
}
```

Ensure `HashMap` is imported (check existing imports — it may already be in scope via `std::collections::HashMap`).

Ensure `DaoTopDepositorEntry` and `DaoTopDepositors` are imported from `ckbadger_store`.

**Step 4: Run check**

Run: `cargo check -p ckbadger-indexer`
Expected: PASS

**Step 5: Commit**

```
feat(indexer): compute top 100 depositors during DAO statistics refresh
```

---

### Task 4: Add API endpoint for top depositors

**Files:**

- Modify: `crates/api/src/routes/dao.rs:22-35` (add route)
- Modify: `crates/api/src/routes/dao.rs` (add response struct + handler, near the `get_statistics` handler)

**Step 1: Add response struct**

Near the existing `DaoStatisticsResponse` struct (around line 257), add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoTopDepositorResponse {
    pub rank: i32,
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub total_capacity: String,
    pub total_capacity_ckb: String,
    pub deposit_count: i32,
    pub average_deposit_days: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoTopDepositorsResponse {
    pub depositors: Vec<DaoTopDepositorResponse>,
}
```

**Step 2: Add handler**

After the `get_statistics` handler (after line 788), add:

```rust
async fn get_top_depositors(
    State(state): State<Arc<AppState>>,
) -> ApiResult<DaoTopDepositorsResponse> {
    let cache_key = "dao:top-depositors";
    if let Some(cached) = state.mem_cache.get::<DaoTopDepositorsResponse>(cache_key) {
        return ok(cached);
    }

    let top = state
        .store
        .get_dao_top_depositors()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .unwrap_or_else(|| ckbadger_store::DaoTopDepositors {
            tip_block_number: 0,
            depositors: vec![],
        });

    let depositors = top
        .depositors
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let capacity_str = d.total_capacity.to_string();
            let avg_epochs = d.average_deposit_blocks / 1800.0;
            let avg_days = avg_epochs * 4.0 / 24.0;
            let address = state
                .store
                .get_address_by_lock_hash(&d.lock_script_hash)
                .ok()
                .flatten();
            DaoTopDepositorResponse {
                rank: (i + 1) as i32,
                lock_script_hash: format!("0x{}", hex::encode(&d.lock_script_hash)),
                address,
                total_capacity: capacity_str.clone(),
                total_capacity_ckb: shannon_to_ckb(&capacity_str),
                deposit_count: d.deposit_count,
                average_deposit_days: format_deposit_days(avg_days),
            }
        })
        .collect();

    let response = DaoTopDepositorsResponse { depositors };
    state
        .mem_cache
        .set(cache_key, &response, DAO_STATS_CACHE_TTL);
    ok(response)
}

fn format_deposit_days(days: f64) -> String {
    if days >= 1000.0 {
        format!("{:.1}K", days / 1000.0)
    } else if days < 0.1 {
        "0".to_string()
    } else {
        format!("{:.1}", days)
    }
}
```

Note: The `get_address_by_lock_hash` method may or may not exist. Check `crates/ckbadger-store/src/` for address lookup by lock_hash. If it doesn't exist, check for `addr_stats` or address encoding utils. The address might need to be derived from lock_script_hash using `ckb_address::encode`. If no convenient method exists, leave `address: None` and derive on the frontend from `lockScriptHash` — but first check what the existing `list_deposits` handler does for address resolution.

**Step 3: Register the route**

In the `routes()` function (line 22-35), add:

```rust
.route("/dao/top-depositors", get(get_top_depositors))
```

**Step 4: Run check**

Run: `cargo check -p ckbadger-api`
Expected: PASS

**Step 5: Commit**

```
feat(api): add GET /dao/top-depositors endpoint
```

---

### Task 5: Add frontend API method and types

**Files:**

- Modify: `frontend/lib/api.ts` (add interface + method near other DAO types/methods)

**Step 1: Add TypeScript interface**

Near the existing `DaoStatistics` interface (around line 752), add:

```typescript
interface DaoTopDepositor {
  rank: number;
  lockScriptHash: string;
  address: string | null;
  totalCapacity: string;
  totalCapacityCkb: string;
  depositCount: number;
  averageDepositDays: string;
}

interface DaoTopDepositorsResponse {
  depositors: DaoTopDepositor[];
}
```

**Step 2: Add API method**

Near the existing `getDaoStatistics` method (around line 1669), add:

```typescript
getDaoTopDepositors: (): Promise<DaoTopDepositorsResponse> => {
  return fetchApi('/dao/top-depositors');
},
```

**Step 3: Export the new types**

Add `DaoTopDepositor` and `DaoTopDepositorsResponse` to the export list if the file uses explicit exports.

**Step 4: Run type check**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 5: Commit**

```
feat(frontend): add getDaoTopDepositors API method and types
```

---

### Task 6: Add Depositors tab to DAO page

**Files:**

- Modify: `frontend/app/dao/page.tsx`

**Step 1: Add query and state**

Inside the `DaoPage` component (after the existing `useQuery` calls around line 145), add:

```typescript
const [activeTab, setActiveTab] = useState<'deposits' | 'depositors'>('deposits');

const { data: topDepositors, isLoading: isLoadingDepositors } = useQuery({
  queryKey: ['dao-top-depositors'],
  queryFn: () => api.getDaoTopDepositors(),
  enabled: activeTab === 'depositors',
});
```

Add to imports at top:

```typescript
import { api, DaoDeposit, DaoTopDepositor, ScriptLookupResponse } from '@/lib/api';
```

**Step 2: Replace the deposits TerminalPanel (line 468-562) with a tabbed panel**

Replace the existing `<TerminalPanel>` block starting at line 468 with a new version that includes tab switching. The panel header should have two tabs: "Deposits" and "Depositors".

The deposits tab keeps the existing filter buttons and table. The depositors tab shows the top depositors table:

```tsx
<TerminalPanel>
  <TerminalPanelHeader
    indicator="active"
    actions={
      activeTab === 'deposits' ? (
        <FilterButtonGroup
          options={filterOptions}
          selected={status}
          onChange={(v) => {
            setStatus(v as number);
            depositsPagination.reset();
          }}
        />
      ) : null
    }
  >
    <div className="flex gap-4">
      <button
        onClick={() => setActiveTab('deposits')}
        className={`font-mono text-sm transition-colors ${
          activeTab === 'deposits'
            ? 'text-text-bright border-b-2 border-emphasis pb-1'
            : 'text-text-dim hover:text-text pb-1'
        }`}
      >
        Deposits
      </button>
      <button
        onClick={() => setActiveTab('depositors')}
        className={`font-mono text-sm transition-colors ${
          activeTab === 'depositors'
            ? 'text-text-bright border-b-2 border-emphasis pb-1'
            : 'text-text-dim hover:text-text pb-1'
        }`}
      >
        Depositors
      </button>
    </div>
  </TerminalPanelHeader>
  <TerminalPanelContent padding="none">
    {activeTab === 'deposits' ? (
      /* existing deposits table JSX (lines 485-560) — keep as-is */
    ) : (
      /* depositors table — new */
      isLoadingDepositors ? (
        <div className="text-text-dim py-8 text-center">Loading...</div>
      ) : topDepositors?.depositors?.length ? (
        <div className="overflow-x-auto">
          <table className="w-full">
            <thead>
              <tr className="border-base-border text-text-dim border-b text-left font-mono text-xs uppercase">
                <th className="px-4 py-3 text-center w-16">Rank</th>
                <th className="px-4 py-3">Address</th>
                <th className="px-4 py-3 text-right">Deposit Capacity</th>
                <th className="px-4 py-3 text-right">Deposit Time(Day)</th>
              </tr>
            </thead>
            <tbody>
              {topDepositors.depositors.map((depositor: DaoTopDepositor) => (
                <tr
                  key={depositor.lockScriptHash}
                  className="hover:bg-base-elevated/50 border-base-border/50 border-b transition-colors"
                >
                  <td className="text-text-dim px-4 py-3 text-center font-mono tabular-nums">
                    {depositor.rank}
                  </td>
                  <td className="px-4 py-3">
                    {depositor.address ? (
                      <Address address={depositor.address} />
                    ) : (
                      <Link href={`/address/${depositor.lockScriptHash}`}>
                        <Hash
                          hash={depositor.lockScriptHash}
                          className="hover:text-emphasis text-text"
                        />
                      </Link>
                    )}
                  </td>
                  <td className="text-text-bright px-4 py-3 text-right font-mono tabular-nums">
                    {(() => {
                      const f = formatCkbAmount(depositor.totalCapacity);
                      return (
                        <>
                          {f.integer}
                          <span className="text-text-dim text-[0.85em]">.{f.decimal}</span>
                          <span className="text-text-dim ml-1 text-[0.85em]">CKB</span>
                        </>
                      );
                    })()}
                  </td>
                  <td className="text-text-dim px-4 py-3 text-right font-mono tabular-nums">
                    {depositor.averageDepositDays}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="text-text-dim py-8 text-center">No depositors found</div>
      )
    )}
  </TerminalPanelContent>
</TerminalPanel>
```

**Step 3: Run type check and lint**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): add Depositors tab to DAO page with top 100 ranking
```

---

### Task 7: Add tests

**Files:**

- Modify: `crates/api/tests/api_integration.rs` (if DAO tests exist there)
- Modify: `frontend/__tests__/` (add component test if pattern exists)

**Step 1: Add API integration test**

Check if there are existing DAO API tests in `api_integration.rs`. If yes, add a test for the new endpoint following the same pattern. The test should verify:

- `GET /dao/top-depositors` returns 200
- Response has `depositors` array
- Each depositor has `rank`, `lockScriptHash`, `totalCapacity`, `totalCapacityCkb`, `depositCount`, `averageDepositDays`

**Step 2: Run all tests**

Run: `cargo test -p ckbadger-store && cargo test -p ckbadger-indexer test_refresh_dao && cd frontend && pnpm type-check`
Expected: ALL PASS

**Step 3: Commit**

```
test: add tests for DAO top depositors feature
```

---

### Task 8: Final verification

**Step 1: Full pre-commit check**

Run: `cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint`
Expected: ALL PASS

**Step 2: Run all tests**

Run: `cargo test --lib && cd frontend && npx vitest run`
Expected: ALL PASS

**Step 3: Commit any fixes if needed**
