'use client';

import { useQuery } from '@tanstack/react-query';
import { SyncBanner } from '@/components/stats-cards';
import { StatsBar } from '@/components/stats-bar';
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

      {/* Stats Bar */}
      <div className="mt-3">
        <StatsBar stats={stats ?? null} />
      </div>

      {/* Row 1: CKBytes */}
      <div className="mt-3">
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
