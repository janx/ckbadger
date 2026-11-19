import { describe, expect, it } from 'vitest';
import {
  exactUnsignedInteger,
  formatCompactInteger,
  formatScaledInteger,
  cutScaledInteger,
  formatDecimal,
  exactDecimalRatio,
  exactSignedDecimalRatio,
  formatCompactRatio,
  formatExactDecimal,
} from '@/lib/exact-integer';

describe('exact integer display', () => {
  // Re-pinned for the coordinator's ruling of 2026-11-19 (a shortened figure is cut, never
  // rounded): a compact form takes the next unit where the value reaches it, no longer where it
  // rounds to it (999,999.5 read 1.00M; it reads 999.99k, every digit on chain).
  it('promotes the unit where the value reaches the next compact boundary', () => {
    // CH-14 (re-pinned): thousands are a lowercase k, as the ticks write them
    const suffixes = ['k', 'M', 'B', 'T', 'P', 'E'];
    for (let index = 0; index < suffixes.length; index++) {
      const unit = BigInt(1000) ** BigInt(index + 1);
      expect(formatCompactRatio(unit, BigInt(1))).toBe(`1.00${suffixes[index]}`);
      if (index > 0) {
        expect(formatCompactInteger(unit.toString(), 'count')).toBe(`1.00${suffixes[index]}`);
        expect(formatCompactInteger((unit - BigInt(1)).toString(), 'count')).toBe(
          `999.99${suffixes[index - 1]}`
        );
        expect(formatCompactRatio(unit * BigInt(100000) - BigInt(1), BigInt(100000))).toBe(
          `999.99${suffixes[index - 1]}`
        );
      }
    }
  });

  // Re-pinned for the ruling: 9.007… reads 9.00 (was half up, 9.01), 1.5 reads 1 (was 2).
  it('keeps bits beyond Number.MAX_SAFE_INTEGER and cuts, never rounds', () => {
    expect(exactUnsignedInteger('9007199254740993', 'hash rate')).toBe(BigInt('9007199254740993'));
    expect(formatScaledInteger(BigInt('9007199254740993'), BigInt('1000000000000000'), 2)).toBe(
      '9.00'
    );
    expect(formatScaledInteger(BigInt(15), BigInt(10), 0)).toBe('1');
    expect(formatCompactInteger('9007199254740993', 'difficulty')).toBe('9.00P');
    expect(formatCompactInteger('999999', 'count')).toBe('999.99k');
  });

  it('rejects missing, signed and fractional API values', () => {
    for (const value of ['', '-1', '1.5', 'NaN']) {
      expect(() => exactUnsignedInteger(value, 'daily hash rate')).toThrow('daily hash rate');
    }
  });

  it('groups an exact decimal without rounding any digit', () => {
    expect(formatExactDecimal('65053585849.85425816', 'supply')).toBe('65,053,585,849.85425816');
    expect(formatExactDecimal('+208801027.17121727', 'delta')).toBe('+208,801,027.17121727');
    expect(formatExactDecimal('-30.00000000', 'flow')).toBe('-30.00000000');
    expect(formatExactDecimal('0', 'zero')).toBe('0');
    expect(() => formatExactDecimal('1e3', 'bad')).toThrow('bad');
  });

  // Re-pinned for the ruling: 12.6499… reads 12.64 (was half up, 12.65).
  it('cuts a decimal ratio at the display boundary without floating point', () => {
    expect(formatDecimal('0.125', 1, 100)).toBe('12.5');
    expect(formatDecimal('0.126499999999999999', 2, 100)).toBe('12.64');
  });

  it('formats decimal chart amounts from exact rational values', () => {
    expect(exactDecimalRatio('1.005', 'chart')).toEqual([BigInt(1005), BigInt(1000)]);
    expect(exactSignedDecimalRatio('-30.00000000', 'flow')).toEqual([
      BigInt(-3000000000),
      BigInt(100000000),
    ]);
    expect(() => exactDecimalRatio('-30.00000000', 'unsigned')).toThrow('unsigned');
    // re-pinned for the ruling: cut (was half up, 1.01)
    expect(formatCompactRatio(BigInt(1005), BigInt(1000))).toBe('1.00');
    // Two fixed decimals: a non-whole amount that rounds to ".00" must not look exact (CH-02).
    expect(formatCompactRatio(BigInt(600), BigInt(1))).toBe('600.00');
    expect(formatCompactRatio(BigInt(9900000001), BigInt(100000000))).toBe('99.00');
    expect(formatCompactRatio(BigInt('9007199254740993'), BigInt(1))).toBe('9.00P');
    expect(() => exactDecimalRatio('1e3', 'chart')).toThrow('chart');
  });
});

// Lane D4 follow-up (the coordinator's item 3; the house rule, readability R5 and the brief's rule
// 5: a shortened figure is the exact value CUT, never rounded, as a rounded figure can show a digit
// that is not on chain). The H-multiplier was shortened by formatScaledInteger, which rounded
// half-up: 1.005 read "1.01". Its cut is cutScaledInteger. The coordinator's ruling of 2026-11-19
// (lane D1's follow-up): formatScaledInteger cuts too, for every caller, and the API writes the
// epoch-durations buckets' labels cut, so the half-up form is gone (re-pinned: 1.005 read 1.01).
describe('an exact ratio cut, never rounded', () => {
  it('cuts at the display precision where half-up and cut differ', () => {
    expect(cutScaledInteger(BigInt(1005), BigInt(1000), 2)).toBe('1.00');
    expect(formatScaledInteger(BigInt(1005), BigInt(1000), 2)).toBe('1.00');
    expect(cutScaledInteger(BigInt(1999), BigInt(1000), 2)).toBe('1.99');
    expect(cutScaledInteger(BigInt(15), BigInt(10), 0)).toBe('1');
    expect(cutScaledInteger(BigInt('9007199254740993'), BigInt('1000000000000000'), 2)).toBe(
      '9.00'
    );
    expect(cutScaledInteger(BigInt(150190999), BigInt(10000), 2)).toBe('15019.09');
  });

  it('refuses what formatScaledInteger refuses', () => {
    expect(() => cutScaledInteger(BigInt(-1), BigInt(1), 2)).toThrow('invalid exact integer');
    expect(() => cutScaledInteger(BigInt(1), BigInt(0), 2)).toThrow('invalid exact integer');
    expect(() => cutScaledInteger(BigInt(1), BigInt(1), -1)).toThrow('invalid exact integer');
  });
});
