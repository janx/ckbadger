# Activity Breakdown & Stats Charts — Design

## Goal

1. Split the homepage Latest Activities section into a 2-column layout: activity list (left, 6 items) + activity breakdown stats (right, from today's data)
2. Add activity stats charts to the charts page (historical daily data)

## Principle Alignment

- **CKB Native**: Surfaces CKB-native activity type distribution (DAO, Spore, xUDT, .bit, mNFT) as first-class stats
- **Local First**: Derived stats in domain store — recalculable, rebuild-friendly
- **Agent Friendly**: New REST endpoint `GET /stats/daily-activities` for programmatic access

## Data Layer

### New CF: `CF_DAILY_ACTIVITY_STATS` (domain store)

- Key: `date` as `YYYYMMDD` u32
- Value: `DailyActivityStats`

```rust
struct DailyActivityStats {
    transfer_count: u32,
    dao_deposit_count: u32,
    dao_withdraw_request_count: u32,
    dao_withdraw_complete_count: u32,
    token_count: u32,
    nft_count: u32,        // Spore + .bit + M-NFT
    coinbase_count: u32,
    unique_address_count: u32,
    total_ckb_moved: u128,  // absolute sum of all CKB deltas
}
```

Updated by the indexer during block processing. The writer accumulates counts from each block's activities, reads the current day's row, increments, and writes back.

Reorg handling: recalculate the affected day's row from scratch or decrement counts for rolled-back blocks.

## Homepage Layout Change

```
Before:                          After:
┌──────────────────────────┐     ┌────────────┬─────────────┐
│   Latest Activities (8)  │     │ Latest     │ Activity    │
│   (full width)           │     │ Activities │ Breakdown   │
│                          │     │ (6 items)  │ (donut +    │
│                          │     │            │  summary)   │
└──────────────────────────┘     └────────────┴─────────────┘
```

Both panels in a `lg:grid-cols-2` grid, matching height.

### Left Panel: Latest Activities

- Existing `LatestActivities` component
- Reduce display count from 8 to 6
- No other changes

### Right Panel: Activity Breakdown

- Donut chart: activity type distribution (today's data)
  - Segments: Transfer, DAO Deposit, DAO Withdraw, Token, NFT/Spore, Coinbase
- Below donut: summary stats
  - Total activities today
  - Unique addresses today
  - Total CKB volume today

Data source: `GET /stats/daily-activities?days=1`

## Charts Page

Four new chart sections using the same `CF_DAILY_ACTIVITY_STATS` data:

1. **Activity Volume** — daily total activity count (line chart)
2. **Activity Type Breakdown** — stacked area chart (transfer / DAO / token / NFT / coinbase)
3. **Active Addresses** — daily unique address count (line chart)
4. **CKB Volume** — daily total CKB moved (bar chart)

Data source: `GET /stats/daily-activities?days=30|90|365`

## API

### `GET /stats/daily-activities`

Query params:

- `days` (optional, default 30, max 365): number of days to return

Response:

```json
{
  "data": [
    {
      "date": "2026-03-09",
      "transferCount": 1234,
      "daoDepositCount": 56,
      "daoWithdrawRequestCount": 12,
      "daoWithdrawCompleteCount": 8,
      "tokenCount": 340,
      "nftCount": 89,
      "coinbaseCount": 8640,
      "uniqueAddressCount": 2100,
      "totalCkbMoved": "1234567800000000"
    }
  ]
}
```

## Component Breakdown

| Component    | File                                              | Purpose                                                            |
| ------------ | ------------------------------------------------- | ------------------------------------------------------------------ |
| CF + types   | `crates/ckbadger-store/src/`                      | `CF_DAILY_ACTIVITY_STATS`, `DailyActivityStats` type, key encoding |
| Store ops    | `crates/ckbadger-store/src/activity_stats_ops.rs` | Read/write ops for daily activity stats                            |
| Writer       | `crates/indexer/src/db/writer/`                   | Accumulate and persist daily stats during block processing         |
| API endpoint | `crates/api/src/routes/stats.rs`                  | `GET /stats/daily-activities` handler                              |
| Frontend API | `frontend/lib/api.ts`                             | `getDailyActivityStats()` method + types                           |
| Homepage     | `frontend/components/home-content.tsx`            | 2-col grid wrapping activities + breakdown                         |
| Breakdown    | `frontend/components/activity-breakdown.tsx`      | Donut chart + summary stats (new)                                  |
| Charts       | `frontend/components/home-charts.tsx` or new      | Four new chart sections                                            |

## Unique Address Counting

For `unique_address_count`: the indexer maintains a per-day `HashSet<[u8; 32]>` (lock_hash) in memory during processing. At each batch commit, the set's length is written as the count. The set is reset at day boundaries.

Memory cost: ~32 bytes per unique address. Even 100k unique addresses/day = ~3.2 MB. Acceptable.

## Reorg Handling

Daily stats are in domain store, so mutable. On reorg:

- The simplest approach: recalculate the affected day's stats from CF_ACTIVITIES (append-only, SSOT).
- Since reorgs are rare and only affect the current day, this is inexpensive.

## Out of Scope

- Hourly granularity (daily is sufficient for both homepage and charts)
- WebSocket push for stats updates (10s polling is fine)
- Per-address activity stats (this is global/network-level)
- Activity type filtering on homepage
