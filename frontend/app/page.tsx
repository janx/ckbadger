import { Header } from '@/components/layout/header';
import { HomeContent } from '@/components/home-content';
import { DeepForkAlert } from '@/components/deep-fork-alert';
import {
  NetworkStats,
  BlockListItem,
  Transaction,
  ChartResponse,
  CursorPaginatedResponse,
} from '@/lib/api';

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

interface PaginatedResponse<T> {
  data: T[];
  total: number;
  page: number;
  limit: number;
}

export default async function Home() {
  const [stats, blocksRes, txRes, blockTimeChart, hashRateChart] = await Promise.all([
    fetchServerData<NetworkStats>('/statistics/network'),
    fetchServerData<CursorPaginatedResponse<BlockListItem>>('/blocks?limit=10'),
    fetchServerData<PaginatedResponse<Transaction>>('/transactions?limit=10'),
    fetchServerData<ChartResponse>('/charts/average-block-time'),
    fetchServerData<ChartResponse>('/charts/hash-rate'),
  ]);

  const initialData = {
    stats,
    blocks: blocksRes?.data ?? [],
    transactions: txRes?.data ?? [],
    blockTimeChart,
    hashRateChart,
  };

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      {stats && stats.deepForkStatus && <DeepForkAlert status={stats.deepForkStatus} />}
      <HomeContent initialData={initialData} />
    </div>
  );
}
