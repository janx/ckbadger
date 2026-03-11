'use client';

import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { SyncBanner } from '@/components/stats-cards';
import { HeroStatRow } from '@/components/hero-stat-row';
import { LatestActivities } from '@/components/latest-activities';
import { DaoOverview } from '@/components/dao-overview';
import { AssetEcosystem } from '@/components/asset-ecosystem';
import { ActivityTrend } from '@/components/activity-trend';
import { KnowledgeSizeTrend, NetworkHealth, ScriptUtilization } from '@/components/home-layer2';
import { LatestBlocks } from '@/components/latest-blocks';
import { LatestTransactions } from '@/components/latest-transactions';
import { useRealtimeData } from '@/hooks/useRealtimeStore';
import { DeepForkAlert } from '@/components/deep-fork-alert';
import Link from '@/components/ui/link';

export function HomeContent() {
  const { data: stats } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    staleTime: 0,
    refetchInterval: 10_000,
  });

  const { isConnected } = useRealtimeData();

  const showSyncBanner = stats?.syncStatus?.isSyncing && stats?.syncStatus?.syncMode === 'bulk';

  return (
    <div className="container mx-auto px-4 py-4 sm:py-6">
      {/* Deep fork alert */}
      {stats && stats.deepForkStatus && <DeepForkAlert status={stats.deepForkStatus} />}

      {/* Sync Banner — only during bulk sync */}
      {showSyncBanner && stats && (
        <div className="mt-2">
          <SyncBanner stats={stats} />
        </div>
      )}

      {/* Hero Stat Row */}
      <div className="mt-4">
        <HeroStatRow stats={stats ?? null} />
      </div>

      {/* ═══ LAYER 1: DOMAIN KNOWLEDGE ═══ */}
      <div className="mt-6 grid gap-4 lg:grid-cols-2">
        <LatestActivities isRealtime={isConnected} />
        <DaoOverview />
      </div>

      <div className="mt-4 grid gap-4 lg:grid-cols-2">
        <AssetEcosystem />
        <ActivityTrend />
      </div>

      {/* ═══ LAYER 2: AGGREGATIONS ═══ */}
      <div className="mt-6 grid gap-4 lg:grid-cols-3">
        <KnowledgeSizeTrend />
        <NetworkHealth stats={stats ?? null} />
        <ScriptUtilization />
      </div>

      {/* Link cards */}
      <div className="mt-3 flex gap-4">
        <Link
          href="/charts/total-supply"
          className="text-text-dim hover:text-text-bright font-mono text-xs transition-colors"
        >
          Supply &amp; Economics →
        </Link>
        <Link
          href="/charts"
          className="text-text-dim hover:text-text-bright font-mono text-xs transition-colors"
        >
          All Charts →
        </Link>
      </div>

      {/* ═══ LAYER 0: RAW DATA ═══ */}
      <div className="mt-6 grid gap-4 lg:grid-cols-2">
        <LatestBlocks isRealtime={isConnected} compact />
        <LatestTransactions isRealtime={isConnected} compact />
      </div>

      {/* Live indicator */}
      {isConnected && (
        <div className="fixed bottom-4 right-4 z-50">
          <div className="terminal-card border-jade/30 bg-base-surface/80 flex items-center gap-1.5 border px-2 py-1 backdrop-blur-sm">
            <span className="bg-jade h-1.5 w-1.5 animate-pulse rounded-full" />
            <span className="text-jade font-mono text-[10px] uppercase tracking-wider">Live</span>
          </div>
        </div>
      )}
    </div>
  );
}
