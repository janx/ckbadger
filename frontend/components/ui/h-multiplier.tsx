'use client';

import { HMulHelpPopover } from '@/components/ui/hmul-info';
import { formatCkbAmount, formatCkbCompact } from '@/lib/utils';
import { formatScaledInteger } from '@/lib/exact-integer';
import { parseShannons } from '@/lib/ckb-decimal';

interface HMultiplierProps {
  totalCapacity: string; // total capacity in shannons (BigInt string)
  commonKnowledgeSize: string; // common knowledge size in shannons (BigInt string)
  totalLabel?: string; // defaults to 'Cells Capacity'
  className?: string;
}

export function HMultiplier({
  totalCapacity,
  commonKnowledgeSize,
  totalLabel = 'Owned Capacity',
  className,
}: HMultiplierProps) {
  const zero = BigInt(0);

  const total = parseShannons(totalCapacity, 'totalCapacity');
  const used = parseShannons(commonKnowledgeSize, 'commonKnowledgeSize');
  if (total < zero || used < zero || used > total)
    throw new Error(`invalid HMul capacity split ${total}/${used}`);
  if (used === zero) {
    return null;
  }

  const usedBarPercent = (Number(used) / Number(total)) * 100;
  const hmul = formatScaledInteger(total, used, 2);

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
          style={{ width: `${usedBarPercent}%` }}
        />
        <div className="bg-gold/30 flex-1" />
      </div>
      <div className="mt-1.5 flex items-center justify-between">
        <span
          className="text-warning font-mono text-xs"
          title={formatCkbAmount(used.toString()).full + ' CKB'}
        >
          Common Knowledge: {formatCkbCompact(used.toString()).value} CKB
        </span>
        <span className="inline-flex items-center gap-1.5">
          <span className="text-gold font-mono text-xs tabular-nums">HMul: {hmul}x</span>
          <HMulHelpPopover align="end" />
        </span>
      </div>
    </div>
  );
}
