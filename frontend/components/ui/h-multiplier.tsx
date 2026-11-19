'use client';

import { HMUL_EXPLANATION } from '@/components/ui/hmul-info';
import { cutScaledInteger } from '@/lib/exact-integer';
import { groupCount } from '@/lib/format-count';

/**
 * The H-multiplier as every place writes it, from its exact ratio (CKB held ÷ occupied): "×" and the
 * ratio cut at two decimals, never rounded (the house rule: ×1.005 reads ×1.00), its whole part
 * grouped (PA-09: "×15019.09" read as no figure of ours). The one form of the figure: the reading
 * below, the inventory's column and its phone rows.
 */
export function formatHMultiplier(numerator: bigint, denominator: bigint): string {
  const [whole, fraction] = cutScaledInteger(numerator, denominator, 2).split('.');
  return `×${groupCount(BigInt(whole))}.${fraction}`;
}

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
        H-multiplier {formatHMultiplier(held, occupied)}
      </span>
      : CKB held ÷ occupied
    </span>
  );
}
