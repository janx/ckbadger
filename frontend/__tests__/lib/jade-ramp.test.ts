import { describe, expect, it } from 'vitest';
import { HUE_RAMP, jadeRamp } from '@/lib/design-tokens';

describe('jadeRamp: n distinct shades of the one data hue', () => {
  it('is the designed stops while they last', () => {
    expect(HUE_RAMP).toEqual([
      'var(--c-jade)',
      'var(--c-ramp-1)',
      'var(--c-ramp-2)',
      'var(--c-ramp-3)',
      'var(--c-ramp-4)',
      'var(--c-ramp-5)',
    ]);
    expect(jadeRamp(1)).toEqual([HUE_RAMP[0]]);
    expect(jadeRamp(4)).toEqual(HUE_RAMP.slice(0, 4));
    expect(jadeRamp(6)).toEqual([...HUE_RAMP]);
  });

  it('spreads more bands along the same ramp instead of repeating its first shades (C#4)', () => {
    // the HODL wave's eight age bands: brightest to darkest, every one its own shade — the
    // designed stops where a band lands on one, a mix of its two neighbours between them, so the
    // ramp follows the theme
    expect(jadeRamp(8)).toEqual([
      'var(--c-jade)',
      'color-mix(in srgb, var(--c-ramp-1) 71.4%, var(--c-jade))',
      'color-mix(in srgb, var(--c-ramp-2) 42.9%, var(--c-ramp-1))',
      'color-mix(in srgb, var(--c-ramp-3) 14.3%, var(--c-ramp-2))',
      'color-mix(in srgb, var(--c-ramp-3) 85.7%, var(--c-ramp-2))',
      'color-mix(in srgb, var(--c-ramp-4) 57.1%, var(--c-ramp-3))',
      'color-mix(in srgb, var(--c-ramp-5) 28.6%, var(--c-ramp-4))',
      'var(--c-ramp-5)',
    ]);
    for (const n of [7, 8, 12]) expect(new Set(jadeRamp(n)).size).toBe(n);
  });

  it('refuses a ramp of no bands', () => {
    expect(() => jadeRamp(0)).toThrow('jade ramp of 0 shades');
    expect(() => jadeRamp(2.5)).toThrow('jade ramp of 2.5 shades');
  });
});
