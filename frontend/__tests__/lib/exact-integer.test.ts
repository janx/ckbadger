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
  it('promotes the unit when rounding reaches the next compact boundary', () => {
    // CH-14 (re-pinned): thousands are a lowercase k, as the ticks write them
    const suffixes = ['k', 'M', 'B', 'T', 'P', 'E'];
    for (let index = 0; index < suffixes.length; index++) {
      const unit = BigInt(1000) ** BigInt(index + 1);
      const threshold = unit * BigInt(100000) - unit / BigInt(2);
      expect(formatCompactRatio(threshold, BigInt(100000))).toBe(`1.00${suffixes[index]}`);
      if (index > 0) {
        expect(formatCompactInteger((threshold / BigInt(100000)).toString(), 'count')).toBe(
          `1.00${suffixes[index]}`
        );
        expect(formatCompactRatio(threshold - BigInt(1), BigInt(100000))).toBe(
          `999.99${suffixes[index - 1]}`
        );
      }
    }
  });

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

  it('groups an exact decimal without rounding any digit', () => {
    expect(formatExactDecimal('65053585849.85425816', 'supply')).toBe('65,053,585,849.85425816');
    expect(formatExactDecimal('+208801027.17121727', 'delta')).toBe('+208,801,027.17121727');
    expect(formatExactDecimal('-30.00000000', 'flow')).toBe('-30.00000000');
    expect(formatExactDecimal('0', 'zero')).toBe('0');
    expect(() => formatExactDecimal('1e3', 'bad')).toThrow('bad');
  });

  it('rounds a decimal ratio at the display boundary without floating point', () => {
    expect(formatDecimal('0.125', 1, 100)).toBe('12.5');
    expect(formatDecimal('0.126499999999999999', 2, 100)).toBe('12.65');
  });

  it('formats decimal chart amounts from exact rational values', () => {
    expect(exactDecimalRatio('1.005', 'chart')).toEqual([BigInt(1005), BigInt(1000)]);
    expect(exactSignedDecimalRatio('-30.00000000', 'flow')).toEqual([
      BigInt(-3000000000),
      BigInt(100000000),
    ]);
    expect(() => exactDecimalRatio('-30.00000000', 'unsigned')).toThrow('unsigned');
    expect(formatCompactRatio(BigInt(1005), BigInt(1000))).toBe('1.01');
    // Two fixed decimals: a non-whole amount that rounds to ".00" must not look exact (CH-02).
    expect(formatCompactRatio(BigInt(600), BigInt(1))).toBe('600.00');
    expect(formatCompactRatio(BigInt(9900000001), BigInt(100000000))).toBe('99.00');
    expect(formatCompactRatio(BigInt('9007199254740993'), BigInt(1))).toBe('9.01P');
    expect(() => exactDecimalRatio('1e3', 'chart')).toThrow('chart');
  });
});

// Lane D4 follow-up (the coordinator's item 3; the house rule, readability R5 and the brief's rule
// 5: a shortened figure is the exact value CUT, never rounded, as a rounded figure can show a digit
// that is not on chain). The H-multiplier was shortened by formatScaledInteger, which rounds
// half-up: 1.005 read "1.01". Its cut is cutScaledInteger. formatScaledInteger rounds still, as its
// other callers read it — chart readouts and labels, the home network figures, the compact forms, a
// token's largest share and its holders' bar geometry, the orphan-rate target — and the API writes
// the epoch-durations buckets' labels by the same half-up rule, which the chart's grid check
// reproduces with it: whether those cut is the coordinator's ruling (the lane's hand-back lists
// each, old and new).
describe('an exact ratio cut, never rounded', () => {
  it('cuts at the display precision where half-up and cut differ', () => {
    expect(cutScaledInteger(BigInt(1005), BigInt(1000), 2)).toBe('1.00');
    expect(formatScaledInteger(BigInt(1005), BigInt(1000), 2)).toBe('1.01');
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
