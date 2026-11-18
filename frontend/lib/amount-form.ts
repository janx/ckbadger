/**
 * How an amount is written (readability plan 2026-11-18, ruling R5): one rule, four forms, every
 * one computed in BigInt base units (shannons for CKB, a token's own base units), no float
 * anywhere. A shortened figure is the exact value CUT, never rounded — a rounded figure can show
 * a digit that is not on chain (999.995 is not 1,000.00) — and it carries the exact value in its
 * title; nothing shortened is ever an operand. The renderer is `components/ui/ckb-amount.tsx`;
 * a call site names the form of its context (`frontend/docs/ui-color-guidelines.md`, "Amounts"):
 *
 * - `exact`: the value, its trailing zeros dropped, no point for a whole number ("122,000",
 *   "0.0003", "559.5125321"): an amount that is the subject of its page, a chart's readout;
 * - `column`: exact, padded to the unit's decimals, the padding apart (drawn in the ghost) so the
 *   points of a numeric column of subject amounts align ("750.00000000");
 * - `summary`: whole from 1,000 up ("8,131,306,075"), at most two decimals from 1 to 1,000
 *   ("999.99", "12.5"), the exact value below 1 ("0.00000001"), or, for a token, its first four
 *   significant digits ("0.0001234"): a hero, a stat tile, a list column, a chart's head figure,
 *   a feed line, a phone row, prose;
 * - `compact`: two decimals of the largest unit reached, k, M, B or T ("9.02B", "123.45k"), and
 *   two decimals below 1,000 ("999.99"): a tile or an axis.
 */
import { MINUS } from '@/lib/chart-ticks';

export type AmountForm = 'exact' | 'column' | 'summary' | 'compact';

/** An amount below 1 in the summary: CKB keeps its exact value, a token four significant digits. */
export type BelowOne = 'exact' | 'significant';

export interface AmountDigits {
  /** The value's sign: `+`, `−` (U+2212), or none for zero. */
  sign: '' | '+' | typeof MINUS;
  /** The whole part shown, in thousands groups (compact: of its unit). */
  integer: string;
  /** `.` and the decimals shown, or none. */
  fraction: string;
  /** The compact unit (`k`, `M`, `B`, `T`), or none. */
  suffix: string;
  /** A column's padding zeros after the decimals shown (with the `.` when none is significant). */
  padding: string;
  /** The exact value, unsigned, as `exact` writes it: what a shortened figure's title names. */
  exact: string;
  /** The figure is not the exact value written out (digits cut, or a compact unit): title it. */
  shortened: boolean;
}

const ZERO = BigInt(0);
const TEN = BigInt(10);
const HUNDRED = BigInt(100);
const THOUSAND = BigInt(1000);
/** The compact units, largest first, in whole units of the amount. */
const COMPACT_UNITS = [
  ['T', TEN ** BigInt(12)],
  ['B', TEN ** BigInt(9)],
  ['M', TEN ** BigInt(6)],
  ['k', THOUSAND],
] as const;

function grouped(n: bigint): string {
  return n.toLocaleString('en-US');
}

/** `value` hundredths of a unit as `9.02`: the whole part grouped, two decimals. */
function hundredths(value: bigint): { integer: string; fraction: string } {
  return {
    integer: grouped(value / HUNDRED),
    fraction: `.${(value % HUNDRED).toString().padStart(2, '0')}`,
  };
}

/**
 * The digits of `value` base units of an amount with `decimals` decimals in `form`. Throws on a
 * negative number of decimals: the caller's unit is wrong, and no form can hide that.
 */
export function amountDigits(
  value: bigint,
  decimals: number,
  form: AmountForm,
  belowOne: BelowOne = 'exact'
): AmountDigits {
  if (!Number.isInteger(decimals) || decimals < 0)
    throw new Error(`amount ${value}: ${decimals} is not a number of decimals`);
  const sign: AmountDigits['sign'] = value > ZERO ? '+' : value < ZERO ? MINUS : '';
  const abs = value < ZERO ? -value : value;
  const scale = TEN ** BigInt(decimals);
  const whole = abs / scale;
  const digits = decimals === 0 ? '' : (abs % scale).toString().padStart(decimals, '0');
  const significant = digits.replace(/0+$/, '');
  const exact = `${grouped(whole)}${significant ? `.${significant}` : ''}`;
  const written = (
    integer: string,
    fraction: string,
    shortened: boolean,
    suffix = ''
  ): AmountDigits => ({
    sign,
    integer,
    fraction,
    suffix,
    padding: '',
    exact,
    shortened,
  });
  const exactly = written(grouped(whole), significant ? `.${significant}` : '', false);
  if (abs === ZERO || form === 'exact') return exactly;
  if (form === 'column')
    return {
      ...exactly,
      padding: decimals === 0 ? '' : significant ? digits.slice(significant.length) : `.${digits}`,
    };
  if (form === 'compact') {
    for (const [suffix, unit] of COMPACT_UNITS) {
      const unitScale = unit * scale;
      if (abs >= unitScale) {
        const shown = (abs * HUNDRED) / unitScale;
        const { integer, fraction } = hundredths(shown);
        return written(integer, fraction, true, suffix);
      }
    }
    if (decimals === 0) return exactly;
    // Two decimals below 1,000 units; a unit with fewer writes what it has, never a zero of its own
    const places = Math.min(decimals, 2);
    return written(
      grouped(whole),
      `.${digits.slice(0, places)}`,
      digits.slice(places).replace(/0+$/, '') !== ''
    );
  }
  // summary
  if (whole >= THOUSAND) return written(grouped(whole), '', significant !== '');
  if (whole >= BigInt(1)) {
    const kept = digits.slice(0, 2).replace(/0+$/, '');
    return written(grouped(whole), kept ? `.${kept}` : '', significant.length > 2);
  }
  if (belowOne === 'exact') return exactly;
  const first = digits.search(/[1-9]/);
  const kept = digits.slice(0, first + 4).replace(/0+$/, '');
  return written('0', `.${kept}`, significant.length > first + 4);
}

/** The sign a figure shows: none on a figure of zeros (a compact "0.00"; its title has it). */
export function shownSign(d: AmountDigits): AmountDigits['sign'] {
  return /^[0,.]*$/.test(`${d.integer}${d.fraction}`) ? '' : d.sign;
}

/** The text of an amount's digits: the sign when `signed`. */
export function amountText(d: AmountDigits, signed: boolean): string {
  return `${signed ? shownSign(d) : ''}${d.integer}${d.fraction}${d.suffix}${d.padding}`;
}

/** A CKB amount, from shannons, as text in `form`, without its unit. */
export function ckbAmountText(shannons: bigint, form: AmountForm, signed = false): string {
  return amountText(amountDigits(shannons, 8, form), signed);
}

/** The exact CKB amount and its unit, for a shortened figure's title: `+1,000.99999999 CKB`. */
export function ckbAmountTitle(shannons: bigint, signed: boolean): string {
  return `${ckbAmountText(shannons, 'exact', signed)} CKB`;
}

/** The title of a CKB figure written as text in `form`: its exact value when it was shortened. */
export function ckbShortenedTitle(
  shannons: bigint,
  form: AmountForm,
  signed = false
): string | undefined {
  return amountDigits(shannons, 8, form).shortened ? ckbAmountTitle(shannons, signed) : undefined;
}

const DECIMAL_RE = /^([+-])?(\d+)(?:\.(\d+))?$/;

/**
 * A decimal string in display units (a chart's `"8131306075.44505907"`, `"+30.5"`) as its base
 * units and its number of decimals, exactly as written: no digit is dropped or rounded.
 */
export function parseDecimalAmount(raw: string): {
  value: bigint;
  decimals: number;
  plus: boolean;
} {
  const m = DECIMAL_RE.exec(raw);
  if (!m) throw new Error(`"${raw}" is not a decimal amount`);
  const [, sign, whole, fraction = ''] = m;
  const abs = BigInt(`${whole}${fraction}`);
  return { value: sign === '-' ? -abs : abs, decimals: fraction.length, plus: sign === '+' };
}
