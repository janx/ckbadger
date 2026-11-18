/**
 * How an amount is written (readability plan 2026-11-18, ruling R5, lane B2). Today's writing
 * under the forms' names, so their tests fail on what changes: every form is the padded exact
 * amount `ExactCkb` draws, and the compact one is `formatCkbCompact`'s, rounded.
 */
import { MINUS } from '@/lib/chart-ticks';
import { exactCkbText, splitCkbAmount } from '@/lib/ckb-decimal';
import { formatCkbCompact } from '@/lib/utils';

export type AmountForm = 'exact' | 'column' | 'summary' | 'compact';

export interface AmountDigits {
  sign: '' | '+' | typeof MINUS;
  integer: string;
  fraction: string;
  suffix: string;
  padding: string;
  exact: string;
  shortened: boolean;
}

export function amountDigits(
  value: bigint,
  decimals: number,
  form: AmountForm,
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  belowOne: 'exact' | 'significant' = 'exact'
): AmountDigits {
  if (decimals < 0) throw new Error(`amount ${value}: ${decimals} decimals`);
  const parts = splitCkbAmount(value * BigInt(10) ** BigInt(8 - Math.min(decimals, 8)));
  const sign = parts.sign as AmountDigits['sign'];
  if (form === 'compact')
    return {
      sign,
      integer: formatCkbCompact(value).value,
      fraction: '',
      suffix: '',
      padding: '',
      exact: exactCkbText(value, false),
      shortened: false,
    };
  return {
    sign,
    integer: parts.integer,
    fraction: parts.fraction,
    suffix: '',
    padding: parts.ghost,
    exact: exactCkbText(value, false),
    shortened: false,
  };
}

export function ckbAmountText(shannons: bigint, form: AmountForm, signed = false): string {
  const d = amountDigits(shannons, 8, form);
  return `${signed ? d.sign : ''}${d.integer}${d.fraction}${d.suffix}${d.padding}`;
}

export function ckbAmountTitle(shannons: bigint, signed: boolean): string {
  return `${exactCkbText(shannons, signed)} CKB`;
}
