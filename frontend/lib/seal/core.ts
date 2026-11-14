/**
 * The round core of a transaction, which its cells are cut from: a rim (one of five mouths of Song
 * dishes and mirrors), a written border (one of eight, 20 or 24 repeats, turned one way or the
 * other) and the glyph. A transaction's seal and every one of its cells' seals read the core from
 * the transaction's hash in one order:
 *
 *   glyph (8 bits, mod 40, then its variant bits) · border (3 bits) · its count (1 bit: 24 or 20)
 *   · its turn (1 bit) · rim (8 bits, mod 5)
 *
 * And the owner of a cell, read from its lock's hash: the lock's glyph, then 8 bits mod 11, one of
 * the eleven hours of a ring (the sixth, at the foot, is where a cell writes its index).
 *
 * Ported from the design prototype's `kinds5.js` (`RIMS`, `rimR`, `rimPath`, `RINGS`,
 * `ringStrokes`, `coreOf`, `ownerOf`, `cutGaps`) number for number.
 */

import { BitReader, valueOf } from '@/lib/seal/bits';
import type { Pt } from '@/lib/seal/brush';
import { glyphOf, TICK, type ChosenGlyph } from '@/lib/seal/glyphs';
import type { FigureStroke } from '@/lib/seal/textures';

export const TAU = Math.PI * 2;

/* ---------- the rim ---------- */

/** Plain, eight-sided, the mallow mouth (six nicks), the caltrop flower, the flower mouth. */
export const RIMS = ['round', 'oct', 'mallow6', 'caltrop8', 'scallop12'] as const;
export type RimName = (typeof RIMS)[number];

export const RIM_LABEL: Readonly<Record<RimName, readonly [string, string]>> = {
  round: ['plain', '圆口'],
  oct: ['eight-sided', '八方'],
  mallow6: ['mallow mouth', '葵口'],
  caltrop8: ['caltrop flower', '菱花'],
  scallop12: ['flower mouth', '花口'],
};

/** The least a rim's radius comes to, over its greatest. */
export const RIM_MIN = 0.915;

/** A rim's radius at an angle `th` from the top, over its greatest radius (RIM_MIN–1). */
export function rimR(shape: RimName, th: number): number {
  switch (shape) {
    case 'round':
      return 1;
    case 'oct': {
      const a = Math.PI / 4,
        m = (((th % a) + a) % a) - a / 2;
      return Math.cos(a / 2) / Math.cos(m);
    }
    case 'mallow6': {
      const a = TAU / 6,
        d = Math.abs((((th % a) + a) % a) - a / 2);
      return 1 - 0.085 * Math.exp(-Math.pow(d / 0.075, 2));
    }
    case 'caltrop8':
      return 0.915 + 0.085 * Math.pow(Math.abs(Math.cos(4 * th)), 1.3);
    case 'scallop12':
      return 0.95 + 0.05 * Math.abs(Math.sin(6 * th));
  }
}

/** The rim's outline round (cx, cy), greatest radius R: 180 points clockwise from the top. */
export function rimPoints(shape: RimName, cx: number, cy: number, R: number): Pt[] {
  const N = 180,
    pts: Pt[] = [];
  for (let i = 0; i < N; i++) {
    const th = (i * TAU) / N,
      r = R * rimR(shape, th);
    pts.push([cx + r * Math.sin(th), cy - r * Math.cos(th)]);
  }
  return pts;
}

/* ---------- the written border ---------- */

export const RINGS = ['zig', 'wave', 'ticks', 'stitch', 'scales', 'key', 'rope', 'comb'] as const;
export type RingName = (typeof RINGS)[number];

/**
 * The border `name` written round (cx, cy) on the circle of radius Rm, `a` either side of it,
 * `k` repeats; `flip` turns the key border's hooks the other way.
 */
export function ringStrokes(
  name: RingName,
  Rm: number,
  a: number,
  k: number,
  cx: number,
  cy: number,
  flip: number
): FigureStroke[] {
  const P = (r: number, th: number): Pt => [cx + r * Math.cos(th), cy + r * Math.sin(th)];
  const out: FigureStroke[] = [];
  const step = TAU / k;
  switch (name) {
    case 'zig': {
      const pts: Pt[] = [];
      for (let i = 0; i <= 2 * k + 1; i++) pts.push(P(Rm + (i % 2 ? a : -a), (i * step) / 2));
      out.push({
        pts,
        brush: {
          curve: 'corner',
          r: 0.05,
          over: 0,
          entry: 'point',
          entryLen: 0.3,
          exit: 'taper',
          tail: 0.4,
          pulse: 0,
          bulge: 0.2,
          bulgeSigma: 0.18,
          hmul: 1,
        },
      });
      break;
    }
    case 'wave':
    case 'rope':
      for (const s of name === 'rope' ? [1, -1] : [1]) {
        const pts: Pt[] = [];
        for (let i = 0; i <= 6 * k + 2; i++) {
          const th = (i * step) / 6;
          pts.push(P(Rm + s * a * Math.sin(k * th * 0.5), th));
        }
        out.push({
          pts,
          brush: {
            curve: 'spline',
            tension: 6,
            entry: 'point',
            entryLen: 0.3,
            exit: 'taper',
            tail: 0.5,
            pulse: 0,
            bulge: 0,
            hmul: 1,
          },
        });
      }
      break;
    case 'ticks':
    case 'comb':
      for (let i = 0; i < k; i++) {
        const th = i * step,
          short = name === 'comb' && i % 2 === 1;
        out.push({ pts: [P(Rm - a, th), P(Rm + (short ? 0 : a), th)], brush: TICK });
      }
      break;
    case 'stitch':
      for (let i = 0; i < k; i++) {
        const th = i * step,
          r = Rm + (i % 2 ? a * 0.6 : -a * 0.6);
        out.push({
          pts: [P(r, th - step * 0.32), P(r, th), P(r, th + step * 0.32)],
          brush: {
            curve: 'spline',
            tension: 6,
            entry: 'point',
            entryLen: 0.3,
            exit: 'taper',
            tail: 0.4,
            pulse: 0,
            bulge: 0,
            hmul: 1,
          },
        });
      }
      break;
    case 'scales':
      for (let i = 0; i < k; i++) {
        const th = i * step,
          pts: Pt[] = [];
        for (let j = 0; j <= 6; j++)
          pts.push(P(Rm - a + 2 * a * Math.sin((Math.PI * j) / 6), th + (j / 6) * step * 0.94));
        out.push({
          pts,
          brush: {
            curve: 'spline',
            tension: 6,
            entry: 'point',
            entryLen: 0.25,
            exit: 'taper',
            tail: 0.35,
            pulse: 0,
            bulge: 0,
            hmul: 1,
          },
        });
      }
      break;
    case 'key':
      for (let i = 0; i < k; i++) {
        const th = i * step,
          d = flip ? -1 : 1;
        out.push({
          pts: [
            P(Rm - a, th),
            P(Rm + a, th),
            P(Rm + a, th + d * step * 0.55),
            P(Rm - a * 0.1, th + d * step * 0.55),
          ],
          brush: {
            curve: 'corner',
            r: 0.05,
            over: 0,
            entry: 'point',
            entryLen: 0.25,
            exit: 'taper',
            tail: 0.35,
            pulse: 0,
            bulge: 0.2,
            bulgeSigma: 0.18,
            hmul: 1,
          },
        });
      }
      break;
  }
  return out;
}

/* ---------- what the hashes say ---------- */

/** What a transaction's core is made of, read from its hash. */
export interface Core {
  readonly glyph: ChosenGlyph;
  readonly ring: RingName;
  /** How many times the border repeats round the ring. */
  readonly k: 20 | 24;
  /** 1 turns the border's hooks the other way. */
  readonly flip: number;
  readonly rim: RimName;
  /** How many of the hash's bits the core reads. */
  readonly bitsUsed: number;
}

/** The core of the transaction `txHash`, read in one order by its seal and by its cells'. */
export function coreOf(txHash: string): Core {
  const rd = new BitReader(txHash);
  const glyph = glyphOf(rd);
  const ring = RINGS[valueOf(rd.read(3))];
  const k = valueOf(rd.read(1)) ? 24 : 20;
  const flip = valueOf(rd.read(1));
  const rim = RIMS[valueOf(rd.read(8)) % RIMS.length];
  return { glyph, ring, k, flip, rim, bitsUsed: rd.at };
}

/** A cell's owner, read from its lock's hash: the lock's glyph and its hour. */
export interface Owner {
  readonly glyph: ChosenGlyph;
  /** 0–11 but 6: hour 0 at the top, clockwise; the sixth (the foot) is the index's. */
  readonly hour: number;
  readonly bitsUsed: number;
}

export function ownerOf(lockHash: string): Owner {
  const rd = new BitReader(lockHash);
  const glyph = glyphOf(rd);
  const v = valueOf(rd.read(8)) % 11;
  return { glyph, hour: v >= 6 ? v + 1 : v, bitsUsed: rd.at };
}

/** The angle of an hour of a ring: hour 0 at the top, clockwise (y down). */
export const hourAngle = (h: number): number => -Math.PI / 2 + (h * TAU) / 12;

/** An angular gap round a centre: its middle and its half width, in radians. */
export type Gap = readonly [mid: number, half: number];

/** The parts of `strokes` outside the angular `gaps` round (cx, cy); a part keeps two points. */
export function cutGaps(
  strokes: readonly FigureStroke[],
  cx: number,
  cy: number,
  gaps: readonly Gap[]
): FigureStroke[] {
  const inGap = ([x, y]: Pt) => {
    const a = Math.atan2(y - cy, x - cx);
    return gaps.some(([mid, half]) => {
      let d = a - mid;
      while (d > Math.PI) d -= TAU;
      while (d < -Math.PI) d += TAU;
      return Math.abs(d) < half;
    });
  };
  const out: FigureStroke[] = [];
  for (const st of strokes) {
    let run: Pt[] = [];
    for (const pt of st.pts) {
      if (inGap(pt)) {
        if (run.length > 1) out.push({ ...st, pts: run });
        run = [];
      } else run.push(pt);
    }
    if (run.length > 1) out.push({ ...st, pts: run });
  }
  return out;
}
