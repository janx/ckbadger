# Homepage Layout v2 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restructure homepage: CKBytes progress card at top, insights row below, merged activity card, pipeline pushed down.

**Architecture:** Create 2 new components (CKBytesCard, ActivityCard), modify 2 existing components to remove headers (KnowledgeSizeTrend, DaoOverview), rewrite home-content.tsx layout order.

**Tech Stack:** React 19, TanStack Query v5, Tailwind CSS 3.4

---

### Task 1: Create CKBytesCard component

**Files:**

- Create: `frontend/components/ckbytes-card.tsx`

**Step 1: Create the component**

This component receives `NetworkStats | null` as props and renders a stacked horizontal progress bar showing how Circulating Supply is distributed across Knowledge, DAO, and Free.

```tsx
'use client';

import type { NetworkStats } from '@/lib/api';

interface CKBytesCardProps {
  stats: NetworkStats | null;
}

function shannonsToCkb(shannons: string): number {
  return Number(BigInt(shannons) / BigInt(1e4)) / 1e4;
}

function formatCkb(ckb: number): string {
  if (ckb >= 1e9) return `${(ckb / 1e9).toFixed(2)}B`;
  if (ckb >= 1e6) return `${(ckb / 1e6).toFixed(2)}M`;
  return ckb.toLocaleString();
}

interface Segment {
  label: string;
  value: number;
  pct: number;
  color: string;
  textColor: string;
}

export function CKBytesCard({ stats }: CKBytesCardProps) {
  if (!stats?.circulatingSupply || !stats?.knowledgeSize || !stats?.daoLocked) {
    return (
      <div className="border-base-border bg-base-surface rounded-lg border p-4">
        <div className="text-text-dim mb-3 font-mono text-xs uppercase tracking-wider">CKBytes</div>
        <div className="bg-base-elevated h-6 w-full animate-pulse rounded-full" />
      </div>
    );
  }

  const circulating = shannonsToCkb(stats.circulatingSupply);
  const knowledge = shannonsToCkb(stats.knowledgeSize);
  const dao = shannonsToCkb(stats.daoLocked);
  const free = Math.max(0, circulating - knowledge - dao);

  const segments: Segment[] = [
    {
      label: 'Knowledge',
      value: knowledge,
      pct: (knowledge / circulating) * 100,
      color: 'bg-jade',
      textColor: 'text-jade',
    },
    {
      label: 'Free',
      value: free,
      pct: (free / circulating) * 100,
      color: 'bg-text-dim',
      textColor: 'text-text',
    },
    {
      label: 'DAO',
      value: dao,
      pct: (dao / circulating) * 100,
      color: 'bg-gold',
      textColor: 'text-gold',
    },
  ];

  return (
    <div className="border-base-border bg-base-surface rounded-lg border p-4">
      <div className="text-text-dim mb-3 font-mono text-xs uppercase tracking-wider">
        CKBytes <span className="text-text-bright">{formatCkb(circulating)} CKB</span>
      </div>

      {/* Stacked progress bar */}
      <div className="flex h-5 w-full overflow-hidden rounded-full">
        {segments.map((seg) => (
          <div
            key={seg.label}
            className={`${seg.color} transition-all duration-500`}
            style={{ width: `${Math.max(seg.pct, 0.5)}%` }}
            title={`${seg.label}: ${formatCkb(seg.value)} CKB (${seg.pct.toFixed(1)}%)`}
          />
        ))}
      </div>

      {/* Legend */}
      <div className="mt-3 flex flex-wrap gap-x-6 gap-y-1">
        {segments.map((seg) => (
          <div key={seg.label} className="flex items-center gap-2">
            <span className={`${seg.color} inline-block h-2.5 w-2.5 rounded-full`} />
            <span className="text-text-dim font-mono text-xs">{seg.label}</span>
            <span className={`${seg.textColor} font-mono text-xs font-bold tabular-nums`}>
              {formatCkb(seg.value)} CKB
            </span>
            <span className="text-text-dim font-mono text-[10px] tabular-nums">
              {seg.pct.toFixed(1)}%
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
```

**Step 2: Verify TypeScript compiles**

```bash
cd frontend && pnpm type-check
```

Expected: PASS

**Step 3: Commit**

```bash
git add frontend/components/ckbytes-card.tsx
git commit -m "feat: add CKBytesCard component with stacked progress bar"
```

---

### Task 2: Create ActivityCard (merge ActivityTrend + ActivityBreakdown)

**Files:**

- Create: `frontend/components/activity-card.tsx`

**Step 1: Create the merged component**

This combines ActivityTrend's 14-day bar chart + type breakdown with ActivityBreakdown's pie chart + 24h summary stats. Uses the same API queries as both originals.

```tsx
'use client';

import { useQuery } from '@tanstack/react-query';
import { api, type ActivitySummary24h } from '@/lib/api';
import { PieChart } from '@/components/ui/pie-chart';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { formatCkbCompact } from '@/lib/utils';
import { CHART_PRIMARY_COLOR } from '@/lib/chart-colors';
import Link from '@/components/ui/link';

function formatCompact(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}k`;
  return n.toString();
}

const ACTIVITY_COLORS: Record<string, string> = {
  Transfer: '#00ffaa',
  'DAO Deposit': '#44ee77',
  'DAO Withdraw': '#2daa55',
  Token: '#ff66aa',
  Object: '#bb88ff',
  Identity: '#44bbff',
  'Script Call': '#ff8800',
};

function buildPieData(stats: ActivitySummary24h) {
  return [
    { label: 'Transfer', value: stats.transferCount, color: ACTIVITY_COLORS.Transfer },
    { label: 'DAO Deposit', value: stats.daoDepositCount, color: ACTIVITY_COLORS['DAO Deposit'] },
    {
      label: 'DAO Withdraw',
      value: stats.daoWithdrawRequestCount + stats.daoWithdrawCompleteCount,
      color: ACTIVITY_COLORS['DAO Withdraw'],
    },
    { label: 'Token', value: stats.tokenCount, color: ACTIVITY_COLORS.Token },
    { label: 'Object', value: stats.objectCount, color: ACTIVITY_COLORS.Object },
    { label: 'Identity', value: stats.identityCount, color: ACTIVITY_COLORS.Identity },
    { label: 'Script Call', value: stats.scriptCallCount, color: ACTIVITY_COLORS['Script Call'] },
  ].filter((s) => s.value > 0);
}

interface ActivityCardProps {
  isRealtime?: boolean;
}

export function ActivityCard({ isRealtime = false }: ActivityCardProps) {
  const { data: dailyStats, isLoading: isDailyLoading } = useQuery({
    queryKey: ['daily-activity-stats', 14],
    queryFn: () => api.getDailyActivityStats(14),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });

  const { data: summary, isLoading: isSummaryLoading } = useQuery({
    queryKey: ['activity-summary-24h'],
    queryFn: () => api.getActivitySummary24h(),
    refetchInterval: 30000,
  });

  const isLoading = isDailyLoading || isSummaryLoading;

  const barData =
    dailyStats?.map((d) => ({
      date: d.date,
      total:
        d.transferCount +
        d.daoDepositCount +
        d.daoWithdrawRequestCount +
        d.daoWithdrawCompleteCount +
        d.tokenCount +
        d.objectCount +
        d.identityCount +
        d.scriptCallCount,
    })) ?? [];

  const maxVal = Math.max(...barData.map((d) => d.total), 1);

  const daoTotal = summary
    ? summary.daoDepositCount + summary.daoWithdrawRequestCount + summary.daoWithdrawCompleteCount
    : 0;

  const breakdownItems = summary
    ? [
        { label: 'Transfers', value: formatCompact(summary.transferCount) },
        { label: 'DAO', value: formatCompact(daoTotal) },
        { label: 'Tokens', value: formatCompact(summary.tokenCount) },
        { label: 'Objects', value: formatCompact(summary.objectCount) },
      ]
    : [];

  const pieData = summary ? buildPieData(summary) : [];

  const headerActions = (
    <Link
      href="/charts"
      className="text-text-dim hover:text-jade font-mono text-xs transition-colors"
    >
      VIEW ALL &rarr;
    </Link>
  );

  return (
    <TerminalPanel variant="default" glow={isRealtime}>
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'} actions={headerActions}>
        <Link href="/charts" className="hover:text-jade transition-colors">
          Activity
        </Link>
      </TerminalPanelHeader>
      <TerminalPanelContent padding="md">
        {/* 14-day bar chart */}
        <div className="mb-4">
          {isLoading || barData.length === 0 ? (
            <div className="bg-base-elevated h-16 w-full animate-pulse rounded" />
          ) : (
            <div className="flex h-16 items-end gap-[2px]">
              {barData.map((d) => (
                <div
                  key={d.date}
                  className="flex-1 rounded-t-sm"
                  style={{
                    height: `${Math.max((d.total / maxVal) * 100, 2)}%`,
                    backgroundColor: CHART_PRIMARY_COLOR,
                    opacity: 0.8,
                  }}
                  title={`${d.date}: ${d.total.toLocaleString()} activities`}
                />
              ))}
            </div>
          )}
        </div>

        {/* Type breakdown */}
        {!isLoading && (
          <div className="mb-4">
            <div className="text-text-dim flex flex-wrap gap-x-3 gap-y-0.5 font-mono text-[10px]">
              {breakdownItems.map((item) => (
                <span key={item.label}>
                  {item.label}: <span className="text-text-bright">{item.value}</span>
                </span>
              ))}
            </div>
          </div>
        )}

        {/* Pie chart */}
        {!isLoading && pieData.length > 0 && (
          <div className="mb-4 flex justify-center">
            <PieChart data={pieData} size={160} formatValue={(v) => v.toLocaleString()} />
          </div>
        )}

        {/* 24h stats */}
        <div className="grid grid-cols-3 gap-x-4 gap-y-2">
          <StatItem
            label="Activities"
            value={
              summary
                ? formatCompact(
                    summary.transferCount +
                      summary.daoDepositCount +
                      summary.daoWithdrawRequestCount +
                      summary.daoWithdrawCompleteCount +
                      summary.tokenCount +
                      summary.objectCount +
                      summary.identityCount +
                      summary.scriptCallCount
                  )
                : '\u2014'
            }
            isLoading={isLoading}
          />
          <StatItem
            label="Addresses"
            value={summary ? summary.uniqueAddressCount.toLocaleString() : '\u2014'}
            isLoading={isLoading}
          />
          <StatItem
            label="Volume"
            value={summary ? formatCkbCompact(summary.totalCkbMoved).value + ' CKB' : '\u2014'}
            isLoading={isLoading}
          />
        </div>
      </TerminalPanelContent>
    </TerminalPanel>
  );
}

function StatItem({
  label,
  value,
  isLoading,
}: {
  label: string;
  value: string;
  isLoading: boolean;
}) {
  return (
    <div className="text-center">
      <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">{label}</div>
      {isLoading ? (
        <div className="bg-base-elevated mx-auto mt-1 h-4 w-12 animate-pulse rounded" />
      ) : (
        <div className="text-emphasis mt-1 font-mono text-sm">{value}</div>
      )}
    </div>
  );
}
```

**Step 2: Verify TypeScript compiles**

```bash
cd frontend && pnpm type-check
```

Expected: PASS

**Step 3: Commit**

```bash
git add frontend/components/activity-card.tsx
git commit -m "feat: add ActivityCard merging trend + breakdown"
```

---

### Task 3: Modify KnowledgeSizeTrend and DaoOverview to remove headers

**Files:**

- Modify: `frontend/components/home-layer2.tsx` (KnowledgeSizeTrend function)
- Modify: `frontend/components/dao-overview.tsx`

**Step 1: Modify KnowledgeSizeTrend in home-layer2.tsx**

Replace the `KnowledgeSizeTrend` export. Remove the `ChartCard` wrapper — render sparkline in a plain card with no header. The `ChartCard` import may still be used by `NetworkHealth` and `ScriptUtilization` in the same file, so keep the import.

Current `KnowledgeSizeTrend` (lines 16-39 of `home-layer2.tsx`):

```tsx
export function KnowledgeSizeTrend() {
  // ... uses ChartCard wrapper with title="Knowledge Size" and href
}
```

Replace with:

```tsx
export function KnowledgeSizeTrend() {
  const { data: chart, isLoading } = useQuery({
    queryKey: ['knowledge-size-chart'],
    queryFn: () => api.getKnowledgeSizeChart(),
    staleTime: 300_000,
    refetchInterval: 300_000,
  });

  const sparkData = useMemo(
    () => chart?.data?.slice(-30).map((d) => parseFloat(d.value)) ?? [],
    [chart]
  );

  return (
    <div className="border-base-border bg-base-surface rounded-lg border p-4">
      {isLoading ? (
        <div className="bg-base-elevated h-16 w-full animate-pulse rounded" />
      ) : (
        <>
          <div className="text-text-dim mb-2 font-mono text-[10px] uppercase tracking-wider">
            Knowledge Size — 30 Day Trend
          </div>
          <SparkChart data={sparkData} height={60} color={CHART_PRIMARY_COLOR} />
        </>
      )}
    </div>
  );
}
```

**Step 2: Modify DaoOverview in dao-overview.tsx**

Remove the `TerminalPanel`/`TerminalPanelHeader` wrapper. Render content in a plain card. Remove the "VIEW ALL" link header and the Link wrapper on the title.

Replace the entire component with:

```tsx
export function DaoOverview() {
  const { data: daoStats, isLoading } = useQuery({
    queryKey: ['dao-statistics'],
    queryFn: () => api.getDaoStatistics(),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  const { data: depositChart } = useQuery({
    queryKey: ['dao-total-deposit-chart'],
    queryFn: () => api.getDaoTotalDepositChart(),
    staleTime: 300_000,
    refetchInterval: 300_000,
  });

  const sparkData = depositChart?.data?.slice(-30).map((d) => parseFloat(d.value)) ?? [];

  return (
    <div className="border-base-border bg-base-surface rounded-lg border p-4">
      {/* Total Deposited */}
      <div className="mb-4">
        <div className="text-text-dim text-xs uppercase tracking-wider">Total Deposited</div>
        {isLoading ? (
          <div className="bg-base-elevated mt-1 inline-block h-6 w-32 animate-pulse rounded" />
        ) : (
          <div className="text-emphasis mt-1 font-mono text-lg font-bold tabular-nums">
            {formatCkb(daoStats?.totalDepositedCkb)}{' '}
            <span className="text-text-dim text-sm font-normal">CKB</span>
          </div>
        )}
      </div>

      {/* APC and Depositors row */}
      <div className="mb-4 grid grid-cols-2 gap-4">
        <div>
          <div className="text-text-dim text-xs uppercase tracking-wider">APC</div>
          {isLoading ? (
            <div className="bg-base-elevated mt-1 inline-block h-5 w-16 animate-pulse rounded" />
          ) : (
            <div className="text-jade mt-1 font-mono text-base font-bold tabular-nums">
              {daoStats?.estimatedApc ? `${daoStats.estimatedApc}%` : '\u2014'}
            </div>
          )}
        </div>
        <div>
          <div className="text-text-dim text-xs uppercase tracking-wider">Depositors</div>
          {isLoading ? (
            <div className="bg-base-elevated mt-1 inline-block h-5 w-16 animate-pulse rounded" />
          ) : (
            <div className="text-text-bright mt-1 font-mono text-base font-bold tabular-nums">
              {daoStats?.totalDepositors != null
                ? daoStats.totalDepositors.toLocaleString()
                : '\u2014'}
            </div>
          )}
        </div>
      </div>

      {/* 30-day trend sparkline */}
      <div>
        <div className="text-text-dim mb-1 text-[10px] uppercase tracking-wider">30-Day Trend</div>
        {sparkData.length > 0 ? (
          <SparkChart data={sparkData} height={40} />
        ) : (
          <div className="bg-base-elevated h-10 w-full animate-pulse rounded" />
        )}
      </div>
    </div>
  );
}
```

Note: DaoOverview uses `SparkChart` — add the import if not already present. Remove unused TerminalPanel imports and the Link import if no longer needed.

**Step 3: Verify TypeScript compiles**

```bash
cd frontend && pnpm type-check
```

Expected: PASS

**Step 4: Commit**

```bash
git add frontend/components/home-layer2.tsx frontend/components/dao-overview.tsx
git commit -m "refactor: remove headers from KnowledgeSizeTrend and DaoOverview"
```

---

### Task 4: Rewrite home-content.tsx layout

**Files:**

- Modify: `frontend/components/home-content.tsx`

**Step 1: Rewrite home-content.tsx**

New layout order. Remove HeroStatRow import. Replace ActivityTrend + ActivityBreakdown with ActivityCard. Add CKBytesCard. Reorder sections.

```tsx
'use client';

import { useQuery } from '@tanstack/react-query';
import { SyncBanner } from '@/components/stats-cards';
import { CKBytesCard } from '@/components/ckbytes-card';
import { HomeCharts } from '@/components/home-charts';
import { MiniStatsCards } from '@/components/mini-stats-cards';
import { EpochProgress } from '@/components/chain-wave/epoch-progress';
import { PipelinePreview } from '@/components/chain-wave/pipeline-preview';
import { DaoOverview } from '@/components/dao-overview';
import { KnowledgeSizeTrend } from '@/components/home-layer2';
import { LatestActivities } from '@/components/latest-activities';
import { ActivityCard } from '@/components/activity-card';
import { LatestBlocks } from '@/components/latest-blocks';
import { LatestTransactions } from '@/components/latest-transactions';
import { useRealtimeData } from '@/hooks/useRealtimeStore';
import { api, NetworkStats, Block, Transaction, ChartResponse } from '@/lib/api';

interface InitialData {
  stats: NetworkStats | null;
  blocks: Block[];
  transactions: Transaction[];
  blockTimeChart: ChartResponse | null;
  hashRateChart: ChartResponse | null;
}

interface HomeContentProps {
  initialData: InitialData;
}

export function HomeContent({ initialData }: HomeContentProps) {
  const { isConnected } = useRealtimeData();

  const { data: stats, isLoading: statsLoading } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    initialData: initialData.stats ?? undefined,
    staleTime: 0,
    refetchInterval: 10000,
  });

  return (
    <main className="container mx-auto px-4 py-4 sm:py-6">
      {stats && <SyncBanner stats={stats} />}

      {/* Row 1: CKBytes */}
      <div className="mt-4">
        <CKBytesCard stats={stats ?? null} />
      </div>

      {/* Row 2: Knowledge Size | Nervos DAO (no headers) */}
      <div className="mt-4 grid gap-4 lg:grid-cols-2">
        <KnowledgeSizeTrend />
        <DaoOverview />
      </div>

      {/* Row 3: Latest Activities | Activity Card */}
      <div className="mt-4 grid gap-4 lg:grid-cols-2">
        <LatestActivities isRealtime={isConnected} />
        <ActivityCard isRealtime={isConnected} />
      </div>

      {/* Row 4: Transaction Pipeline */}
      <div className="mt-4">
        <PipelinePreview initialBlocks={initialData.blocks} />
      </div>

      {/* Row 5: Network Charts */}
      <div className="mt-4">
        <HomeCharts
          stats={stats}
          isLoading={statsLoading}
          initialBlockTimeChart={initialData.blockTimeChart}
          initialHashRateChart={initialData.hashRateChart}
        />
      </div>

      {/* Row 6: Epoch + Tx Stats */}
      <div className="mt-4 grid gap-4 lg:grid-cols-2">
        <EpochProgress
          epochNumber={parseEpochInfo(stats).epochNumber}
          epochIndex={parseEpochInfo(stats).epochIndex}
          epochLength={parseEpochInfo(stats).epochLength}
          latestBlock={stats?.latestBlock ?? 0}
          estimatedTimeRemaining={stats?.estimatedEpochTime}
        />
        <MiniStatsCards />
      </div>

      {/* Row 7: Latest Blocks & Transactions */}
      <div className="mt-5 grid gap-4 lg:grid-cols-2">
        <LatestBlocks isRealtime={isConnected} initialBlocks={initialData.blocks} />
        <LatestTransactions
          isRealtime={isConnected}
          initialTransactions={initialData.transactions}
        />
      </div>

      <LiveIndicator isConnected={isConnected} />
    </main>
  );
}

function LiveIndicator({ isConnected }: { isConnected: boolean }) {
  if (!isConnected) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50">
      <div className="border-jade/50 bg-base-surface/90 flex items-center gap-2 rounded-full border px-3 py-1.5 backdrop-blur-sm">
        <div className="indicator-light" />
        <span className="text-jade font-mono text-xs uppercase tracking-wider">Live</span>
      </div>
    </div>
  );
}

function parseEpochInfo(stats: NetworkStats | null | undefined): {
  epochNumber: number;
  epochIndex: number;
  epochLength: number;
} {
  if (!stats?.epoch) {
    return { epochNumber: 0, epochIndex: 0, epochLength: 1800 };
  }

  const match = stats.epoch.match(/(\d+)\((\d+)\/(\d+)\)/);
  if (match) {
    return {
      epochNumber: parseInt(match[1], 10),
      epochIndex: parseInt(match[2], 10),
      epochLength: parseInt(match[3], 10),
    };
  }

  return { epochNumber: 0, epochIndex: 0, epochLength: 1800 };
}
```

**Step 2: Verify TypeScript + lint**

```bash
cd frontend && pnpm type-check && pnpm lint
```

Expected: PASS

**Step 3: Commit**

```bash
git add frontend/components/home-content.tsx
git commit -m "feat: homepage layout v2 — CKBytes card, merged activity, reordered sections"
```

---

### Task 5: Run tests, format, verify

**Step 1: Run frontend tests**

```bash
cd frontend && npx vitest run
```

Expected: PASS

**Step 2: Format**

```bash
pnpm format
```

**Step 3: Commit formatting if changed**

```bash
git add -A && git diff --cached --quiet || git commit -m "style: format homepage v2 changes"
```
