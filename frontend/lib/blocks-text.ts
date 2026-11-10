/**
 * Counts as block characters (rubbing × Song print): a progress bar of whole cells (`██████░░░░`)
 * and a sparkline of eighth blocks (`▁▂▃▄▅▆▇█`), the terminal's own way to draw a number.
 */

/** A filled cell of a bar. */
export const BAR_FULL = '█';
/** A cell still to fill. */
export const BAR_EMPTY = '░';
/** The eight heights of a sparkline, lowest first. */
export const SPARK_GLYPHS = '▁▂▃▄▅▆▇█';

/**
 * How many of a bar's `width` cells a `fraction` fills: a cell fills only once its whole share is
 * reached, so the bar is full only at 1. The fraction must lie in [0, 1] and the width be a whole
 * number of cells; anything else is an error, not a clamp.
 */
export function blockBarCells(fraction: number, width: number): number {
  if (!Number.isInteger(width) || width < 1)
    throw new Error(`blockBar: width ${width} is not a whole number of cells, at least 1`);
  if (!Number.isFinite(fraction) || fraction < 0 || fraction > 1)
    throw new Error(`blockBar: fraction ${fraction} is outside [0, 1]`);
  return Math.floor(fraction * width);
}

/** The bar as text: the filled cells, then the empty ones (`blockBar(0.6, 10)` → `██████░░░░`). */
export function blockBar(fraction: number, width: number): string {
  const full = blockBarCells(fraction, width);
  return BAR_FULL.repeat(full) + BAR_EMPTY.repeat(width - full);
}

/**
 * One eighth block per value, its height the value's share of the largest, from zero: 0 is `▁`
 * (the floor), the largest `█`, the rest rounded to the nearest eighth in between. The values are
 * counts — finite and not negative — and there is at least one; a series of zeros is all floor.
 */
export function sparkline(values: readonly number[]): string {
  if (values.length === 0) throw new Error('sparkline: no values');
  for (const v of values)
    if (!Number.isFinite(v) || v < 0)
      throw new Error(`sparkline: ${v} is not a count (finite, ≥ 0)`);
  const max = Math.max(...values);
  return values.map((v) => SPARK_GLYPHS[max === 0 ? 0 : Math.round((7 * v) / max)]).join('');
}
