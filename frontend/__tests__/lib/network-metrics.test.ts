import { describe, expect, it } from 'vitest';
import { formatPercentShort, hashRateUnit, orphanRateTargetPercent } from '@/lib/network-metrics';

describe('hashRateUnit', () => {
  it('picks the largest unit the maximum reaches', () => {
    expect(hashRateUnit(93.91e15)).toEqual({ suffix: 'PH/s', divisor: 1e15 });
    // The pre-fix 1000×-low mainnet series reads as TH/s: the unit shows the scale, it does not fix it.
    expect(hashRateUnit(91.75e12)).toEqual({ suffix: 'TH/s', divisor: 1e12 });
    expect(hashRateUnit(1.2e18)).toEqual({ suffix: 'EH/s', divisor: 1e18 });
    expect(hashRateUnit(0)).toEqual({ suffix: 'H/s', divisor: 1 });
  });

  it('rejects a negative or non-finite rate', () => {
    expect(() => hashRateUnit(-1)).toThrow(/non-negative/);
    expect(() => hashRateUnit(Number.NaN)).toThrow(/non-negative/);
  });
});

describe('orphanRateTargetPercent', () => {
  it('turns the consensus ratio into a percentage', () => {
    expect(orphanRateTargetPercent({ numerator: 1, denominator: 40 })).toBe(2.5);
    expect(orphanRateTargetPercent({ numerator: 0, denominator: 7 })).toBe(0);
  });

  it('throws when the API did not send the target', () => {
    expect(() => orphanRateTargetPercent(undefined)).toThrow(
      'the uncle-rate response has no orphanRateTarget'
    );
  });

  it('throws on a malformed ratio', () => {
    expect(() => orphanRateTargetPercent({ numerator: 1, denominator: 0 })).toThrow(/denominator/);
    expect(() => orphanRateTargetPercent({ numerator: -1, denominator: 40 })).toThrow(/numerator/);
    expect(() => orphanRateTargetPercent({ numerator: 0.5, denominator: 40 })).toThrow(/numerator/);
  });
});

describe('formatPercentShort', () => {
  it('drops trailing zeros', () => {
    expect(formatPercentShort(2.5)).toBe('2.5');
    expect(formatPercentShort(2.64)).toBe('2.64');
    expect(formatPercentShort(3)).toBe('3');
    expect(formatPercentShort(10)).toBe('10');
  });
});
