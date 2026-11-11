/**
 * The compact encoding of a seal's strokes, for the seals drawn many times on a page (the row
 * seal, 28 px in every list row). The prototype's markup writes every sample of every outline
 * (a point every quarter unit) in absolute coordinates: ~10 KB for a 28 px seal. Here the outline
 * keeps only the points that matter at the size it is drawn (Ramer–Douglas–Peucker at a fraction
 * of a pixel), each point rounded to a tenth of a unit and written relative to the last, the
 * numbers in their shortest form; the strokes share one fill on a group. The geometry is the
 * same outline the prototype's markup writes, to within the tolerance plus 0.05 units.
 */

import type { Cap, Pt } from '@/lib/talisman/brush';

/**
 * Ramer–Douglas–Peucker: the fewest points of `pts` (first and last always kept) such that no
 * dropped point lies farther than `tol` from the segment that replaces it.
 */
export function simplify(pts: readonly Pt[], tol: number): Pt[] {
  if (!(tol >= 0)) throw new Error(`talisman: simplify tolerance ${tol}`);
  if (pts.length < 3) return pts.slice();
  const keep = new Uint8Array(pts.length);
  keep[0] = 1;
  keep[pts.length - 1] = 1;
  const stack: [number, number][] = [[0, pts.length - 1]];
  while (stack.length) {
    const [a, b] = stack.pop() as [number, number];
    const [ax, ay] = pts[a],
      [bx, by] = pts[b];
    const dx = bx - ax,
      dy = by - ay,
      len = Math.hypot(dx, dy);
    let worst = -1,
      at = -1;
    for (let i = a + 1; i < b; i++) {
      const [px, py] = pts[i];
      const d =
        len === 0 ? Math.hypot(px - ax, py - ay) : Math.abs(dy * (px - ax) - dx * (py - ay)) / len;
      if (d > worst) {
        worst = d;
        at = i;
      }
    }
    if (worst > tol) {
      keep[at] = 1;
      stack.push([a, at], [at, b]);
    }
  }
  return pts.filter((_, i) => keep[i] === 1);
}

/** A count of tenths in its shortest SVG form: 14 → "1.4", 4 → ".4", -4 → "-.4", 30 → "3". */
export function tenths(n: number): string {
  if (!Number.isInteger(n)) throw new Error(`talisman: ${n} is not a whole number of tenths`);
  const sign = n < 0 ? '-' : '';
  const a = Math.abs(n);
  const whole = Math.floor(a / 10),
    frac = a % 10;
  if (frac === 0) return sign + whole;
  return sign + (whole === 0 ? '' : whole) + '.' + frac;
}

/** Numbers joined with only the separators SVG's number grammar needs. */
export function joinNumbers(nums: readonly string[]): string {
  let out = '';
  nums.forEach((s, i) => {
    if (i > 0) {
      const prev = nums[i - 1];
      const needs = s[0] !== '-' && !(s[0] === '.' && prev.includes('.'));
      if (needs) out += ' ';
    }
    out += s;
  });
  return out;
}

const toTenths = (v: number): number => Math.round(v * 10);

/** A closed outline as compact path data: an absolute move, then relative lines, closed. */
export function compactOutline(polygon: readonly Pt[], tol: number): string {
  const pts = simplify(polygon, tol).map(([x, y]) => [toTenths(x), toTenths(y)] as const);
  const [x0, y0] = pts[0];
  const deltas: string[] = [];
  let [lx, ly] = pts[0];
  for (let i = 1; i < pts.length; i++) {
    const [x, y] = pts[i];
    if (x === lx && y === ly) continue;
    deltas.push(tenths(x - lx), tenths(y - ly));
    lx = x;
    ly = y;
  }
  const move = 'M' + joinNumbers([tenths(x0), tenths(y0)]);
  return deltas.length ? `${move}l${joinNumbers(deltas)}z` : `${move}z`;
}

/** A cap (a round blot of ink) in the compact form. */
export function compactCap(c: Cap): string {
  return `<circle cx="${tenths(toTenths(c[0]))}" cy="${tenths(toTenths(c[1]))}" r="${tenths(toTenths(c[2]))}"/>`;
}
