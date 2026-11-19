import { describe, expect, it } from 'vitest';
import { formatOrphanRateTarget, orphanRateTargetPercent } from '@/lib/network-metrics';

describe('orphanRateTargetPercent', () => {
  it('turns the consensus ratio into a percentage', () => {
    expect(orphanRateTargetPercent({ numerator: 1, denominator: 40 })).toBe(2.5);
    expect(orphanRateTargetPercent({ numerator: 0, denominator: 7 })).toBe(0);
    // re-pinned for the coordinator's ruling of 2026-11-19, a shortened figure is cut: 1.005% is 1.00, its zeros dropped (was half up, 1.01)
    expect(formatOrphanRateTarget({ numerator: 201, denominator: 20000 })).toBe('1');
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
