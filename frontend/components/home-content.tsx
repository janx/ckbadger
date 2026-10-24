'use client';

import { useQuery } from '@tanstack/react-query';
import { SyncBanner } from '@/components/stats-cards';
import { PipelinePanel } from '@/components/pipeline/pipeline-panel';
import { NetworkSection } from '@/components/home/network-section';
import { CkbytesSection } from '@/components/home/ckbytes-section';
import { ActivitySection } from '@/components/home/activity-section';
import { useRealtimeData } from '@/hooks/useRealtimeStore';
import { api } from '@/lib/api';

/**
 * The homepage, top to bottom: the sync banner (while bulk syncing), the transaction pipeline
 * (the only framed panel), then the unframed Network, CKBytes and Activity sections. The chain
 * vitals live in the header's strip.
 */
export function HomeContent() {
  // The homepage holds the realtime socket: new blocks, transactions and reorgs.
  useRealtimeData();
  const { data: stats } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    staleTime: 0,
    refetchInterval: 10000,
  });

  return (
    <main className="pb-14 pt-3">
      <div className="container mx-auto flex flex-col gap-[52px] px-4">
        {stats && <SyncBanner stats={stats} />}
        <PipelinePanel />
        <NetworkSection />
        <CkbytesSection />
        <ActivitySection />
      </div>
    </main>
  );
}
