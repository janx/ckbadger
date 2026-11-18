import { describe, expect, it } from 'vitest';
import { amountDigits, ckbAmountText, ckbAmountTitle, type AmountForm } from '@/lib/amount-form';
import { MINUS } from '@/lib/chart-ticks';

/**
 * How an amount is written (readability plan 2026-11-18, ruling R5, lane B2): four forms from one
 * function, every one computed in BigInt base units (no float anywhere), a shortened figure CUT,
 * never rounded, so it shows no digit that is not on chain.
 *
 * - exact: the value, its trailing zeros dropped, no point for a whole number;
 * - column: exact, padded to the unit's decimals (the padding apart, for the ghost);
 * - summary: whole from 1,000 up; at most two decimals from 1 to 1,000; the exact value below 1
 *   (a token keeps four significant digits there);
 * - compact: "9.02B", two decimals of the largest unit reached (k, M, B, T), cut.
 */

const CKB = BigInt(100000000);
/** A CKB amount written as a decimal, in shannons (the test's own parse, never a float). */
function ckb(text: string): bigint {
  const [whole, fraction = ''] = text.replace(/,/g, '').split('.');
  const negative = whole.startsWith('-');
  const abs = BigInt(negative ? whole.slice(1) : whole) * CKB + BigInt(fraction.padEnd(8, '0'));
  return negative ? -abs : abs;
}

/** The four forms of one amount, unsigned, and whether each needs the exact value in its title. */
function forms(shannons: bigint) {
  return Object.fromEntries(
    (['exact', 'column', 'summary', 'compact'] as AmountForm[]).map((form) => {
      const d = amountDigits(shannons, 8, form);
      return [form, `${d.integer}${d.fraction}${d.suffix}${d.padding}${d.shortened ? ' †' : ''}`];
    })
  );
}

describe('the four forms of a CKB amount, at their boundaries (R5)', () => {
  it('writes zero as one 0 in every form, never shortened', () => {
    expect(forms(BigInt(0))).toEqual({ exact: '0', column: '0', summary: '0', compact: '0' });
  });

  it('writes an amount below 1 CKB exactly in the summary', () => {
    expect(forms(ckb('0.00000001'))).toEqual({
      exact: '0.00000001',
      column: '0.00000001',
      summary: '0.00000001',
      compact: '0.00 †',
    });
    expect(forms(ckb('0.99999999'))).toEqual({
      exact: '0.99999999',
      column: '0.99999999',
      summary: '0.99999999',
      compact: '0.99 †',
    });
  });

  it('cuts to two decimals from 1 to 1,000, dropping trailing zeros, never rounding up', () => {
    expect(forms(ckb('1'))).toEqual({
      exact: '1',
      column: '1.00000000',
      summary: '1',
      compact: '1.00',
    });
    expect(forms(ckb('999.99999999'))).toEqual({
      exact: '999.99999999',
      column: '999.99999999',
      summary: '999.99 †',
      compact: '999.99 †',
    });
    // 999.995 rounds to 1,000.00 at two places: cut, it stays 999.99
    expect(forms(ckb('999.995'))).toEqual({
      exact: '999.995',
      column: '999.99500000',
      summary: '999.99 †',
      compact: '999.99 †',
    });
    expect(amountDigits(ckb('12.5'), 8, 'summary')).toMatchObject({
      fraction: '.5',
      shortened: false,
    });
    expect(amountDigits(ckb('12.10000001'), 8, 'summary')).toMatchObject({
      fraction: '.1',
      shortened: true,
    });
  });

  it('writes whole CKB from 1,000 up, the fraction cut', () => {
    expect(forms(ckb('1000'))).toEqual({
      exact: '1,000',
      column: '1,000.00000000',
      summary: '1,000',
      compact: '1.00k †',
    });
    expect(forms(ckb('1000.99999999'))).toEqual({
      exact: '1,000.99999999',
      column: '1,000.99999999',
      summary: '1,000 †',
      compact: '1.00k †',
    });
  });

  it('cuts the compact form too, choosing the unit the amount reaches (999,995 CKB is 999.99k)', () => {
    const compact = (text: string) => ckbAmountText(ckb(text), 'compact');
    expect(compact('123456')).toBe('123.45k');
    expect(compact('999995')).toBe('999.99k');
    expect(compact('999999995')).toBe('999.99M');
    expect(compact('999999999995')).toBe('999.99B');
    expect(compact('9024511234.5')).toBe('9.02B');
    expect(compact('33600000000')).toBe('33.60B');
    expect(compact('1200000000000')).toBe('1.20T');
    // the review's item J was about rounding: cut, the boundary is the unit itself
    expect(compact('1004999999999.99999999')).toBe('1.00T');
    expect(compact('1005000000000')).toBe('1.00T');
  });

  it('writes a negative with the minus sign in every form, when signed', () => {
    const signed = (text: string, form: AmountForm) => ckbAmountText(ckb(text), form, true);
    expect(signed('-0.00000001', 'summary')).toBe(`${MINUS}0.00000001`);
    expect(signed('-0.99999999', 'summary')).toBe(`${MINUS}0.99999999`);
    expect(signed('-1', 'summary')).toBe(`${MINUS}1`);
    expect(signed('-999.99999999', 'summary')).toBe(`${MINUS}999.99`);
    expect(signed('-999.995', 'summary')).toBe(`${MINUS}999.99`);
    expect(signed('-1000', 'summary')).toBe(`${MINUS}1,000`);
    expect(signed('-1000.99999999', 'summary')).toBe(`${MINUS}1,000`);
    expect(signed('-1000.99999999', 'exact')).toBe(`${MINUS}1,000.99999999`);
    expect(signed('-1', 'column')).toBe(`${MINUS}1.00000000`);
    expect(signed('-9024511234.5', 'compact')).toBe(`${MINUS}9.02B`);
    expect(signed('12.5', 'summary')).toBe('+12.5');
    // a compact figure that shows only zeros has no sign; its title keeps it
    expect(signed('-0.00000001', 'compact')).toBe('0.00');
    expect(ckbAmountTitle(ckb('-0.00000001'), true)).toBe(`${MINUS}0.00000001 CKB`);
    // unsigned, the magnitude
    expect(ckbAmountText(ckb('-999.995'), 'summary')).toBe('999.99');
  });

  it('keeps a 20-digit integer part whole, in BigInt, with no float anywhere', () => {
    const big = ckb('12345678901234567890.12345678');
    expect(forms(big)).toEqual({
      exact: '12,345,678,901,234,567,890.12345678',
      column: '12,345,678,901,234,567,890.12345678',
      summary: '12,345,678,901,234,567,890 †',
      compact: '12,345,678.90T †',
    });
    expect(ckbAmountText(-big, 'summary', true)).toBe(`${MINUS}12,345,678,901,234,567,890`);
    expect(ckbAmountText(BigInt('18446744073709551615'), 'summary')).toBe('184,467,440,737');
  });

  it('names the exact value and its unit for a title, signed when the figure is', () => {
    expect(ckbAmountTitle(ckb('1000.99999999'), false)).toBe('1,000.99999999 CKB');
    expect(ckbAmountTitle(ckb('1000.99999999'), true)).toBe('+1,000.99999999 CKB');
    expect(ckbAmountTitle(ckb('1000'), false)).toBe('1,000 CKB');
  });
});

describe('the four forms of a token amount: its own decimals', () => {
  const token = (raw: string, decimals: number, form: AmountForm) => {
    const d = amountDigits(BigInt(raw), decimals, form, 'significant');
    return `${d.integer}${d.fraction}${d.suffix}${d.padding}${d.shortened ? ' †' : ''}`;
  };

  it('keeps four significant digits below 1 in the summary, cut', () => {
    // 0.000123456789 of an 18-decimal token
    expect(token('123456789000000', 18, 'summary')).toBe('0.0001234 †');
    expect(token('100000000000000000', 18, 'summary')).toBe('0.1');
    expect(token('123450000000000000', 18, 'summary')).toBe('0.1234 †');
    expect(token('999999999999999999', 18, 'summary')).toBe('0.9999 †');
    expect(token('123400000000000000', 18, 'exact')).toBe('0.1234');
  });

  it('cuts to two decimals from 1 to 1,000 and to whole tokens from 1,000 up', () => {
    expect(token('1500000000000000000', 18, 'summary')).toBe('1.5');
    expect(token('1234500000000000000000', 18, 'summary')).toBe('1,234 †');
    expect(token('1234500000000000000000', 18, 'exact')).toBe('1,234.5');
    expect(token('1234500000000000000000', 18, 'column')).toBe('1,234.500000000000000000');
    expect(token('1234500000000000000000', 18, 'compact')).toBe('1.23k †');
  });

  it('writes a token with no decimals whole in every form but the compact', () => {
    expect(token('1234', 0, 'exact')).toBe('1,234');
    expect(token('1234', 0, 'column')).toBe('1,234');
    expect(token('1234', 0, 'summary')).toBe('1,234');
    expect(token('1234', 0, 'compact')).toBe('1.23k †');
    expect(token('999', 0, 'compact')).toBe('999');
  });

  it('refuses a negative number of decimals', () => {
    expect(() => amountDigits(BigInt(1), -1, 'exact')).toThrow(/decimals/);
  });
});
