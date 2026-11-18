// Relative, not `@/lib/…`: tailwind.config.ts loads this module outside Vite, where the alias does
// not resolve (lib/type-scale.ts and lib/palette.ts, which it loads too, import nothing).
import { TYPE_SCALE, type TypeStep } from './type-scale';

/**
 * What the type scale measures on screen (readability plan 2026-11-18, lane F3): the advance of a
 * glyph, a run of glyphs and the line of a step, for every layout that budgets its text in pixels —
 * a column's minimum width, a label that must fit its segment, the line a list row stands on. Each
 * is computed from `lib/type-scale.ts`, so a change of the scale moves every budget with it; SVG
 * text takes its sizes from `lib/chart-type.ts`, which reads the scale too.
 *
 * The face is JetBrains Mono at every weight and in italic (app/globals.css): monospaced, every
 * glyph 600 units wide on a 1000-unit em (hhea.advanceWidthMax and OS/2.xAvgCharWidth, with
 * post.isFixedPitch set). `__tests__/lib/type-metrics.test.ts` reads those from the five
 * `public/fonts/jetbrains-mono-*.woff2` files the site serves.
 */

/** JetBrains Mono's advance in font units, and its units per em. */
const ADVANCE_UNITS = 600;
const UNITS_PER_EM = 1000;

/** JetBrains Mono's advance as a fraction of the size: every glyph 0.6 em wide. */
export const MONO_ADVANCE_EM = ADVANCE_UNITS / UNITS_PER_EM;

/**
 * The advance Chromium was observed to draw the figures' small SVG text at (lane D, 2026-10: 7 px
 * a glyph at 11 px, as its snapping to whole pixels gives), as an em: the face's 0.6 and 0.04 more.
 * The figures that keep their labels apart by an estimate (the DAO history's names, the VM lanes'
 * plot name, the fork tree's miner, the allele gel's tags) measure at it.
 */
export const OBSERVED_ADVANCE_EM = MONO_ADVANCE_EM + 0.04;

/** The font size of a step of the type scale, px. */
export function fontPx(step: TypeStep): number {
  return parseFloat(TYPE_SCALE[step][0]);
}

/**
 * The width of `chars` glyphs set at `px`: 0.6 em each, computed as one product over the em so a
 * whole count comes out as the nearest double (13 glyphs at 12 px → 93.6, where 13 × 7.2 adds up
 * to 93.60000000000001).
 */
export function textPx(chars: number, px: number): number {
  return (chars * px * ADVANCE_UNITS) / UNITS_PER_EM;
}

/** The advance of one glyph set at `px` (6.6 at 11 px, 7.2 at 12, 7.8 at 13). */
export function advancePx(px: number): number {
  return textPx(1, px);
}

/** The advance of one glyph at a step of the type scale. */
export function charPx(step: TypeStep): number {
  return advancePx(fontPx(step));
}

/**
 * A glyph's advance where the browser may snap it to whole pixels: Chromium draws JetBrains Mono
 * at 11 and 12 px on a 7 px advance and at 13 px on 8; a browser that does not snap uses 0.6 em.
 * A label that must fit (a segment's words, an icicle box's) is measured at whichever is wider.
 */
export function snappedAdvancePx(px: number): number {
  const advance = advancePx(px);
  return Math.max(Math.round(advance), advance);
}

/** The line of a step: its size times its line height (16.5 at meta, 18 at small, 20.8 at body). */
export function lineHeightPx(step: TypeStep): number {
  return fontPx(step) * parseFloat(TYPE_SCALE[step][1]);
}

/**
 * The one line a list row stands on (table-rows plan T2, T5, T7): the small step's line rounded up
 * to a whole pixel, so a row of `py-[11px]` is a whole number of pixels and its 1 px rule stays on
 * the pixel grid. A row's key (`text-body`), its notes (`text-small`) and a row stone measure from
 * it. Written in classes as `leading-row` (tailwind.config.ts).
 */
export const ROW_LINE_PX = Math.ceil(lineHeightPx('small'));

/** The leading of a line set tight: the least that keeps two rows of labels clear. */
const TIGHT_LEADING_PX = 3;

/** A line of text at `px` set tight: its size and 3 px of leading (14 at 11 px, 16 at 13). */
export function tightLinePx(px: number): number {
  return px + TIGHT_LEADING_PX;
}

/**
 * The line of a label set tight at the meta step, as a version tick over a timeline or a plot's
 * upgrade labels are: rows of them stack this far apart. Written in classes as `leading-label`.
 */
export const LABEL_LINE_PX = tightLinePx(fontPx('meta'));

/**
 * A budget and the allowance a layout keeps on top of it (a measured advance wider than 0.6 em, a
 * margin that keeps labels apart, the spare of a column), kept to the hundredth of a pixel the
 * allowances are written in: in binary floating point 6.6 + 0.1 is 6.699999999999999.
 */
export function withAllowance(px: number, allowance: number): number {
  return Math.round((px + allowance) * 100) / 100;
}

/**
 * A list column's minimum width (`ListTable`): what it must hold — its widest text measured here,
 * the marks and gaps beside it — rounded up to a whole pixel, and the whole pixels it keeps spare
 * on top. A table budgeted to its container hands its spare to the columns that need it; each
 * column's comment records what it holds and why it keeps any spare.
 */
export function columnPx(content: number, spare = 0): number {
  return Math.ceil(withAllowance(content, 0)) + spare;
}
