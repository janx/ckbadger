# Homepage Redesign Design

**Date:** 2026-03-11
**Status:** Approved

## Goal

Redesign the homepage to follow the Information Design principles (docs/prompts/INFORMATION_DESIGN.md): lead with Domain Knowledge (Layer 1), support with Aggregations (Layer 2), keep Raw Data (Layer 0) reachable at bottom. Serve all audiences (developers, token holders, newcomers) with clear information hierarchy.

## Principle Alignment

- **CKB Native:** Knowledge Size as hero metric, DAO panel, capacity-based asset breakdown
- **Local First:** All data from local RocksDB store, no external dependencies
- **Agent Friendly:** Clear section boundaries, semantic data-first

## Layout (top → bottom)

```
┌─────────────────────────────────────────────────┐
│ SYNC BANNER (conditional, only during bulk sync)│
├─────────────────────────────────────────────────┤
│ HERO STAT ROW                                   │
│ Knowledge Size | Circulating | DAO Locked       │
│ Block Height   | Epoch Number                   │
├─────────────────────────────────────────────────┤
│ ═══ LAYER 1: DOMAIN KNOWLEDGE ═══              │
├────────────────────┬────────────────────────────┤
│ Live Activities    │ DAO Overview               │
│ 6-8 entries,       │ Total Deposited + APC +    │
│ real-time feed     │ Depositor Count            │
│                    │ + 30-day deposit trend mini │
├────────────────────┼────────────────────────────┤
│ Asset Ecosystem    │ Activity Trend             │
│ Top 3-5 tokens     │ 7-14 day volume bar chart  │
│ by holders         │ + compact type breakdown   │
│ + capacity bar     │ (transfers/DAO/tokens/etc) │
│ by category        │ + unique addrs + CKB moved │
├────────────────────┴────────────────────────────┤
│ ═══ LAYER 2: AGGREGATIONS ═══                  │
├──────────┬──────────┬───────────────────────────┤
│Knowledge │ Network  │ Script Utilization        │
│Size trend│ Health   │ Top 5 scripts by          │
│30d chart │ Block    │ capacity bar chart        │
│          │ time +   │                           │
│          │ hash rate│                           │
├──────────┴──────────┼───────────────────────────┤
│ Link: Supply Charts │ Link: All Charts          │
├─────────────────────┴───────────────────────────┤
│ ═══ LAYER 0: RAW DATA ═══                      │
├────────────────────┬────────────────────────────┤
│ Latest Blocks      │ Latest Transactions        │
│ 3-4 compact rows   │ 3-4 compact rows           │
└────────────────────┴────────────────────────────┘
```

## Section Specifications

### 1. Sync Banner

Identical to current. Only visible during bulk sync. Shows progress %, speed, ETA.

### 2. Hero Stat Row

Single horizontal row, 5 metrics with large monospace numbers, labels below:

| Metric         | Source                                | Format        | Links To                    |
| -------------- | ------------------------------------- | ------------- | --------------------------- |
| Knowledge Size | DAO `U` field via network stats       | "XX.X GB"     | `/charts/knowledge-size`    |
| Circulating    | total issuance - 8.4B burnt           | "XX.XX B CKB" | `/charts/total-supply`      |
| DAO Locked     | `CF_STATS_DAO` total deposit capacity | "XX.XX B CKB" | `/nervos-dao`               |
| Block Height   | latest block number                   | "#XX,XXX,XXX" | `/blocks/{number}`          |
| Epoch Number   | current epoch                         | "#X,XXX"      | `/charts/epoch-time-length` |

No charts, no decoration. Clean stat row.

### 3. Live Activities (Layer 1)

Real-time activity feed, 6-8 entries. Same content as current:

- Address, activity type badge, time ago
- Transaction hash, block number
- CKB delta with +/- color
- Asset change badges

Real-time highlight on new entries. "View All →" links to `/activities`.

### 4. DAO Overview (Layer 1)

Card with:

- **Total Deposited CKB** — large number
- **Current APC** — annual percentage compensation
- **Depositor Count** — number of unique depositors
- **30-day deposit trend** — mini line chart (spark-style)
- Clickable → `/nervos-dao`

Data source: `CF_STATS_DAO` (daily snapshots), existing DAO calculation logic.

### 5. Asset Ecosystem (Layer 1)

Card with two parts:

- **Top tokens**: top 3-5 tokens by holder count, each showing: name, holder count, capacity
- **Capacity breakdown bar**: horizontal stacked bar showing CKB capacity by category (Tokens | Objects | Identities | Scripts | Other)

Data sources: `CF_TOKENS`, `CF_TOKEN_HOLDERS`, `CF_SPORE_DATA`, `CF_OBJECT_DATA`, `CF_IDENTITY_DATA`, `CF_SCRIPT_INFO`.

Items clickable → respective asset/token pages.

### 6. Activity Trend (Layer 1/2)

Card with:

- **7-14 day daily activity volume bar chart** — one bar per day, total activities
- **Compact type breakdown text**: "Transfers: 1.2k | DAO: 340 | Tokens: 89 | Objects: 12"
- **Unique addresses (24h)** + **Total CKB moved (24h)**
- Clickable → `/charts/daily-activities`

Data source: `CF_STATS_CHAIN` (daily activity stats), existing `getActivitySummary24h()` endpoint.

### 7. Knowledge Size Trend (Layer 2)

30-day mini line chart of total occupied capacity growth. Compact card.
Clickable → `/charts/knowledge-size`.

Data source: `CF_STATS_CHAIN` (daily stats, `knowledge_size` field).

### 8. Network Health (Layer 2)

Combined mini card:

- Average block time: number + 14-day spark line
- Hash rate: number + 14-day spark line
- Clickable → respective chart pages

Data source: existing `getAverageBlockTimeChart()`, `getHashRateChart()`.

### 9. Script Utilization (Layer 2)

Top 5 scripts by capacity as horizontal bar chart. Each bar shows script name + capacity occupied.
Clickable → `/charts/most-utilized-scripts`.

Data source: `CF_SCRIPT_INFO`, existing `getMostUtilizedScriptsChart()`.

### 10. Link Cards

Simple navigation links, no charts:

- "Supply & Economics →" → `/charts/total-supply`
- "All Charts →" → `/charts`

### 11. Latest Blocks (Layer 0)

3-4 rows, compact: block number, tx count, time ago.
"View All →" to `/blocks`.

### 12. Latest Transactions (Layer 0)

3-4 rows, compact: tx hash, block number, inputs/outputs count.
"View All →" to `/transactions`.

## Removed Components

| Component                               | Reason                                             |
| --------------------------------------- | -------------------------------------------------- |
| Mempool pipeline (PipelinePreview)      | Niche L0 concept, hard to interpret for most users |
| Epoch progress bar (standalone section) | Epoch number in hero row is sufficient             |
| Block time + hash rate as hero charts   | Demoted to L2 Network Health section               |
| Activity breakdown pie charts           | Replaced with trend bar chart + compact text       |
| Mini stats cards (TX count cards)       | Replaced by Activity Trend panel                   |

## New Components

| Component                       | Data Source                                                                          |
| ------------------------------- | ------------------------------------------------------------------------------------ |
| Hero stat row (5 metrics)       | `CF_STATS_DAO`, `CF_STATS_CHAIN`, network stats                                      |
| DAO Overview panel              | `CF_STATS_DAO` (daily snapshots)                                                     |
| Asset Ecosystem panel           | `CF_TOKENS`, `CF_SPORE_DATA`, `CF_OBJECT_DATA`, `CF_IDENTITY_DATA`, `CF_SCRIPT_INFO` |
| Activity Trend (daily bar)      | `CF_STATS_CHAIN` (daily activity stats)                                              |
| Knowledge Size trend chart      | `CF_STATS_CHAIN` (daily stats)                                                       |
| Script Utilization chart        | `CF_SCRIPT_INFO`                                                                     |
| Link cards (Supply, All Charts) | Navigation only                                                                      |

## New API Endpoints Needed

| Endpoint                                      | Returns                               | Source                             |
| --------------------------------------------- | ------------------------------------- | ---------------------------------- |
| `/statistics/dao-summary`                     | Total deposited, APC, depositor count | `CF_STATS_DAO` + existing DAO calc |
| `/statistics/asset-ecosystem`                 | Top tokens + capacity by category     | Multiple CFs aggregated            |
| `/charts/dao-deposit-trend` (if not existing) | 30-day deposit trend                  | `CF_STATS_DAO` daily snapshots     |

Existing endpoints reused: `getNetworkStats()`, `getLatestActivities()`, `getActivitySummary24h()`, `getAverageBlockTimeChart()`, `getHashRateChart()`, `getKnowledgeSizeChart()`, `getMostUtilizedScriptsChart()`, `getBlocks()`, `getTransactions()`, `getDailyActivities()`.

## Cross-Layer Traceability

Every panel links both up (aggregated views) and down (raw evidence):

- Hero Knowledge Size → Knowledge Size trend (L2) → cell list (L0)
- DAO Overview → DAO charts (L2) → deposit cells (L0)
- Activity feed entries → address page (L1) → transaction (L0)
- Asset Ecosystem tokens → token page (L1) → token cells (L0)
- Script Utilization → script detail (L1) → script cells (L0)
