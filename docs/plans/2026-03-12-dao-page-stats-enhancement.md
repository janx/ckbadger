# DAO Page Stats Enhancement

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add addresses, claimed compensation, unclaimed compensation stats to the DAO page, plus 24hr change deltas on 4 of the 6 stats (all except Estimated APC and Average Deposit Time).

**Architecture:** Add `unclaimed_compensation` field to `DaoDailySnapshot` and `DaoSnapshotInput` so the indexer stores it daily. Compute 24hr deltas server-side in the API by diffing today's vs yesterday's snapshots. Return delta fields in `DaoStatisticsResponse`. Frontend renders a 2x3 stat grid with trend indicators.

**Tech Stack:** Rust (store types, indexer writer, API route), TypeScript/React (frontend page)

---

### Task 1: Add `unclaimed_compensation` to `DaoDailySnapshot`

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs:142-173` (DaoDailySnapshot struct)

**Step 1: Add the field**

In `DaoDailySnapshot`, add after `cum_treasury`:

```rust
    /// Unclaimed DAO compensation at end of day (shannons).
    #[serde(default)]
    pub unclaimed_compensation: u128,
```

The `#[serde(default)]` ensures backward compat with existing serialized snapshots (they'll deserialize as 0).

**Step 2: Run tests to verify no breakage**

Run: `cargo test -p ckbadger-store -- dao`
Expected: All existing DAO tests pass (serde(default) makes this backward-compatible).

**Step 3: Commit**

```
feat(store): add unclaimed_compensation to DaoDailySnapshot
```

---

### Task 2: Wire `unclaimed_compensation` through indexer snapshot pipeline

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs:37-58` (DaoSnapshotInput struct)
- Modify: `crates/indexer/src/db/writer/statistics.rs:490-519` (update_dao_daily_snapshot)
- Modify: `crates/indexer/src/sync/batch.rs:4742-4755` (DaoSnapshotInput construction)

**Step 1: Add field to `DaoSnapshotInput`**

In `crates/indexer/src/db/writer/statistics.rs`, add to `DaoSnapshotInput` after `cum_treasury`:

```rust
    /// Unclaimed DAO compensation at this point (shannons).
    pub unclaimed_compensation: u128,
```

**Step 2: Pass it through in `update_dao_daily_snapshot`**

In `update_dao_daily_snapshot`, update the `DaoDailySnapshot` construction to include:

```rust
            unclaimed_compensation: dao_snapshot.unclaimed_compensation,
```

**Step 3: Supply the value in `batch.rs`**

In `crates/indexer/src/sync/batch.rs` where `DaoSnapshotInput` is constructed (~line 4742), we need to compute unclaimed compensation. This requires knowing tip_ar and iterating active deposits — but we're already tracking `running_total_deposited` (net active deposit total).

The unclaimed compensation is protocol-level: it equals the total AR-based compensation accrued by all active deposits. In the batch sync context, we don't have per-deposit AR data available at snapshot time (that's computed in `refresh_latest_dao_statistics`).

**Simpler approach:** Set `unclaimed_compensation: 0` in batch.rs (bulk sync). The value will be correctly populated by `refresh_latest_dao_statistics` which already computes it and stores it in `DaoLatestStatistics`. For the 24hr delta, we only need the _latest_ unclaimed value (from `DaoLatestStatistics`) and yesterday's snapshot.

Actually — rethinking this. Computing unclaimed comp in batch sync requires per-deposit AR scanning which is expensive. Instead, let's compute the delta differently for unclaimed: use `DaoLatestStatistics.unclaimed_compensation` (current) and we don't need the snapshot field at all for the delta — we just need yesterday's `DaoLatestStatistics` or yesterday's value cached somewhere.

**Revised approach:** Don't add unclaimed_compensation to `DaoDailySnapshot` at all. Instead:

- For deposit, depositors, claimed compensation: compute 24hr delta from daily snapshots (today vs yesterday).
- For unclaimed compensation: the delta is volatile (changes every block as AR grows). Show no delta for unclaimed, OR compute it from the two most recent daily snapshots' state.

Wait — the user approved adding it to `DaoDailySnapshot`. Let's keep it simple: set it to 0 during bulk sync, and have `refresh_latest_dao_statistics` update the _current day's_ snapshot with the computed unclaimed value. This way the latest snapshot always has the correct unclaimed value, and yesterday's is also correct (it was the latest when yesterday ended).

In `batch.rs`, add:

```rust
unclaimed_compensation: 0,
```

This is acceptable because bulk sync snapshots are historical and won't be "today's" snapshot when the API reads deltas.

**Step 4: Update `refresh_latest_dao_statistics` to also write unclaimed to today's snapshot**

In `crates/indexer/src/db/writer/statistics.rs`, after line 996 (after writing DaoLatestStatistics), add code to update today's DaoDailySnapshot with the computed unclaimed_compensation value:

```rust
// Update today's dao daily snapshot with the latest unclaimed compensation
if let Some(mut today_snapshot) = self.store.get_latest_dao_daily_snapshot()? {
    today_snapshot.unclaimed_compensation = unclaimed_compensation;
    let date_key = today_snapshot.date.replace('-', "");
    let snap_key = keys::encode_stats_key(
        keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
        date_key.as_bytes(),
    );
    let snap_value = bincode::serialize(&today_snapshot)?;
    self.store.put_stats_key(&snap_key, &snap_value)?;
}
```

**Step 5: Run tests**

Run: `cargo test -p ckbadger-indexer -- dao`
Expected: PASS

**Step 6: Commit**

```
feat(indexer): wire unclaimed_compensation through DAO snapshot pipeline
```

---

### Task 3: Add 24hr delta fields to API response

**Files:**

- Modify: `crates/api/src/routes/dao.rs:257-276` (DaoStatisticsResponse)
- Modify: `crates/api/src/routes/dao.rs:532-558` (dao_latest_to_response)
- Modify: `crates/api/src/routes/dao.rs:691-788` (get_statistics)

**Step 1: Add delta fields to `DaoStatisticsResponse`**

After `burnt_ckb`, add:

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deposit_change_24h: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depositors_change_24h: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_compensation_change_24h: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unclaimed_compensation_change_24h: Option<String>,
```

All CKB delta values are in CKB string format (not shannons) for frontend convenience.

**Step 2: Add helper to compute deltas**

Add a struct and function before `get_statistics`:

```rust
#[derive(Default)]
struct DaoDeltas {
    deposit_change: Option<String>,
    depositors_change: Option<i32>,
    claimed_compensation_change: Option<String>,
    unclaimed_compensation_change: Option<String>,
}

fn compute_dao_24h_deltas(state: &AppState) -> DaoDeltas {
    let Ok(Some(latest)) = state.store.get_latest_dao_daily_snapshot() else {
        return DaoDeltas::default();
    };
    // Get previous day's date
    let Ok(latest_date) = chrono::NaiveDate::parse_from_str(&latest.date, "%Y-%m-%d") else {
        return DaoDeltas::default();
    };
    let prev_date = latest_date - chrono::Duration::days(1);
    let prev_key = prev_date.format("%Y%m%d").to_string();
    let Ok(Some(prev)) = state.store.get_dao_daily_snapshot(&prev_key) else {
        return DaoDeltas::default();
    };

    let deposit_delta = latest.total_deposited - prev.total_deposited;
    let depositors_delta = latest.depositors_count - prev.depositors_count;
    let claimed_delta = latest.compensation - prev.compensation;
    let unclaimed_delta = latest.unclaimed_compensation as i128 - prev.unclaimed_compensation as i128;

    DaoDeltas {
        deposit_change: Some(shannon_to_ckb(&deposit_delta.to_string())),
        depositors_change: Some(depositors_delta as i32),
        claimed_compensation_change: Some(shannon_to_ckb(&claimed_delta.to_string())),
        unclaimed_compensation_change: Some(shannon_to_ckb(&unclaimed_delta.to_string())),
    }
}
```

**Step 3: Wire deltas into `get_statistics`**

In the `get_statistics` function, before building the response, add:

```rust
let deltas = compute_dao_24h_deltas(&state);
```

Then in the `DaoStatisticsResponse` construction, add the delta fields:

```rust
        deposit_change_24h: deltas.deposit_change,
        depositors_change_24h: deltas.depositors_change,
        claimed_compensation_change_24h: deltas.claimed_compensation_change,
        unclaimed_compensation_change_24h: deltas.unclaimed_compensation_change,
```

**Step 4: Update `dao_latest_to_response`**

This path uses pre-cached `DaoLatestStatistics` — it also needs deltas. Pass `state` to it, or compute deltas in the caller. Simplest: compute deltas in the caller and pass them:

Change `dao_latest_to_response` signature to accept deltas:

```rust
fn dao_latest_to_response(latest: &ckbadger_store::DaoLatestStatistics, deltas: DaoDeltas) -> DaoStatisticsResponse {
```

And add the delta fields to the response construction.

In `get_statistics`, the early-return path (~line 698) becomes:

```rust
if latest.tip_block_number == latest_block_number {
    let deltas = compute_dao_24h_deltas(&state);
    return ok(dao_latest_to_response(&latest, deltas));
}
```

And move the `compute_dao_24h_deltas` call to after the slow-path computation too.

**Step 5: Run `cargo check`**

Run: `cargo check -p ckbadger-api`
Expected: PASS

**Step 6: Commit**

```
feat(api): add 24hr delta fields to DAO statistics response
```

---

### Task 4: Update frontend TypeScript types

**Files:**

- Modify: `frontend/lib/api.ts:752-769` (DaoStatistics interface)

**Step 1: Add delta fields to `DaoStatistics` interface**

```typescript
interface DaoStatistics {
  totalDeposited: string;
  totalDepositedCkb: string;
  totalDepositors: number;
  activeDeposits: number;
  totalCompensationPaid: string;
  totalCompensationPaidCkb: string;
  unclaimedCompensation: string;
  unclaimedCompensationCkb: string;
  averageDepositDays: string;
  estimatedApc: string;
  miningReward: string;
  miningRewardCkb: string;
  depositCompensation: string;
  depositCompensationCkb: string;
  burnt: string;
  burntCkb: string;
  depositChange24h?: string;
  depositorsChange24h?: number;
  claimedCompensationChange24h?: string;
  unclaimedCompensationChange24h?: string;
}
```

**Step 2: Commit**

```
feat(frontend): add 24hr delta fields to DaoStatistics type
```

---

### Task 5: Add `formatCompactCkbDelta` utility

**Files:**

- Modify: `frontend/lib/utils.ts` (add helper function)

**Step 1: Add compact delta formatter**

This formats a CKB delta string (can be negative) into a compact form like "2.17M", "631.62K", etc. for use in trend indicators.

```typescript
export function formatCompactCkbDelta(ckbDelta: string): {
  compact: string;
  direction: 'up' | 'down' | 'neutral';
} {
  const num = parseFloat(ckbDelta);
  if (isNaN(num) || num === 0) return { compact: '0', direction: 'neutral' };

  const direction = num > 0 ? 'up' : num < 0 ? 'down' : 'neutral';
  const abs = Math.abs(num);

  let compact: string;
  if (abs >= 1_000_000_000) {
    compact = `${(abs / 1_000_000_000).toFixed(2)}B`;
  } else if (abs >= 1_000_000) {
    compact = `${(abs / 1_000_000).toFixed(2)}M`;
  } else if (abs >= 1_000) {
    compact = `${(abs / 1_000).toFixed(2)}K`;
  } else if (abs >= 1) {
    compact = abs.toFixed(2);
  } else {
    compact = abs.toFixed(4);
  }

  return { compact, direction };
}
```

**Step 2: Commit**

```
feat(frontend): add formatCompactCkbDelta utility
```

---

### Task 6: Redesign DAO page stats to 2x3 grid with trends

**Files:**

- Modify: `frontend/app/dao/page.tsx:297-348` (stats panel section)

**Step 1: Replace the hero + 3-card layout with a 2x3 grid**

Replace the `<TerminalPanel className="mb-4" glow>` block (lines 314-348) with:

```tsx
<TerminalPanel className="mb-4" glow>
  <TerminalPanelContent>
    <div className="grid gap-6 md:grid-cols-3">
      <StatCard
        label="Deposit"
        value={
          stats
            ? (() => {
                const f = formatCkbValue(stats.totalDepositedCkb);
                return (
                  <>
                    {f.integer}
                    <span className="text-text-bright/50 text-[0.85em]">.{f.decimal}</span>
                    <span className="text-text-dim ml-1 text-[0.85em]">CKB</span>
                  </>
                );
              })()
            : '...'
        }
        trend={
          stats?.depositChange24h
            ? (() => {
                const d = formatCompactCkbDelta(stats.depositChange24h);
                return { direction: d.direction, value: d.compact };
              })()
            : undefined
        }
      />
      <StatCard
        label="Claimed Compensation"
        value={
          stats
            ? (() => {
                const f = formatCkbValue(stats.totalCompensationPaidCkb);
                return (
                  <>
                    {f.integer}
                    <span className="text-text-bright/50 text-[0.85em]">.{f.decimal}</span>
                    <span className="text-text-dim ml-1 text-[0.85em]">CKB</span>
                  </>
                );
              })()
            : '...'
        }
        trend={
          stats?.claimedCompensationChange24h
            ? (() => {
                const d = formatCompactCkbDelta(stats.claimedCompensationChange24h);
                return { direction: d.direction, value: d.compact };
              })()
            : undefined
        }
      />
      <StatCard
        label="Estimated APC"
        value={stats?.estimatedApc ? `${stats.estimatedApc}%` : '...'}
        valueClassName="font-display"
      />
      <StatCard
        label="Addresses"
        value={stats ? formatNumber(stats.totalDepositors) : '...'}
        trend={
          stats?.depositorsChange24h
            ? {
                direction:
                  stats.depositorsChange24h > 0
                    ? 'up'
                    : stats.depositorsChange24h < 0
                      ? 'down'
                      : 'neutral',
                value: Math.abs(stats.depositorsChange24h).toLocaleString(),
              }
            : undefined
        }
      />
      <StatCard label="Average Deposit Time" value={stats?.averageDepositDays || '...'} />
      <StatCard
        label="Unclaimed Compensation"
        value={
          stats
            ? (() => {
                const f = formatCkbValue(stats.unclaimedCompensationCkb);
                return (
                  <>
                    {f.integer}
                    <span className="text-text-bright/50 text-[0.85em]">.{f.decimal}</span>
                    <span className="text-text-dim ml-1 text-[0.85em]">CKB</span>
                  </>
                );
              })()
            : '...'
        }
        trend={
          stats?.unclaimedCompensationChange24h
            ? (() => {
                const d = formatCompactCkbDelta(stats.unclaimedCompensationChange24h);
                return { direction: d.direction, value: d.compact };
              })()
            : undefined
        }
      />
    </div>
  </TerminalPanelContent>
</TerminalPanel>
```

**Step 2: Add import for `formatCompactCkbDelta`**

Update the import from `@/lib/utils`:

```typescript
import {
  formatTimeAgo,
  formatCkbAmount,
  formatCkbValue,
  formatNumber,
  formatCompactCkbDelta,
} from '@/lib/utils';
```

**Step 3: Run frontend checks**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 4: Commit**

```
feat(frontend): redesign DAO page stats to 2x3 grid with 24hr deltas
```

---

### Task 7: Final verification

**Step 1: Run all backend tests**

Run: `cargo test -- dao`
Expected: PASS

**Step 2: Run frontend tests**

Run: `cd frontend && npx vitest run`
Expected: PASS

**Step 3: Run pre-commit checks**

Run: `cargo check && cargo clippy && cd frontend && pnpm type-check && pnpm lint`
Expected: PASS
