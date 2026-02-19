'use client';

import { formatCkbAmount, formatCkbCompact } from '@/lib/utils';

interface CapacityUtilizationProps {
  totalCapacity: string;
  occupiedCapacity: string;
  label?: string;
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
  label = 'Capacity Utilization',
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
        <span className="font-mono text-xs uppercase tracking-wider text-slate-500">{label}</span>
        <span className="font-mono text-xs text-slate-400">{ratio.toFixed(1)}% occupied</span>
      </div>
      <div className="flex h-3 w-full overflow-hidden rounded-sm bg-slate-800">
        <div
          className="bg-amber transition-all duration-300"
          style={{ width: `${Math.max(ratio, 0.5)}%` }}
        />
        <div className="bg-terminal-green/30 flex-1" />
      </div>
      <div className="mt-1.5 flex items-center justify-between">
        <span
          className="text-amber font-mono text-xs"
          title={formatCkbAmount(occupied.toString()).full + ' CKB'}
        >
          Occupied: {formatCkbCompact(occupied.toString()).value} CKB
        </span>
        <span
          className="text-terminal-green font-mono text-xs"
          title={formatCkbAmount(unoccupied.toString()).full + ' CKB'}
        >
          Unoccupied: {formatCkbCompact(unoccupied.toString()).value} CKB
        </span>
      </div>
    </div>
  );
}
