import { describe, expect, it } from 'vitest';
import {
  exactUnsignedInteger,
  formatCompactInteger,
  formatScaledInteger,
  formatDecimal,
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
});
