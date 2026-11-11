import type { Pt } from '@/lib/seal/brush';

/** Absolute points of a compact outline `M x y l dx dy … z` (`lib/seal/encode.ts`). */
export function decode(d: string): Pt[] {
  const m = /^M([^l]+)(?:l(.*))?z$/.exec(d);
  if (!m) throw new Error(`not a compact outline: ${d}`);
  const nums = (s: string) => (s.match(/-?(\d+\.?\d*|\.\d+)/g) ?? []).map(Number);
  const [x0, y0] = nums(m[1]);
  const out: Pt[] = [[x0, y0]];
  const rel = nums(m[2] ?? '');
  for (let i = 0; i < rel.length; i += 2) {
    const [lx, ly] = out[out.length - 1];
    out.push([Math.round((lx + rel[i]) * 10) / 10, Math.round((ly + rel[i + 1]) * 10) / 10]);
  }
  return out;
}

/** The distance from a point to the nearest segment of a polyline. */
export const distToPolyline = ([px, py]: Pt, line: Pt[]): number => {
  let best = Infinity;
  for (let i = 0; i + 1 < line.length; i++) {
    const [ax, ay] = line[i],
      [bx, by] = line[i + 1];
    const dx = bx - ax,
      dy = by - ay,
      l2 = dx * dx + dy * dy;
    const t = l2 ? Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / l2)) : 0;
    best = Math.min(best, Math.hypot(px - (ax + t * dx), py - (ay + t * dy)));
  }
  return best;
};
