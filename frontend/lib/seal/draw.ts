/**
 * The stone's hand (the prototype's `drawer`, kinds5.js): strokes in a stone's units become brush
 * outlines, and outlines become markup in one of two encodings.
 *
 * - `PROTOTYPE`: the prototype's own markup, a filled `<path>` per outline (every sample, two
 *   decimals cut to one) and a `<circle>` per cap, each with its fill; a page seal is written in
 *   it, so it can be compared with the prototype byte for byte.
 * - `compactEncoding(tol)`: the row seal's (`encode.ts`), drawn many times on a page: an outline
 *   keeps only the points that matter within `tol` units, in tenths, relative; one fill on a
 *   group per run of strokes. A rim's outline is written the same way.
 */

import { f2, shape, stroke, type Outline, type Pt, type StrokeOptions } from '@/lib/seal/brush';
import { compactCap, compactOutline } from '@/lib/seal/encode';
import type { BrushSpec, FigureStroke } from '@/lib/seal/textures';

/** A stone's stroke weight: `w0` is the stroke over the lattice unit, `p = 1 + w0` the pitch. */
export interface Hand {
  readonly w0: number;
  readonly p: number;
}

/** The brush width over the stroke weight (the prototype's `o.w`). */
const WIDTH = 0.84;

/**
 * The outlines of `strokes` drawn with `brush` (each stroke's own brush over it), the points
 * placed by `X` / `Y`, `weight` times the hand's width; lengths in pitches of `pitch` (the hand's
 * own when absent).
 */
export function outlinesOf(
  strokes: readonly FigureStroke[],
  brush: BrushSpec,
  X: (x: number) => number,
  Y: (y: number) => number,
  weight: number,
  hand: Hand,
  pitch?: number
): Outline[] {
  const pp = pitch || hand.p;
  return strokes.map((st) => {
    const P = st.pts.map(([x, y]): Pt => [X(x), Y(y)]);
    const sp: BrushSpec = { ...brush, ...(st.brush ?? {}) };
    let o: StrokeOptions = {
      w: hand.w0 * WIDTH * (weight || 1) * (st.weight || 1),
      r: (sp.r ?? 0.3) * pp,
      over: sp.over ?? 0.3,
      entry: sp.entry || 'point',
      entryLen: (sp.entryLen ?? 0.6) * pp,
      press0: sp.press0 ?? 0.08,
      exit: sp.exit || 'taper',
      tail: (sp.tail ?? 1.2) * pp,
      bulge: sp.even ? 0 : (sp.bulge ?? 0.14),
      bulgeSigma: sp.bulgeSigma,
      hmul: sp.hmul ?? (sp.even ? 1 : 0.86),
      pulse: sp.even ? 0 : (sp.pulse ?? 0.22),
      unit: pp,
      curve: sp.curve || 'corner',
      tension: sp.tension || 4,
      step: pp / 4,
      spm: 5,
      qm: 6,
    };
    if (st.horiz === true)
      o = { ...o, entry: 'point', entryLen: 0.4 * pp, exit: 'blunt', pulse: 0, curve: 'corner' };
    if (st.horiz === false)
      o = {
        ...o,
        entry: 'round',
        press0: 0.2,
        exit: 'taper',
        tail: 0.7 * pp,
        pulse: 0,
        curve: 'corner',
      };
    return stroke(P, o);
  });
}

export interface Encoding {
  readonly name: 'prototype' | 'compact';
  /** A run of outlines in one paint (a validated colour). */
  ink(outlines: readonly Outline[], paint: string): string;
  /** A closed line through `points` as path data. */
  ring(points: readonly Pt[]): string;
}

/** The prototype's markup: every sample, two decimals cut to one. */
export const PROTOTYPE: Encoding = {
  name: 'prototype',
  ink: (outlines, paint) =>
    outlines
      .map((r) => shape(r, paint))
      .join('')
      .replace(/(\d+\.\d)\d+/g, '$1'),
  ring: (points) => points.map(([x, y], i) => (i ? 'L' : 'M') + f2(x) + ' ' + f2(y)).join('') + 'Z',
};

/** The compact markup of the row seal: outlines within `tol` units, one fill per run. */
export function compactEncoding(tol: number): Encoding {
  if (!(Number.isFinite(tol) && tol > 0)) throw new Error(`seal: compact tolerance ${tol}`);
  return {
    name: 'compact',
    ink: (outlines, paint) =>
      outlines.length
        ? `<g fill="${paint}">${outlines
            .map(
              (r) =>
                `<path d="${compactOutline(r.polygon, tol)}"/>` + r.caps.map(compactCap).join('')
            )
            .join('')}</g>`
        : '',
    ring: (points) => compactOutline(points, tol),
  };
}
