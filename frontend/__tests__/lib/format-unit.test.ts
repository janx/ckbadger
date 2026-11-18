import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';
import { formatCompactInteger, formatCompactRatio } from '@/lib/exact-integer';
import { ckbAmountText } from '@/lib/amount-form';
import { formatTick } from '@/lib/chart-ticks';
import { percentFigure, percentOf, withUnit } from '@/lib/format-unit';

/**
 * One form per quantity (UI review 2026-11-14, CH-14): a compact count or CKB amount takes a
 * lowercase k for thousands, then M, B, T — the readouts read "123.46K" beside the ticks' "500k";
 * a percentage has two decimals and no space before its sign ("24.79%", never "24.79 %").
 */
describe('compact numbers: one form (CH-14)', () => {
  it('writes thousands with a lowercase k in every compact formatter, as the ticks do', () => {
    expect(formatTick(500_000, 100_000)).toBe('500k');
    expect(formatCompactInteger('123456', 'count')).toBe('123.46k');
    expect(formatCompactRatio(BigInt(1234567), BigInt(1000))).toBe('1.23k');
    // re-pinned on purpose (R5): a CKB amount's compact form is cut, 123,456 CKB is 123.45k
    expect(ckbAmountText(BigInt('12345600000000'), 'compact')).toBe('123.45k');
  });

  it('keeps M, B and T for the larger magnitudes', () => {
    expect(formatCompactInteger('123456789', 'count')).toBe('123.46M');
    expect(ckbAmountText(BigInt('3360000000000000000'), 'compact')).toBe('33.60B');
    expect(ckbAmountText(BigInt('120000000000000000000'), 'compact')).toBe('1.20T');
  });

  // The review's item J chose the unit for the ROUNDED value ('1.00M' for 999,995 CKB, never
  // '1000.00k'). Readability plan R5 (re-pinned on purpose): a CKB amount is cut, never rounded,
  // so the unit is the one the amount itself reaches and no figure shows a digit that is not on
  // chain: 999,995 CKB is 999.99k, 999.995 CKB is 999.99.
  it('chooses the unit the cut amount reaches at every boundary', () => {
    const ckb = (n: string) => BigInt(n) * BigInt(100000000);
    const compact = (shannons: bigint) => ckbAmountText(shannons, 'compact');
    expect(compact(ckb('999994'))).toBe('999.99k');
    expect(compact(ckb('999995'))).toBe('999.99k');
    expect(compact(ckb('999999995'))).toBe('999.99M');
    expect(compact(ckb('999999999995'))).toBe('999.99B');
    expect(compact(ckb('1000000'))).toBe('1.00M');
    expect(compact(BigInt('99999500000'))).toBe('999.99'); // 999.995 CKB
    expect(compact(BigInt('99999400000'))).toBe('999.99'); // 999.994 CKB
  });
});

describe('percent: one form (CH-14)', () => {
  it('writes a share with two decimals and no space before its sign', () => {
    expect(percentOf(BigInt(2479), BigInt(10000))).toBe('24.79%');
    expect(percentOf(BigInt(1), BigInt(3))).toBe('33.33%');
    expect(percentOf(BigInt(0), BigInt(5))).toBe('0.00%');
    expect(withUnit('24.79', '%')).toBe('24.79%');
  });

  it('refuses a share of nothing', () => {
    expect(() => percentOf(BigInt(1), BigInt(0))).toThrow();
  });

  // The review's item I (the lead's ruling): rounding half up at two decimals is right, but two
  // printed results would state something false; only those two change. A share above nothing
  // that rounds to 0.00 prints <0.01%; a share below the whole that rounds to 100.00 prints
  // >99.99%. Exact zero and the exact whole print as before.
  it('never prints a false 0.00% or 100.00%', () => {
    const M = BigInt(1_000_000);
    expect(percentOf(BigInt(1), M)).toBe('<0.01%');
    expect(percentOf(BigInt(49), M)).toBe('<0.01%'); // 0.0049 %
    expect(percentOf(BigInt(50), M)).toBe('0.01%'); // 0.005 % rounds half up
    expect(percentOf(M - BigInt(1), M)).toBe('>99.99%');
    expect(percentOf(M - BigInt(50), M)).toBe('>99.99%'); // 99.995 % would round to 100.00
    expect(percentOf(M - BigInt(51), M)).toBe('99.99%');
    expect(percentOf(BigInt(0), M)).toBe('0.00%');
    expect(percentOf(M, M)).toBe('100.00%');
    // the figure alone, for a place that writes the sign apart (a headline's unit)
    expect(percentFigure(BigInt(1), M)).toBe('<0.01');
    expect(percentFigure(M - BigInt(1), M)).toBe('>99.99');
    expect(percentFigure(BigInt(2479), BigInt(10000))).toBe('24.79');
  });
});

/** Sources whose text writes a percent sign after a space: `${share} %`, `{' '}%`. */
const SPACED_PERCENT = [/\}\s%[`'"]/, /' %'/, /\{' '\}\s*\n\s*%\s*\n/];

describe('no page writes a percent sign after a space (CH-14)', () => {
  const root = join(__dirname, '..', '..');
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.(tsx|ts)$/.test(name) && !name.endsWith('.d.ts')) files.push(path);
    }
  };
  for (const dir of ['app', 'components', 'lib']) walk(join(root, dir));

  it('finds the forms it looks for', () => {
    expect(SPACED_PERCENT.some((re) => re.test('`${percent} %`'))).toBe(true);
    expect(SPACED_PERCENT.some((re) => re.test("{' '}\n              %\n"))).toBe(true);
    expect(SPACED_PERCENT.some((re) => re.test('`${percent}%`'))).toBe(false);
  });

  it('writes every percent sign straight after its number', () => {
    const offenders = files.flatMap((file) => {
      const code = readFileSync(file, 'utf8');
      return SPACED_PERCENT.some((re) => re.test(code)) ? [relative(root, file)] : [];
    });
    expect(offenders).toEqual([]);
  });
});
