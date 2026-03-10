# Rolling 24h Activity Stats — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the calendar-day activity breakdown with a rolling 24-hour window so the homepage widget always shows meaningful data (not near-zero at the start of each new UTC day).

**Architecture:** Add hourly activity stats (keyed by `YYYYMMDDHH`) alongside existing daily stats, using the same `DailyActivityStats` type and accumulation logic. New API endpoint aggregates the last 24 hourly buckets. Frontend calls the new endpoint. No new column families — hourly stats go into `CF_STATS_CHAIN` with a new prefix byte `ACTIVITY_HOURLY = 0x1E`.

**Tech Stack:** Rust (bincode, serde, rocksdb), Axum 0.8, React 19, TanStack Query v5

**Note on unique_address_count:** The 24h aggregate sums hourly unique counts, which overcounts addresses active in multiple hours. This is acceptable for a dashboard widget. Daily charts continue using the exact per-day count.

**Requires re-sync:** Yes — hourly stats are only written for newly indexed blocks.

---

### Task 1: Store — Add `ACTIVITY_HOURLY` key prefix

**Files:**

- Modify: `crates/ckbadger-store/src/keys.rs:262` (stats_prefix module)
- Modify: `crates/ckbadger-store/src/keys.rs:294` (flat re-exports)

**Step 1: Add prefix constant**

In `stats_prefix` module (after `ACTIVITY_DAILY: u8 = 0x1D`), add:

```rust
    pub const ACTIVITY_HOURLY: u8 = 0x1E;
```

Add flat re-export after `STATS_PREFIX_ACTIVITY_DAILY`:

```rust
pub const STATS_PREFIX_ACTIVITY_HOURLY: u8 = stats_prefix::ACTIVITY_HOURLY;
```

**Step 2: Run cargo check**

```bash
cargo check -p ckbadger-store
```

Expected: compiles.

**Step 3: Commit**

```bash
git add crates/ckbadger-store/src/keys.rs
git commit -m "feat(store): add ACTIVITY_HOURLY key prefix for hourly activity stats"
```

---

### Task 2: Store — Add hourly activity stats read/write operations

**Files:**

- Modify: `crates/ckbadger-store/src/stats_ops.rs:444-485` (after daily activity stats methods)

**Step 1: Write tests**

Add to the existing `#[cfg(test)] mod tests` in `stats_ops.rs`:

```rust
    #[test]
    fn test_get_hourly_activity_stats_missing_returns_none() {
        let (_dir, store) = open_test_store();
        let result = store.get_hourly_activity_stats("2026030912").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_put_and_get_hourly_activity_stats_roundtrip() {
        let (_dir, store) = open_test_store();
        let stats = DailyActivityStats {
            transfer_count: 42,
            total_ckb_moved: 100_00000000,
            unique_address_count: 5,
            ..Default::default()
        };
        store.put_hourly_activity_stats("2026030912", &stats).unwrap();
        let got = store.get_hourly_activity_stats("2026030912").unwrap().unwrap();
        assert_eq!(got.transfer_count, 42);
        assert_eq!(got.total_ckb_moved, 100_00000000);
        assert_eq!(got.unique_address_count, 5);
    }

    #[test]
    fn test_list_hourly_activity_stats_since_returns_range() {
        let (_dir, store) = open_test_store();
        let s1 = DailyActivityStats { transfer_count: 10, ..Default::default() };
        let s2 = DailyActivityStats { transfer_count: 20, ..Default::default() };
        let s3 = DailyActivityStats { transfer_count: 30, ..Default::default() };
        store.put_hourly_activity_stats("2026030910", &s1).unwrap();
        store.put_hourly_activity_stats("2026030911", &s2).unwrap();
        store.put_hourly_activity_stats("2026030912", &s3).unwrap();

        // Query from hour 11 onwards
        let results = store.list_hourly_activity_stats_since("2026030911").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "2026030911");
        assert_eq!(results[0].1.transfer_count, 20);
        assert_eq!(results[1].0, "2026030912");
        assert_eq!(results[1].1.transfer_count, 30);
    }
```

**Step 2: Run tests to verify they fail**

```bash
cargo test -p ckbadger-store hourly_activity_stats -- --nocapture
```

Expected: FAIL — methods not found.

**Step 3: Implement store methods**

Add after `list_daily_activity_stats()` (~line 485):

```rust
    pub fn put_hourly_activity_stats(
        &self,
        hour_key: &str, // "YYYYMMDDHH"
        stats: &DailyActivityStats,
    ) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_HOURLY, hour_key.as_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats_chain(), &key, &value)
    }

    pub fn get_hourly_activity_stats(
        &self,
        hour_key: &str,
    ) -> anyhow::Result<Option<DailyActivityStats>> {
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_HOURLY, hour_key.as_bytes());
        match self.get_cf(self.cf_stats_chain(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// List hourly activity stats from `since_hour` (inclusive) onwards.
    /// `since_hour` is a "YYYYMMDDHH" string.
    pub fn list_hourly_activity_stats_since(
        &self,
        since_hour: &str,
    ) -> anyhow::Result<Vec<(String, DailyActivityStats)>> {
        let start_key =
            keys::encode_stats_key(keys::stats_prefix::ACTIVITY_HOURLY, since_hour.as_bytes());
        let prefix = [keys::stats_prefix::ACTIVITY_HOURLY];
        let iter = self.iterator_cf(
            self.cf_stats_chain(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_chain in list_hourly_activity_stats_since: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let hour_bytes = &key[1..];
            let hour_str = std::str::from_utf8(hour_bytes)
                .map_err(|e| {
                    anyhow::anyhow!("invalid UTF-8 hour in hourly activity stats key: {}", e)
                })?
                .to_string();
            let stats: DailyActivityStats = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize hourly activity stats: hour={}, error={}",
                    hour_str,
                    e
                )
            })?;
            results.push((hour_str, stats));
        }
        Ok(results)
    }
```

**Step 4: Run tests to verify they pass**

```bash
cargo test -p ckbadger-store hourly_activity_stats -- --nocapture
```

Expected: 3 tests PASS.

**Step 5: Commit**

```bash
git add crates/ckbadger-store/src/stats_ops.rs
git commit -m "feat(store): add hourly activity stats read/write/range operations"
```

---

### Task 3: Writer — Add `update_hourly_activity_stats` merge method

**Files:**

- Modify: `crates/indexer/src/db/writer/statistics.rs:632` (after `update_daily_activity_stats`)

**Step 1: Add tests**

In the existing `#[cfg(test)] mod activity_stats_tests` in `statistics.rs`, add:

```rust
    #[test]
    fn test_update_hourly_activity_stats_creates_new() {
        let (_dir, store) = open_test_unified();
        let writer = BatchWriter::new(&store);
        let mut batch = StoreBatch::new(&store);
        let stats = DailyActivityStats {
            transfer_count: 5,
            total_ckb_moved: 50_00000000,
            ..Default::default()
        };
        writer
            .update_hourly_activity_stats("2026030912", &stats, 3, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let got = store.get_hourly_activity_stats("2026030912").unwrap().unwrap();
        assert_eq!(got.transfer_count, 5);
        assert_eq!(got.unique_address_count, 3);
    }

    #[test]
    fn test_update_hourly_activity_stats_merges_existing() {
        let (_dir, store) = open_test_unified();
        let writer = BatchWriter::new(&store);

        // First write
        let mut batch = StoreBatch::new(&store);
        let s1 = DailyActivityStats {
            transfer_count: 5,
            total_ckb_moved: 50_00000000,
            ..Default::default()
        };
        writer
            .update_hourly_activity_stats("2026030912", &s1, 3, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        // Merge write
        let mut batch2 = StoreBatch::new(&store);
        let s2 = DailyActivityStats {
            transfer_count: 10,
            dao_deposit_count: 2,
            total_ckb_moved: 100_00000000,
            ..Default::default()
        };
        writer
            .update_hourly_activity_stats("2026030912", &s2, 7, &mut batch2)
            .unwrap();
        batch2.commit().unwrap();

        let got = store.get_hourly_activity_stats("2026030912").unwrap().unwrap();
        assert_eq!(got.transfer_count, 15);
        assert_eq!(got.dao_deposit_count, 2);
        assert_eq!(got.total_ckb_moved, 150_00000000);
        assert_eq!(got.unique_address_count, 7); // replaced, not summed
    }
```

**Step 2: Run tests to verify they fail**

```bash
cargo test -p ckbadger-indexer update_hourly_activity_stats -- --nocapture
```

Expected: FAIL — method not found.

**Step 3: Implement**

Add after `update_daily_activity_stats()`:

```rust
    /// Write accumulated hourly activity stats for an hour key.
    /// Reads existing stats for the hour, merges with accumulated, writes back.
    pub fn update_hourly_activity_stats(
        &self,
        hour_key: &str,
        accumulated: &DailyActivityStats,
        unique_addresses: u32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_hourly_activity_stats(hour_key)?;
        let merged = match existing {
            Some(mut e) => {
                e.transfer_count += accumulated.transfer_count;
                e.dao_deposit_count += accumulated.dao_deposit_count;
                e.dao_withdraw_request_count += accumulated.dao_withdraw_request_count;
                e.dao_withdraw_complete_count += accumulated.dao_withdraw_complete_count;
                e.token_count += accumulated.token_count;
                e.object_count += accumulated.object_count;
                e.identity_count += accumulated.identity_count;
                e.coinbase_count += accumulated.coinbase_count;
                e.unique_address_count = unique_addresses;
                e.total_ckb_moved = e
                    .total_ckb_moved
                    .saturating_add(accumulated.total_ckb_moved);
                for (code_hash, count) in &accumulated.script_counts {
                    *e.script_counts.entry(code_hash.clone()).or_insert(0) += count;
                }
                e
            }
            None => {
                let mut s = accumulated.clone();
                s.unique_address_count = unique_addresses;
                s
            }
        };
        let key = keys::encode_stats_key(
            keys::stats_prefix::ACTIVITY_HOURLY,
            hour_key.as_bytes(),
        );
        let value = bincode::serialize(&merged)?;
        batch.put_stats(&key, &value);
        Ok(())
    }
```

**Step 4: Run tests to verify they pass**

```bash
cargo test -p ckbadger-indexer update_hourly_activity_stats -- --nocapture
```

Expected: 2 tests PASS.

**Step 5: Commit**

```bash
git add crates/indexer/src/db/writer/statistics.rs
git commit -m "feat(writer): add update_hourly_activity_stats merge method"
```

---

### Task 4: Indexer — Accumulate hourly stats in live sync path

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs:3724-3725` (live sync variable declarations)
- Modify: `crates/indexer/src/sync/batch.rs:6017-6029` (live sync accumulation loop)
- Modify: `crates/indexer/src/sync/batch.rs:6354-6362` (live sync finalization)

**Step 1: Add hourly accumulators alongside daily ones**

At ~line 3724, after `daily_activity_addrs`, add:

```rust
        let mut hourly_activity_accum: HashMap<String, DailyActivityStats> = HashMap::new();
        let mut hourly_activity_addrs: HashMap<String, HashSet<[u8; 32]>> = HashMap::new();
```

**Step 2: Accumulate hourly stats in the activity loop**

At ~line 6017-6029, after the daily accumulation, add hourly accumulation:

```rust
                        // Accumulate hourly activity stats
                        let hour = ckbadger_common::block_date_from_ms(entry.timestamp)
                            .format("%Y%m%d%H")
                            .to_string();
                        let hour_stats =
                            hourly_activity_accum.entry(hour.clone()).or_default();
                        BatchWriter::accumulate_activity_stats(&entry, &scripts, hour_stats);
                        if !entry.is_cellbase && lock_hash.len() == 32 {
                            let mut hash = [0u8; 32];
                            hash.copy_from_slice(&lock_hash);
                            hourly_activity_addrs
                                .entry(hour)
                                .or_default()
                                .insert(hash);
                        }
```

**Step 3: Write hourly stats during finalization**

At ~line 6354-6362, after the daily stats write loop, add:

```rust
            // Write accumulated hourly activity stats
            for (hour, stats) in &hourly_activity_accum {
                let unique_count =
                    hourly_activity_addrs.get(hour).map_or(0, |s| s.len() as u32);
                self.writer.update_hourly_activity_stats(
                    hour,
                    stats,
                    unique_count,
                    &mut stats_batch,
                )?;
            }
```

**Step 4: Run cargo check**

```bash
cargo check -p ckbadger-indexer
```

Expected: compiles.

**Step 5: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "feat(indexer): accumulate hourly activity stats in live sync path"
```

---

### Task 5: Indexer — Accumulate hourly stats in bulk sync path

**Files:**

- Modify: `crates/indexer/src/sync/batch.rs:4769-4771` (bulk sync thread accumulators)
- Modify: `crates/indexer/src/sync/batch.rs:4826-4835` (bulk sync accumulation loop)
- Modify: `crates/indexer/src/sync/batch.rs:4859-4864` (bulk sync thread return)
- Modify: `crates/indexer/src/sync/batch.rs:4878-4884` (bulk sync thread join)
- Modify: `crates/indexer/src/sync/batch.rs:3740-3744` (outer destructuring)

This task mirrors Task 4 but for the bulk sync parallel thread (`h_act`). The exact line numbers may differ from this plan — use the patterns to locate the code.

**Step 1: Add hourly accumulators in the bulk thread**

In the `h_act` thread closure (~line 4769), add after `act_stats_addrs`:

```rust
                            let mut hourly_accum: HashMap<String, DailyActivityStats> =
                                HashMap::new();
                            let mut hourly_addrs: HashMap<String, HashSet<[u8; 32]>> =
                                HashMap::new();
```

**Step 2: Accumulate hourly stats in the bulk activity loop**

In the activity loop (~line 4826-4835), after daily accumulation, add:

```rust
                                    let hour =
                                        ckbadger_common::block_date_from_ms(entry.timestamp)
                                            .format("%Y%m%d%H")
                                            .to_string();
                                    let hour_stats =
                                        hourly_accum.entry(hour.clone()).or_default();
                                    BatchWriter::accumulate_activity_stats(
                                        &entry, &scripts, hour_stats,
                                    );
                                    if lock_hash.len() == 32 {
                                        let mut hash = [0u8; 32];
                                        hash.copy_from_slice(&lock_hash);
                                        hourly_addrs
                                            .entry(hour)
                                            .or_default()
                                            .insert(hash);
                                    }
```

**Step 3: Return hourly accumulators from thread**

Update the thread return tuple (~line 4859-4864) to include hourly data:

```rust
                            Ok((
                                t.elapsed().as_secs_f64() * 1000.0,
                                commit_ms,
                                act_stats_accum,
                                act_stats_addrs,
                                hourly_accum,
                                hourly_addrs,
                            ))
```

**Step 4: Destructure hourly data at join site**

At the `h_act` join site (~line 4878-4884), update to capture hourly data:

```rust
                    let (t_act_ms, t_act_commit_ms, act_accum, act_addrs, hourly_accum, hourly_addrs) = match h_act {
                        Some(h) => {
                            let (ms, commit_ms, accum, addrs, h_accum, h_addrs) =
                                h.join().expect("T_ACT panicked")?;
                            (ms, commit_ms, accum, addrs, h_accum, h_addrs)
                        }
                        None => (0.0, 0.0, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new()),
                    };
```

**Step 5: Pass hourly data to the outer scope**

Update the outer destructuring (~line 3740-3744) to include `hourly_activity_accum` and `hourly_activity_addrs`. Ensure these are available alongside `daily_activity_accum` and `daily_activity_addrs` for the finalization block.

The bulk sync finalization block at ~line 6354 already handles daily stats. Add hourly stats writing there (same code as Task 4 Step 3) — the hourly accumulators from the bulk thread need to be merged into the outer `hourly_activity_accum`/`hourly_activity_addrs` maps before finalization, or written directly.

**Step 6: Run cargo check**

```bash
cargo check -p ckbadger-indexer
```

Expected: compiles.

**Step 7: Commit**

```bash
git add crates/indexer/src/sync/batch.rs
git commit -m "feat(indexer): accumulate hourly activity stats in bulk sync path"
```

---

### Task 6: API — Add `GET /stats/activity-summary-24h` endpoint

**Files:**

- Modify: `crates/api/src/routes/statistics.rs:35-107` (routes + handler)

**Step 1: Add response type**

After `DailyActivityStatsResponse` (~line 3141), add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySummary24hResponse {
    pub transfer_count: u32,
    pub dao_deposit_count: u32,
    pub dao_withdraw_request_count: u32,
    pub dao_withdraw_complete_count: u32,
    pub token_count: u32,
    pub object_count: u32,
    pub identity_count: u32,
    pub coinbase_count: u32,
    /// Sum of per-hour unique address counts (approximate: overcounts cross-hour addresses)
    pub unique_address_count: u32,
    pub total_ckb_moved: String,
    pub script_counts: Vec<ScriptCountEntry>,
    /// Number of hourly buckets aggregated (0-24)
    pub hours_covered: u32,
}
```

**Step 2: Add handler**

After `get_daily_activity_stats` handler, add:

```rust
async fn get_activity_summary_24h(
    State(state): State<Arc<AppState>>,
) -> ApiResult<ActivitySummary24hResponse> {
    let cache_key = "stats:activity-summary-24h";

    if let Some(cached) = state
        .cache
        .get::<ActivitySummary24hResponse>(cache_key)
        .await
    {
        return ok(cached);
    }

    // Compute the hour key for 24 hours ago
    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::hours(24);
    let since_hour = cutoff.format("%Y%m%d%H").to_string();

    let hourly_stats = state
        .store
        .list_hourly_activity_stats_since(&since_hour)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Aggregate all hourly buckets
    let mut agg = ckbadger_store::DailyActivityStats::default();
    let mut agg_script_counts: HashMap<String, u32> = HashMap::new();
    let hours_covered = hourly_stats.len() as u32;

    for (_hour, s) in &hourly_stats {
        agg.transfer_count += s.transfer_count;
        agg.dao_deposit_count += s.dao_deposit_count;
        agg.dao_withdraw_request_count += s.dao_withdraw_request_count;
        agg.dao_withdraw_complete_count += s.dao_withdraw_complete_count;
        agg.token_count += s.token_count;
        agg.object_count += s.object_count;
        agg.identity_count += s.identity_count;
        agg.coinbase_count += s.coinbase_count;
        agg.unique_address_count += s.unique_address_count;
        agg.total_ckb_moved = agg.total_ckb_moved.saturating_add(s.total_ckb_moved);
        for (code_hash, count) in &s.script_counts {
            *agg_script_counts.entry(code_hash.clone()).or_insert(0) += count;
        }
    }

    // Resolve script names
    let mut name_cache: HashMap<String, Option<String>> = HashMap::new();
    for code_hash_hex in agg_script_counts.keys() {
        if let Ok(bytes) = hex::decode(code_hash_hex) {
            let name = state
                .store
                .get_script_info(&bytes)
                .ok()
                .flatten()
                .and_then(|info| info.name);
            name_cache.insert(code_hash_hex.clone(), name);
        }
    }

    let mut script_counts: Vec<ScriptCountEntry> = agg_script_counts
        .iter()
        .map(|(ch, &count)| ScriptCountEntry {
            code_hash: format!("0x{}", ch),
            name: name_cache.get(ch).cloned().flatten(),
            count,
        })
        .collect();
    script_counts.sort_by(|a, b| b.count.cmp(&a.count));

    let result = ActivitySummary24hResponse {
        transfer_count: agg.transfer_count,
        dao_deposit_count: agg.dao_deposit_count,
        dao_withdraw_request_count: agg.dao_withdraw_request_count,
        dao_withdraw_complete_count: agg.dao_withdraw_complete_count,
        token_count: agg.token_count,
        object_count: agg.object_count,
        identity_count: agg.identity_count,
        coinbase_count: agg.coinbase_count,
        unique_address_count: agg.unique_address_count,
        total_ckb_moved: agg.total_ckb_moved.to_string(),
        script_counts,
        hours_covered,
    };

    state
        .cache
        .set(cache_key, &result, CacheTtl::NETWORK_STATS)
        .await;
    ok(result)
}
```

**Step 3: Register route**

Add to `routes()` (~line 107):

```rust
        .route("/stats/activity-summary-24h", get(get_activity_summary_24h))
```

**Step 4: Run cargo check**

```bash
cargo check -p ckbadger-api
```

Expected: compiles.

**Step 5: Commit**

```bash
git add crates/api/src/routes/statistics.rs
git commit -m "feat(api): add GET /stats/activity-summary-24h endpoint with hourly aggregation"
```

---

### Task 7: Frontend — Add API method for 24h summary

**Files:**

- Modify: `frontend/lib/api.ts:463-476` (types)
- Modify: `frontend/lib/api.ts:1433-1435` (api methods)
- Modify: `frontend/lib/api.ts:1303-1309` (exports)

**Step 1: Add TypeScript type**

After `DailyActivityStats` interface (~line 476), add:

```typescript
interface ActivitySummary24h {
  transferCount: number;
  daoDepositCount: number;
  daoWithdrawRequestCount: number;
  daoWithdrawCompleteCount: number;
  tokenCount: number;
  objectCount: number;
  identityCount: number;
  coinbaseCount: number;
  uniqueAddressCount: number;
  totalCkbMoved: string;
  scriptCounts: ScriptCountEntry[];
  hoursCovered: number;
}
```

**Step 2: Add API method**

After `getDailyActivityStats` (~line 1435), add:

```typescript
  getActivitySummary24h: (): Promise<ActivitySummary24h> => {
    return fetchApi('/stats/activity-summary-24h');
  },
```

**Step 3: Add to exports**

Add `ActivitySummary24h` to the type exports (~line 1308).

**Step 4: Run type-check**

```bash
cd frontend && pnpm type-check
```

Expected: passes.

**Step 5: Commit**

```bash
git add frontend/lib/api.ts
git commit -m "feat(frontend): add ActivitySummary24h type and API method"
```

---

### Task 8: Frontend — Update activity breakdown to use 24h endpoint

**Files:**

- Modify: `frontend/components/activity-breakdown.tsx`

**Step 1: Update component to use new endpoint**

Replace the existing query and data handling:

```typescript
import { api, type ActivitySummary24h } from '@/lib/api';

// ...

export function ActivityBreakdown({ isRealtime = false }: ActivityBreakdownProps) {
  const { data: summary, isLoading } = useQuery({
    queryKey: ['activity-summary-24h'],
    queryFn: () => api.getActivitySummary24h(),
    refetchInterval: 30000,
  });

  const chartData = summary ? buildChartData(summary) : [];
  const scriptChartData = summary ? buildScriptChartData(summary) : [];
  const totalActivities = summary
    ? summary.transferCount +
      summary.daoDepositCount +
      summary.daoWithdrawRequestCount +
      summary.daoWithdrawCompleteCount +
      summary.tokenCount +
      summary.objectCount +
      summary.identityCount
    : 0;
```

Update `buildChartData` and `buildScriptChartData` parameter types from `DailyActivityStats` to `ActivitySummary24h`:

```typescript
function buildChartData(stats: ActivitySummary24h) {
  // ... same body, just parameter type changes
}

function buildScriptChartData(stats: ActivitySummary24h) {
  // ... same body, just parameter type changes
}
```

Update the render section to use `summary` instead of `today`:

```typescript
        ) : !summary ? (
          // ...
        ) : (
          <div className="flex flex-col items-center gap-4">
            <PieChart data={chartData} size={200} formatValue={(v) => v.toLocaleString()} />
            <div className="grid w-full grid-cols-3 gap-x-4 gap-y-2">
              <StatItem label="Activities" value={totalActivities.toLocaleString()} />
              <StatItem label="Addresses" value={summary.uniqueAddressCount.toLocaleString()} />
              <StatItem
                label="Volume"
                value={formatCkbCompact(summary.totalCkbMoved).value + ' CKB'}
              />
            </div>
            {scriptChartData.length > 0 && (
              // ... same script chart section
            )}
          </div>
```

Remove unused `DailyActivityStats` import — only import `ActivitySummary24h`.

**Step 2: Update header to show "Last 24h"**

In the `TerminalPanelHeader`, update the label:

```typescript
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'}>
        Activity Breakdown (24h)
      </TerminalPanelHeader>
```

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
git add frontend/components/activity-breakdown.tsx
git commit -m "feat(frontend): switch activity breakdown to rolling 24h window"
```

---

### Task 9: Verification

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

**Step 5: Verify store boundary**

- `CF_STATS_CHAIN` is a domain CF (mutable) — correct for hourly stats that merge on write.
- No changes to append-only store (`CF_CELLS`).
- Domain vs append-only target confirmed: yes. Append-only update/delete path check: pass (not touched).
