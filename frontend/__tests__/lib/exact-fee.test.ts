import { describe, expect, it } from 'vitest';
import { exactFeeRate, formatExactFeeRate, medianExactFeeRate } from '@/lib/exact-fee';

describe('exact fee-rate readouts', () => {
  it('rounds a half-cent boundary upward using integers', () => {
    expect(formatExactFeeRate(exactFeeRate('201', 200, 'test'))).toBe('1.01 sh/B');
  });

  it('finds the exact median above the safe JavaScript integer range', () => {
    const rates = [
      exactFeeRate('9007199254740992', 1, 'lower'),
      exactFeeRate('9007199254740993', 1, 'higher'),
    ];
    expect(medianExactFeeRate(rates)).toBe('9007199254740992.50 sh/B');
  });

  it('rejects malformed fee and size instead of inventing zero', () => {
    expect(() => exactFeeRate('1e9', 1, 'bad')).toThrow('bad');
    expect(() => exactFeeRate('10', 0, 'bad')).toThrow('bad');
    expect(medianExactFeeRate([null])).toBe('N/A');
  });
});
