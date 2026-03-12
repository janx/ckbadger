# Homepage Layout Adjustment v2

## Goal

Restructure homepage layout: replace HeroStatRow with CKBytes progress card, move insights and activities up, push pipeline and network charts down.

## Layout (top to bottom)

```
Row 1: CKBytes Card (full width)
        └─ stacked bar: Knowledge | Free | DAO as segments of Circulating Supply

Row 2: Knowledge Size (no header) | Nervos DAO (no header)     [2-col]

Row 3: Latest Activities | Activity Card (merged trend+breakdown) [2-col]

Row 4: Transaction Pipeline (full width)

Row 5: HomeCharts — block time + hash rate                      [2-col]

Row 6: EpochProgress | MiniStatsCards                           [2-col]

Row 7: LatestBlocks | LatestTransactions                        [2-col]

LiveIndicator (fixed bottom-right)
```

## Component Changes

### New: CKBytesCard

- Stacked horizontal bar showing 3 segments of Circulating Supply
- Knowledge (knowledgeSize) — jade green
- DAO (daoLocked) — gold
- Free (circulatingSupply - knowledgeSize - daoLocked) — dim/neutral
- Segment labels with CKB values and percentages below bar
- Data source: NetworkStats (already fetched)

### Modified: KnowledgeSizeTrend

- Remove ChartCard wrapper/header — render sparkline in a plain borderless card
- Keep sparkline chart content

### Modified: DaoOverview

- Remove TerminalPanel header — render content in a plain borderless card
- Keep Total Deposited, APC, Depositors, 30-day trend sparkline

### New: ActivityCard (merges ActivityTrend + ActivityBreakdown)

- Top: 14-day bar chart from ActivityTrend
- Middle: type breakdown text from ActivityTrend
- Bottom: 24h stats (activities, addresses, volume) from ActivityBreakdown
- Optionally: pie chart from ActivityBreakdown if space allows

### Removed from layout

- HeroStatRow — replaced by CKBytesCard
- Standalone ActivityTrend — merged into ActivityCard
- Standalone ActivityBreakdown — merged into ActivityCard

## Scope

- Create: `frontend/components/ckbytes-card.tsx`
- Create: `frontend/components/activity-card.tsx`
- Modify: `frontend/components/home-layer2.tsx` (KnowledgeSizeTrend: remove header)
- Modify: `frontend/components/dao-overview.tsx` (remove header)
- Modify: `frontend/components/home-content.tsx` (new layout order)
- No storage/schema impact
