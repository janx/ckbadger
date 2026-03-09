# Activity Breakdown V2 — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Exclude coinbase from asset type pie chart. Add a second pie chart showing activity counts by script (both lock and type scripts).

**Architecture:** Extend `DailyActivityStats` with `script_counts: HashMap<String, u32>`. Propagate `lock_code_hash` through `InputCellView` and track all involved script code_hashes per activity in `OwnerAccum`. API resolves code_hash → name via `CF_SCRIPT_INFO`. Frontend renders second pie chart.

**Tech Stack:** Rust (bincode, serde, rocksdb), Axum 0.8, React 19, TanStack Query v5, PieChart SVG component

---

### Task 1: Store — Add `script_counts` to `DailyActivityStats`

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs:923-943`

**Step 1: Add HashMap import and field**

At the top of `types.rs`, add `use std::collections::HashMap;` if not already present.

In `DailyActivityStats` (after `total_ckb_moved` field, ~line 942), add:

```rust
    /// Per-script activity counts: hex code_hash -> count
    #[serde(default)]
    pub script_counts: HashMap<String, u32>,
```

**Step 2: Run cargo check**

```bash
cargo check -p ckbadger-store
```

Expected: compiles. `#[serde(default)]` makes it backward-compatible during deserialization of old records (empty map).

**Step 3: Commit**

```bash
git add crates/ckbadger-store/src/types.rs
git commit -m "feat(store): add script_counts field to DailyActivityStats"
```

---

### Task 2: Activity builder — Add `lock_code_hash` to `InputCellView` and track scripts

**Files:**

- Modify: `crates/indexer/src/db/writer/activities.rs:77-88` (InputCellView)
- Modify: `crates/indexer/src/db/writer/activities.rs:120-151` (OwnerAccum)
- Modify: `crates/indexer/src/db/writer/activities.rs:156-325` (build_tx_activities + build_activities_for_block)
- Modify: `crates/indexer/src/db/writer/activities.rs:525-669` (tests)

**Step 1: Add `lock_code_hash` to `InputCellView`**

In `InputCellView` (~line 78), add field:

```rust
pub struct InputCellView {
    pub lock_script_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,   // NEW
    pub capacity: i64,
    pub occupied_capacity: i64,
    pub type_code_hash: Option<Vec<u8>>,
    pub type_script_hash: Option<Vec<u8>>,
    pub type_args: Option<Vec<u8>>,
    pub udt_amount: Option<u128>,
    pub data: Vec<u8>,
    pub is_dao_withdraw_request: bool,
}
```

**Step 2: Add `involved_scripts` to `OwnerAccum`**

Add to `OwnerAccum` struct (~line 122), at the end before closing brace:

```rust
    /// Distinct script code_hashes involved (lock + type)
    involved_scripts: HashSet<Vec<u8>>,
```

Add `use std::collections::HashSet;` at the top if not already imported.

**Step 3: Populate `involved_scripts` during processing**

In `build_tx_activities`, when processing **inputs** (~line 164-184):

- After `let accum = owners.entry(input.lock_script_hash.clone()).or_default();`, add:

```rust
        accum.involved_scripts.insert(input.lock_code_hash.clone());
```

- Inside the `if let Some(ref type_code_hash) = input.type_code_hash` block (~line 172), before `classify_input(...)`, add:

```rust
            accum.involved_scripts.insert(type_code_hash.clone());
```

When processing **outputs** (~line 188-213):

- After `let accum = owners.entry(cell.lock_script_hash.clone()).or_default();`, add:

```rust
        accum.involved_scripts.insert(cell.lock_code_hash.clone());
```

- Inside the `if let Some(ref type_code_hash) = cell.type_code_hash` block (~line 203), before `classify_output(...)`, add:

```rust
            accum.involved_scripts.insert(type_code_hash.clone());
```

**Step 4: Change return type to include scripts**

Change `build_activities_for_block` signature and return (~line 105-118):

```rust
/// Build activities for all transactions in a block.
///
/// Returns `(lock_hash, script_code_hashes, ActivityEntry)` triples — one per owner per transaction.
pub fn build_activities_for_block(
    txs: &[TxView<'_>],
    token_info_cache: &HashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Vec<(Vec<u8>, Vec<Vec<u8>>, ActivityEntry)> {
    let hashes = code_hashes();
    let mut all_activities = Vec::new();

    for tx in txs {
        let activities = build_tx_activities(tx, hashes, token_info_cache);
        all_activities.extend(activities);
    }

    all_activities
}
```

Change `build_tx_activities` return type (~line 156-160):

```rust
fn build_tx_activities(
    tx: &TxView<'_>,
    hashes: &CodeHashes,
    token_info_cache: &HashMap<Vec<u8>, (Option<String>, Option<u8>)>,
) -> Vec<(Vec<u8>, Vec<Vec<u8>>, ActivityEntry)> {
```

Change the result push at ~line 321:

```rust
        let scripts: Vec<Vec<u8>> = accum.involved_scripts.iter().cloned().collect();
        result.push((lock_hash.clone(), scripts, entry));
```

**Step 5: Update tests**

Update `make_input` helper (~line 555) to include `lock_code_hash`:

```rust
    fn make_input(lock_hash_byte: u8, capacity: i64, occupied: i64) -> InputCellView {
        InputCellView {
            lock_script_hash: vec![lock_hash_byte; 32],
            lock_code_hash: vec![0x11; 32],
            capacity,
            occupied_capacity: occupied,
            type_code_hash: None,
            type_script_hash: None,
            type_args: None,
            udt_amount: None,
            data: vec![],
            is_dao_withdraw_request: false,
        }
    }
```

Update all test assertions that destructure activities from 2-tuple to 3-tuple. For each test:

- `test_simple_ckb_transfer` (~line 591): change `activities.iter().find(|(lh, _)|` → `activities.iter().find(|(lh, _, _)|` and `.map(|(_, e)| e)` → `.map(|(_, _, e)| e)`. Also update `let (lock_hash, entry) =` patterns to `let (lock_hash, _, entry) =`.
- `test_cellbase_reward` (~line 632): `let (lock_hash, _, entry) = &activities[0];`
- `test_occupied_delta_computed` (~line 663): `let (_, _, entry) = &activities[0];`
- Any other tests in the file: same pattern.

Add a new test for script tracking:

```rust
    #[test]
    fn test_scripts_tracked_for_transfer() {
        let alice = 0xAA;
        let bob = 0xBB;

        let outputs = vec![
            make_output(bob, 100_00000000, None, None, None, vec![]),
            make_output(alice, 200_00000000, None, None, None, vec![]),
        ];

        let tx = TxView {
            tx_hash: &[0x01; 32],
            block_hash: &[0xA1; 32],
            tx_index: 1,
            block_number: 1000,
            timestamp: 1_700_000_000,
            is_cellbase: false,
            inputs: vec![make_input(alice, 300_00000000, 61_00000000)],
            outputs: &outputs,
        };

        let activities = build_activities_for_block(&[tx], &HashMap::new());
        // Alice has lock_code_hash from both input and output
        let (_, alice_scripts, _) = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![alice; 32])
            .unwrap();
        // lock_code_hash = 0x11 (from make_output and make_input)
        assert!(alice_scripts.contains(&vec![0x11; 32]));

        // Bob only has output lock_code_hash
        let (_, bob_scripts, _) = activities
            .iter()
            .find(|(lh, _, _)| lh == &vec![bob; 32])
            .unwrap();
        assert!(bob_scripts.contains(&vec![0x11; 32]));
    }
```

**Step 6: Run tests**

```bash
cargo test -p ckbadger-indexer -- activities --nocapture
```

Expected: all tests pass including the new one.

**Step 7: Commit**

```bash
git add crates/indexer/src/db/writer/activities.rs
git commit -m "feat(activities): track involved script code_hashes per activity"
```

---

### Task 3: Update call sites — batch.rs and to_latest_items

**Files:**

- Modify: `crates/indexer/src/sync/latest_activities.rs:78-99`
- Modify: `crates/indexer/src/sync/batch.rs:4722-4758` (bulk sync)
- Modify: `crates/indexer/src/sync/batch.rs:5895-5932` (live sync)

**Step 1: Update `to_latest_items` signature**

In `latest_activities.rs` (~line 78-99), change the parameter type:

```rust
pub fn to_latest_items(
    activities: &[(Vec<u8>, Vec<Vec<u8>>, ActivityEntry)],
    lock_scripts: &HashMap<Vec<u8>, LockScriptInfo>,
) -> Vec<LatestActivityItem> {
    activities
        .iter()
        .filter(|(_, _, entry)| !entry.is_cellbase)
        .map(|(lock_hash, _, entry)| {
            let (code_hash, hash_type, args) = lock_scripts
                .get(lock_hash)
                .cloned()
                .unwrap_or_else(|| (Vec::new(), 0, Vec::new()));
            LatestActivityItem {
                lock_hash: lock_hash.clone(),
                lock_code_hash: code_hash,
                lock_hash_type: hash_type,
                lock_args: args,
                entry: entry.clone(),
            }
        })
        .collect()
}
```

**Step 2: Update `build_activity_input_views` in batch.rs**

In `build_activity_input_views` (~line 119), add `lock_code_hash`:

```rust
            Ok(crate::db::writer::activities::InputCellView {
                lock_script_hash: info.lock_script_hash.clone(),
                lock_code_hash: info.lock_code_hash.clone(),  // NEW
                capacity: info.capacity,
                occupied_capacity: info.occupied_capacity,
                type_code_hash: info.type_code_hash.clone(),
                type_script_hash: info.type_script_hash.clone(),
                type_args: info.type_args.clone(),
                udt_amount: info.udt_amount,
                data: Vec::new(),
                is_dao_withdraw_request,
            })
```

**Step 3: Update bulk sync activity loop (~line 4739-4758)**

Change the destructuring to include scripts:

```rust
                                for (lock_hash, scripts, entry) in activities {
                                    // Accumulate daily activity stats
                                    let date = ckbadger_common::block_date_from_ms(entry.timestamp)
                                        .format("%Y%m%d")
                                        .to_string();
                                    let day_stats =
                                        act_stats_accum.entry(date.clone()).or_default();
                                    BatchWriter::accumulate_activity_stats(&entry, &scripts, day_stats);
                                    if lock_hash.len() == 32 {
                                        let mut hash = [0u8; 32];
                                        hash.copy_from_slice(&lock_hash);
                                        act_stats_addrs.entry(date).or_default().insert(hash);
                                    }

                                    activity_batch.put_activity(
                                        &lock_hash,
                                        entry.block_number,
                                        entry.tx_index,
                                        &entry,
                                    );
                                }
```

**Step 4: Update live sync activity loop (~line 5911-5932)**

Same pattern:

```rust
                    for (lock_hash, scripts, entry) in activities {
                        // Accumulate daily activity stats
                        let date = ckbadger_common::block_date_from_ms(entry.timestamp)
                            .format("%Y%m%d")
                            .to_string();
                        let day_stats = daily_activity_accum.entry(date.clone()).or_default();
                        BatchWriter::accumulate_activity_stats(&entry, &scripts, day_stats);
                        if lock_hash.len() == 32 {
                            let mut hash = [0u8; 32];
                            hash.copy_from_slice(&lock_hash);
                            daily_activity_addrs.entry(date).or_default().insert(hash);
                        }

                        put_activity_with_undo_log(
                            &mut data_batch,
                            &mut activity_batch,
                            &mut append_undo_seq_by_block,
                            &lock_hash,
                            entry.block_number,
                            entry.tx_index,
                            &entry,
                        );
                    }
```

**Step 5: Run cargo check**

```bash
cargo check -p ckbadger-indexer
```

Expected: compiles. This step will fail until Task 4 updates the `accumulate_activity_stats` signature, so Tasks 3 and 4 should be committed together.

**Step 6: Commit (after Task 4 completes)**

Combined with Task 4 commit.

---

### Task 4: Statistics — Update accumulate and merge for script counts

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs:521-610`

**Step 1: Update `accumulate_activity_stats` signature and add script counting**

Change the function (~line 523):

```rust
    pub fn accumulate_activity_stats(
        entry: &ActivityEntry,
        scripts: &[Vec<u8>],
        stats: &mut DailyActivityStats,
    ) {
        // Total CKB moved (absolute value)
        stats.total_ckb_moved = stats
            .total_ckb_moved
            .saturating_add(entry.ckb_delta.unsigned_abs());

        // Count each involved script
        for code_hash in scripts {
            let hex = hex::encode(code_hash);
            *stats.script_counts.entry(hex).or_insert(0) += 1;
        }

        // Classify by type
        if entry.is_cellbase {
            stats.coinbase_count += 1;
            return;
        }

        // ... rest unchanged (has_dao, has_token, has_nft logic stays the same)
```

**Step 2: Update `update_daily_activity_stats` merge to include script_counts**

In the merge logic (~line 585-605), add script_counts merging inside the `Some(mut e)` arm, after the `total_ckb_moved` merge:

```rust
                // Merge script counts
                for (code_hash, count) in &accumulated.script_counts {
                    *e.script_counts.entry(code_hash.clone()).or_insert(0) += count;
                }
```

**Step 3: Update existing tests**

In the existing test helper `make_entry` and test calls, add the `scripts` parameter. Update each test that calls `accumulate_activity_stats`:

```rust
    #[test]
    fn test_coinbase_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let entry = make_entry(500_00000000, true, vec![]);
        let scripts = vec![vec![0x11; 32]]; // lock script
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.coinbase_count, 1);
        assert_eq!(stats.transfer_count, 0);
        assert_eq!(stats.total_ckb_moved, 500_00000000);
        // Script counted even for coinbase
        assert_eq!(*stats.script_counts.get(&hex::encode(&[0x11; 32])).unwrap(), 1);
    }
```

Apply same pattern to all other tests: pass `&scripts` as second arg.

Add a test for script counting:

```rust
    #[test]
    fn test_script_counts_accumulated() {
        let mut stats = DailyActivityStats::default();
        let lock_ch = vec![0xAA; 32];
        let type_ch = vec![0xBB; 32];

        // Activity with lock + type script
        let entry = make_entry(-100_00000000, false, vec![
            AssetChange::DaoDeposit { capacity: 100_00000000 },
        ]);
        let scripts = vec![lock_ch.clone(), type_ch.clone()];
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);

        assert_eq!(*stats.script_counts.get(&hex::encode(&lock_ch)).unwrap(), 1);
        assert_eq!(*stats.script_counts.get(&hex::encode(&type_ch)).unwrap(), 1);

        // Second activity with same lock script
        let entry2 = make_entry(-50_00000000, false, vec![]);
        let scripts2 = vec![lock_ch.clone()];
        BatchWriter::accumulate_activity_stats(&entry2, &scripts2, &mut stats);

        assert_eq!(*stats.script_counts.get(&hex::encode(&lock_ch)).unwrap(), 2);
        assert_eq!(*stats.script_counts.get(&hex::encode(&type_ch)).unwrap(), 1);
    }
```

**Step 4: Run tests**

```bash
cargo test -p ckbadger-indexer -- activity_stats --nocapture
```

Expected: all tests pass.

**Step 5: Run cargo check on full indexer**

```bash
cargo check -p ckbadger-indexer
```

Expected: compiles (with Task 3 changes).

**Step 6: Commit (together with Task 3)**

```bash
git add crates/indexer/src/db/writer/statistics.rs crates/indexer/src/db/writer/activities.rs crates/indexer/src/sync/batch.rs crates/indexer/src/sync/latest_activities.rs
git commit -m "feat(indexer): track per-script activity counts in daily stats"
```

---

### Task 5: API — Add script counts to response with name resolution

**Files:**

- Modify: `crates/api/src/routes/statistics.rs:3121-3194`

**Step 1: Add `ScriptCountEntry` response type**

After `DailyActivityStatsResponse` (~line 3138), add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCountEntry {
    pub code_hash: String,
    pub name: Option<String>,
    pub count: u32,
}
```

**Step 2: Add `script_counts` to `DailyActivityStatsResponse`**

Add field after `total_ckb_moved`:

```rust
    pub script_counts: Vec<ScriptCountEntry>,
```

**Step 3: Update the handler to resolve names**

In `get_daily_activity_stats` (~line 3150-3194), after fetching `all_stats`, build a name cache by bulk-reading script infos. Then populate `script_counts` in the map:

```rust
async fn get_daily_activity_stats(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DailyActivityStatsParams>,
) -> ApiResult<Vec<DailyActivityStatsResponse>> {
    let days = params.days.clamp(1, 365);
    let cache_key = format!("stats:daily-activity-stats:{}", days);

    if let Some(cached) = state
        .cache
        .get::<Vec<DailyActivityStatsResponse>>(&cache_key)
        .await
    {
        return ok(cached);
    }

    let all_stats = state
        .store
        .list_daily_activity_stats()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Collect all unique code_hashes across all days for name resolution
    let mut all_code_hashes: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, s) in &all_stats {
        for ch in s.script_counts.keys() {
            all_code_hashes.insert(ch.clone());
        }
    }

    // Resolve names from CF_SCRIPT_INFO
    let mut name_cache: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for hex_ch in &all_code_hashes {
        if let Ok(bytes) = hex::decode(hex_ch) {
            if let Ok(Some(info)) = state.store.get_script_info(&bytes) {
                name_cache.insert(hex_ch.clone(), info.name);
            }
        }
    }

    // Take the last N days (list is sorted ascending by date)
    let result: Vec<DailyActivityStatsResponse> = all_stats
        .into_iter()
        .rev()
        .take(days as usize)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|(date, s)| {
            let script_counts: Vec<ScriptCountEntry> = s
                .script_counts
                .iter()
                .map(|(ch, count)| ScriptCountEntry {
                    code_hash: format!("0x{}", ch),
                    name: name_cache.get(ch).cloned().flatten(),
                    count: *count,
                })
                .collect();

            DailyActivityStatsResponse {
                date,
                transfer_count: s.transfer_count,
                dao_deposit_count: s.dao_deposit_count,
                dao_withdraw_request_count: s.dao_withdraw_request_count,
                dao_withdraw_complete_count: s.dao_withdraw_complete_count,
                token_count: s.token_count,
                nft_count: s.nft_count,
                coinbase_count: s.coinbase_count,
                unique_address_count: s.unique_address_count,
                total_ckb_moved: s.total_ckb_moved.to_string(),
                script_counts,
            }
        })
        .collect();

    state.cache.set(&cache_key, &result, CacheTtl::CHART).await;
    ok(result)
}
```

**Step 4: Run cargo check**

```bash
cargo check -p ckbadger-api
```

Expected: compiles.

**Step 5: Commit**

```bash
git add crates/api/src/routes/statistics.rs
git commit -m "feat(api): add script_counts with name resolution to daily activity stats"
```

---

### Task 6: Frontend — Exclude coinbase + add scripts pie chart

**Files:**

- Modify: `frontend/lib/api.ts:457-468`
- Modify: `frontend/components/activity-breakdown.tsx`

**Step 1: Update TypeScript types**

In `api.ts`, update `DailyActivityStats` interface (~line 457):

```typescript
interface ScriptCountEntry {
  codeHash: string;
  name: string | null;
  count: number;
}

interface DailyActivityStats {
  date: string;
  transferCount: number;
  daoDepositCount: number;
  daoWithdrawRequestCount: number;
  daoWithdrawCompleteCount: number;
  tokenCount: number;
  nftCount: number;
  coinbaseCount: number;
  uniqueAddressCount: number;
  totalCkbMoved: string;
  scriptCounts: ScriptCountEntry[];
}
```

**Step 2: Update `activity-breakdown.tsx`**

Remove Coinbase from `ACTIVITY_COLORS` and `buildChartData`:

```typescript
const ACTIVITY_COLORS: Record<string, string> = {
  Transfer: '#8ce00a',
  'DAO Deposit': '#00d7eb',
  'DAO Withdraw': '#ffb900',
  Token: '#a78bfa',
  NFT: '#f472b6',
};

function buildChartData(stats: DailyActivityStats) {
  return [
    { label: 'Transfer', value: stats.transferCount, color: ACTIVITY_COLORS.Transfer },
    { label: 'DAO Deposit', value: stats.daoDepositCount, color: ACTIVITY_COLORS['DAO Deposit'] },
    {
      label: 'DAO Withdraw',
      value: stats.daoWithdrawRequestCount + stats.daoWithdrawCompleteCount,
      color: ACTIVITY_COLORS['DAO Withdraw'],
    },
    { label: 'Token', value: stats.tokenCount, color: ACTIVITY_COLORS.Token },
    { label: 'NFT', value: stats.nftCount, color: ACTIVITY_COLORS.NFT },
  ].filter((s) => s.value > 0);
}
```

Update `totalActivities` to exclude coinbase:

```typescript
const totalActivities = today
  ? today.transferCount +
    today.daoDepositCount +
    today.daoWithdrawRequestCount +
    today.daoWithdrawCompleteCount +
    today.tokenCount +
    today.nftCount
  : 0;
```

Add script chart data builder:

```typescript
const SCRIPT_COLORS = [
  '#8ce00a',
  '#00d7eb',
  '#ffb900',
  '#a78bfa',
  '#f472b6',
  '#64748b',
  '#f59e0b',
  '#10b981',
  '#ef4444',
  '#6366f1',
];

function buildScriptChartData(stats: DailyActivityStats) {
  return stats.scriptCounts
    .filter((s) => s.count > 0)
    .sort((a, b) => b.count - a.count)
    .map((s, i) => ({
      label: s.name || `${s.codeHash.slice(0, 10)}...`,
      value: s.count,
      color: SCRIPT_COLORS[i % SCRIPT_COLORS.length],
    }));
}
```

Add second pie chart in the component's render, after the existing one. The full component return becomes:

```tsx
<div className="flex flex-col items-center gap-4">
  <PieChart data={chartData} size={200} formatValue={(v) => v.toLocaleString()} />
  <div className="grid w-full grid-cols-3 gap-x-4 gap-y-2">
    <StatItem label="Activities" value={totalActivities.toLocaleString()} />
    <StatItem label="Addresses" value={today.uniqueAddressCount.toLocaleString()} />
    <StatItem label="Volume" value={formatCkbCompact(today.totalCkbMoved).value + ' CKB'} />
  </div>
  {scriptChartData.length > 0 && (
    <>
      <div className="text-text-muted mt-2 font-mono text-[10px] uppercase tracking-wider">
        Script Usage
      </div>
      <PieChart data={scriptChartData} size={200} formatValue={(v) => v.toLocaleString()} />
    </>
  )}
</div>
```

Add `const scriptChartData = today ? buildScriptChartData(today) : [];` after `chartData`.

**Step 3: Run type-check + lint**

```bash
cd frontend && pnpm type-check && pnpm lint
```

Expected: passes.

**Step 4: Format**

```bash
pnpm format
```

**Step 5: Commit**

```bash
git add frontend/lib/api.ts frontend/components/activity-breakdown.tsx
git commit -m "feat(frontend): exclude coinbase from asset breakdown, add scripts pie chart"
```

---

### Task 7: Verification — Build and test everything

**Step 1: Run all Rust checks**

```bash
cargo check && cargo clippy
```

Expected: no errors or warnings.

**Step 2: Run all Rust tests**

```bash
cargo test
```

Expected: all pass.

**Step 3: Run frontend checks**

```bash
cd frontend && pnpm type-check && pnpm lint && npx vitest run
```

Expected: all pass.

**Step 4: Format**

```bash
pnpm format
```

Commit any formatting changes if needed.
