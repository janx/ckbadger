import { Header } from '@/components/layout/header';
import { DeepForkAlert } from '@/components/deep-fork-alert';
import { PipelineDashboard } from '@/components/pipeline/pipeline-dashboard';
import { Block, CursorPaginatedResponse, NetworkStats } from '@/lib/api';

const API_BASE = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001/api/v1';

async function fetchServerData<T>(endpoint: string): Promise<T | null> {
  try {
    const res = await fetch(`${API_BASE}${endpoint}`, {
      next: { revalidate: 10 },
    });

    if (!res.ok) return null;
    return res.json();
  } catch {
    return null;
  }
}

export default async function PipelinePage() {
  const [stats, blocksRes] = await Promise.all([
    fetchServerData<NetworkStats>('/statistics/network'),
    fetchServerData<CursorPaginatedResponse<Block>>('/blocks?limit=4'),
  ]);

  const blocks = blocksRes?.data ?? [];

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      {stats?.deepForkStatus && <DeepForkAlert status={stats.deepForkStatus} />}

      <main className="container mx-auto px-4 py-6 sm:py-8">
        <div className="mb-5">
          <h1 className="text-2xl font-bold tracking-tight text-white sm:text-3xl">Pipeline</h1>
          <p className="mt-1 text-sm text-slate-400">
            Mempool to proposal queue to committed blocks in one continuous view.
          </p>
        </div>

        <PipelineDashboard initialBlocks={blocks} />
      </main>
    </div>
  );
}
