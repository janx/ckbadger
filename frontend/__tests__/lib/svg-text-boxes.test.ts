import { describe, expect, it } from 'vitest';
import { meetings, pastPx, type TextLine } from '@/__tests__/utils/svg-text-boxes';

/**
 * The two measures a known fault of a figure is pinned by (review F-B's test finding 1): how far
 * two words overlap across, and how far a word stands past its box. Boxes are written by hand.
 */
const line = (text: string, left: number, right: number, top = 0, bottom = 15): TextLine => ({
  text,
  left,
  right,
  top,
  bottom,
  size: 12,
});

describe('meetings', () => {
  it('names every two lines that meet, with the width they share across', () => {
    const a = line('15:47:24.564', 100, 186.4);
    const b = line('15:47:24.564', 177.6, 264);
    const c = line('clear of both', 300, 393.6);
    expect(meetings([a, b, c])).toEqual([{ a: '15:47:24.564', b: '15:47:24.564', by: 8.8 }]);
  });

  it('finds none where lines stand apart, side by side or a tight line apart', () => {
    expect(meetings([line('a', 0, 10), line('b', 10.5, 20)])).toEqual([]);
    // stacked: one line's foot is the next one's top
    expect(meetings([line('a', 0, 10, 0, 15), line('b', 0, 10, 15, 30)])).toEqual([]);
  });

  it('counts a touch as a meeting of no width', () => {
    expect(meetings([line('a', 0, 10), line('b', 10, 20)])).toEqual([{ a: 'a', b: 'b', by: 0 }]);
  });
});

describe('pastPx', () => {
  const box = { left: 0, top: 0, right: 326, bottom: 100 };
  it('measures how far a line stands past the box on its farthest side, to the hundredth', () => {
    expect(pastPx(line('6 epochs', 269.48, 327.08), box)).toBe(1.08);
    expect(pastPx(line('below', 10, 20, 95, 110), box)).toBe(10);
    expect(pastPx(line('left', -4.2, 20), box)).toBe(4.2);
  });

  it('is 0 for a line flush with the box and negative inside it', () => {
    expect(pastPx(line('flush', 300, 326), box)).toBe(0);
    expect(pastPx(line('inside', 10, 20, 10, 25), box)).toBe(-10);
  });
});
