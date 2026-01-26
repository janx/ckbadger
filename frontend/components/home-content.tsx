'use client';

import { useQuery } from '@tanstack/react-query';
import { SyncBanner } from '@/components/stats-cards';
import { HomeCharts } from '@/components/home-charts';
import { MiniStatsCards } from '@/components/mini-stats-cards';
import { EpochProgress } from '@/components/chain-wave/epoch-progress';
import { LatestBlocks } from '@/components/latest-blocks';
import { LatestTransactions } from '@/components/latest-transactions';
import { ChainWave } from '@/components/chain-wave';
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
    <main className="container mx-auto px-4 py-6 sm:py-8">
      {stats && <SyncBanner stats={stats} />}

      <div className="mt-6">
        <HomeCharts
          stats={stats}
          isLoading={statsLoading}
          initialBlockTimeChart={initialData.blockTimeChart}
          initialHashRateChart={initialData.hashRateChart}
        />
      </div>

      <div className="mt-6 grid gap-6 lg:grid-cols-2">
        <EpochProgress
          epochNumber={parseEpochInfo(stats).epochNumber}
          epochIndex={parseEpochInfo(stats).epochIndex}
          epochLength={parseEpochInfo(stats).epochLength}
          latestBlock={stats?.latestBlock ?? 0}
          estimatedTimeRemaining={stats?.estimatedEpochTime}
        />
        <MiniStatsCards />
      </div>

      <div className="mt-6">
        <ChainWave initialBlocks={initialData.blocks} />
      </div>

      <div className="mt-8 grid gap-6 lg:grid-cols-2">
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
      <div className="border-terminal-dark/50 flex items-center gap-2 rounded-full border bg-slate-900/90 px-3 py-1.5 backdrop-blur-sm">
        <div className="indicator-light" />
        <span className="text-terminal-green font-mono text-xs uppercase tracking-wider">Live</span>
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
