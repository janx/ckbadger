'use client';

import { useState } from 'react';
import { formatPercent, parseShannons } from '@/lib/ckb-decimal';
import { formatCkbAmount, formatCkbCompact } from '@/lib/utils';

interface CapacityUtilizationProps {
  /** What the capacity belongs to, e.g. `spore 0x…` — named in any error. */
  subject: string;
  /** Shannons; must be a positive integer. */
  totalCapacity: string;
  /** Shannons; an integer in [0, totalCapacity]. */
  commonKnowledgeSize: string;
  totalLabel?: string;
  className?: string;
}

type HoverTarget = 'used' | 'free' | null;

interface CapacitySplit {
  total: bigint;
  used: bigint;
}

/** The split the bar draws, or throws naming the value that breaks it — never a clamp. */
function readCapacitySplit(totalCapacity: string, commonKnowledgeSize: string): CapacitySplit {
  const total = parseShannons(totalCapacity, 'totalCapacity');
  const used = parseShannons(commonKnowledgeSize, 'commonKnowledgeSize');
  if (total <= BigInt(0)) throw new Error(`total capacity ${total} is not positive`);
  if (used < BigInt(0)) throw new Error(`common knowledge ${used} is negative`);
  if (used > total) throw new Error(`common knowledge ${used} exceeds total capacity ${total}`);
  return { total, used };
}

export function CapacityUtilization({
  subject,
  totalCapacity,
  commonKnowledgeSize,
  totalLabel = 'Total Capacity',
  className,
}: CapacityUtilizationProps) {
  const [hover, setHover] = useState<HoverTarget>(null);

  let split: CapacitySplit;
  try {
    split = readCapacitySplit(totalCapacity, commonKnowledgeSize);
  } catch (error) {
    return (
      <div
        role="alert"
        data-testid="capacity-utilization-error"
        className={`text-rouge font-mono text-xs ${className ?? ''}`}
      >
        {totalLabel} of {subject}: {error instanceof Error ? error.message : String(error)}
      </div>
    );
  }
  const { total, used } = split;
  const unused = total - used;
  const share = formatPercent(used, total, 1);
  // Bar width only (display geometry); the printed share is the exact string.
  const ratio = Number(share);

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
          <span className={`text-text-dim ml-1.5`}>({share}% share)</span>
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
