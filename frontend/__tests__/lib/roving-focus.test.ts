import { describe, expect, it } from 'vitest';
import { rovingTarget } from '@/lib/roving-focus';

/**
 * The keyboard of a row of controls with one stop in the tab order (WAI-ARIA's roving tabindex):
 * a tab row moves along its line (← →), a radio group by any arrow (← ↑ back, → ↓ on), both wrap
 * at the ends, and Home and End go to the first and the last. Any other key is not theirs.
 */
describe('rovingTarget', () => {
  it('moves a tab row along its line and wraps at its ends', () => {
    expect(rovingTarget('ArrowRight', 0, 3, 'line')).toBe(1);
    expect(rovingTarget('ArrowRight', 2, 3, 'line')).toBe(0);
    expect(rovingTarget('ArrowLeft', 0, 3, 'line')).toBe(2);
    expect(rovingTarget('ArrowLeft', 2, 3, 'line')).toBe(1);
    expect(rovingTarget('Home', 2, 3, 'line')).toBe(0);
    expect(rovingTarget('End', 0, 3, 'line')).toBe(2);
    expect(rovingTarget('ArrowDown', 0, 3, 'line')).toBeNull();
    expect(rovingTarget('ArrowUp', 1, 3, 'line')).toBeNull();
  });

  it('moves a radio group by any arrow', () => {
    expect(rovingTarget('ArrowDown', 0, 4, 'any')).toBe(1);
    expect(rovingTarget('ArrowUp', 0, 4, 'any')).toBe(3);
    expect(rovingTarget('ArrowRight', 3, 4, 'any')).toBe(0);
    expect(rovingTarget('ArrowLeft', 1, 4, 'any')).toBe(0);
    expect(rovingTarget('End', 1, 4, 'any')).toBe(3);
  });

  it('leaves every other key alone', () => {
    for (const key of ['Enter', ' ', 'Tab', 'a', 'PageDown'])
      expect(rovingTarget(key, 1, 3, 'any')).toBeNull();
  });

  it('refuses a row with nothing in it, or a place outside it', () => {
    expect(() => rovingTarget('ArrowRight', 0, 0, 'line')).toThrow('no item');
    expect(() => rovingTarget('ArrowRight', 3, 3, 'line')).toThrow('not in a row of 3');
  });
});
