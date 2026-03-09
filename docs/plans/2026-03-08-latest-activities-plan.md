# Latest Activities Homepage Section — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a "Latest Activities" section to the homepage showing a global cross-address activity feed with real-time updates.

**Architecture:** Indexer maintains an in-memory ring buffer (VecDeque<64>) of latest activities across all addresses. After each batch commit, it serializes the buffer to `CF_SYNC_META` key `LATEST_ACTIVITIES` (existing CF, no new CF). API reads this key from secondary store. WS broadcaster polls and pushes new activities to connected clients. Frontend renders cards in a `TerminalPanel`.

**Tech Stack:** Rust (serde/bincode, rocksdb), TypeScript/React (TanStack Query, WebSocket), Tailwind CSS

**Design doc:** `docs/plans/2026-03-08-latest-activities-homepage-design.md`

**Cross-process note:** Indexer and API run as separate OS processes (supervisor spawns child processes). They communicate through RocksDB (indexer writes primary, API reads secondary). `CF_SYNC_META` is already used for sync_status, runtime_status, memory_stats, etc. Adding one more key is the simplest cross-process communication.

---

### Task 1: Store layer — types + sync_meta key

**Files:**

- Modify: `crates/ckbadger-store/src/types.rs`
- Modify: `crates/ckbadger-store/src/keys.rs:954-965` (sync_meta_keys module)
- Modify: `crates/ckbadger-store/src/sync_ops.rs`

**Step 1: Add `LatestActivityItem` type**

In `crates/ckbadger-store/src/types.rs`, add near the `ActivityEntry` definition:

```rust
/// A single activity item for the global latest-activities feed.
/// Includes lock script components so the API can compute CKB addresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestActivityItem {
    pub lock_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub entry: ActivityEntry,
}
```

**Step 2: Add sync_meta key constant**

In `crates/ckbadger-store/src/keys.rs`, add to `sync_meta_keys` module:

```rust
pub const LATEST_ACTIVITIES: &[u8] = b"latest_activities";
```

**Step 3: Add put/get methods on CkbadgerStore**

In `crates/ckbadger-store/src/sync_ops.rs`:

```rust
pub fn put_latest_activities(&self, items: &[LatestActivityItem]) -> anyhow::Result<()> {
    let data = bincode::serialize(items)?;
    self.put_cf(self.cf_sync_meta(), sync_meta_keys::LATEST_ACTIVITIES, &data)
}

pub fn get_latest_activities(&self) -> anyhow::Result<Vec<LatestActivityItem>> {
    match self.get_cf(self.cf_sync_meta(), sync_meta_keys::LATEST_ACTIVITIES)? {
        Some(data) => Ok(bincode::deserialize(&data)?),
        None => Ok(Vec::new()),
    }
}
```

**Step 4: Write tests**

```rust
#[test]
fn test_latest_activities_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let store = CkbadgerStore::open_domain(dir.path()).unwrap();

    let items = vec![LatestActivityItem {
        lock_hash: vec![0xAA; 32],
        lock_code_hash: vec![0xBB; 32],
        lock_hash_type: 1,
        lock_args: vec![0xCC; 20],
        entry: ActivityEntry {
            tx_hash: vec![0x01; 32],
            block_hash: vec![0x02; 32],
            block_number: 100,
            tx_index: 1,
            timestamp: 1_700_000_000,
            ckb_delta: 500_00000000,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![],
            peers: vec![],
        },
    }];

    store.put_latest_activities(&items).unwrap();
    let loaded = store.get_latest_activities().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].lock_hash, vec![0xAA; 32]);
    assert_eq!(loaded[0].entry.block_number, 100);
}

#[test]
fn test_latest_activities_empty_when_unset() {
    let dir = tempfile::tempdir().unwrap();
    let store = CkbadgerStore::open_domain(dir.path()).unwrap();
    let loaded = store.get_latest_activities().unwrap();
    assert!(loaded.is_empty());
}
```

**Step 5: Run tests**

Run: `cargo test -p ckbadger-store test_latest_activities`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/ckbadger-store/src/types.rs crates/ckbadger-store/src/keys.rs crates/ckbadger-store/src/sync_ops.rs
git commit -m "feat(store): add LatestActivityItem type and sync_meta storage"
```

---

### Task 2: Indexer ring buffer — populate and write to sync_meta

**Files:**

- Create: `crates/indexer/src/sync/latest_activities.rs`
- Modify: `crates/indexer/src/sync/mod.rs` (add module)
- Modify: `crates/indexer/src/sync/batch.rs` (integrate ring buffer)
- Modify: `crates/indexer/src/sync/indexer.rs` (hold ring buffer in Indexer struct)

**Context:** `build_activities_for_block()` returns `Vec<(Vec<u8>, ActivityEntry)>` where Vec<u8> is lock_hash. To get lock script components for CKB address computation, we build a `lock_hash -> script` mapping from the parsed cells available in the batch writer.

**Step 1: Create ring buffer module**

Create `crates/indexer/src/sync/latest_activities.rs`:

```rust
//! In-memory ring buffer for global latest-activities feed.
//!
//! Maintains the most recent N activities across all addresses.
//! Serialized to CF_SYNC_META after each batch commit for cross-process
//! access by the API.

use std::collections::VecDeque;
use std::sync::Mutex;

use ckbadger_store::types::LatestActivityItem;

/// Maximum items in the ring buffer.
const RING_BUFFER_CAPACITY: usize = 64;

pub struct LatestActivitiesBuffer {
    items: Mutex<VecDeque<LatestActivityItem>>,
}

impl LatestActivitiesBuffer {
    pub fn new() -> Self {
        Self {
            items: Mutex::new(VecDeque::with_capacity(RING_BUFFER_CAPACITY)),
        }
    }

    /// Push new activities (newest first). Evicts oldest when full.
    pub fn push_batch(&self, new_items: Vec<LatestActivityItem>) {
        let mut buf = self.items.lock().expect("ring buffer lock poisoned");
        for item in new_items {
            if buf.len() >= RING_BUFFER_CAPACITY {
                buf.pop_back();
            }
            buf.push_front(item);
        }
    }

    /// Snapshot current buffer contents (newest first).
    pub fn snapshot(&self) -> Vec<LatestActivityItem> {
        let buf = self.items.lock().expect("ring buffer lock poisoned");
        buf.iter().cloned().collect()
    }
}
```

**Step 2: Add module to sync/mod.rs**

In `crates/indexer/src/sync/mod.rs`, add:

```rust
pub mod latest_activities;
```

**Step 3: Add ring buffer to Indexer struct**

In `crates/indexer/src/sync/indexer.rs`, add field to the `Indexer` struct:

```rust
use crate::sync::latest_activities::LatestActivitiesBuffer;
use std::sync::Arc;

// Add field:
pub(crate) latest_activities: Arc<LatestActivitiesBuffer>,
```

Initialize in the constructor/builder:

```rust
latest_activities: Arc::new(LatestActivitiesBuffer::new()),
```

**Step 4: Build lock_script mapping helper**

In `crates/indexer/src/sync/latest_activities.rs`, add:

```rust
use std::collections::HashMap;
use crate::db::writer::activities::InputCellView;
use crate::parser::cell::ParsedCell;
use ckbadger_store::types::ActivityEntry;

/// Build lock_hash -> (code_hash, hash_type, args) mapping from parsed tx data.
pub fn collect_lock_scripts(
    inputs: &[InputCellView],
    outputs: &[ParsedCell],
) -> HashMap<Vec<u8>, (Vec<u8>, i16, Vec<u8>)> {
    let mut map = HashMap::new();
    for cell in outputs {
        if cell.lock_script_hash.len() == 32 {
            map.entry(cell.lock_script_hash.clone())
                .or_insert_with(|| {
                    (cell.lock_code_hash.clone(), cell.lock_hash_type, cell.lock_args.clone())
                });
        }
    }
    // Inputs don't carry lock script components in InputCellView.
    // If an address only appears as input (not output), we won't have its script.
    // This is acceptable — the API will fall back to hex display.
    let _ = inputs; // acknowledged: input-only addresses won't have CKB address
    map
}

/// Convert activity pairs + lock script map into LatestActivityItems.
pub fn to_latest_items(
    activities: &[(Vec<u8>, ActivityEntry)],
    lock_scripts: &HashMap<Vec<u8>, (Vec<u8>, i16, Vec<u8>)>,
) -> Vec<LatestActivityItem> {
    activities
        .iter()
        .map(|(lock_hash, entry)| {
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

**Step 5: Integrate into batch writer — both bulk and live sync paths**

In `crates/indexer/src/sync/batch.rs`, after activities are built and written (both bulk sync path ~line 4698 and live sync path ~line 5836):

1. Collect lock scripts from the current block's parsed cells
2. Convert to LatestActivityItems
3. Push to ring buffer
4. After batch commit, write ring buffer snapshot to sync_meta

The exact integration points depend on the batch.rs structure. Look for where `build_activities_for_block` is called and where the batch is committed. Add after the activity write loop:

```rust
// After activity write loop, before batch commit:
{
    let lock_scripts = crate::sync::latest_activities::collect_lock_scripts(
        &tx_view.inputs, // or the collected inputs
        tx_view.outputs,
    );
    let items = crate::sync::latest_activities::to_latest_items(&activities, &lock_scripts);
    self.latest_activities.push_batch(items);
}

// After batch commit succeeds:
{
    let snapshot = self.latest_activities.snapshot();
    if let Err(e) = self.store.put_latest_activities(&snapshot) {
        tracing::warn!("failed to write latest activities to sync_meta: {}", e);
    }
}
```

Note: `collect_lock_scripts` must be called per-block since we iterate per-block activity building. Collect lock scripts from all blocks in the batch, then push all activities at once.

**Step 6: Write tests for ring buffer**

In `crates/indexer/src/sync/latest_activities.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(block_num: i64) -> LatestActivityItem {
        LatestActivityItem {
            lock_hash: vec![block_num as u8; 32],
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            entry: ActivityEntry {
                tx_hash: vec![block_num as u8; 32],
                block_hash: vec![0xA0; 32],
                block_number: block_num,
                tx_index: 0,
                timestamp: 1_700_000_000 + block_num,
                ckb_delta: 100_00000000,
                occupied_delta: 0,
                is_cellbase: false,
                asset_changes: vec![],
                peers: vec![],
            },
        }
    }

    #[test]
    fn test_ring_buffer_push_and_snapshot() {
        let buf = LatestActivitiesBuffer::new();
        buf.push_batch(vec![make_item(1), make_item(2)]);
        let snap = buf.snapshot();
        assert_eq!(snap.len(), 2);
        // Newest first (push_front)
        assert_eq!(snap[0].entry.block_number, 2);
        assert_eq!(snap[1].entry.block_number, 1);
    }

    #[test]
    fn test_ring_buffer_evicts_oldest() {
        let buf = LatestActivitiesBuffer::new();
        // Push more than capacity
        for i in 0..70 {
            buf.push_batch(vec![make_item(i)]);
        }
        let snap = buf.snapshot();
        assert_eq!(snap.len(), 64);
        // Newest should be 69, oldest should be 6 (70 - 64 = 6)
        assert_eq!(snap[0].entry.block_number, 69);
        assert_eq!(snap[63].entry.block_number, 6);
    }
}
```

**Step 7: Run tests**

Run: `cargo test -p ckbadger-indexer latest_activities`
Expected: PASS

**Step 8: Commit**

```bash
git add crates/indexer/src/sync/latest_activities.rs crates/indexer/src/sync/mod.rs crates/indexer/src/sync/batch.rs crates/indexer/src/sync/indexer.rs
git commit -m "feat(indexer): add latest activities ring buffer with sync_meta persistence"
```

---

### Task 3: API endpoint — GET /activities/latest

**Files:**

- Modify: `crates/api/src/routes/activities.rs`

**Context:** Read `LATEST_ACTIVITIES` from domain store (secondary). Convert lock script components to CKB addresses using `script_to_address()` from `crates/api/src/utils/address.rs`.

**Step 1: Add response type and handler**

In `crates/api/src/routes/activities.rs`:

```rust
use crate::utils::address::script_to_address;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalActivityResponse {
    pub address: String,
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub ckb_delta: String,
    pub occupied_delta: String,
    pub is_cellbase: bool,
    pub asset_changes: Vec<AssetChangeResponse>,
    pub peers: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct LatestActivityParams {
    #[serde(default = "default_latest_limit")]
    limit: usize,
}

fn default_latest_limit() -> usize {
    8
}

async fn get_latest_activities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LatestActivityParams>,
) -> ApiResult<Vec<GlobalActivityResponse>> {
    let limit = params.limit.clamp(1, 64);
    let items = state
        .store
        .get_latest_activities()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let network = &state.ckb_network;
    let activities: Vec<GlobalActivityResponse> = items
        .into_iter()
        .take(limit)
        .map(|item| {
            let address = if !item.lock_code_hash.is_empty() {
                script_to_address(
                    &item.lock_code_hash,
                    item.lock_hash_type,
                    &item.lock_args,
                    network,
                )
                .unwrap_or_else(|_| format!("0x{}", hex::encode(&item.lock_hash)))
            } else {
                format!("0x{}", hex::encode(&item.lock_hash))
            };

            GlobalActivityResponse {
                address,
                tx_hash: format!("0x{}", hex::encode(&item.entry.tx_hash)),
                block_number: item.entry.block_number,
                tx_index: item.entry.tx_index,
                timestamp: item.entry.timestamp.to_string(),
                ckb_delta: item.entry.ckb_delta.to_string(),
                occupied_delta: item.entry.occupied_delta.to_string(),
                is_cellbase: item.entry.is_cellbase,
                asset_changes: item.entry.asset_changes.iter().map(convert_asset_change).collect(),
                peers: item.entry.peers.iter().map(|h| format!("0x{}", hex::encode(h))).collect(),
            }
        })
        .collect();

    ok(activities)
}
```

**Step 2: Register route**

In the `routes()` function of `activities.rs`, add:

```rust
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/addresses/{addr}/activities", get(get_address_activities))
        .route("/activities/latest", get(get_latest_activities))
}
```

**Step 3: Run test**

Run: `cargo check -p ckbadger-api`
Expected: PASS (compiles)

**Step 4: Commit**

```bash
git add crates/api/src/routes/activities.rs
git commit -m "feat(api): add GET /activities/latest endpoint"
```

---

### Task 4: WS broadcast — extend block broadcaster

**Files:**

- Modify: `crates/api/src/ws/manager.rs` (add `LatestActivities` broadcast message variant)
- Modify: `crates/api/src/ws/broadcaster.rs` (read and broadcast latest activities)
- Modify: `crates/api/src/ws/handler.rs` (add subscription channel)

**Step 1: Add BroadcastMessage variant**

In `crates/api/src/ws/manager.rs`, add to `BroadcastMessage` enum:

```rust
LatestActivities {
    activities: Vec<serde_json::Value>,
},
```

Add channel to `WsManager`:

```rust
// In struct:
activity_sender: broadcast::Sender<BroadcastMessage>,

// In new():
let (activity_sender, _) = broadcast::channel(256);

// Add methods:
pub fn subscribe_activities(&self) -> broadcast::Receiver<BroadcastMessage> {
    self.activity_sender.subscribe()
}

pub fn broadcast_activities(&self, msg: BroadcastMessage) {
    let _ = self.activity_sender.send(msg);
}
```

**Step 2: Extend block broadcaster**

In `crates/api/src/ws/broadcaster.rs`, inside `start_block_broadcaster`:

After broadcasting a new block (around where the block message is sent), also check for and broadcast latest activities:

```rust
// After broadcasting block, also broadcast latest activities
if sync_mode == SyncMode::Realtime {
    match store.get_latest_activities() {
        Ok(items) if !items.is_empty() => {
            let network = "mainnet"; // or derive from config
            let activities: Vec<serde_json::Value> = items
                .into_iter()
                .take(8)
                .map(|item| {
                    // Serialize to JSON matching GlobalActivityResponse shape
                    serde_json::json!({
                        "address": if !item.lock_code_hash.is_empty() {
                            crate::utils::address::script_to_address(
                                &item.lock_code_hash,
                                item.lock_hash_type,
                                &item.lock_args,
                                network,
                            ).unwrap_or_else(|_| format!("0x{}", hex::encode(&item.lock_hash)))
                        } else {
                            format!("0x{}", hex::encode(&item.lock_hash))
                        },
                        // ... remaining fields
                    })
                })
                .collect();
            ws_manager.broadcast_activities(BroadcastMessage::LatestActivities { activities });
        }
        _ => {}
    }
}
```

Note: The block broadcaster needs access to the domain store. It already has `store: Arc<CkbadgerStore>`. Since `get_latest_activities()` reads from `CF_SYNC_META` in the domain store, this works directly.

**Step 3: Add activity subscription to WS handler**

In `crates/api/src/ws/handler.rs`, add an `activity_rx` receiver alongside `block_rx`, `tx_rx`, `reorg_rx`. Add `"latest_activity"` to the subscription channels. Add a `tokio::select!` branch for activity messages.

**Step 4: Compile check**

Run: `cargo check -p ckbadger-api`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/api/src/ws/manager.rs crates/api/src/ws/broadcaster.rs crates/api/src/ws/handler.rs
git commit -m "feat(ws): broadcast latest activities on new block"
```

---

### Task 5: Frontend types + API method

**Files:**

- Modify: `frontend/lib/api.ts`

**Step 1: Add GlobalActivity type**

Near the existing `Activity` interface in `frontend/lib/api.ts`:

```typescript
interface GlobalActivity {
  address: string;
  txHash: string;
  blockNumber: number;
  txIndex: number;
  timestamp: string;
  ckbDelta: string;
  occupiedDelta: string;
  isCellbase: boolean;
  assetChanges: ActivityAssetChange[];
  peers: string[];
}
```

**Step 2: Add API method**

In the `api` object:

```typescript
getLatestActivities: (limit: number = 8): Promise<GlobalActivity[]> => {
  return fetchApi(`/activities/latest?limit=${limit}`);
},
```

**Step 3: Export type**

Add `GlobalActivity` to the exports.

**Step 4: Commit**

```bash
git add frontend/lib/api.ts
git commit -m "feat(frontend): add GlobalActivity type and getLatestActivities API"
```

---

### Task 6: Frontend component — LatestActivities

**Files:**

- Create: `frontend/components/latest-activities.tsx`

**Context:** Follow patterns from `latest-blocks.tsx` and `latest-transactions.tsx`. Use `TerminalPanel`, `TerminalRow`, `HexDisplay`. Reuse `AssetChangeBadge` pattern from `frontend/app/address/[addr]/client-page.tsx:264-323` (extract to shared component or inline).

**Step 1: Create LatestActivities component**

Create `frontend/components/latest-activities.tsx`:

```tsx
'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { api, type GlobalActivity, type ActivityAssetChange } from '@/lib/api';
import { formatTimeAgo, cn } from '@/lib/utils';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { HexDisplay } from '@/components/ui/hex-display';
import { formatCkbAmount } from '@/lib/utils';

interface LatestActivitiesProps {
  isRealtime?: boolean;
}

export function LatestActivities({ isRealtime = false }: LatestActivitiesProps) {
  const [newTxHash, setNewTxHash] = useState<string | null>(null);
  const prevRef = useRef<string[]>([]);

  const { data: activities, isLoading } = useQuery({
    queryKey: ['latest-activities'],
    queryFn: () => api.getLatestActivities(8),
    refetchInterval: 10000,
  });

  const items = activities ?? [];
  const showSkeleton = isLoading || items.length === 0;

  useEffect(() => {
    if (items.length > 0) {
      const currentKeys = items.map((a) => `${a.txHash}:${a.address}`);
      const prevKeys = prevRef.current;
      if (prevKeys.length > 0) {
        const newItem = currentKeys.find((k) => !prevKeys.includes(k));
        if (newItem) {
          setNewTxHash(newItem);
          setTimeout(() => setNewTxHash(null), 2000);
        }
      }
      prevRef.current = currentKeys;
    }
  }, [items]);

  return (
    <TerminalPanel variant="default" glow={isRealtime}>
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'}>
        Latest Activities
      </TerminalPanelHeader>
      <TerminalPanelContent padding="none">
        {showSkeleton
          ? Array.from({ length: 4 }).map((_, i) => (
              <TerminalRow key={i} hoverable={false}>
                <div className="flex animate-pulse flex-col gap-2">
                  <div className="flex items-center justify-between">
                    <div className="h-4 w-48 rounded bg-slate-800" />
                    <div className="h-4 w-16 rounded bg-slate-800" />
                  </div>
                  <div className="h-3 w-32 rounded bg-slate-800" />
                </div>
              </TerminalRow>
            ))
          : items.slice(0, 8).map((activity, idx) => (
              <TerminalRow
                key={`${activity.txHash}:${activity.address}:${idx}`}
                className={cn(
                  'transition-all duration-500',
                  newTxHash === `${activity.txHash}:${activity.address}` &&
                    'bg-cyan-500/10 shadow-[0_0_8px_rgba(6,182,212,0.15)]'
                )}
              >
                <ActivityCard activity={activity} />
              </TerminalRow>
            ))}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}

function ActivityCard({ activity }: { activity: GlobalActivity }) {
  const delta = BigInt(activity.ckbDelta);
  const typeBadge = getTypeBadge(activity);

  const addressDisplay =
    activity.address.startsWith('ckb1') || activity.address.startsWith('ckt1')
      ? `${activity.address.slice(0, 8)}...${activity.address.slice(-6)}`
      : undefined;

  return (
    <div className="space-y-1.5">
      {/* Row 1: Address | Type badge | Time */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Link
            href={`/address/${activity.address}`}
            className="truncate font-mono text-sm text-cyan-400 hover:text-cyan-300"
          >
            {addressDisplay ?? (
              <HexDisplay
                value={activity.address}
                truncate
                startChars={8}
                endChars={6}
                color="green"
                size="sm"
                showGroupHighlight={false}
              />
            )}
          </Link>
          <span
            className={cn(
              'rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wider',
              typeBadge.className
            )}
          >
            {typeBadge.label}
          </span>
        </div>
        <span className="shrink-0 text-xs text-slate-500">{formatTimeAgo(activity.timestamp)}</span>
      </div>

      {/* Row 2: Tx hash | Block number */}
      <div className="flex items-center gap-3 text-xs">
        <Link href={`/tx/${activity.txHash}`}>
          <HexDisplay
            value={activity.txHash}
            truncate
            startChars={8}
            endChars={6}
            color="amber"
            size="sm"
            showGroupHighlight={false}
          />
        </Link>
        <span className="text-slate-500">Block</span>
        <Link
          href={`/blocks/${activity.blockNumber}`}
          className="hover:text-terminal-green font-mono text-slate-400"
        >
          #{activity.blockNumber.toLocaleString()}
        </Link>
      </div>

      {/* Row 3: CKB delta (if non-zero) */}
      {delta !== 0n && (
        <div className={cn('font-mono text-sm', delta > 0n ? 'text-emerald-400' : 'text-red-400')}>
          {delta > 0n ? '+' : ''}
          {formatCkbAmount(activity.ckbDelta)} CKB
        </div>
      )}

      {/* Row 4: Asset change badges */}
      {activity.assetChanges.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {activity.assetChanges.map((change, i) => (
            <AssetBadge key={i} change={change} />
          ))}
        </div>
      )}
    </div>
  );
}

function getTypeBadge(activity: GlobalActivity): { label: string; className: string } {
  if (activity.isCellbase) {
    return {
      label: 'Coinbase',
      className: 'bg-purple-900/50 text-purple-300 border border-purple-700/50',
    };
  }
  const delta = BigInt(activity.ckbDelta);
  if (delta > 0n) {
    return {
      label: 'Received',
      className: 'bg-emerald-900/50 text-emerald-300 border border-emerald-700/50',
    };
  }
  if (delta < 0n) {
    return { label: 'Sent', className: 'bg-red-900/50 text-red-300 border border-red-700/50' };
  }
  return { label: 'Self', className: 'bg-slate-800 text-slate-400 border border-slate-700/50' };
}

function AssetBadge({ change }: { change: ActivityAssetChange }) {
  switch (change.type) {
    case 'token': {
      const delta = BigInt(change.delta);
      const sign = delta > 0n ? '+' : '';
      const color = delta > 0n ? 'text-emerald-300' : 'text-red-300';
      const label = change.symbol ?? `${change.typeScriptHash.slice(0, 10)}...`;
      return (
        <span
          className={cn(
            'rounded border border-slate-700/60 bg-slate-800/80 px-1.5 py-0.5 font-mono text-[10px]',
            color
          )}
        >
          {label} {sign}
          {change.delta}
        </span>
      );
    }
    case 'dob':
      return (
        <span className="rounded border border-slate-700/60 bg-slate-800/80 px-1.5 py-0.5 text-[10px] text-slate-300">
          {change.standard === 'did_ckb' ? 'did:ckb' : 'Spore'} {change.action}
        </span>
      );
    case 'nft':
      return (
        <span className="rounded border border-slate-700/60 bg-slate-800/80 px-1.5 py-0.5 text-[10px] text-slate-300">
          {change.standard === 'm-nft' ? 'M-NFT' : '.bit'} {change.action}
        </span>
      );
    case 'daoDeposit':
      return (
        <span className="rounded border border-slate-700/60 bg-slate-800/80 px-1.5 py-0.5 text-[10px] text-slate-300">
          DAO Deposit
        </span>
      );
    case 'daoWithdrawRequest':
      return (
        <span className="rounded border border-amber-700/50 bg-amber-900/30 px-1.5 py-0.5 text-[10px] text-amber-300">
          DAO Withdraw Request
        </span>
      );
    case 'daoWithdrawComplete':
      return (
        <span className="rounded border border-emerald-700/50 bg-emerald-900/30 px-1.5 py-0.5 text-[10px] text-emerald-300">
          DAO Withdraw Complete
        </span>
      );
    default:
      return null;
  }
}
```

**Step 2: Type check**

Run: `cd frontend && pnpm type-check`
Expected: PASS

**Step 3: Commit**

```bash
git add frontend/components/latest-activities.tsx
git commit -m "feat(frontend): add LatestActivities component with activity cards"
```

---

### Task 7: Homepage integration

**Files:**

- Modify: `frontend/components/home-content.tsx`

**Step 1: Import and add to layout**

```tsx
import { LatestActivities } from '@/components/latest-activities';
```

Add between `PipelinePreview` and the `LatestBlocks + LatestTransactions` grid:

```tsx
<div className="mt-6">
  <PipelinePreview initialBlocks={initialData.blocks} />
</div>

{/* NEW: Latest Activities */}
<div className="mt-6">
  <LatestActivities isRealtime={isConnected} />
</div>

<div className="mt-8 grid gap-6 lg:grid-cols-2">
```

**Step 2: Type check + lint**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 3: Commit**

```bash
git add frontend/components/home-content.tsx
git commit -m "feat(frontend): add Latest Activities section to homepage"
```

---

### Task 8: Full integration test

**Step 1: Cargo check + clippy**

Run: `cargo check && cargo clippy`
Expected: PASS

**Step 2: Rust tests**

Run: `cargo test --lib`
Expected: PASS

**Step 3: Frontend tests**

Run: `cd frontend && pnpm type-check && pnpm lint`
Expected: PASS

**Step 4: Final commit (if any fixups needed)**

---

## Integration Notes

### Data Flow Summary

```
Indexer (write process)              API (read process)              Frontend
┌─────────────────────┐             ┌──────────────────┐           ┌──────────────┐
│ build_activities     │             │ secondary store  │           │ useQuery     │
│ for_block()         │             │ .refresh() 1s    │           │ 10s poll     │
│       │             │             │       │          │           │      │       │
│       ▼             │             │       ▼          │           │      ▼       │
│ push to VecDeque<64>│             │ get_latest_      │ GET       │ LatestActivities
│       │             │             │ activities()     │ /latest   │ component    │
│       ▼             │  RocksDB    │       │          │◄──────────│              │
│ put_latest_         │────────────►│       ▼          │           │              │
│ activities()        │  sync_meta  │ GlobalActivity   │ WS push   │              │
│ to CF_SYNC_META     │  key        │ Response         │──────────►│              │
└─────────────────────┘             └──────────────────┘           └──────────────┘
```

### Key Decisions

1. **Cross-process via sync_meta**: Single key in existing CF_SYNC_META, no new CF
2. **Lock script in ring buffer**: Output cells provide lock script for CKB address; input-only addresses fall back to hex display
3. **Realtime only broadcast**: WS activity broadcast only in realtime mode (not during fast sync) to avoid noise
4. **No deduplication**: Each address perspective is a distinct card
5. **No cursor pagination**: Fixed limit (max 64), no cursor — this is a live feed, not an explorer
