import { advancePx, fontPx, withAllowance } from '@/lib/type-metrics';

/**
 * The sizes of SVG text by role (readability plan 2026-11-18, lane F3): every `fontSize` on an
 * SVG text element reads this table (the gate is `__tests__/lib/chart-type-gate.test.ts`), and
 * the baselines and widths that follow from a size are computed from it here or beside the figure.
 *
 * Ruling R3 (hierarchy plan 2026-11-02) brought HTML onto the type scale and left a chart's own
 * text where it was, so most of these sizes sit between the scale's steps. Each role is written as
 * the step it was set under and the pixels it stands below it, so the whole table moves with the
 * scale (lib/type-scale.ts); ruling R4 (readability 2026-11-18) then sets no SVG text under 12 px,
 * which is a change of this table, not of the figures.
 */
export const SVG_TEXT = {
  /** 10 px: a chart's axis ticks and rulers, and a figure's smallest marks (a tile's hash and time, a gel's lane digits, a band's label). */
  tick: fontPx('meta') - 1,
  /** 10.5 px: a second line under a name (a date, an epoch, a sub-label) and a figure's notes. */
  note: fontPx('meta') - 0.5,
  /** 11 px, the meta step: a plot's name and key, a figure's labels, a time axis set at the label size. */
  label: fontPx('meta'),
  /** 11.5 px: a figure's figures set under or beside its labels (a count, a byte range, a box's value). */
  value: fontPx('small') - 0.5,
  /** 12 px, the small step: the line that leads an item in a figure (a tile's number, a gene's name, a lane's head, a key's words). */
  head: fontPx('small'),
  /** 12.5 px: a figure's titles over a rule (an edition's name). */
  title: fontPx('body') - 0.5,
  /** 13 px, the body step: a value read out over a plot (an end label, the hover reading, words in a tall box). */
  reading: fontPx('body'),
} as const;

/**
 * The shift from a line's y to the baseline that centres a run of digits on it: 0.3 em (3 px for
 * a 10 px tick on its gridline).
 */
export function centreShift(px: number): number {
  return (px * 3) / 10;
}

/** The pitch of stacked rows of labels at `px`: 1.2 em (12 px for 10 px ticks). */
export function lineStep(px: number): number {
  return (px * 6) / 5;
}

/**
 * A fifth of a pixel a glyph over the face's 0.6 em, so that two labels judged apart by their
 * estimated widths stand apart on screen too (an axis's x labels, the network map's places).
 */
const LABEL_APART_PX = 0.2;

/** The advance a label kept clear of its neighbours is measured at: 6.2 px a glyph at 10 px. */
export function labelAdvancePx(px: number): number {
  return withAllowance(advancePx(px), LABEL_APART_PX);
}
