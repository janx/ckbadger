/**
 * The window round a cell's glyph: the class of its type script, one of the six `AssetClass`
 * keys. Plain CKB has none (a cell with no type is a bare stone); the Nervos DAO a doubled ring;
 * tokens a square; objects a hexagon; identities an arch; any other script a ring.
 *
 * Ported from the design prototype's `kinds5.js` (`windowOf`) number for number.
 */

import type { AssetClass } from '@/lib/design-tokens';
import { f1, f2 } from '@/lib/seal/brush';

/** The six classes, in the order the design names them. */
export const WINDOW_CLASSES = [
  'ckb',
  'dao',
  'tokens',
  'objects',
  'identities',
  'other',
] as const satisfies readonly AssetClass[];

type Window = (cx: number, cy: number, r: number, paint: string, sw: number) => string;

const line = (d: string, paint: string, sw: number) =>
  `<path d="${d}" fill="none" stroke="${paint}" stroke-width="${f2(sw)}" stroke-linejoin="round"/>`;
const circ = (cx: number, cy: number, r: number, paint: string, sw: number) =>
  `<circle cx="${f1(cx)}" cy="${f1(cy)}" r="${f2(r)}" fill="none" stroke="${paint}" stroke-width="${f2(sw)}"/>`;

const WINDOWS: Readonly<Record<AssetClass, Window>> = {
  ckb: () => '',
  other: (cx, cy, r, paint, sw) => circ(cx, cy, r, paint, sw),
  dao: (cx, cy, r, paint, sw) => circ(cx, cy, r, paint, sw) + circ(cx, cy, r - sw * 2.4, paint, sw),
  tokens: (cx, cy, r, paint, sw) => {
    const h = (r / Math.SQRT2) * 0.99;
    return `<rect x="${f2(cx - h)}" y="${f2(cy - h)}" width="${f2(2 * h)}" height="${f2(2 * h)}" rx="${f2(h * 0.08)}" fill="none" stroke="${paint}" stroke-width="${f2(sw)}"/>`;
  },
  objects: (cx, cy, r, paint, sw) => {
    let d = '';
    for (let i = 0; i < 6; i++) {
      const a = -Math.PI / 2 + (i * Math.PI) / 3;
      d +=
        (i ? 'L' : 'M') + f2(cx + r * 1.04 * Math.cos(a)) + ' ' + f2(cy + r * 1.04 * Math.sin(a));
    }
    return line(d + 'Z', paint, sw);
  },
  identities: (cx, cy, r, paint, sw) => {
    const hw = r * 0.7,
      yb = cy + r * 0.72,
      ys = cy - r * 0.12;
    return line(
      `M${f2(cx - hw)} ${f2(yb)}L${f2(cx - hw)} ${f2(ys)}A${f2(hw)} ${f2(hw)} 0 0 1 ${f2(cx + hw)} ${f2(ys)}L${f2(cx + hw)} ${f2(yb)}Z`,
      paint,
      sw
    );
  },
};

/** Whether `value` is one of the six classes. */
export const isAssetClass = (value: unknown): value is AssetClass =>
  typeof value === 'string' && Object.prototype.hasOwnProperty.call(WINDOWS, value);

/** The window of class `cls` round (cx, cy), radius `r`, a line `sw` wide in `paint`. */
export function windowOf(
  cls: AssetClass,
  cx: number,
  cy: number,
  r: number,
  paint: string,
  sw: number
): string {
  if (!isAssetClass(cls)) throw new Error(`seal: no such class ${JSON.stringify(cls)}`);
  return WINDOWS[cls](cx, cy, r, paint, sw);
}
