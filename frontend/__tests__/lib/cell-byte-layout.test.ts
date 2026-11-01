import { describe, expect, it } from 'vitest';
import { hexRowBytes, hexRowWidth, mapSegmentMinWidth } from '@/lib/cell-byte-layout';

describe('hexRowBytes', () => {
  // Responsive review A#11: at 390 the 24-byte rows (755 px) scrolled inside a 358 px box.
  it('fits the most whole bytes a row the box holds: 24, else 16, else 8', () => {
    expect(hexRowBytes(358, 4)).toBe(8); // 390 px phone
    expect(hexRowBytes(608, 4)).toBe(16); // 640 px window
    expect(hexRowBytes(846, 4)).toBe(24); // desktop column
  });

  it('never picks a row wider than the box, five-digit offsets included', () => {
    for (const digits of [4, 5] as const)
      for (let box = Math.ceil(hexRowWidth(8, digits) + 34); box <= 900; box += 7) {
        const n = hexRowBytes(box, digits);
        expect(hexRowWidth(n, digits) + 34).toBeLessThanOrEqual(box);
      }
    // A five-digit offset is 12 px wider, so 24 bytes need more room.
    expect(hexRowBytes(790, 4)).toBe(24);
    expect(hexRowBytes(790, 5)).toBe(16);
  });

  it('keeps 8 bytes, the smallest row, in a box too narrow even for that', () => {
    expect(hexRowBytes(326, 4)).toBe(8);
  });
});

describe('mapSegmentMinWidth', () => {
  // Responsive review A#10: "8 | lock 53 | type 34 | 2" drew its 2-byte data part as an 8 px
  // sliver round its "2", cut at the box edge.
  it('keeps a part wide enough for its label inside its 4 px padding', () => {
    expect(mapSegmentMinWidth('2', 8)).toBeCloseTo(6.3 + 8, 6);
    expect(mapSegmentMinWidth('lock 53', 8)).toBeCloseTo(7 * 6.3 + 8, 6);
  });

  it('keeps the 28 px floor of a big cell, whose small parts read by count only', () => {
    expect(mapSegmentMinWidth('8', 28)).toBe(28);
    expect(mapSegmentMinWidth('data 100,312', 28)).toBeCloseTo(12 * 6.3 + 8, 6);
  });

  it('refuses a part with no label', () => {
    expect(() => mapSegmentMinWidth('', 8)).toThrow(/no label/);
  });
});
