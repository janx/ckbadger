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
 * `public/fonts/jetbrains-mono-*.woff2` files the site serves. One model of the advance: in the
 * Chrome the owner reads with (DPR 1 and 2), a glyph advances exactly 0.6 em at every size from 10
 * to 17 px, in HTML text and in SVG text, at 400 and 600 (the readability review's
 * `lanes/checks/advance.cjs`): no snapping to whole pixels, so no budget counts a glyph wider.
 */

/** JetBrains Mono's advance and descender in font units, and its units per em. */
const ADVANCE_UNITS = 600;
const DESCENT_UNITS = 300;
const UNITS_PER_EM = 1000;

/** JetBrains Mono's advance as a fraction of the size: every glyph 0.6 em wide. */
export const MONO_ADVANCE_EM = ADVANCE_UNITS / UNITS_PER_EM;

/** JetBrains Mono's descender as a fraction of the size: 0.3 em under the baseline (hhea). */
export const MONO_DESCENT_EM = DESCENT_UNITS / UNITS_PER_EM;

/** How far a line of text at `px` reaches under its baseline: its descent, 0.3 em (3.6 at 12 px). */
export function descentPx(px: number): number {
  return (px * DESCENT_UNITS) / UNITS_PER_EM;
}

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
 * The line of a step: its size times its line height (18 at meta, 19.5 at small, 22.4 at body), as
 * one product over a hundred so it comes out as the nearest double (14 × 1.6 is 22.400000000000002).
 */
export function lineHeightPx(step: TypeStep): number {
  return (fontPx(step) * Math.round(parseFloat(TYPE_SCALE[step][1]) * 100)) / 100;
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
 * A sum of widths to the hundredth of a pixel the budgets are written in: in binary floating point
 * 56.5 + 132.6 + 16 can land a hair over 205.1.
 */
function toHundredth(px: number): number {
  return Math.round(px * 100) / 100;
}

/**
 * A list column's minimum width (`ListTable`): what it must hold — its widest text measured here,
 * the marks and gaps beside it — rounded up to a whole pixel, and the whole pixels it keeps spare
 * on top. A table budgeted to its container hands its spare to the columns that need it; each
 * column's comment records what it holds and why it keeps any spare.
 */
export function columnPx(content: number, spare = 0): number {
  return Math.ceil(toHundredth(content)) + spare;
}
