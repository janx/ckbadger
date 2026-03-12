# Homepage Merge Redesign — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Merge Phase 2 and Phase 3 homepage designs into a unified layout (Approach B: Domain-Grouped).

**Architecture:** Restore 3 deleted Phase 2 components from git history, rewrite `home-content.tsx` to combine all Phase 2 sections + Phase 3 insights row, update `page.tsx` to match Phase 2's data-passing pattern.

**Tech Stack:** React 19, TanStack Query v5, Tailwind CSS 3.4, Vite 5

---

### Task 1: Restore deleted Phase 2 components

**Files:**

- Create: `frontend/components/home-charts.tsx` (from `git show afbcae8f~1:frontend/components/home-charts.tsx`)
- Create: `frontend/components/mini-stats-cards.tsx` (from `git show afbcae8f~1:frontend/components/mini-stats-cards.tsx`)
- Create: `frontend/components/activity-breakdown.tsx` (from `git show afbcae8f~1:frontend/components/activity-breakdown.tsx`)

**Step 1: Restore home-charts.tsx**

```bash
git show afbcae8f~1:frontend/components/home-charts.tsx > frontend/components/home-charts.tsx
```

**Step 2: Restore mini-stats-cards.tsx**

```bash
git show afbcae8f~1:frontend/components/mini-stats-cards.tsx > frontend/components/mini-stats-cards.tsx
```

**Step 3: Restore activity-breakdown.tsx**

```bash
git show afbcae8f~1:frontend/components/activity-breakdown.tsx > frontend/components/activity-breakdown.tsx
```

**Step 4: Verify TypeScript compiles**

```bash
cd frontend && pnpm type-check
```

Expected: PASS — all imports (`api`, `PieChart`, `chart-colors`, `TerminalPanel`, etc.) still exist.

**Step 5: Commit**

```bash
git add frontend/components/home-charts.tsx frontend/components/mini-stats-cards.tsx frontend/components/activity-breakdown.tsx
git commit -m "restore Phase 2 homepage components from git history"
```

---

### Task 2: Rewrite page.tsx (Phase 2 data-passing pattern)

**Files:**

- Modify: `frontend/app/page.tsx`

**Step 1: Rewrite page.tsx**

Replace current 13-line simple wrapper with Phase 2's pattern that fetches network stats and passes initialData. Also includes DeepForkAlert.

```tsx
'use client';

import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { HomeContent } from '@/components/home-content';
import { DeepForkAlert } from '@/components/deep-fork-alert';
import { api } from '@/lib/api';

export default function Home() {
  const { data: stats } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    staleTime: 0,
    refetchInterval: 10000,
  });

  const initialData = {
    stats: stats ?? null,
    blocks: [],
    transactions: [],
    blockTimeChart: null,
    hashRateChart: null,
  };

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      {stats && stats.deepForkStatus && <DeepForkAlert status={stats.deepForkStatus} />}
      <HomeContent initialData={initialData} />
    </div>
  );
}
```

**Step 2: Verify TypeScript compiles** (will fail until Task 3 updates HomeContent props)

Skip type-check — Task 3 will fix the interface mismatch.

---

### Task 3: Rewrite home-content.tsx (merged layout)

**Files:**

- Modify: `frontend/components/home-content.tsx`

**Step 1: Rewrite home-content.tsx**

Full replacement with merged layout. Key differences from Phase 2:

- Adds HeroStatRow at top (Phase 3)
- Adds 3-col insights row: DaoOverview | KnowledgeSizeTrend | ActivityTrend (Phase 3)
- Keeps all Phase 2 sections in order
- KnowledgeSizeTrend imported from `home-layer2.tsx`

```tsx
'use client';

import { useQuery } from '@tanstack/react-query';
import { SyncBanner } from '@/components/stats-cards';
import { HeroStatRow } from '@/components/hero-stat-row';
import { HomeCharts } from '@/components/home-charts';
import { MiniStatsCards } from '@/components/mini-stats-cards';
import { EpochProgress } from '@/components/chain-wave/epoch-progress';
import { PipelinePreview } from '@/components/chain-wave/pipeline-preview';
import { DaoOverview } from '@/components/dao-overview';
import { KnowledgeSizeTrend } from '@/components/home-layer2';
import { ActivityTrend } from '@/components/activity-trend';
import { LatestActivities } from '@/components/latest-activities';
import { ActivityBreakdown } from '@/components/activity-breakdown';
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

      {/* Hero Stats */}
      <div className="mt-4">
        <HeroStatRow stats={stats ?? null} />
      </div>

      {/* Network Charts */}
      <div className="mt-4">
        <HomeCharts
          stats={stats}
          isLoading={statsLoading}
          initialBlockTimeChart={initialData.blockTimeChart}
          initialHashRateChart={initialData.hashRateChart}
        />
      </div>

      {/* Epoch + Tx Stats */}
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

      {/* Transaction Pipeline */}
      <div className="mt-4">
        <PipelinePreview initialBlocks={initialData.blocks} />
      </div>

      {/* Domain Insights (Phase 3) */}
      <div className="mt-4 grid gap-4 lg:grid-cols-3">
        <DaoOverview />
        <KnowledgeSizeTrend />
        <ActivityTrend />
      </div>

      {/* Activities */}
      <div className="mt-4 grid gap-4 lg:grid-cols-2">
        <LatestActivities isRealtime={isConnected} />
        <ActivityBreakdown isRealtime={isConnected} />
      </div>

      {/* Latest Blocks & Transactions */}
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

**Step 2: Verify TypeScript compiles**

```bash
cd frontend && pnpm type-check
```

Expected: PASS

**Step 3: Verify lint passes**

```bash
cd frontend && pnpm lint
```

Expected: PASS

**Step 4: Commit**

```bash
git add frontend/app/page.tsx frontend/components/home-content.tsx
git commit -m "feat: merge Phase 2 + Phase 3 homepage with domain-grouped layout"
```

---

### Task 4: Run tests and verify

**Step 1: Run frontend tests**

```bash
cd frontend && npx vitest run
```

Expected: PASS — existing tests should still pass since we're using the same component interfaces.

**Step 2: Run lint + type-check**

```bash
cd frontend && pnpm type-check && pnpm lint
```

Expected: PASS

**Step 3: Visual check**

```bash
cd frontend && pnpm dev
```

Open http://localhost:3000 and verify:

1. HeroStatRow shows 5 metrics at top
2. HomeCharts shows block time + hash rate
3. EpochProgress + MiniStatsCards side by side
4. PipelinePreview shows transaction pipeline
5. 3-col insights: DaoOverview | KnowledgeSizeTrend | ActivityTrend
6. LatestActivities + ActivityBreakdown side by side
7. LatestBlocks + LatestTransactions (10 items each)
8. Live indicator in bottom-right when WebSocket connected

**Step 4: Format**

```bash
pnpm format
```

**Step 5: Final commit**

```bash
git add -A
git commit -m "style: format homepage merge changes"
```
