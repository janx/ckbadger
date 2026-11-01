import { describe, expect, it } from 'vitest';
import { HUE_RAMP, jadeRamp } from '@/lib/design-tokens';

describe('jadeRamp: n distinct shades of the one data hue', () => {
  it('is the designed stops while they last', () => {
    expect(jadeRamp(1)).toEqual([HUE_RAMP[0]]);
    expect(jadeRamp(4)).toEqual(HUE_RAMP.slice(0, 4));
    expect(jadeRamp(6)).toEqual([...HUE_RAMP]);
  });

  it('spreads more bands along the same ramp instead of repeating its first shades (C#4)', () => {
    // the HODL wave's eight age bands: brightest to darkest, every one its own shade
    expect(jadeRamp(8)).toEqual([
      '#2edba3',
      '#27c191',
      '#24a981',
      '#259373',
      '#287f66',
      '#2a6c5b',
      '#2e5b51',
      '#344e48',
    ]);
    for (const n of [7, 8, 12]) expect(new Set(jadeRamp(n)).size).toBe(n);
  });

  it('refuses a ramp of no bands', () => {
    expect(() => jadeRamp(0)).toThrow('jade ramp of 0 shades');
    expect(() => jadeRamp(2.5)).toThrow('jade ramp of 2.5 shades');
  });
});
