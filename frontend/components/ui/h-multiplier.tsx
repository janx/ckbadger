'use client';

import { formatCkbAmount, formatCkbCompact } from '@/lib/utils';

interface HMultiplierProps {
  totalCapacity: string; // total capacity in shannons (BigInt string)
  usedCapacity: string; // used capacity in shannons (BigInt string)
  totalLabel?: string; // defaults to 'Cells Capacity'
  className?: string;
}

function parseBigInt(value: string): bigint | null {
  try {
    return BigInt(value);
  } catch {
    return null;
  }
}

export function HMultiplier({
  totalCapacity,
  usedCapacity,
  totalLabel = 'Cells Capacity',
  className,
}: HMultiplierProps) {
  const zero = BigInt(0);

  const total = parseBigInt(totalCapacity);
  const used = parseBigInt(usedCapacity);
  if (total == null || used == null || total <= zero || used <= zero) {
    return null;
  }

  const ratioScale = BigInt(10000);
  const usedBarPercent = Number((used * ratioScale) / total) / 100;
  const hmul = Number((total * ratioScale) / used) / 10000;

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
          style={{ width: `${Math.max(usedBarPercent, 0.5)}%` }}
        />
        <div className="bg-gold/30 flex-1" />
      </div>
      <div className="mt-1.5 flex items-center justify-between">
        <span
          className="text-warning font-mono text-xs"
          title={formatCkbAmount(used.toString()).full + ' CKB'}
        >
          Used: {formatCkbCompact(used.toString()).value} CKB
        </span>
        <span className="text-gold font-mono text-xs tabular-nums">HMul: {hmul.toFixed(2)}x</span>
      </div>
    </div>
  );
}
