'use client';

import type { NetworkStats } from '@/lib/api';

interface CKBytesCardProps {
  stats: NetworkStats | null;
}

function shannonsToCkb(shannons: string): number {
  return Number(BigInt(shannons) / BigInt(1e4)) / 1e4;
}

function formatCkb(ckb: number): string {
  if (ckb >= 1e9) return `${(ckb / 1e9).toFixed(2)}B`;
  if (ckb >= 1e6) return `${(ckb / 1e6).toFixed(2)}M`;
  return ckb.toLocaleString();
}

interface Segment {
  label: string;
  value: number;
  pct: number;
  color: string;
  textColor: string;
}

export function CKBytesCard({ stats }: CKBytesCardProps) {
  if (!stats?.circulatingSupply || !stats?.knowledgeSize || !stats?.daoLocked) {
    return (
      <div className="border-base-border bg-base-surface rounded-lg border p-4">
        <div className="text-text-dim mb-3 font-mono text-xs uppercase tracking-wider">CKBytes</div>
        <div className="bg-base-elevated h-6 w-full animate-pulse rounded-full" />
      </div>
    );
  }

  const circulating = shannonsToCkb(stats.circulatingSupply);
  const knowledge = shannonsToCkb(stats.knowledgeSize);
  const dao = shannonsToCkb(stats.daoLocked);
  const free = Math.max(0, circulating - knowledge - dao);

  const segments: Segment[] = [
    {
      label: 'Knowledge',
      value: knowledge,
      pct: (knowledge / circulating) * 100,
      color: 'bg-jade',
      textColor: 'text-jade',
    },
    {
      label: 'Free',
      value: free,
      pct: (free / circulating) * 100,
      color: 'bg-text-dim',
      textColor: 'text-text',
    },
    {
      label: 'DAO',
      value: dao,
      pct: (dao / circulating) * 100,
      color: 'bg-gold',
      textColor: 'text-gold',
    },
  ];

  return (
    <div className="border-base-border bg-base-surface rounded-lg border p-4">
      <div className="text-text-dim mb-3 font-mono text-xs uppercase tracking-wider">
        CKBytes <span className="text-text-bright">{formatCkb(circulating)} CKB</span>
      </div>

      {/* Stacked progress bar */}
      <div className="flex h-5 w-full overflow-hidden rounded-full">
        {segments.map((seg) => (
          <div
            key={seg.label}
            className={`${seg.color} transition-all duration-500`}
            style={{ width: `${Math.max(seg.pct, 0.5)}%` }}
            title={`${seg.label}: ${formatCkb(seg.value)} CKB (${seg.pct.toFixed(1)}%)`}
          />
        ))}
      </div>

      {/* Legend */}
      <div className="mt-3 flex flex-wrap gap-x-6 gap-y-1">
        {segments.map((seg) => (
          <div key={seg.label} className="flex items-center gap-2">
            <span className={`${seg.color} inline-block h-2.5 w-2.5 rounded-full`} />
            <span className="text-text-dim font-mono text-xs">{seg.label}</span>
            <span className={`${seg.textColor} font-mono text-xs font-bold tabular-nums`}>
              {formatCkb(seg.value)} CKB
            </span>
            <span className="text-text-dim font-mono text-[10px] tabular-nums">
              {seg.pct.toFixed(1)}%
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
