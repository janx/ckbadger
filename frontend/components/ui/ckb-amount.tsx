import { cn } from '@/lib/utils';
import { MINUS } from '@/lib/chart-ticks';
import { splitCkbAmount } from '@/lib/ckb-decimal';
import type { PaletteKey } from '@/lib/palette';

/**
 * The ink of an amount by its sign: the integer in the sign's own ink, the fraction digits a step
 * quieter in a named ink of that sign (UI review 2026-11-14, R4, DY-06) — each at 4.5:1 on every
 * ground in both themes (`__tests__/lib/palette.test.ts`), never the sign's ink at an opacity.
 * The one table every amount renderer reads: HTML takes the class (`fraction`), SVG text the
 * palette key as a fill (`color(fractionKey)`); the type keeps the class the key's.
 */
export const AMOUNT_INK = {
  positive: { integer: 'text-positive', fraction: 'text-jade-dim', fractionKey: 'jade-dim' },
  negative: { integer: 'text-negative', fraction: 'text-rouge-dim', fractionKey: 'rouge-dim' },
  neutral: { integer: 'text-text', fraction: 'text-text-dim', fractionKey: 'text-dim' },
} as const satisfies Record<string, AmountInk>;
/** An amount's inks: the fraction's class is its palette key's text class, a literal for Tailwind. */
type AmountInk = {
  [K in PaletteKey]: { integer: string; fraction: `text-${K}`; fractionKey: K };
}[PaletteKey];
export type AmountSign = keyof typeof AMOUNT_INK;

/** The sign of a split amount (`+`, `−` or none) as an {@link AMOUNT_INK} key. */
export function amountSign(sign: string): AmountSign {
  if (sign === '+') return 'positive';
  if (sign === MINUS) return 'negative';
  if (sign === '') return 'neutral';
  throw new Error(`amount sign "${sign}" is not +, ${MINUS} or none`);
}

/**
 * Exact CKB, all 8 decimals: integer, the significant decimals, then the trailing zeros ghosted,
 * so right-aligned amounts line up on the decimal point. `tone="sign"` colours by sign, the
 * fraction in the sign's quieter ink (`AMOUNT_INK`). Zero is "0 CKB" with no fraction, for every
 * caller (consistency plan C4): nothing moved, so there are no decimals to read. That 0 is a
 * datum, in the dim ink; the ghost is only for the padding zeros of a non-zero amount (R4).
 */
export function ExactCkb({
  shannons,
  signed = true,
  tone = 'sign',
  wrap = false,
  unit = true,
}: {
  shannons: bigint;
  signed?: boolean;
  tone?: 'sign' | 'plain';
  /**
   * Allow a line break before the decimal, preserving every digit. The unit stays with the last
   * digits: a number never parts from its unit (U-16).
   */
  wrap?: boolean;
  /** F2 blocks: false when a column header already names the unit ("Reward, CKB", BA-46). */
  unit?: boolean;
}) {
  const unitEl = unit && <span className="text-text-dim text-meta whitespace-pre"> CKB</span>;
  if (shannons === BigInt(0))
    return (
      <span data-ckb-amount className="whitespace-nowrap tabular-nums">
        <span data-zero className="text-text-dim">
          0
        </span>
        {unitEl}
      </span>
    );
  const parts = splitCkbAmount(shannons);
  const ink = AMOUNT_INK[tone === 'plain' ? 'neutral' : amountSign(parts.sign)];
  return (
    <span
      data-ckb-amount
      className={cn(
        wrap
          ? 'inline-flex max-w-full flex-wrap justify-end tabular-nums'
          : 'whitespace-nowrap tabular-nums',
        ink.integer
      )}
    >
      <span className="whitespace-nowrap">
        {signed ? parts.sign : ''}
        {parts.integer}
      </span>
      <span className="whitespace-nowrap">
        <span data-fraction className={ink.fraction}>
          {parts.fraction}
        </span>
        <span data-ghost className="text-text-ghost">
          {parts.ghost}
        </span>
        {unitEl}
      </span>
    </span>
  );
}
