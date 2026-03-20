'use client';

import { useState } from 'react';
import { formatCkbAmount, formatCkbCompact } from '@/lib/utils';

interface CapacityUtilizationProps {
  totalCapacity: string;
  commonKnowledgeSize: string;
  totalLabel?: string;
  className?: string;
}

type HoverTarget = 'used' | 'free' | null;

function parseBigInt(value: string): bigint | null {
  try {
    return BigInt(value);
  } catch {
    return null;
  }
}

export function CapacityUtilization({
  totalCapacity,
  commonKnowledgeSize,
  totalLabel = 'Total Capacity',
  className,
}: CapacityUtilizationProps) {
  const [hover, setHover] = useState<HoverTarget>(null);
  const zero = BigInt(0);
  const ratioScale = BigInt(10000);

  const total = parseBigInt(totalCapacity);
  const usedRaw = parseBigInt(commonKnowledgeSize);
  if (total == null || usedRaw == null || total <= zero) {
    return null;
  }

  const used = usedRaw < zero ? zero : usedRaw > total ? total : usedRaw;
  const unused = total - used;
  const ratio = Number((used * ratioScale) / total) / 100;

  const gold = '#f2c55c';
  const glow = `0 0 8px ${gold}aa, 0 0 2px ${gold}`;

  // Hovered segment: full brightness + glow. Non-hovered: nearly invisible.
  const usedBg = hover === 'free' ? `${gold}18` : gold;
  const freeBg = hover === 'used' ? `${gold}08` : hover === 'free' ? `${gold}cc` : `${gold}4d`;
  const usedShadow = hover === 'used' ? glow : 'none';
  const freeShadow = hover === 'free' ? glow : 'none';

  return (
    <div className={className}>
      <div className="mb-2 flex items-center justify-between">
        <span className="text-text-dim font-mono text-xs uppercase tracking-wider">
          {totalLabel}
        </span>
        <span
          className="text-text-bright font-mono text-xs tabular-nums"
          title={formatCkbAmount(total.toString()).full + ' CKB'}
        >
          {formatCkbCompact(total.toString()).value} CKB
        </span>
      </div>
      <div className="bg-base-elevated flex h-3 w-full overflow-hidden rounded-sm">
        <div
          className="transition-[background-color,box-shadow] duration-200"
          style={{
            width: `${Math.max(ratio, 0.5)}%`,
            backgroundColor: usedBg,
            boxShadow: usedShadow,
          }}
          onMouseEnter={() => setHover('used')}
          onMouseLeave={() => setHover(null)}
        />
        <div
          className="flex-1 transition-[background-color,box-shadow] duration-200"
          style={{
            backgroundColor: freeBg,
            boxShadow: freeShadow,
          }}
          onMouseEnter={() => setHover('free')}
          onMouseLeave={() => setHover(null)}
        />
      </div>
      <div className="mt-1.5 flex items-center justify-between">
        <span
          className={`cursor-default font-mono text-xs transition-opacity duration-200 ${hover === 'free' ? 'opacity-25' : ''} text-warning`}
          title={formatCkbAmount(used.toString()).full + ' CKB'}
          onMouseEnter={() => setHover('used')}
          onMouseLeave={() => setHover(null)}
        >
          Common Knowledge: {formatCkbCompact(used.toString()).value} CKB
          <span className={`text-text-dim ml-1.5`}>({ratio.toFixed(1)}% share)</span>
        </span>
        <span
          className={`cursor-default font-mono text-xs transition-opacity duration-200 ${hover === 'used' ? 'opacity-25' : ''} text-gold`}
          title={formatCkbAmount(unused.toString()).full + ' CKB'}
          onMouseEnter={() => setHover('free')}
          onMouseLeave={() => setHover(null)}
        >
          Free Capacity: {formatCkbCompact(unused.toString()).value} CKB
        </span>
      </div>
    </div>
  );
}
