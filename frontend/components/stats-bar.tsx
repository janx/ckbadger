'use client';

import Link from '@/components/ui/link';
import type { NetworkStats } from '@/lib/api';

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
