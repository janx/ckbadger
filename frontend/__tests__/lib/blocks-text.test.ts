import { describe, expect, it } from 'vitest';
import {
  BAR_EMPTY,
  BAR_FULL,
  SPARK_GLYPHS,
  blockBar,
  blockBarCells,
  sparkline,
} from '@/lib/blocks-text';

describe('blockBar', () => {
  it('draws whole cells: full ones, then empty ones', () => {
    expect(blockBar(0.6, 10)).toBe('██████░░░░');
    expect(blockBar(0, 10)).toBe('░░░░░░░░░░');
    expect(blockBar(1, 10)).toBe('██████████');
    expect(blockBar(0.5, 4)).toBe('██░░');
    expect([...blockBar(0.37, 10)]).toHaveLength(10);
    expect(BAR_FULL).toBe('█');
    expect(BAR_EMPTY).toBe('░');
  });

  it('fills a cell only once its whole share is reached: full only at 1', () => {
    expect(blockBarCells(0.0999, 10)).toBe(0);
    expect(blockBarCells(0.1, 10)).toBe(1);
    expect(blockBarCells(0.999, 10)).toBe(9);
    // an epoch's last block is not its end
    expect(blockBarCells(1589 / 1590, 10)).toBe(9);
    expect(blockBarCells(246 / 1590, 10)).toBe(1);
    expect(blockBarCells(693 / 1132, 10)).toBe(6);
    // every tenth lands on its own cell, despite binary fractions
    for (let k = 0; k <= 10; k++) expect(blockBarCells(k / 10, 10)).toBe(k);
    for (let k = 0; k <= 5; k++) expect(blockBarCells(k / 5, 5)).toBe(k);
  });

  it('refuses a fraction outside [0, 1] or a width that is not whole cells', () => {
    expect(() => blockBar(-0.01, 10)).toThrow('blockBar: fraction -0.01 is outside [0, 1]');
    expect(() => blockBar(1.01, 10)).toThrow('outside [0, 1]');
    expect(() => blockBar(Number.NaN, 10)).toThrow('outside [0, 1]');
    expect(() => blockBar(0.5, 0)).toThrow('blockBar: width 0 is not a whole number of cells');
    expect(() => blockBar(0.5, 2.5)).toThrow('not a whole number of cells');
  });
});

describe('sparkline', () => {
  it('gives each value an eighth block, from zero to the largest', () => {
    expect(sparkline([0, 1, 2, 3, 4, 5, 6, 7])).toBe('▁▂▃▄▅▆▇█');
    expect(sparkline([7, 0])).toBe('█▁');
    expect(sparkline([533])).toBe('█');
    // from zero, not from the smallest: close values read as close
    expect(sparkline([500, 600, 700])).toBe('▆▇█');
    expect(SPARK_GLYPHS).toBe('▁▂▃▄▅▆▇█');
  });

  it('rounds to the nearest eighth', () => {
    // 7 × 50 / 100 = 3.5 → 4 (▅); 7 × 49 / 100 = 3.43 → 3 (▄)
    expect(sparkline([100, 50, 49])).toBe('█▅▄');
  });

  it('draws a series of zeros on the floor', () => {
    expect(sparkline([0, 0, 0])).toBe('▁▁▁');
  });

  it('refuses no values, a negative or a non-finite one', () => {
    expect(() => sparkline([])).toThrow('sparkline: no values');
    expect(() => sparkline([1, -1])).toThrow('sparkline: -1 is not a count (finite, ≥ 0)');
    expect(() => sparkline([1, Infinity])).toThrow('sparkline: Infinity is not a count');
  });
});
