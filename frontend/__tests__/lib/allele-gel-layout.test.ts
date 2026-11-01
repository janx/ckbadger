import { describe, expect, it } from 'vitest';
import { ALLELE_GEL, alleleGelLayout } from '@/lib/allele-gel-layout';

const names = (n: number, name = (i: number) => `g${i}`) =>
  Array.from({ length: n }, (_, i) => name(i));
const NERVAPE = ['prev.type', 'relief', 'prev.bg', 'Color', 'Serial', 'Note'];

describe('alleleGelLayout (R3: lane floor 12 px, wrap, ranked list above 32 lanes below md)', () => {
  it('wraps six lanes to two rows of three at 358 px, in order, so each lane can label its bands', () => {
    const layout = alleleGelLayout({ genes: NERVAPE, width: 358, subject: 'Nervape 3x' });
    if (layout.form !== 'gel') throw new Error('expected a gel');
    expect(layout.rows).toEqual([
      [0, 1, 2],
      [3, 4, 5],
    ]);
    expect(layout.laneWidth).toBeGreaterThanOrEqual(ALLELE_GEL.labelledLane);
    // gene names fit across a lane this wide, so they are written horizontally
    expect(layout.captions).toBe('across');
    // every lane sits inside the measured width
    for (const row of layout.rows)
      row.forEach((_, k) =>
        expect(layout.gutter + (k + 1) * layout.laneWidth).toBeLessThanOrEqual(
          358 - ALLELE_GEL.right
        )
      );
  });

  it('keeps six lanes in one row where they have room (the 640 viewport and up)', () => {
    for (const width of [556, 608, 896, 1248]) {
      const layout = alleleGelLayout({ genes: NERVAPE, width, subject: 'Nervape 3x' });
      if (layout.form !== 'gel') throw new Error('expected a gel');
      expect(layout.rows).toEqual([[0, 1, 2, 3, 4, 5]]);
    }
  });

  it('turns more than 32 genes into the ranked list below md, and a gel from md up', () => {
    expect(alleleGelLayout({ genes: names(72), width: 358, subject: 'deeds' }).form).toBe('list');
    expect(alleleGelLayout({ genes: names(72), width: 735, subject: 'deeds' }).form).toBe('list');
    const md = alleleGelLayout({ genes: names(72), width: 736, subject: 'deeds' });
    if (md.form !== 'gel') throw new Error('expected a gel at md');
    // 72 lanes at the 12 px floor do not fit one 736 px row: two balanced rows of 36
    expect(md.rows.map((row) => row.length)).toEqual([36, 36]);
    expect(md.rows.flat()).toEqual(names(72).map((_, i) => i));
    expect(md.laneWidth).toBeGreaterThanOrEqual(ALLELE_GEL.laneFloor);
    expect(md.captions).toBe('rotated');
    const wide = alleleGelLayout({ genes: names(72), width: 1248, subject: 'deeds' });
    if (wide.form !== 'gel') throw new Error('expected a gel');
    expect(wide.rows).toHaveLength(1);
    expect(wide.laneWidth).toBeGreaterThanOrEqual(ALLELE_GEL.laneFloor);
  });

  it('keeps up to 32 genes a gel on a phone, wrapping at the 12 px floor', () => {
    const layout = alleleGelLayout({ genes: names(32), width: 358, subject: 'x' });
    if (layout.form !== 'gel') throw new Error('expected a gel');
    expect(layout.rows.flat()).toHaveLength(32);
    expect(layout.rows.length).toBeGreaterThan(1);
    expect(layout.laneWidth).toBeGreaterThanOrEqual(ALLELE_GEL.laneFloor);
  });

  it('widens the left gutter to hold the longest genotype label instead of cutting it', () => {
    const layout = alleleGelLayout({
      genes: NERVAPE,
      width: 1248,
      subject: 'x',
      overlayLabels: ['this one', '0x8e38215e…7e6f'],
    });
    if (layout.form !== 'gel') throw new Error('expected a gel');
    expect(layout.gutter).toBeGreaterThanOrEqual(15 * ALLELE_GEL.charWidth + 4);
  });

  it('refuses a width under one 12 px lane instead of drawing an empty gel', () => {
    expect(() => alleleGelLayout({ genes: NERVAPE, width: 90, subject: 'Nervape 3x' })).toThrow(
      /Nervape 3x: a 90 px allele gel cannot hold a 12 px lane/
    );
    expect(() => alleleGelLayout({ genes: [], width: 358, subject: 'empty' })).toThrow(
      /empty: allele gel has no genes/
    );
  });
});
