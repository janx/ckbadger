# Homepage Merge Redesign

## Goal

Merge Phase 2 (original) and Phase 3 (domain-layered) homepage designs into a unified layout that preserves the Phase 2 structure while adding Phase 3's domain insights.

## Principle Alignment

- CKB Native: DAO, Knowledge Size, Activity Trend are CKB-native insights given prominent placement
- Local First: No new API calls — all data already fetched by existing components
- Agent Friendly: Clear section structure, no hidden state

## Layout (Approach B: Domain-Grouped)

```
1. HeroStatRow                              — 5 key metrics (Phase 3)
2. HomeCharts                               — block time + hash rate (Phase 2, recovered)
3. EpochProgress | MiniStatsCards           — 2-col (Phase 2)
4. PipelinePreview                          — full-width (Phase 2)
5. DaoOverview | KnowledgeSizeTrend | ActivityTrend  — 3-col insights (Phase 3)
6. LatestActivities | ActivityBreakdown     — 2-col (Phase 2, ActivityBreakdown recovered)
7. LatestBlocks | LatestTransactions        — 2-col full-size 10 items (Phase 2)
8. LiveIndicator                            — bottom-right (Phase 2)
```

## Components

### Recovered from git (deleted in commit afbcae8f):

- `home-charts.tsx` — HomeCharts (block time + hash rate SVG line charts)
- `mini-stats-cards.tsx` — MiniStatsCards (tx bar charts with hover)
- `activity-breakdown.tsx` — ActivityBreakdown (pie charts + 24h stats)

### Kept from Phase 3 (already exist):

- `hero-stat-row.tsx` — HeroStatRow
- `dao-overview.tsx` — DaoOverview
- `home-layer2.tsx` — KnowledgeSizeTrend (only this one used; NetworkHealth/ScriptUtilization dropped)
- `activity-trend.tsx` — ActivityTrend

### Kept from Phase 2 (still exist):

- `chain-wave/epoch-progress.tsx` — EpochProgress
- `chain-wave/pipeline-preview.tsx` — PipelinePreview
- `latest-activities.tsx` — LatestActivities
- `latest-blocks.tsx` — LatestBlocks (full-size, 10 items, not compact)
- `latest-transactions.tsx` — LatestTransactions (full-size, 10 items, not compact)

### Removed from current Phase 3:

- AssetEcosystem — replaced by ActivityBreakdown from Phase 2
- NetworkHealth, ScriptUtilization — replaced by HomeCharts from Phase 2
- Link cards (Supply & Economics, All Charts) — removed

## Layout & Color Optimization

- Consistent spacing: mt-4 between sections, gap-4 within grids
- 3-col insights row uses ChartCard/TerminalPanel for visual consistency
- Full-size LatestBlocks/LatestTransactions (10 items, not compact)
- page.tsx fetches stats and passes initialData (Phase 2 pattern)

## Scope

- Files changed: `home-content.tsx`, `page.tsx`
- Files recovered: `home-charts.tsx`, `mini-stats-cards.tsx`, `activity-breakdown.tsx`
- Files kept as-is: hero-stat-row, dao-overview, activity-trend, home-layer2, chain-wave/_, latest-_
- No storage/schema impact
