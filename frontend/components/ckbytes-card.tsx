'use client';

import { useState } from 'react';
import Link from '@/components/ui/link';
import type { NetworkStats } from '@/lib/api';

interface CKBytesCardProps {
  stats: NetworkStats | null;
}

function shannonsToCkb(shannons: string): number {
  return Number(BigInt(shannons) / BigInt(1e4)) / 1e4;
}

function formatCkbPrecise(ckb: number): string {
  return ckb.toLocaleString(undefined, { minimumFractionDigits: 8, maximumFractionDigits: 8 });
}

function formatCkbCompact(ckb: number): string {
  if (ckb >= 1e9) return `${(ckb / 1e9).toFixed(2)}B`;
  if (ckb >= 1e6) return `${(ckb / 1e6).toFixed(2)}M`;
  return ckb.toLocaleString();
}

function ckbToGB(ckb: number): string {
  // 1 CKB = 1 byte of storage
  const bytes = ckb;
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`;
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${bytes.toLocaleString()} B`;
}

interface Segment {
  label: string;
  value: number;
  pct: number;
  color: string;
  hoverColor: string;
  textColor: string;
}

export function CKBytesCard({ stats }: CKBytesCardProps) {
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);

  if (!stats?.circulatingSupply || !stats?.knowledgeSize || !stats?.daoLocked) {
    return (
      <div className="rounded-lg p-4">
        <div className="text-text-dim mb-3 font-mono text-xs uppercase tracking-wider">
          CKBytes Circulation
        </div>
        <div className="bg-base-elevated h-6 w-full animate-pulse rounded-full" />
      </div>
    );
  }

  const circulating = shannonsToCkb(stats.circulatingSupply);
  // Common Knowledge Size as /statistics/network now reports it: DAO `U` minus
  // the genesis virtual occupied capacity, the same quantity
  // /charts/knowledge-size plots. Raw `U` overstated this segment by the
  // network's virtual occupied capacity (5.04B CKB on mainnet), stealing that
  // much from Free.
  const knowledge = shannonsToCkb(stats.knowledgeSize);
  const dao = shannonsToCkb(stats.daoLocked);
  // Exact remainder — never clamped. A negative remainder means the three
  // API values contradict each other, which must be visible, not painted over
  // with a plausible-looking bar.
  const free = circulating - knowledge - dao;

  if (free < 0) {
    return (
      <div className="rounded-lg p-4">
        <div className="text-text-dim mb-3 font-mono text-xs uppercase tracking-wider">
          CKBytes Circulation
        </div>
        <div
          data-testid="ckbytes-allocation-error"
          className="border-negative/40 text-negative rounded border px-3 py-2 font-mono text-xs"
        >
          Inconsistent supply data: knowledge {formatCkbPrecise(knowledge)} CKB + DAO{' '}
          {formatCkbPrecise(dao)} CKB exceed circulating {formatCkbPrecise(circulating)} CKB.
        </div>
      </div>
    );
  }

  const segments: Segment[] = [
    {
      label: 'Knowledge',
      value: knowledge,
      pct: (knowledge / circulating) * 100,
      color: 'bg-jade/70',
      hoverColor: 'bg-jade',
      textColor: 'text-jade',
    },
    {
      label: 'Free',
      value: free,
      pct: (free / circulating) * 100,
      color: 'bg-text-dim/50',
      hoverColor: 'bg-text-dim',
      textColor: 'text-text',
    },
    {
      label: 'DAO',
      value: dao,
      pct: (dao / circulating) * 100,
      color: 'bg-gold/70',
      hoverColor: 'bg-gold',
      textColor: 'text-gold',
    },
  ];

  return (
    <Link href="/charts/total-supply" className="block">
      <div className="rounded-lg p-4">
        <div className="text-text-dim mb-3 font-mono text-xs uppercase tracking-wider">
          CKBytes Circulation{' '}
          <span className="text-text-bright font-bold">
            {(() => {
              const formatted = formatCkbPrecise(circulating);
              const dotIndex = formatted.indexOf('.');
              if (dotIndex === -1) return <>{formatted}</>;
              return (
                <>
                  {formatted.slice(0, dotIndex)}
                  <span className="text-text-dim font-normal">{formatted.slice(dotIndex)}</span>
                </>
              );
            })()}
          </span>
        </div>

        {/* Stacked progress bar */}
        <div className="flex h-5 w-full overflow-hidden rounded-full">
          {segments.map((seg, i) => (
            <div
              key={seg.label}
              className={`${hoveredIndex === i ? seg.hoverColor : hoveredIndex !== null ? seg.color + ' opacity-40' : seg.color} cursor-pointer transition-all duration-200`}
              style={{ width: `${Math.max(seg.pct, 0.5)}%` }}
              title={`${seg.label}: ${formatCkbCompact(seg.value)} CKB (${seg.pct.toFixed(1)}%)`}
              onMouseEnter={() => setHoveredIndex(i)}
              onMouseLeave={() => setHoveredIndex(null)}
            />
          ))}
        </div>

        {/* Legend */}
        <div className="mt-3 flex flex-wrap gap-x-6 gap-y-1">
          {segments.map((seg, i) => (
            <div
              key={seg.label}
              className={`flex cursor-pointer items-center gap-2 transition-opacity duration-200 ${hoveredIndex !== null && hoveredIndex !== i ? 'opacity-40' : ''}`}
              onMouseEnter={() => setHoveredIndex(i)}
              onMouseLeave={() => setHoveredIndex(null)}
            >
              <span
                className={`${hoveredIndex === i ? seg.hoverColor : seg.color} inline-block h-2.5 w-2.5 rounded-full transition-all duration-200`}
              />
              <span className="text-text-dim font-mono text-xs">{seg.label}</span>
              <span className={`${seg.textColor} font-mono text-xs font-bold tabular-nums`}>
                {formatCkbCompact(seg.value)} CKB
              </span>
              <span className="text-text-dim font-mono text-[10px] tabular-nums">
                ({ckbToGB(seg.value)})
              </span>
              <span className="text-text-dim font-mono text-[10px] tabular-nums">
                {seg.pct.toFixed(1)}%
              </span>
            </div>
          ))}
        </div>
      </div>
    </Link>
  );
}
