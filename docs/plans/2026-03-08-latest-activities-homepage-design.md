# Latest Activities Homepage Section — Design

## Goal

Add a "Latest Activities" section to the homepage below the Transaction Pipeline, showing a global cross-address activity feed as card-based UI with real-time WebSocket updates.

## Principle Alignment

- **CKB Native**: Surfaces CKB-native activity types (DAO, Spore, xUDT, .bit, mNFT) on the homepage
- **Local First**: In-memory ring buffer — no new CF, zero persistence overhead, rebuild-friendly
- **Agent Friendly**: New REST endpoint `GET /activities/latest` for programmatic access

## Architecture

```
Indexer process                    API process                   Frontend
┌──────────────┐    IPC push      ┌─────────────┐   REST/WS    ┌──────────┐
│ Ring buffer   │ ──────────────→  │ /activities  │ ──────────→  │ Cards UI │
│ VecDeque<64>  │  new activities  │ /latest      │  poll+push   │ 5-8 rows │
│               │                  │ WS broadcast │              │          │
└──────────────┘                   └─────────────┘              └──────────┘
```

### Ring Buffer (Indexer)

- Lives in indexer process as `Arc<Mutex<VecDeque<GlobalActivityItem>>>`
- Capacity: 64 entries (oldest evicted on overflow)
- Populated as indexer builds activities during block processing
- Each `(lock_hash, ActivityEntry)` pair is one entry — one tx with 5 addresses = 5 entries
- On restart: buffer starts empty, fills as new blocks are indexed
- Bulk sync: buffer fills normally (latest 64 near tip)

### IPC Extension

New command `GetLatestActivities { limit: usize }` in `crates/ipc/src/`.
Returns `Vec<GlobalActivityItem>` where:

```rust
struct GlobalActivityItem {
    lock_hash: Vec<u8>,
    entry: ActivityEntry,
}
```

### API Endpoint

`GET /activities/latest?limit=8` (max 64, default 8).
Proxies to indexer via IPC `GetLatestActivities` command.

Response:

```json
{
  "data": [
    {
      "address": "ckb1qzd...r4x8",
      "txHash": "0x...",
      "blockNumber": 14523001,
      "txIndex": 0,
      "timestamp": "1700000000",
      "ckbDelta": "125000000000",
      "occupiedDelta": "0",
      "isCellbase": false,
      "assetChanges": [...],
      "peers": ["0x..."]
    }
  ]
}
```

### WebSocket Extension

New event type on existing WS connection: `latestActivities`.
Pushed when indexer processes a new block. Payload: array of `GlobalActivity` items from that block.

### Frontend Component

**`LatestActivities`** (`frontend/components/latest-activities.tsx`)

- Full-width `TerminalPanel` between Pipeline and Latest Blocks/Transactions
- Header: "Latest Activities" with active/inactive indicator, "VIEW ALL →" link
- Initial load: `GET /activities/latest?limit=8`
- Real-time: subscribe to WS `latestActivities` events, prepend new cards with slide-down animation + brief highlight glow
- Poll fallback: `refetchInterval: 10000` (consistent with other homepage sections)

### Card Layout

```
┌────────────────────────────────────────────────────────────────┐
│  ckb1qzd...r4x8                        Received   3 min ago   │
│  0xa3f8...2c4d  Block #14,523,001                             │
│  +1,250.00000000 CKB                                          │
│  [SEAL +12,000.00] [Spore mint] [DAO Deposit 500 CKB]         │
└────────────────────────────────────────────────────────────────┘
```

- **Row 1**: Truncated CKB address (linked) | Type badge | Relative time
- **Row 2**: Truncated tx hash (amber, linked) | Block # (green, linked)
- **Row 3**: CKB delta (green positive / red negative / omitted if zero)
- **Row 4**: Asset change badges (reuse `AssetChangeBadge` pattern from address page)

### Activity Type Badge

- `isCellbase` → **Coinbase** (purple)
- `ckbDelta > 0` → **Received** (green)
- `ckbDelta < 0` → **Sent** (red)
- `ckbDelta == 0 && has peers` → **Self** (slate)

### lock_hash → Address Conversion

The ring buffer stores raw `lock_hash` bytes. The API must convert to CKB address format for the response. Use the existing `lock_script_hash_to_address()` utility (or add one if needed) that reconstructs the address from the lock script stored in the domain DB, or falls back to displaying truncated lock_hash hex.

## Data Shape

```typescript
interface GlobalActivity {
  address: string; // CKB address (from lock_hash)
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

## Component Breakdown

| Component          | File                                                  | Purpose                                              |
| ------------------ | ----------------------------------------------------- | ---------------------------------------------------- |
| Ring buffer        | `crates/indexer/src/` (new module or extend existing) | `VecDeque<GlobalActivityItem>` behind `Arc<Mutex<>>` |
| IPC command        | `crates/ipc/src/`                                     | `GetLatestActivities` request/response               |
| API endpoint       | `crates/api/src/routes/activities.rs`                 | `GET /activities/latest` handler                     |
| WS message         | `crates/api/src/ws/`                                  | New `latestActivities` event type                    |
| `LatestActivities` | `frontend/components/latest-activities.tsx`           | Panel with cards, WS subscription                    |
| Frontend API       | `frontend/lib/api.ts`                                 | `getLatestActivities()` method                       |

## Homepage Layout (Updated)

```tsx
<SyncBanner />
<HomeCharts />
<EpochProgress + MiniStatsCards />     {/* 2-col grid */}
<PipelinePreview />
<LatestActivities />                   {/* NEW */}
<LatestBlocks + LatestTransactions />   {/* 2-col grid */}
<LiveIndicator />
```

## Out of Scope

- Activities explorer page (future — "VIEW ALL →" link can be a placeholder)
- Activity filtering on homepage (no filter buttons, show all types)
- Persistence of ring buffer across restarts
- Deduplication by tx_hash (each address perspective is distinct)
