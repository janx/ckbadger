import { describe, expect, it } from 'vitest';
import { SVG_TEXT, centreShift, labelAdvancePx, lineStep } from '@/lib/chart-type';
import { advancePx, fontPx } from '@/lib/type-metrics';

/**
 * SVG text by role (readability plan 2026-11-18, lane F3, step 1): the sizes the figures used as
 * literals, now read from the type scale, so the table moves with it.
 */
describe('SVG text by role', () => {
  it('gives the sizes the figures were drawn at, smallest first', () => {
    expect(SVG_TEXT).toEqual({
      tick: 10,
      note: 10.5,
      label: 11,
      value: 11.5,
      head: 12,
      title: 12.5,
      reading: 13,
    });
  });

  it('writes every role as a step of the scale and the pixels it stands below it', () => {
    expect(SVG_TEXT.tick).toBe(fontPx('meta') - 1);
    expect(SVG_TEXT.note).toBe(fontPx('meta') - 0.5);
    expect(SVG_TEXT.label).toBe(fontPx('meta'));
    expect(SVG_TEXT.value).toBe(fontPx('small') - 0.5);
    expect(SVG_TEXT.head).toBe(fontPx('small'));
    expect(SVG_TEXT.title).toBe(fontPx('body') - 0.5);
    expect(SVG_TEXT.reading).toBe(fontPx('body'));
  });

  it('keeps the roles in size order, a role apart from the next', () => {
    const sizes = Object.values(SVG_TEXT);
    for (let i = 1; i < sizes.length; i++) expect(sizes[i]).toBeGreaterThan(sizes[i - 1]);
  });
});

describe('what follows from a size', () => {
  it('centres digits on a line 0.3 em under it', () => {
    expect(centreShift(SVG_TEXT.tick)).toBe(3);
    expect(centreShift(12)).toBe(3.6);
  });

  it('stacks rows of labels 1.2 em apart', () => {
    expect(lineStep(SVG_TEXT.tick)).toBe(12);
    expect(lineStep(12)).toBe(14.4);
  });

  it('measures a label kept clear of its neighbours a fifth of a pixel wider a glyph', () => {
    expect(labelAdvancePx(SVG_TEXT.tick)).toBe(6.2);
    expect(labelAdvancePx(12)).toBe(7.4);
    for (const px of Object.values(SVG_TEXT))
      expect(labelAdvancePx(px)).toBeCloseTo(advancePx(px) + 0.2, 10);
  });
});
