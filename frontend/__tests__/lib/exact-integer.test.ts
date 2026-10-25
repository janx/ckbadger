import { describe, expect, it } from 'vitest';
import {
  exactUnsignedInteger,
  formatCompactInteger,
  formatScaledInteger,
  formatDecimal,
  exactDecimalRatio,
  formatCompactRatio,
  formatExactShare,
} from '@/lib/exact-integer';

describe('exact integer display', () => {
  it('keeps bits beyond Number.MAX_SAFE_INTEGER and rounds half-up', () => {
    expect(exactUnsignedInteger('9007199254740993', 'hash rate')).toBe(BigInt('9007199254740993'));
    expect(formatScaledInteger(BigInt('9007199254740993'), BigInt('1000000000000000'), 2)).toBe(
      '9.01'
    );
    expect(formatScaledInteger(BigInt(15), BigInt(10), 0)).toBe('2');
    expect(formatCompactInteger('9007199254740993', 'difficulty')).toBe('9.01P');
  });

  it('rejects missing, signed and fractional API values', () => {
    for (const value of ['', '-1', '1.5', 'NaN']) {
      expect(() => exactUnsignedInteger(value, 'daily hash rate')).toThrow('daily hash rate');
    }
  });

  it('rounds a decimal ratio at the display boundary without floating point', () => {
    expect(formatDecimal('0.125', 1, 100)).toBe('12.5');
    expect(formatDecimal('0.126499999999999999', 2, 100)).toBe('12.65');
  });

  it('formats decimal chart amounts and shares from exact rational values', () => {
    expect(exactDecimalRatio('1.005', 'chart')).toEqual([BigInt(1005), BigInt(1000)]);
    expect(formatCompactRatio(BigInt(1005), BigInt(1000))).toBe('1.01');
    expect(formatCompactRatio(BigInt('9007199254740993'), BigInt(1))).toBe('9.01P');
    expect(
      formatExactShare(
        [
          [BigInt(201), BigInt(1)],
          [BigInt(19799), BigInt(1)],
        ],
        0
      )
    ).toBe('1.01%');
    expect(
      formatExactShare(
        [
          [BigInt(1), BigInt(2)],
          [BigInt(1), BigInt(3)],
        ],
        0
      )
    ).toBe('60.00%');
    expect(formatExactShare([[BigInt(0), BigInt(1)]], 0)).toBe('—');
    expect(() => exactDecimalRatio('1e3', 'chart')).toThrow('chart');
  });
});
