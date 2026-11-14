'use client';

import { HMUL_EXPLANATION } from '@/components/ui/hmul-info';
import { formatScaledInteger } from '@/lib/exact-integer';

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
