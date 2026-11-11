/**
 * Counting rods (算筹): how the seals write a count. A digit is 1–5 standing rods, 6–9 a lying
 * rod over 1–4 standing ones, 0 a ring (the Song zero); a count is its digits one under the
 * other, the greatest on top, all in the standing form. A zone takes so many digits; a greater
 * count is a full block, which is not a number and cannot be mistaken for one.
 *
 * Ported from the design prototype's `kinds5.js` (`rodDigit`, `rodCount`) number for number.
 */

import { ARC, ell } from '@/lib/seal/glyphs';
import type { BrushSpec, FigureStroke } from '@/lib/seal/textures';

/** A standing rod: set down round, lifted. */
export const ROD: BrushSpec = {
  curve: 'corner',
  entry: 'round',
  press0: 0.16,
  exit: 'taper',
  tail: 0.45,
  pulse: 0,
  hmul: 1,
};
/** The lying rod of 6–9: an exposed entry, a blunt stop. */
export const ROD_FLAT: BrushSpec = {
  curve: 'corner',
  entry: 'point',
  entryLen: 0.3,
  press0: 0.1,
  exit: 'blunt',
  pulse: 0,
  hmul: 1,
};

/** A box in a stone's units: x right, y down. */
export interface Box {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** The decimal digits of a whole number ≥ 0, the greatest first. */
export function decimalDigits(n: number): number[] {
  if (!Number.isSafeInteger(n) || n < 0)
    throw new Error(`seal: a count is a whole number ≥ 0, got ${String(n)}`);
  return String(n).split('').map(Number);
}

/** One digit in a box; `pitch` is the distance between two rods. */
export function rodDigit(d: number, box: Box, pitch: number): FigureStroke[] {
  if (!Number.isInteger(d) || d < 0 || d > 9) throw new Error(`seal: ${String(d)} is not a digit`);
  const { x, y, w, h } = box;
  if (d === 0) {
    const r = Math.min(w, h) * 0.26;
    return [{ pts: ell(x + w / 2, y + h / 2, r, r, -100, 266, 12), brush: ARC }];
  }
  const over = d > 5,
    n = over ? d - 5 : d,
    out: FigureStroke[] = [];
  const top = over ? y + h * 0.3 : y,
    x0 = x + w / 2 - ((n - 1) * pitch) / 2;
  for (let i = 0; i < n; i++)
    out.push({
      pts: [
        [x0 + i * pitch, top],
        [x0 + i * pitch, y + h],
      ],
      brush: ROD,
    });
  if (over) {
    const half = Math.max(((n - 1) * pitch) / 2 + pitch * 0.55, pitch * 0.9);
    out.push({
      pts: [
        [x + w / 2 - half, y + h * 0.06],
        [x + w / 2 + half, y + h * 0.06],
      ],
      brush: ROD_FLAT,
    });
  }
  return out;
}

export interface RodZone {
  /** The distance between two rods. */
  readonly pitch: number;
  /** How many digits the zone takes. */
  readonly maxDigits: number;
  /** The height of one digit; digits stand 0.42 of it apart, centred in the zone. */
  readonly digitH: number;
}

export type RodCount =
  | { readonly full: true }
  | { readonly full: false; readonly strokes: FigureStroke[] };

/** A count in a zone: its digits one under the other, the greatest on top; or a full block. */
export function rodCount(n: number, zone: Box, o: RodZone): RodCount {
  const ds = decimalDigits(n);
  if (ds.length > o.maxDigits) return { full: true };
  const gap = o.digitH * 0.42,
    total = ds.length * o.digitH + (ds.length - 1) * gap;
  let yy = zone.y + (zone.h - total) / 2;
  const strokes: FigureStroke[] = [];
  for (const d of ds) {
    strokes.push(...rodDigit(d, { x: zone.x, y: yy, w: zone.w, h: o.digitH }, o.pitch));
    yy += o.digitH + gap;
  }
  return { full: false, strokes };
}
