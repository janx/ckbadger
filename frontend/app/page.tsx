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
