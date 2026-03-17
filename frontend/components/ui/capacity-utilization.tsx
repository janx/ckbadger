'use client';

import { formatCkbAmount, formatCkbCompact } from '@/lib/utils';

interface CapacityUtilizationProps {
  totalCapacity: string;
  commonKnowledgeSize: string;
  totalLabel?: string;
  className?: string;
}

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
          className="bg-gold transition-all duration-300"
          style={{ width: `${Math.max(ratio, 0.5)}%` }}
        />
        <div className="bg-gold/30 flex-1" />
      </div>
      <div className="mt-1.5 flex items-center justify-between">
        <span
          className="text-warning font-mono text-xs"
          title={formatCkbAmount(used.toString()).full + ' CKB'}
        >
          Common Knowledge: {formatCkbCompact(used.toString()).value} CKB
          <span className="text-text-dim ml-1.5">({ratio.toFixed(1)}% share)</span>
        </span>
        <span
          className="text-gold font-mono text-xs"
          title={formatCkbAmount(unused.toString()).full + ' CKB'}
        >
          Free Capacity: {formatCkbCompact(unused.toString()).value} CKB
        </span>
      </div>
    </div>
  );
}
