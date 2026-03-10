'use client';

import { formatCkbAmount, formatCkbCompact } from '@/lib/utils';

interface CapacityUtilizationProps {
  totalCapacity: string;
  occupiedCapacity: string;
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
  occupiedCapacity,
  totalLabel = 'Total Capacity',
  className,
}: CapacityUtilizationProps) {
  const zero = BigInt(0);
  const ratioScale = BigInt(10000);

  const total = parseBigInt(totalCapacity);
  const occupiedRaw = parseBigInt(occupiedCapacity);
  if (total == null || occupiedRaw == null || total <= zero) {
    return null;
  }

  const occupied = occupiedRaw < zero ? zero : occupiedRaw > total ? total : occupiedRaw;
  const unoccupied = total - occupied;
  const ratio = Number((occupied * ratioScale) / total) / 100;

  return (
    <div className={className}>
      <div className="mb-2 flex items-center justify-between">
        <span className="text-text-muted font-mono text-xs uppercase tracking-wider">
          {totalLabel}
        </span>
        <span
          className="text-text-primary font-mono text-xs tabular-nums"
          title={formatCkbAmount(total.toString()).full + ' CKB'}
        >
          {formatCkbCompact(total.toString()).value} CKB
        </span>
      </div>
      <div className="bg-base-elevated flex h-3 w-full overflow-hidden rounded-sm">
        <div
          className="bg-warning transition-all duration-300"
          style={{ width: `${Math.max(ratio, 0.5)}%` }}
        />
        <div className="bg-emphasis/30 flex-1" />
      </div>
      <div className="mt-1.5 flex items-center justify-between">
        <span
          className="text-warning font-mono text-xs"
          title={formatCkbAmount(occupied.toString()).full + ' CKB'}
        >
          Occupied: {formatCkbCompact(occupied.toString()).value} CKB
          <span className="text-text-muted ml-1.5">({ratio.toFixed(1)}% occupied)</span>
        </span>
        <span
          className="text-emphasis font-mono text-xs"
          title={formatCkbAmount(unoccupied.toString()).full + ' CKB'}
        >
          Unoccupied: {formatCkbCompact(unoccupied.toString()).value} CKB
        </span>
      </div>
    </div>
  );
}
