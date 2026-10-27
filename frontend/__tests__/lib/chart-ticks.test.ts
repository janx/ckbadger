import { describe, expect, it } from 'vitest';
import {
  MINUS,
  clipToCap,
  formatCompactSigned,
  formatTick,
  niceCeil,
  niceStep,
  niceTicks,
  signedAxisCap,
  stepDecimals,
} from '@/lib/chart-ticks';

describe('niceStep', () => {
  it('rounds a raw step up to 1 / 2 / 2.5 / 5 × 10^k', () => {
    expect(niceStep(0.82)).toBe(1);
    expect(niceStep(4.8)).toBe(5);
    expect(niceStep(0.14)).toBeCloseTo(0.2, 12);
    expect(niceStep(292.7)).toBe(500);
    expect(niceStep(2)).toBe(2);
    expect(niceStep(2.4)).toBe(2.5);
    expect(niceStep(6)).toBe(10);
  });

  it('rejects a non-positive or non-finite step', () => {
    expect(() => niceStep(0)).toThrow(/positive/);
    expect(() => niceStep(Number.NaN)).toThrow(/finite/);
  });
});

describe('stepDecimals', () => {
  it.each([
    [5e-7, 7],
    [1e-8, 8],
    [2.5e-8, 9],
    [1e-12, 12],
    [-5e-7, 7],
  ])(
    'does not round a nonzero %s step down to a zero integer at insufficient precision',
    (step, decimals) => expect(stepDecimals(step)).toBe(decimals)
  );

  it('counts the decimals a step needs', () => {
    expect(stepDecimals(500)).toBe(0);
    expect(stepDecimals(2.5)).toBe(1);
    expect(stepDecimals(0.2)).toBe(1);
    expect(stepDecimals(0.025)).toBe(3);
  });
});

describe('niceTicks', () => {
  it('preserves distinct micro-CKB ticks and an exact nonzero anchor', () => {
    const ticks = niceTicks(0, 0.0000012);
    expect(ticks.step).toBe(0.0000005);
    expect(ticks.values).toEqual([0, 0.0000005, 0.000001, 0.0000015]);
    expect(ticks.values.map((value) => formatTick(value, ticks.step))).toEqual([
      '0',
      '0.0000005',
      '0.0000010',
      '0.0000015',
    ]);
    expect(niceTicks(0.00000001, 0.00000003, 4, 0.00000002).values).toEqual([
      0.00000001, 0.00000002, 0.00000003,
    ]);
  });

  it('replaces the old min..max thirds (11.2 / 10.2 / 9.3) with round block-time ticks', () => {
    expect(niceTicks(8.54, 11).values).toEqual([8, 9, 10, 11]);
  });

  it('starts bar axes at zero and ends them on a round number', () => {
    expect(niceTicks(0, 878).values).toEqual([0, 500, 1000]);
    expect(niceTicks(0, 17503).values).toEqual([0, 10000, 20000]);
    expect(niceTicks(0, 7769).values).toEqual([0, 5000, 10000]);
  });

  it('prints decimal ticks without float noise', () => {
    const { values, step } = niceTicks(2.42, 2.84);
    expect(step).toBeCloseTo(0.2, 12);
    expect(values).toEqual([2.4, 2.6, 2.8, 3]);
  });

  it('covers both ends of the data', () => {
    const { values } = niceTicks(88.7, 103.1);
    expect(values[0]).toBeLessThanOrEqual(88.7);
    expect(values[values.length - 1]).toBeGreaterThanOrEqual(103.1);
    expect(values).toEqual([85, 90, 95, 100, 105]);
  });

  it('gives a flat series a real domain instead of a zero-height axis', () => {
    expect(niceTicks(0, 0).values).toEqual([0, 0.5, 1]);
    const flat = niceTicks(10, 10).values;
    expect(flat[0]).toBeLessThan(10);
    expect(flat[flat.length - 1]).toBeGreaterThan(10);
  });

  it('puts an anchor value on a tick, e.g. the 2.5 % orphan-rate target', () => {
    expect(niceTicks(2.42, 2.84, 4, 2.5).values).toEqual([2.3, 2.5, 2.7, 2.9]);
    expect(niceTicks(0, 878, 4, 250).values).toEqual([-250, 250, 750, 1250]);
  });

  it('rejects an inverted or non-finite range', () => {
    expect(() => niceTicks(3, 2)).toThrow(/above max/);
    expect(() => niceTicks(0, Number.POSITIVE_INFINITY)).toThrow(/finite/);
  });
});

describe('formatTick', () => {
  it('labels thousands as k / M / B with trailing zeros trimmed', () => {
    expect(formatTick(0, 500)).toBe('0');
    expect(formatTick(500, 500)).toBe('500');
    expect(formatTick(1000, 500)).toBe('1k');
    expect(formatTick(2500, 2500)).toBe('2.5k');
    expect(formatTick(20000, 10000)).toBe('20k');
    expect(formatTick(15e6, 15e6)).toBe('15M');
    expect(formatTick(2e9, 1e9)).toBe('2B');
  });

  it('keeps the decimals of a sub-thousand step and uses a real minus sign', () => {
    expect(formatTick(2.4, 0.2)).toBe('2.4');
    expect(formatTick(3, 0.2)).toBe('3.0');
    expect(formatTick(-2e6, 2e6)).toBe(`${MINUS}2M`);
  });
});

describe('signedAxisCap', () => {
  it('rounds up the 2nd-largest |change| × 1.25, so the outlier is the one clipped', () => {
    // Knowledge size: one −8.94M day, the next-largest −1.51M → ±2M.
    expect(signedAxisCap([0.3e6, -8.94e6, -1.51e6, 0.4e6])).toBe(2e6);
    // DAO deposits: one +28.12M day, the next-largest −11.9M → ±15M.
    expect(signedAxisCap([5.2e6, 28.12e6, -11.9e6, 1e6])).toBe(15e6);
  });

  it('builds the cap from the only non-zero value, so nothing clips', () => {
    const cap = signedAxisCap([0, 0, -3]);
    expect(cap).toBeGreaterThanOrEqual(3);
    expect(clipToCap([0, 0, -3], cap).some((b) => b.clipped)).toBe(false);
  });

  it('gives an all-zero series a ±1 axis', () => {
    expect(signedAxisCap([0, 0])).toBe(1);
    expect(signedAxisCap([])).toBe(1);
  });

  it('rejects a non-finite value', () => {
    expect(() => signedAxisCap([1, Number.NaN])).toThrow(/finite/);
  });
});

describe('niceCeil', () => {
  it('rounds up to a round mantissa', () => {
    expect(niceCeil(1.89e6)).toBe(2e6);
    expect(niceCeil(14.9e6)).toBe(15e6);
    expect(niceCeil(2.6)).toBe(3);
    expect(niceCeil(9)).toBe(10);
  });
});

describe('clipToCap', () => {
  it('draws values past the cap at the cap and marks them clipped', () => {
    expect(clipToCap([1, -3, 2.5], 2)).toEqual([
      { value: 1, drawn: 1, clipped: false },
      { value: -3, drawn: -2, clipped: true },
      { value: 2.5, drawn: 2, clipped: true },
    ]);
  });

  it('does not clip a value exactly at the cap', () => {
    expect(clipToCap([2], 2)[0].clipped).toBe(false);
  });
});

describe('formatCompactSigned', () => {
  it('formats a clipped bar label with sign and unit', () => {
    expect(formatCompactSigned(28.12e6)).toBe('+28.12M');
    expect(formatCompactSigned(-8.94e6)).toBe(`${MINUS}8.94M`);
    expect(formatCompactSigned(950_000)).toBe('+950k');
    expect(formatCompactSigned(12_500)).toBe('+12.5k');
    expect(formatCompactSigned(0)).toBe('0');
  });
});
