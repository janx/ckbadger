'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { api, type NetworkStats } from '@/lib/api';

interface StatsBarProps {
  stats: NetworkStats | null;
}

function parseEpoch(epoch: string): { number: number; index: number; length: number } | null {
  const match = epoch.match(/(\d+)\((\d+)\/(\d+)\)/);
  if (!match) return null;
  return {
    number: parseInt(match[1], 10),
    index: parseInt(match[2], 10),
    length: parseInt(match[3], 10),
  };
}

export function StatsBar({ stats }: StatsBarProps) {
  if (!stats) {
    return (
      <div className="flex h-6 items-center gap-6">
        {Array.from({ length: 3 }, (_, i) => (
          <div key={i} className="bg-base-elevated h-3 w-24 animate-pulse rounded" />
        ))}
      </div>
    );
  }

  const epoch = parseEpoch(stats.epoch);
  const epochPct = epoch ? ((epoch.index / epoch.length) * 100).toFixed(1) : '0';

  return (
    <div className="flex flex-wrap items-center gap-x-6 gap-y-1">
      <Link href={`/blocks/${stats.latestBlock}`} className="group flex items-baseline gap-1.5">
        <span className="text-text-dim font-mono text-[10px] uppercase tracking-wider">Block</span>
        <span className="text-text-bright group-hover:text-jade font-mono text-xs font-bold tabular-nums transition-colors">
          #{stats.latestBlock.toLocaleString()}
        </span>
      </Link>

      <Link href="/charts/epoch-time-length" className="group flex items-baseline gap-1.5">
        <span className="text-text-dim font-mono text-[10px] uppercase tracking-wider">Epoch</span>
        <span className="text-text-bright group-hover:text-jade font-mono text-xs font-bold tabular-nums transition-colors">
          {epoch ? `#${epoch.number.toLocaleString()}` : stats.epoch}
        </span>
        {epoch && (
          <span className="text-text-dim font-mono text-[10px] tabular-nums">
            {epoch.index}/{epoch.length} ({epochPct}%)
          </span>
        )}
      </Link>

      <Link href="/charts/hash-rate" className="group flex items-baseline gap-1.5">
        <span className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
          Hash Rate
        </span>
        <span className="text-text-bright group-hover:text-jade font-mono text-xs font-bold tabular-nums transition-colors">
          {stats.hashRate}
        </span>
      </Link>

      <span className="flex items-baseline gap-1.5">
        <span className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
          Block Time
        </span>
        <span className="text-text-bright font-mono text-xs font-bold tabular-nums">
          {stats.avgBlockTime}
        </span>
      </span>
    </div>
  );
}

export function GlobalStatsBar() {
  const { data: stats } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
    staleTime: 0,
    refetchInterval: 10000,
  });

  if (!stats) return null;

  const epoch = parseEpoch(stats.epoch);
  const epochPct = epoch ? ((epoch.index / epoch.length) * 100).toFixed(1) : null;

  return (
    <div className="flex items-center gap-0 overflow-x-auto font-mono text-[11px] tabular-nums leading-none">
      <Link href={`/blocks/${stats.latestBlock}`} className="group flex items-center">
        <span className="text-jade/50 hidden uppercase tracking-wider sm:inline">block</span>
        <span className="text-jade/50 uppercase tracking-wider sm:hidden">#</span>
        <span className="text-jade group-hover:text-emphasis ml-0.5 font-bold transition-colors sm:ml-1.5">
          {stats.latestBlock.toLocaleString()}
        </span>
      </Link>

      <span className="text-jade/20 mx-1.5 select-none sm:mx-2.5">|</span>

      <Link href="/charts/epoch-time-length" className="group flex items-center">
        <span className="text-jade/50 hidden uppercase tracking-wider sm:inline">epoch</span>
        <span className="text-jade/50 uppercase tracking-wider sm:hidden">E</span>
        <span className="text-jade group-hover:text-emphasis ml-0.5 font-bold transition-colors sm:ml-1.5">
          {epoch ? epoch.number.toLocaleString() : stats.epoch}
        </span>
        {epoch && (
          <span className="text-jade/40 ml-1.5 hidden sm:inline">
            {epoch.index}/{epoch.length} {epochPct}%
          </span>
        )}
      </Link>

      <span className="text-jade/20 mx-1.5 select-none sm:mx-2.5">|</span>

      <Link href="/charts/hash-rate" className="group flex items-center">
        <span className="text-jade/50 hidden uppercase tracking-wider sm:inline">hash</span>
        <span className="text-jade group-hover:text-emphasis font-bold transition-colors sm:ml-1.5">
          {stats.hashRate}
        </span>
      </Link>

      <span className="text-jade/20 mx-1.5 select-none sm:mx-2.5">|</span>

      <span className="flex items-center">
        <span className="text-jade/50 hidden uppercase tracking-wider sm:inline">interval</span>
        <span className="text-jade font-bold sm:ml-1.5">{stats.avgBlockTime}</span>
      </span>
    </div>
  );
}
