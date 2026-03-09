# Activity Breakdown & Stats Charts — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add daily activity stats (type breakdown, unique addresses, CKB volume) powering a homepage donut chart and four new charts-page sections.

**Architecture:** New stats prefix `ACTIVITY_DAILY (0x1D)` in existing `CF_STATS_CHAIN` (domain store). Indexer accumulates per-day stats from `build_activities_for_block()` output. API serves via `GET /stats/daily-activities`. Frontend splits homepage activities into 2-col layout (list + donut) and adds four charts.

**Tech Stack:** Rust (bincode, serde, rocksdb), Axum 0.8, React 19, TanStack Query v5, pure SVG charts

---

### Task 1: Store — Add DailyActivityStats Type and Key Prefix

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs` (after `AssetAction` enum, ~line 917)
- Modify: `crates/ckbadger-store/src/keys.rs` (stats_prefix module, ~line 261)
- Modify: `crates/ckbadger-store/src/store.rs` (stats_cf_by_prefix match, ~line 1305)

**Step 1: Add type to types.rs**

Add after `AssetAction` enum (~line 917):

```rust
// ============================================
// Group J: Daily Activity Stats
// ============================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyActivityStats {
    /// Plain CKB transfers (no asset changes, not coinbase)
    pub transfer_count: u32,
    /// DAO deposit activities
    pub dao_deposit_count: u32,
    /// DAO withdraw request activities
    pub dao_withdraw_request_count: u32,
    /// DAO withdraw completion activities
    pub dao_withdraw_complete_count: u32,
    /// Token (xUDT/sUDT) transfer activities
    pub token_count: u32,
    /// NFT activities (Spore + .bit + M-NFT + did:ckb)
    pub nft_count: u32,
    /// Coinbase (miner reward) activities
    pub coinbase_count: u32,
    /// Number of unique addresses active this day
    pub unique_address_count: u32,
    /// Sum of absolute CKB deltas in shannons
    pub total_ckb_moved: u128,
}
```

**Step 2: Add key prefix to keys.rs**

In `stats_prefix` module, after `NFT_COLLECTION_OWNER` (~line 261):

```rust
pub const ACTIVITY_DAILY: u8 = 0x1D;
```

Add flat re-export after the existing ones:

```rust
pub const STATS_PREFIX_ACTIVITY_DAILY: u8 = stats_prefix::ACTIVITY_DAILY;
```

**Step 3: Route prefix to CF_STATS_CHAIN in store.rs**

In `stats_cf_by_prefix()`, add `keys::STATS_PREFIX_ACTIVITY_DAILY` to the CF_STATS_CHAIN match arm (~line 1305):

```rust
keys::STATS_PREFIX_DAILY
| keys::STATS_PREFIX_HOURLY
| keys::STATS_PREFIX_EPOCH
| keys::STATS_PREFIX_MINER
| keys::STATS_PREFIX_BLOCK_TIME_DIST
| keys::STATS_PREFIX_EPOCH_TIME_DIST
| keys::STATS_PREFIX_DAILY_BLOCK
| keys::STATS_PREFIX_ACTIVITY_DAILY => Ok(self.cf_stats_chain()),
```

**Step 4: Run cargo check**

```bash
cargo check -p ckbadger-store
```

Expected: compiles successfully.

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/types.rs crates/ckbadger-store/src/keys.rs crates/ckbadger-store/src/store.rs
git commit -m "feat(store): add DailyActivityStats type and ACTIVITY_DAILY key prefix"
```

---

### Task 2: Store — Add Read/Write Ops for Daily Activity Stats

**Files:**

- Modify: `crates/ckbadger-store/src/stats_ops.rs` (add methods after existing daily stats ops)
- Test: inline `#[cfg(test)]` in same file

**Step 1: Write the failing test**

Add at the bottom of `stats_ops.rs` inside a `#[cfg(test)]` module (create one if it doesn't exist):

```rust
#[cfg(test)]
mod daily_activity_stats_tests {
    use super::*;
    use tempfile::TempDir;

    fn open_test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_get_daily_activity_stats_missing_returns_none() {
        let (_dir, store) = open_test_store();
        let result = store.get_daily_activity_stats("20260309").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_put_and_get_daily_activity_stats_roundtrip() {
        let (_dir, store) = open_test_store();
        let stats = DailyActivityStats {
            transfer_count: 100,
            dao_deposit_count: 10,
            dao_withdraw_request_count: 3,
            dao_withdraw_complete_count: 2,
            token_count: 50,
            nft_count: 20,
            coinbase_count: 8640,
            unique_address_count: 500,
            total_ckb_moved: 1_000_000_00000000,
        };
        store.put_daily_activity_stats("20260309", &stats).unwrap();
        let loaded = store.get_daily_activity_stats("20260309").unwrap().unwrap();
        assert_eq!(loaded.transfer_count, 100);
        assert_eq!(loaded.coinbase_count, 8640);
        assert_eq!(loaded.unique_address_count, 500);
        assert_eq!(loaded.total_ckb_moved, 1_000_000_00000000);
    }

    #[test]
    fn test_list_daily_activity_stats_returns_all_dates() {
        let (_dir, store) = open_test_store();
        let s1 = DailyActivityStats { transfer_count: 10, ..Default::default() };
        let s2 = DailyActivityStats { transfer_count: 20, ..Default::default() };
        store.put_daily_activity_stats("20260308", &s1).unwrap();
        store.put_daily_activity_stats("20260309", &s2).unwrap();

        let all = store.list_daily_activity_stats().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0, "20260308");
        assert_eq!(all[0].1.transfer_count, 10);
        assert_eq!(all[1].0, "20260309");
        assert_eq!(all[1].1.transfer_count, 20);
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test -p ckbadger-store daily_activity_stats -- --nocapture
```

Expected: FAIL — methods `get_daily_activity_stats`, `put_daily_activity_stats`, `list_daily_activity_stats` not found.

**Step 3: Write the implementation**

Add to `stats_ops.rs` inside the `impl CkbadgerStore` block, after existing daily stats methods:

```rust
    // ---- Daily activity stats ----

    pub fn get_daily_activity_stats(
        &self,
        date: &str,
    ) -> anyhow::Result<Option<DailyActivityStats>> {
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_DAILY, date.as_bytes());
        match self.get_cf(self.cf_stats_chain(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_daily_activity_stats(
        &self,
        date: &str,
        stats: &DailyActivityStats,
    ) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_DAILY, date.as_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats_chain(), &key, &value)
    }

    pub fn list_daily_activity_stats(
        &self,
    ) -> anyhow::Result<Vec<(String, DailyActivityStats)>> {
        let prefix = [keys::stats_prefix::ACTIVITY_DAILY];
        let iter = self.prefix_iterator_cf(self.cf_stats_chain(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_chain in list_daily_activity_stats: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let date_bytes = &key[1..]; // skip prefix byte
            let date_str = std::str::from_utf8(date_bytes)
                .map_err(|e| {
                    anyhow::anyhow!("invalid UTF-8 date in daily activity stats key: {}", e)
                })?
                .to_string();
            let stats: DailyActivityStats = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize daily activity stats: date={}, error={}",
                    date_str,
                    e
                )
            })?;
            results.push((date_str, stats));
        }
        Ok(results)
    }
```

**Step 4: Run tests to verify they pass**

```bash
cargo test -p ckbadger-store daily_activity_stats -- --nocapture
```

Expected: 3 tests PASS.

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/stats_ops.rs
git commit -m "feat(store): add read/write ops for daily activity stats"
```

---

### Task 3: Indexer — Accumulate Daily Activity Stats During Block Processing

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs` (add accumulation method)
- Modify: `crates/indexer/src/sync/batch.rs` (call it after `build_activities_for_block`)

This task hooks into the existing activity processing flow. After `build_activities_for_block()` returns `Vec<(lock_hash, ActivityEntry)>`, we classify each entry and accumulate counts into `DailyActivityStats`.

**Step 1: Add classify + accumulate method to statistics.rs**

Add to `impl BatchWriter`:

```rust
    /// Classify an ActivityEntry and accumulate counts into DailyActivityStats.
    /// Call once per (lock_hash, ActivityEntry) pair from build_activities_for_block().
    pub fn accumulate_activity_stats(
        entry: &ActivityEntry,
        stats: &mut DailyActivityStats,
    ) {
        // Total CKB moved (absolute value)
        stats.total_ckb_moved = stats.total_ckb_moved.saturating_add(
            entry.ckb_delta.unsigned_abs(),
        );

        // Classify by type
        if entry.is_cellbase {
            stats.coinbase_count += 1;
            return;
        }

        // Check asset changes for specific types
        let mut has_dao = false;
        let mut has_token = false;
        let mut has_nft = false;

        for change in &entry.asset_changes {
            match change {
                AssetChange::DaoDeposit { .. } => {
                    stats.dao_deposit_count += 1;
                    has_dao = true;
                }
                AssetChange::DaoWithdrawRequest { .. } => {
                    stats.dao_withdraw_request_count += 1;
                    has_dao = true;
                }
                AssetChange::DaoWithdrawComplete { .. } => {
                    stats.dao_withdraw_complete_count += 1;
                    has_dao = true;
                }
                AssetChange::Token { .. } => {
                    has_token = true;
                }
                AssetChange::Dob { .. } | AssetChange::Nft { .. } => {
                    has_nft = true;
                }
            }
        }

        if has_token {
            stats.token_count += 1;
        }
        if has_nft {
            stats.nft_count += 1;
        }
        // Plain transfer: no asset changes, not coinbase
        if !has_dao && !has_token && !has_nft {
            stats.transfer_count += 1;
        }
    }

    /// Write accumulated daily activity stats for a date.
    /// Reads existing stats for the date, merges with accumulated, writes back.
    pub fn update_daily_activity_stats(
        &self,
        date: &str,
        accumulated: &DailyActivityStats,
        unique_addresses: u32,
        batch: &mut StoreBatch,
    ) -> anyhow::Result<()> {
        let existing = self.store.get_daily_activity_stats(date)?;
        let merged = match existing {
            Some(mut e) => {
                e.transfer_count += accumulated.transfer_count;
                e.dao_deposit_count += accumulated.dao_deposit_count;
                e.dao_withdraw_request_count += accumulated.dao_withdraw_request_count;
                e.dao_withdraw_complete_count += accumulated.dao_withdraw_complete_count;
                e.token_count += accumulated.token_count;
                e.nft_count += accumulated.nft_count;
                e.coinbase_count += accumulated.coinbase_count;
                e.unique_address_count = unique_addresses;
                e.total_ckb_moved = e.total_ckb_moved.saturating_add(accumulated.total_ckb_moved);
                e
            }
            None => {
                let mut s = accumulated.clone();
                s.unique_address_count = unique_addresses;
                s
            }
        };
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_DAILY, date.as_bytes());
        let value = bincode::serialize(&merged)?;
        batch.put_stats(&key, &value);
        Ok(())
    }
```

**Step 2: Hook into batch.rs**

In `batch.rs`, the indexer processes blocks. There are two code paths — bulk sync and live sync. In both, `build_activities_for_block()` is called and returns activities. After that call, accumulate stats.

The batch processor needs to maintain per-date accumulators. The pattern is:

1. Before the block loop: create `HashMap<String, DailyActivityStats>` and `HashMap<String, HashSet<[u8; 32]>>` for unique addresses per date
2. Inside the block loop, after `build_activities_for_block()`: for each `(lock_hash, entry)`, compute the date from `entry.timestamp`, call `accumulate_activity_stats()`, insert lock_hash into the date's HashSet
3. After the block loop, before batch commit: for each date in the accumulators, call `update_daily_activity_stats()`

**Important context for the implementer:**

- `entry.timestamp` is Unix epoch in milliseconds. Convert to date string using UTC+8 timezone (CKB convention — see `docs/prompts/BULK_SYNC.md` and existing code that uses `ckb_timestamp_to_utc8_date()` or similar).
- Look for existing date conversion code in `statistics.rs` — there's likely a helper like `timestamp_to_ckb_date()` or `ckb_utc8_date()`. Use the same function.
- Both bulk sync path (~line 4687) and live sync path (~line 5832) need this hook.

**Step 3: Run cargo check**

```bash
cargo check -p ckbadger-indexer
```

Expected: compiles successfully.

**Step 4: Run existing tests to verify no regression**

```bash
cargo test -p ckbadger-indexer -- --nocapture
```

Expected: all existing tests pass.

**Step 5: Commit**

```bash
git add crates/indexer/src/db/writer/statistics.rs crates/indexer/src/sync/batch.rs
git commit -m "feat(indexer): accumulate daily activity stats during block processing"
```

---

### Task 4: API — Add Daily Activity Stats Endpoint

**Files:**

- Modify: `crates/api/src/routes/statistics.rs` (add handler + route)
- Modify: `crates/api/src/response.rs` (add response type, if needed — or inline in statistics.rs)

**Step 1: Add response type and handler**

In `statistics.rs`, add the response type:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyActivityStatsResponse {
    pub date: String,
    pub transfer_count: u32,
    pub dao_deposit_count: u32,
    pub dao_withdraw_request_count: u32,
    pub dao_withdraw_complete_count: u32,
    pub token_count: u32,
    pub nft_count: u32,
    pub coinbase_count: u32,
    pub unique_address_count: u32,
    pub total_ckb_moved: String,
}
```

Add query params:

```rust
#[derive(Debug, Deserialize)]
pub struct DailyActivityStatsParams {
    #[serde(default = "default_days")]
    days: u32,
}

fn default_days() -> u32 {
    30
}
```

Add the handler:

```rust
async fn get_daily_activity_stats(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DailyActivityStatsParams>,
) -> ApiResult<Vec<DailyActivityStatsResponse>> {
    let days = params.days.clamp(1, 365);
    let cache_key = format!("stats:daily-activity-stats:{}", days);

    if let Some(cached) = state.cache.get::<Vec<DailyActivityStatsResponse>>(&cache_key).await {
        return ok(cached);
    }

    let all_stats = state.store.list_daily_activity_stats()?;

    // Take the last N days
    let result: Vec<DailyActivityStatsResponse> = all_stats
        .into_iter()
        .rev()
        .take(days as usize)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|(date, s)| DailyActivityStatsResponse {
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
        })
        .collect();

    state
        .cache
        .set(&cache_key, &result, std::time::Duration::from_secs(30))
        .await;
    ok(result)
}
```

**Step 2: Register the route**

In `statistics.rs`, add to the `routes()` function:

```rust
.route("/stats/daily-activities", get(get_daily_activity_stats))
```

**Step 3: Run cargo check**

```bash
cargo check -p ckbadger-api
```

Expected: compiles successfully.

**Step 4: Commit**

```bash
git add crates/api/src/routes/statistics.rs
git commit -m "feat(api): add GET /stats/daily-activities endpoint"
```

---

### Task 5: Frontend — Add API Types and Method

**Files:**

- Modify: `frontend/lib/api.ts` (add types + method)

**Step 1: Add TypeScript types**

Add after the existing `GlobalActivity` interface:

```typescript
export interface DailyActivityStats {
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
}
```

**Step 2: Add API method**

Add to the `api` object:

```typescript
getDailyActivityStats: async (days: number = 30): Promise<DailyActivityStats[]> => {
  const res = await fetch(`${BASE}/stats/daily-activities?days=${days}`);
  if (!res.ok) throw new Error('Failed to fetch daily activity stats');
  const json = await res.json();
  return json.data;
},
```

**Step 3: Run type-check**

```bash
cd frontend && pnpm type-check
```

Expected: passes.

**Step 4: Commit**

```bash
git add frontend/lib/api.ts
git commit -m "feat(frontend): add DailyActivityStats type and API method"
```

---

### Task 6: Frontend — Create ActivityBreakdown Component (Donut Chart)

**Files:**

- Create: `frontend/components/activity-breakdown.tsx`

**Step 1: Check existing PieChart component**

Read `frontend/components/ui/pie-chart.tsx` to understand the existing donut/pie chart API. Use it directly. The component likely accepts `data: { label, value, color }[]` and renders an SVG donut.

**Step 2: Create the component**

```tsx
'use client';

import { useQuery } from '@tanstack/react-query';
import { api, type DailyActivityStats } from '@/lib/api';
import { PieChart } from '@/components/ui/pie-chart';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { formatCkbAmount } from '@/lib/utils';

interface ActivityBreakdownProps {
  isRealtime?: boolean;
}

const ACTIVITY_COLORS = {
  Transfer: '#8ce00a', // emphasis lime
  'DAO Deposit': '#00d7eb', // interactive cyan
  'DAO Withdraw': '#ffb900', // warning amber
  Token: '#a78bfa', // purple
  NFT: '#f472b6', // pink
  Coinbase: '#64748b', // slate
};

function buildChartData(stats: DailyActivityStats) {
  const segments = [
    { label: 'Transfer', value: stats.transferCount, color: ACTIVITY_COLORS.Transfer },
    { label: 'DAO Deposit', value: stats.daoDepositCount, color: ACTIVITY_COLORS['DAO Deposit'] },
    {
      label: 'DAO Withdraw',
      value: stats.daoWithdrawRequestCount + stats.daoWithdrawCompleteCount,
      color: ACTIVITY_COLORS['DAO Withdraw'],
    },
    { label: 'Token', value: stats.tokenCount, color: ACTIVITY_COLORS.Token },
    { label: 'NFT', value: stats.nftCount, color: ACTIVITY_COLORS.NFT },
    { label: 'Coinbase', value: stats.coinbaseCount, color: ACTIVITY_COLORS.Coinbase },
  ].filter((s) => s.value > 0);

  return segments;
}

export function ActivityBreakdown({ isRealtime = false }: ActivityBreakdownProps) {
  const { data: stats, isLoading } = useQuery({
    queryKey: ['daily-activity-stats-today'],
    queryFn: () => api.getDailyActivityStats(1),
    refetchInterval: 30000,
  });

  const today = stats?.[0];
  const chartData = today ? buildChartData(today) : [];
  const totalActivities = today
    ? today.transferCount +
      today.daoDepositCount +
      today.daoWithdrawRequestCount +
      today.daoWithdrawCompleteCount +
      today.tokenCount +
      today.nftCount +
      today.coinbaseCount
    : 0;

  return (
    <TerminalPanel variant="default" glow={isRealtime}>
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'}>
        Activity Breakdown
      </TerminalPanelHeader>
      <TerminalPanelContent>
        {isLoading || !today ? (
          <div className="flex h-full items-center justify-center">
            <div className="bg-base-elevated h-32 w-32 animate-pulse rounded-full" />
          </div>
        ) : (
          <div className="flex flex-col items-center gap-4">
            <PieChart data={chartData} size={180} />
            <div className="grid w-full grid-cols-2 gap-x-4 gap-y-2">
              <StatItem label="Total Activities" value={totalActivities.toLocaleString()} />
              <StatItem
                label="Unique Addresses"
                value={today.uniqueAddressCount.toLocaleString()}
              />
              <StatItem
                label="CKB Volume"
                value={formatCkbAmount(today.totalCkbMoved).short + ' CKB'}
              />
            </div>
          </div>
        )}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}

function StatItem({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-text-muted font-mono text-[10px] uppercase tracking-wider">{label}</div>
      <div className="text-emphasis font-mono text-sm">{value}</div>
    </div>
  );
}
```

**Important:** Check the actual `PieChart` component props before implementing. The above uses a `data` array with `{ label, value, color }` objects — adjust to match the actual API.

**Step 3: Run type-check + lint**

```bash
cd frontend && pnpm type-check && pnpm lint
```

Expected: passes.

**Step 4: Commit**

```bash
git add frontend/components/activity-breakdown.tsx
git commit -m "feat(frontend): add ActivityBreakdown donut chart component"
```

---

### Task 7: Frontend — Split Homepage Layout to 2-Column Activities

**Files:**

- Modify: `frontend/components/home-content.tsx` (change layout)
- Modify: `frontend/components/latest-activities.tsx` (reduce to 6 items)

**Step 1: Update latest-activities.tsx to show 6 items**

Change the query limit from 8 to 6:

```typescript
queryFn: () => api.getLatestActivities(6),
```

And the slice:

```typescript
activities?.slice(0, 6).map((activity) => {
```

**Step 2: Update home-content.tsx layout**

Replace the full-width `LatestActivities` section:

```tsx
// Before:
<div className="mt-4">
  <LatestActivities isRealtime={isConnected} />
</div>

// After:
<div className="mt-4 grid gap-4 lg:grid-cols-2">
  <LatestActivities isRealtime={isConnected} />
  <ActivityBreakdown isRealtime={isConnected} />
</div>
```

Add the import:

```typescript
import { ActivityBreakdown } from '@/components/activity-breakdown';
```

**Step 3: Run type-check + lint**

```bash
cd frontend && pnpm type-check && pnpm lint
```

Expected: passes.

**Step 4: Commit**

```bash
git add frontend/components/home-content.tsx frontend/components/latest-activities.tsx
git commit -m "feat(frontend): split homepage activities into 2-col layout with breakdown"
```

---

### Task 8: Frontend — Add Activity Charts to Charts Page

**Files:**

- Modify: `frontend/lib/api.ts` (add chart endpoint methods)
- Modify: `frontend/app/charts/page.tsx` (add chart previews to Activities section)
- Create: `frontend/app/charts/activity-volume/page.tsx`
- Create: `frontend/app/charts/activity-type-breakdown/page.tsx`
- Create: `frontend/app/charts/active-addresses/page.tsx`
- Create: `frontend/app/charts/ckb-volume/page.tsx`

**Step 1: Add chart API methods**

The existing `GET /stats/daily-activities?days=N` endpoint returns all the data needed. We don't need separate chart endpoints — the frontend transforms `DailyActivityStats[]` into chart data.

Add helper methods to `api` object in `api.ts`:

```typescript
getActivityVolumeChart: async (): Promise<ChartResponse> => {
  const stats = await api.getDailyActivityStats(365);
  return {
    data: stats.map((s) => ({
      date: `${s.date.slice(0, 4)}-${s.date.slice(4, 6)}-${s.date.slice(6, 8)}`,
      value: String(
        s.transferCount +
          s.daoDepositCount +
          s.daoWithdrawRequestCount +
          s.daoWithdrawCompleteCount +
          s.tokenCount +
          s.nftCount +
          s.coinbaseCount
      ),
    })),
    title: 'Daily Activity Volume',
    yAxisLabel: 'Activities',
  };
},

getActivityTypeBreakdownChart: async (): Promise<StackedAreaChartResponse> => {
  const stats = await api.getDailyActivityStats(365);
  return {
    data: stats.map((s) => ({
      date: `${s.date.slice(0, 4)}-${s.date.slice(4, 6)}-${s.date.slice(6, 8)}`,
      values: {
        transfer: String(s.transferCount),
        dao: String(s.daoDepositCount + s.daoWithdrawRequestCount + s.daoWithdrawCompleteCount),
        token: String(s.tokenCount),
        nft: String(s.nftCount),
        coinbase: String(s.coinbaseCount),
      },
    })),
    series: [
      { key: 'transfer', label: 'Transfer', color: '#8ce00a' },
      { key: 'dao', label: 'DAO', color: '#00d7eb' },
      { key: 'token', label: 'Token', color: '#a78bfa' },
      { key: 'nft', label: 'NFT', color: '#f472b6' },
      { key: 'coinbase', label: 'Coinbase', color: '#64748b' },
    ],
    title: 'Activity Type Breakdown',
  };
},

getActiveAddressesChart: async (): Promise<ChartResponse> => {
  const stats = await api.getDailyActivityStats(365);
  return {
    data: stats.map((s) => ({
      date: `${s.date.slice(0, 4)}-${s.date.slice(4, 6)}-${s.date.slice(6, 8)}`,
      value: String(s.uniqueAddressCount),
    })),
    title: 'Daily Active Addresses',
    yAxisLabel: 'Addresses',
  };
},

getCkbVolumeChart: async (): Promise<ChartResponse> => {
  const stats = await api.getDailyActivityStats(365);
  return {
    data: stats.map((s) => ({
      date: `${s.date.slice(0, 4)}-${s.date.slice(4, 6)}-${s.date.slice(6, 8)}`,
      value: s.totalCkbMoved,
    })),
    title: 'Daily CKB Volume',
    yAxisLabel: 'CKB',
  };
},
```

**Step 2: Add chart previews to charts page**

In `frontend/app/charts/page.tsx`, in the "Activities" `ChartSection`, add the new chart previews after the existing ones:

```tsx
<ChartSection title="Activities">
  {/* existing charts */}
  <LineChartPreview data={transactionCount} href="/charts/transaction-count" />
  <MultiSeriesPreview data={cellCount} href="/charts/cell-count" defaultSeries="liveCells" />
  <StackedAreaPreview data={hodlWave} href="/charts/hodl-wave" isPercentage />
  {/* new activity charts */}
  <LineChartPreview data={activityVolume} href="/charts/activity-volume" />
  <StackedAreaPreview data={activityTypeBreakdown} href="/charts/activity-type-breakdown" />
  <LineChartPreview data={activeAddresses} href="/charts/active-addresses" />
  <LineChartPreview data={ckbVolume} href="/charts/ckb-volume" chartType="bar" />
</ChartSection>
```

Add the corresponding queries at the top of the component (follow the existing pattern with `useQuery`).

**Step 3: Create individual chart pages**

Each page follows the trivial pattern:

`frontend/app/charts/activity-volume/page.tsx`:

```tsx
'use client';
import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function ActivityVolumePage() {
  return (
    <ChartPage
      title="Daily Activity Volume"
      queryKey="activity-volume"
      queryFn={api.getActivityVolumeChart}
    />
  );
}
```

`frontend/app/charts/active-addresses/page.tsx`:

```tsx
'use client';
import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function ActiveAddressesPage() {
  return (
    <ChartPage
      title="Daily Active Addresses"
      queryKey="active-addresses"
      queryFn={api.getActiveAddressesChart}
    />
  );
}
```

`frontend/app/charts/ckb-volume/page.tsx`:

```tsx
'use client';
import { ChartPage } from '@/components/charts/chart-page';
import { api } from '@/lib/api';

export default function CkbVolumePage() {
  return (
    <ChartPage
      title="Daily CKB Volume"
      queryKey="ckb-volume"
      queryFn={api.getCkbVolumeChart}
      chartType="bar"
    />
  );
}
```

For the stacked area chart page, check if `ChartPage` supports stacked area or if a separate `StackedAreaChartPage` wrapper exists. If not, create a simple page component:

`frontend/app/charts/activity-type-breakdown/page.tsx`:

```tsx
'use client';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { PageHeader } from '@/components/ui/page-header';
import { api } from '@/lib/api';

export default function ActivityTypeBreakdownPage() {
  const { data } = useQuery({
    queryKey: ['activity-type-breakdown'],
    queryFn: api.getActivityTypeBreakdownChart,
  });

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-6">
        <PageHeader title="Activity Type Breakdown" backHref="/charts" backLabel="Charts" />
        <div className="mt-6">
          {data && (
            <StackedAreaChart data={data.data} series={data.series} height={400} interactive />
          )}
        </div>
      </main>
    </div>
  );
}
```

**Step 4: Run type-check + lint**

```bash
cd frontend && pnpm type-check && pnpm lint
```

Expected: passes.

**Step 5: Commit**

```bash
git add frontend/lib/api.ts frontend/app/charts/page.tsx frontend/app/charts/activity-volume/ frontend/app/charts/activity-type-breakdown/ frontend/app/charts/active-addresses/ frontend/app/charts/ckb-volume/
git commit -m "feat(frontend): add activity stats charts to charts page"
```

---

### Task 9: Tests — Add Unit Tests for Activity Classification

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs` (add `#[cfg(test)]` module)

**Step 1: Write tests for accumulate_activity_stats**

```rust
#[cfg(test)]
mod activity_stats_tests {
    use super::*;
    use ckbadger_store::types::{ActivityEntry, AssetChange, DailyActivityStats};

    fn make_entry(ckb_delta: i128, is_cellbase: bool, changes: Vec<AssetChange>) -> ActivityEntry {
        ActivityEntry {
            tx_hash: vec![0; 32],
            block_hash: vec![0; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1700000000000,
            ckb_delta,
            occupied_delta: 0,
            is_cellbase,
            asset_changes: changes,
            peers: vec![],
        }
    }

    #[test]
    fn test_coinbase_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let entry = make_entry(500_00000000, true, vec![]);
        BatchWriter::accumulate_activity_stats(&entry, &mut stats);
        assert_eq!(stats.coinbase_count, 1);
        assert_eq!(stats.transfer_count, 0);
        assert_eq!(stats.total_ckb_moved, 500_00000000);
    }

    #[test]
    fn test_plain_transfer_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let entry = make_entry(-100_00000000, false, vec![]);
        BatchWriter::accumulate_activity_stats(&entry, &mut stats);
        assert_eq!(stats.transfer_count, 1);
        assert_eq!(stats.coinbase_count, 0);
        assert_eq!(stats.total_ckb_moved, 100_00000000);
    }

    #[test]
    fn test_dao_deposit_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let entry = make_entry(
            -200_00000000,
            false,
            vec![AssetChange::DaoDeposit { capacity: 200_00000000 }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &mut stats);
        assert_eq!(stats.dao_deposit_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_token_transfer_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let entry = make_entry(
            0,
            false,
            vec![AssetChange::Token {
                type_script_hash: vec![0xAA; 32],
                delta: 1000,
                symbol: Some("SEAL".to_string()),
                decimals: Some(8),
            }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &mut stats);
        assert_eq!(stats.token_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_nft_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let entry = make_entry(
            0,
            false,
            vec![AssetChange::Dob {
                dob_id: vec![0xBB; 32],
                standard: "spore".to_string(),
                action: ckbadger_store::types::AssetAction::Mint,
            }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &mut stats);
        assert_eq!(stats.nft_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_mixed_activity_counts_all_types() {
        let mut stats = DailyActivityStats::default();
        // Token + DAO in same activity
        let entry = make_entry(
            -500_00000000,
            false,
            vec![
                AssetChange::Token {
                    type_script_hash: vec![0xAA; 32],
                    delta: 1000,
                    symbol: None,
                    decimals: None,
                },
                AssetChange::DaoDeposit { capacity: 100_00000000 },
            ],
        );
        BatchWriter::accumulate_activity_stats(&entry, &mut stats);
        assert_eq!(stats.dao_deposit_count, 1);
        assert_eq!(stats.token_count, 1);
        assert_eq!(stats.transfer_count, 0); // not a plain transfer
    }
}
```

**Step 2: Run tests**

```bash
cargo test -p ckbadger-indexer activity_stats_tests -- --nocapture
```

Expected: all tests pass.

**Step 3: Commit**

```bash
git add crates/indexer/src/db/writer/statistics.rs
git commit -m "test(indexer): add unit tests for activity stats classification"
```

---

### Task 10: Verification — End-to-End Smoke Test

**Step 1: Build everything**

```bash
cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint
```

Expected: all pass.

**Step 2: Run all tests**

```bash
cargo test && cd frontend && npx vitest run
```

Expected: all pass.

**Step 3: Verify the full flow conceptually**

- Indexer processes blocks → calls `build_activities_for_block()` → classifies each activity → accumulates into `DailyActivityStats` per date → writes to `CF_STATS_CHAIN` via batch
- API reads from `CF_STATS_CHAIN` → returns `DailyActivityStatsResponse[]`
- Homepage: left panel shows 6 latest activities, right panel shows donut chart from today's stats
- Charts page: 4 new charts in Activities section

**Step 4: Final commit if any formatting fixes needed**

```bash
cd frontend && pnpm format
git add -A && git commit -m "style: format"
```
