import { describe, expect, it } from 'vitest';
import {
  HEX_BOX_INSET_PX,
  hexRowBytes,
  hexRowWidth,
  mapSegmentMinWidth,
} from '@/lib/cell-byte-layout';
import { charPx, fontPx, textPx } from '@/lib/type-metrics';

describe('hexRowBytes', () => {
  // Responsive review A#11: at 390 the 24-byte rows (755 px) scrolled inside a 358 px box.
  // Re-pinned on purpose (lane P1 follow-up, item 5): the frame round the rows is the panel's 1 px
  // border, the well's 8 px margin (m-2), its 1 px border and its 10 px padding (px-2.5), 40 px,
  // where the measure took 34 (the well's border and a 16 px padding) and missed the panel's
  // border and the well's margin: the 8-byte row, 329.2 px, ran 23 px out of the well's 306 px at
  // 390 (Chrome, the integration preview) and scrolled it, 53 px at 360, 93 at 320. A 4-byte row,
  // 211.6 px, is drawn whole at every phone width; 24 at the desktop column and 16 at 640 stay.
  it('fits the most whole bytes a row the box holds: 24, else 16, else 8, else 4', () => {
    expect(HEX_BOX_INSET_PX).toBe(2 * (1 + 8 + 1 + 10));
    expect(hexRowBytes(846, 4)).toBe(24); // the desktop column (1280)
    expect(hexRowBytes(608, 4)).toBe(16); // 640 px window
    expect(hexRowBytes(358, 4)).toBe(4); // 390 px phone: 8 bytes are 369.2 px with the frame
    expect(hexRowBytes(328, 4)).toBe(4); // 360
    expect(hexRowBytes(288, 4)).toBe(4); // 320: 251.6 px with the frame
  });

  it('never picks a row wider than the box, five-digit offsets included', () => {
    for (const digits of [4, 5] as const)
      for (let box = Math.ceil(hexRowWidth(4, digits) + HEX_BOX_INSET_PX); box <= 900; box += 7) {
        const n = hexRowBytes(box, digits);
        expect(hexRowWidth(n, digits) + HEX_BOX_INSET_PX).toBeLessThanOrEqual(box);
      }
    // A five-digit offset is 8 px wider (65 and 73 px at the dump's 13 px: re-pinned, readability
    // R4, from 62 and 74 written by hand at 12 px), so 24 bytes need more room: a 24-byte row is
    // 799.6 px with four digits and 807.6 with five, and the frame adds 40 (re-pinned, it was 34).
    expect(hexRowBytes(846, 4)).toBe(24);
    expect(hexRowBytes(846, 5)).toBe(16);
  });

  it('keeps 4 bytes, the smallest row, in a box too narrow even for that', () => {
    expect(hexRowBytes(240, 4)).toBe(4);
  });
});

describe('mapSegmentMinWidth', () => {
  // Responsive review A#10: "8 | lock 53 | type 34 | 2" drew its 2-byte data part as an 8 px
  // sliver round its "2", cut at the box edge.
  // re-pinned (readability F3): the label's own advance, text-meta at 12 px (7.2), where it was
  // the stale 6.3 of a 10.5 px label
  it('keeps a part wide enough for its label inside its 4 px padding', () => {
    expect(mapSegmentMinWidth('2', 8)).toBeCloseTo(7.2 + 8, 6);
    expect(mapSegmentMinWidth('lock 53', 8)).toBeCloseTo(7 * 7.2 + 8, 6);
  });

  it('keeps the 28 px floor of a big cell, whose small parts read by count only', () => {
    expect(mapSegmentMinWidth('8', 28)).toBe(28);
    expect(mapSegmentMinWidth('data 100,312', 28)).toBeCloseTo(12 * 7.2 + 8, 6);
  });

  // Readability F3 (the defect step 1 found): the budget counted 6.3 px a glyph, measured for the
  // 10.5 px label the map had before the type scale, while the label is set at text-meta.
  it('budgets a label at the advance of the step it is set in, text-meta', () => {
    expect(mapSegmentMinWidth('lock 53', 8)).toBe(textPx(7, fontPx('meta')) + 8);
    expect(mapSegmentMinWidth('data 100,312', 28)).toBe(12 * charPx('meta') + 8);
  });

  it('refuses a part with no label', () => {
    expect(() => mapSegmentMinWidth('', 8)).toThrow(/no label/);
  });
});
