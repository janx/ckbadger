import { fontPx, textPx } from '@/lib/type-metrics';

/**
 * What the status line holds at its width (readability plan 2026-11-18, lane F3-f; the rule in
 * docs/RESPONSIVE_LAYOUT.md "Status line"). The line is one row of the window's width at the meta
 * step: the mode block, the network, the tip, from md the release and the key hints, the clock.
 * Every part is measured from what it writes (lib/type-metrics.ts, a glyph 0.6 em) and the insets
 * its classes draw (components/layout/status-line.tsx), and a part is drawn only where the line
 * holds it: the line measures itself and gives its parts up in one order until the rest fit.
 *
 * The order, first to go: the key hints, the last first (`g b blocks`, `/ search`, `? help`: the
 * help lists them all, and the footer's "Keyboard shortcuts" opens it from md); the tip's word, the
 * number staying (its `#` says it is a block; the word stays for a screen reader); the tip (the
 * vitals strip's first figure on every page); the network (said nowhere else on a phone with one
 * network); the release (from md said only here). The mode and the clock never go: they close the
 * line at its two ends. At the meta step's 12 px and the widest real content (a nine-digit tip,
 * `SYNCING`, `· syncing`) the wide form holds everything from 810 px and gives up `g b blocks` at
 * 768; the narrow form holds everything down to 390, gives up the tip's word at 360 while the mode
 * is a long word, and at 320 keeps the tip's number while `LIVE` and gives up the tip while
 * `SYNCING` or `OFFLINE` (__tests__/lib/status-line-fit.test.ts states each width).
 */

/** What each part writes, as the line writes it. */
export interface StatusLineContent {
  /** The mode block's word: `LIVE`, `SYNCING`, `OFFLINE`, or `—` before the vitals. */
  mode: string;
  /** The network's name. */
  network: string;
  /** The tip as written (`#21,045,159`), or `—`. */
  tip: string;
  /** The release with its sync state (`ckbadger 0.8.2 · synced`). */
  release: string;
  /** The key hints in the help's order (lib/shortcuts `STATUS_KEY_HINTS`). */
  hints: readonly { keys: string; short: string }[];
}

/** The line's two forms: from md, `wide`; below it, `narrow`: no release, no keys (a touch screen). */
export type StatusLineForm = 'wide' | 'narrow';

/** The parts the line draws besides the mode and the clock. */
export interface StatusLineParts {
  network: boolean;
  /** `tip #…` (`labelled`), the number alone (`number`), or no tip. */
  tip: 'labelled' | 'number' | null;
  release: boolean;
  /** How many key hints, from the first. */
  hints: number;
}

/** The word before the tip's number. */
export const TIP_WORD = 'tip';

/** The clock's form, two digits each of hours, minutes and seconds (lib/clock.ts `clockText`). */
export const CLOCK_FORM = '00:00:00';

/** The line's text: the meta step (`text-meta`). */
const LINE_PX = fontPx('meta');
/** The mode block's inset, `px-2.5` each side. */
const MODE_INSET_PX = 2 * 10;
/** A segment's inset, `px-3` each side, and its 1 px line. */
const SEGMENT_INSET_PX = 2 * 12 + 1;
/** The gap between a segment's runs, `gap-1.5`. */
const SEGMENT_GAP_PX = 6;

const chars = (runs: readonly string[]) => runs.reduce((n, run) => n + [...run].length, 0);

/** A segment around its runs of text: their glyphs, the gaps between them and its inset. */
function segmentPx(runs: readonly string[]): number {
  return textPx(chars(runs), LINE_PX) + (runs.length - 1) * SEGMENT_GAP_PX + SEGMENT_INSET_PX;
}

/** The width the line's parts take, to the hundredth of a pixel. */
export function statusLinePx(content: StatusLineContent, parts: StatusLineParts): number {
  const widths = [
    textPx(chars([content.mode]), LINE_PX) + MODE_INSET_PX,
    parts.network ? segmentPx([content.network]) : 0,
    parts.tip === 'labelled' ? segmentPx([TIP_WORD, content.tip]) : 0,
    parts.tip === 'number' ? segmentPx([content.tip]) : 0,
    parts.release ? segmentPx([content.release]) : 0,
    ...content.hints.slice(0, parts.hints).map((h) => segmentPx([h.keys, h.short])),
    segmentPx([CLOCK_FORM]),
  ];
  return Math.round(widths.reduce((a, b) => a + b, 0) * 100) / 100;
}

/**
 * The parts a line `width` px wide draws in its form: all of the form's while it is not measured
 * yet (the render before the layout effect), else the form's parts less those given up, in the
 * order above, until the rest fit. A line too narrow for its mode and its clock keeps those two.
 */
export function fitStatusLine(
  form: StatusLineForm,
  content: StatusLineContent,
  width: number | null
): StatusLineParts {
  const wide = form === 'wide';
  let parts: StatusLineParts = {
    network: true,
    tip: 'labelled',
    release: wide,
    hints: wide ? content.hints.length : 0,
  };
  if (width === null) return parts;
  const giveUp: ((p: StatusLineParts) => StatusLineParts)[] = [
    ...Array.from({ length: parts.hints }, () => (p: StatusLineParts) => ({
      ...p,
      hints: p.hints - 1,
    })),
    (p) => ({ ...p, tip: 'number' }),
    (p) => ({ ...p, tip: null }),
    (p) => ({ ...p, network: false }),
    (p) => ({ ...p, release: false }),
  ];
  for (const step of giveUp) {
    if (statusLinePx(content, parts) <= width) return parts;
    parts = step(parts);
  }
  return parts;
}
