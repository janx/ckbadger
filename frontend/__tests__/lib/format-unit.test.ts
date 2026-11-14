import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';
import { formatCompactInteger, formatCompactRatio } from '@/lib/exact-integer';
import { formatCkbCompact, formatCompactCkbDelta } from '@/lib/utils';
import { formatTick } from '@/lib/chart-ticks';
import { percentOf, withUnit } from '@/lib/format-unit';

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
    expect(formatCkbCompact(BigInt('12345600000000')).value).toBe('123.46k');
    expect(formatCompactCkbDelta('-1234.5').compact).toBe('1.23k');
  });

  it('keeps M, B and T for the larger magnitudes', () => {
    expect(formatCompactInteger('123456789', 'count')).toBe('123.46M');
    expect(formatCkbCompact(BigInt('3360000000000000000')).value).toBe('33.60B');
    expect(formatCkbCompact(BigInt('120000000000000000000')).value).toBe('1.20T');
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
