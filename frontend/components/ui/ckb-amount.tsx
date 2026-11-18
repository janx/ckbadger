import { cn } from '@/lib/utils';
import { MINUS } from '@/lib/chart-ticks';
import {
  amountDigits,
  ckbAmountTitle,
  ckbDigits,
  shownSign,
  type AmountDigits,
  type AmountForm,
} from '@/lib/amount-form';

/**
 * The ink of an amount by its sign, the one table every amount renderer reads. The whole amount,
 * its integer and its decimals, is one ink (readability plan 2026-11-18, ruling R5): a number's
 * significant digits are never quieter than its integer. The ghost is for a column's padding
 * zeros only; the palette pins each ink at text grade on every ground in both themes.
 */
export const AMOUNT_INK = {
  positive: 'text-positive',
  negative: 'text-negative',
  neutral: 'text-text',
} as const;
export type AmountSign = keyof typeof AMOUNT_INK;

/** The sign of an amount (`+`, `−` or none) as an {@link AMOUNT_INK} key. */
export function amountSign(sign: string): AmountSign {
  if (sign === '+') return 'positive';
  if (sign === MINUS) return 'negative';
  if (sign === '') return 'neutral';
  throw new Error(`amount sign "${sign}" is not +, ${MINUS} or none`);
}

/**
 * How an amount is inked: `sign` by its sign (`AMOUNT_INK`), `plain` in the reading ink,
 * `inherit` in its parent's (a display figure in its class colour, a value in a brighter line).
 */
export type AmountTone = 'sign' | 'plain' | 'inherit';

function inkOf(tone: AmountTone, sign: string): string | undefined {
  if (tone === 'inherit') return undefined;
  return AMOUNT_INK[tone === 'sign' ? amountSign(sign) : 'neutral'];
}

/** A column's padding zeros, the one part of an amount in the ghost (R4, R5). */
function Padding({ d }: { d: AmountDigits }) {
  return d.padding ? (
    <span data-ghost className="text-text-ghost">
      {d.padding}
    </span>
  ) : null;
}

/**
 * A CKB amount in the form its context names (`lib/amount-form.ts`: exact, column, summary,
 * compact), in one ink. A shortened figure carries its exact value and unit in `title`, and is
 * never an operand. Zero is "0 CKB" in every form (consistency plan C4): nothing moved, so there
 * are no decimals to read, and that 0 is a datum in the dim ink (R4). Where no sign is written
 * (`signed={false}`) the amount is never negative: one that is fails, as a `TokenAmount` does,
 * rather than read as its absolute value; the rule is the amount module's (`ckbDigits`), which
 * the texts of an amount (`ckbAmountText`, `exactCkbText`, a title) hold to as this does.
 */
export function CkbAmount({
  shannons,
  form,
  signed = true,
  tone = 'sign',
  wrap = false,
  unit = true,
}: {
  shannons: bigint;
  /** The context's form, never a default: see the guidelines' "Amounts". */
  form: AmountForm;
  signed?: boolean;
  tone?: AmountTone;
  /**
   * Allow a line break before the decimal, preserving every digit. The unit stays with the last
   * digits: a number never parts from its unit (U-16).
   */
  wrap?: boolean;
  /** F2 blocks: false when a column header already names the unit ("Reward, CKB", BA-46). */
  unit?: boolean;
}) {
  // The module's rule: where no sign is written the amount is never negative (`ckbDigits` fails).
  const d = ckbDigits(shannons, form, signed);
  const unitEl = unit && <span className="text-text-dim text-meta whitespace-pre"> CKB</span>;
  if (shannons === BigInt(0))
    return (
      <span data-ckb-amount data-form={form} className="whitespace-nowrap tabular-nums">
        <span data-zero className="text-text-dim">
          0
        </span>
        {unitEl}
      </span>
    );
  return (
    <span
      data-ckb-amount
      data-form={form}
      title={d.shortened ? ckbAmountTitle(shannons, signed) : undefined}
      className={cn(
        wrap
          ? 'inline-flex max-w-full flex-wrap justify-end tabular-nums'
          : 'whitespace-nowrap tabular-nums',
        inkOf(tone, d.sign)
      )}
    >
      {/* The sign and the whole part, then (where a line may break, with `wrap`) the decimals, a
          compact unit, a column's padding and the unit after them. */}
      <span className="whitespace-nowrap">
        {signed ? shownSign(d) : ''}
        {d.integer}
      </span>
      <span className="whitespace-nowrap">
        {d.fraction}
        {d.suffix}
        <Padding d={d} />
        {unitEl}
      </span>
    </span>
  );
}

/**
 * A token amount (base units, an integer; a change may be negative) in the form its context
 * names, with the token's own decimals: the same forms as CKB, a summary below 1 keeping four
 * significant digits. Unknown decimals (`null`) are written as the raw integer, whole and marked
 * "(raw)" in every form, never scaled by a guess. A long amount breaks only after a thousands
 * separator. A shortened figure carries its exact value in `title`.
 */
export function TokenAmount({
  amount,
  decimals,
  form,
  signed = false,
  tone = 'inherit',
}: {
  amount: string;
  decimals: number | null;
  form: AmountForm;
  signed?: boolean;
  tone?: AmountTone;
}) {
  if (!/^-?\d+$/.test(amount)) throw new Error(`token amount "${amount}" is not an integer`);
  const value = BigInt(amount);
  if (!signed && value < BigInt(0))
    throw new Error(`token amount ${amount} is negative where a sign is not written`);
  const d =
    decimals === null
      ? amountDigits(value, 0, 'exact')
      : amountDigits(value, decimals, form, 'significant');
  // A digit run never breaks by itself. A long form (exact, column) may break after a thousands
  // separator and nowhere else, so a wrapped amount never strands a digit or splits its decimals;
  // a shortened one is short and stays whole.
  const groups = form === 'exact' || form === 'column' ? d.integer.split(',') : [d.integer];
  return (
    <span
      data-token-amount
      data-form={form}
      title={d.shortened ? `${signed ? d.sign : ''}${d.exact}` : undefined}
      className={cn('tabular-nums', inkOf(tone, d.sign))}
    >
      {signed ? shownSign(d) : ''}
      {groups.flatMap((group, i) =>
        i < groups.length - 1 ? [`${group},`, <wbr key={i} />] : [group]
      )}
      {d.fraction}
      {d.suffix}
      <Padding d={d} />
      {decimals === null && ' (raw)'}
    </span>
  );
}
