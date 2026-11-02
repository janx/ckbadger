'use client';

import { HMUL_EXPLANATION, HMulHelpPopover } from '@/components/ui/hmul-info';
import { formatCkbAmount, formatCkbCompact } from '@/lib/utils';
import { formatScaledInteger } from '@/lib/exact-integer';
import { parseShannons } from '@/lib/ckb-decimal';

/**
 * "H-multiplier ×1.02: CKB held ÷ occupied" as it reads in a summary line — the one way the term is
 * written outside a table column (consistency plan C12): never bare, its definition beside it and
 * the full sentence in the title. The hyphenated term and its value are one unit that never breaks
 * inside ("H-" / "multiplier", B#18).
 */
export function HMultiplierReading({ held, occupied }: { held: bigint; occupied: bigint }) {
  return (
    <span data-role="h-multiplier-reading" title={HMUL_EXPLANATION}>
      <span data-role="h-multiplier" className="whitespace-nowrap">
        H-multiplier ×{formatScaledInteger(held, occupied, 2)}
      </span>
      : CKB held ÷ occupied
    </span>
  );
}

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
        <span className="text-text-dim text-small font-mono uppercase tracking-wider">
          {totalLabel}
        </span>
        <span
          className="text-text-bright text-small font-mono tabular-nums"
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
          className="text-warning text-small font-mono"
          title={formatCkbAmount(used.toString()).full + ' CKB'}
        >
          Common Knowledge: {formatCkbCompact(used.toString()).value} CKB
        </span>
        <span className="inline-flex items-center gap-1.5">
          <span className="text-gold text-small font-mono tabular-nums">HMul: {hmul}x</span>
          <HMulHelpPopover align="end" />
        </span>
      </div>
    </div>
  );
}
