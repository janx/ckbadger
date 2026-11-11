/**
 * The written textures: figures that fill a w × h box of lattice units with a pattern drawn by the
 * brush (the prototype's `REGION`, `docs/superpowers/design/2026-11-10-rubbing-song/src/brush.js`).
 * A texture says how many bits it reads for its box and turns them into strokes (polylines in the
 * box's units) with its own brush habits.
 *
 * The eight here are the ones the seals keep from the talisman: cloud script, thunder (zigzag),
 * water, cloud scrolls (curls), the chain of loops, the meander, the running stitch (dashes) and
 * the hooks. Each is the prototype's own, byte for byte (the fidelity tests compare them).
 */

import { chainSegs, type Curve, type Entry, type Exit, type Pt, type Seg } from '@/lib/seal/brush';

export type Bits = readonly number[];

/** A figure's brush habits; what is absent the seal renderer fills from its stone. */
export interface BrushSpec {
  readonly curve?: Curve;
  readonly tension?: number;
  readonly r?: number;
  readonly over?: number;
  readonly entry?: Entry;
  readonly press0?: number;
  readonly exit?: Exit;
  readonly tail?: number;
  readonly bulge?: number;
  readonly bulgeSigma?: number;
  readonly hmul?: number;
  readonly pulse?: number;
  /** An even line: no press at the turns, no pulse, no thinner horizontals; never reordered. */
  readonly even?: boolean;
}

/** A stroke as a figure or a layout writes it, in lattice units. */
export interface FigureStroke {
  readonly pts: Pt[];
  /** true: a flat horizontal (exposed tip, blunt stop); false: a vertical drawn down and lifted. */
  readonly horiz?: boolean;
  readonly brush?: BrushSpec;
  readonly weight?: number;
}

export interface Figure {
  bits(w: number, h: number): number;
  make(b: Bits, w: number, h: number): FigureStroke[];
  readonly brush: BrushSpec;
}

/** The eight written textures, in the prototype's order (kinds5.js `TEXTURES`). */
export const TEXTURES = [
  'cloud',
  'zigzag',
  'water',
  'curls',
  'loops',
  'meander',
  'dashes',
  'hooks',
] as const;

export type TextureName = (typeof TEXTURES)[number];

/**
 * One line through a w × h lattice, the brush never lifted: from the first free point, a 1 turns
 * (right, then left, then straight), a 0 goes straight (then right, then left); when it is boxed
 * in, the next line starts, up to `starts` lines.
 */
export function walkRect(b: Bits, w: number, h: number, starts: number): Seg[] {
  const s: Seg[] = [];
  const used = new Set<string>();
  const key = (x: number, y: number) => x + ',' + y;
  const dirs: Pt[] = [
    [1, 0],
    [0, 1],
    [-1, 0],
    [0, -1],
  ];
  let k = 0,
    n = 0;
  while (k < b.length && n < starts) {
    let sx = -1,
      sy = -1;
    outer: for (let y = 0; y < h; y++)
      for (let x = 0; x < w; x++)
        if (!used.has(key(x, y))) {
          sx = x;
          sy = y;
          break outer;
        }
    if (sx < 0) break;
    n++;
    let x = sx,
      y = sy,
      hd = 0;
    used.add(key(x, y));
    while (k < b.length) {
      const v = b[k++];
      const order = v ? [(hd + 3) % 4, (hd + 1) % 4, hd] : [hd, (hd + 3) % 4, (hd + 1) % 4];
      let moved = false;
      for (const d of order) {
        const nx = x + dirs[d][0],
          ny = y + dirs[d][1];
        if (nx < 0 || ny < 0 || nx >= w || ny >= h || used.has(key(nx, ny))) continue;
        s.push([x, y, nx, ny]);
        used.add(key(nx, ny));
        x = nx;
        y = ny;
        hd = d;
        moved = true;
        break;
      }
      if (!moved) break;
    }
  }
  return s;
}

export const TEXTURE: Readonly<Record<TextureName, Figure>> = {
  /* cloud script: one line walked through the (w+1) × (h+1) lattice, pressed hard at the turns */
  cloud: {
    bits: (w, h) => 2 * (w + 1) * (h + 1),
    make: (b, w, h) => chainSegs(walkRect(b, w + 1, h + 1, 3)),
    brush: { curve: 'spline', tension: 3.2, bulge: 0.85, bulgeSigma: 0.2, pulse: 0.12 },
  },
  /* thunder: a zigzag per row, its phase and height from two bits */
  zigzag: {
    bits: (_w, h) => 2 * h,
    make: (b, w, h) => {
      const out: FigureStroke[] = [];
      for (let r = 0; r < h; r++) {
        const ph = b[2 * r],
          amp = b[2 * r + 1] ? 0.34 : 0.22,
          pts: Pt[] = [];
        for (let c = 0; c <= 2 * w; c++) pts.push([c / 2, r + 0.5 + ((c + ph) % 2 ? amp : -amp)]);
        out.push({ pts });
      }
      return out;
    },
    brush: { curve: 'corner', r: 0.06, over: 0, even: true, hmul: 1, bulge: 0.25, bulgeSigma: 0.2 },
  },
  /* water: a wave per row, its phase and height from two bits */
  water: {
    bits: (_w, h) => 2 * h,
    make: (b, w, h) => {
      const out: FigureStroke[] = [];
      for (let r = 0; r < h; r++) {
        const ph = b[2 * r],
          amp = b[2 * r + 1] ? 0.3 : 0.18,
          pts: Pt[] = [];
        for (let c = 0; c <= 2 * w; c++) {
          const x = c / 2;
          pts.push([x, r + 0.5 + amp * Math.sin(Math.PI * (x + (ph ? 0.5 : 0)))]);
        }
        out.push({ pts });
      }
      return out;
    },
    brush: { curve: 'spline', tension: 6, even: true, hmul: 1 },
  },
  /* cloud scrolls: a curl in every cell, turned one of four ways by two bits */
  curls: {
    bits: (w, h) => 2 * w * h,
    make: (b, w, h) => {
      const out: FigureStroke[] = [];
      let k = 0;
      for (let r = 0; r < h; r++)
        for (let c = 0; c < w; c++) {
          const o = (b[k++] << 1) | b[k++];
          const cx = c + 0.5,
            cy = r + 0.5,
            pts: Pt[] = [];
          const dir = o & 1 ? 1 : -1;
          const a0 = (o >> 1 ? 0 : Math.PI) + (o & 1 ? 0 : Math.PI / 2);
          for (let i = 0; i <= 7; i++) {
            const a = a0 + dir * i * (Math.PI / 4),
              rad = 0.5 - i * 0.052;
            pts.push([cx + rad * Math.cos(a), cy + rad * Math.sin(a)]);
          }
          out.push({ pts });
        }
      return out;
    },
    brush: { curve: 'spline', tension: 4, entry: 'round', press0: 0.1, pulse: 0.1 },
  },
  /* the chain of loops (罡): one line looping along the box */
  loops: {
    bits: () => 2,
    make: (b, w, h) => {
      const n = Math.max(2, Math.round(w / 1.6)),
        cy = h / 2,
        r = Math.min(h / 2 - 0.1, 0.7),
        dir = b[0] ? 1 : -1,
        pts: Pt[] = [];
      const step = w / n;
      for (let i = 0; i < n; i++) {
        const cx = (i + 0.5) * step;
        for (let k = 0; k <= 8; k++) {
          const a = Math.PI + (k / 8) * 2 * Math.PI * dir * (i % 2 ? -1 : 1);
          pts.push([cx + r * 0.8 * Math.cos(a), cy + r * Math.sin(a)]);
        }
      }
      return [{ pts: b[1] ? pts : pts.slice().reverse() }];
    },
    brush: {
      curve: 'spline',
      tension: 5,
      entry: 'point',
      exit: 'taper',
      tail: 1.2,
      pulse: 0.1,
      bulge: 0,
    },
  },
  /* the meander: the rectangular spiral of the thunder pattern, cut where two bits agree */
  meander: {
    bits: (w, h) => (w + 1) * (h + 1),
    make: (b, w, h) => {
      const pts: Pt[] = [];
      let left = 0,
        right = w,
        top = 0,
        bottom = h;
      pts.push([0, 0]);
      while (left <= right && top <= bottom) {
        if (pts.length > 1) pts.push([left, top]);
        for (let c = left + 1; c <= right; c++) pts.push([c, top]);
        for (let r = top + 1; r <= bottom; r++) pts.push([right, r]);
        if (top < bottom) for (let c = right - 1; c >= left; c--) pts.push([c, bottom]);
        if (left < right) for (let r = bottom - 1; r > top; r--) pts.push([left, r]);
        left++;
        right--;
        top++;
        bottom--;
      }
      const m = b.length,
        segs: Seg[] = [];
      for (let i = 0; i + 1 < pts.length; i++) {
        if (b[(i + 1) % m] && b[(i + 6) % m]) continue;
        const [x1, y1] = pts[i],
          [x2, y2] = pts[i + 1];
        if (x1 === x2 && y1 === y2) continue;
        segs.push([x1, y1, x2, y2]);
      }
      return chainSegs(segs);
    },
    brush: { curve: 'corner', r: 0.3, over: 0.2 },
  },
  /* the running stitch: every rule and column dashed, its phase from one bit */
  dashes: {
    bits: (w, h) => w + h + 2,
    make: (b, w, h) => {
      const out: FigureStroke[] = [];
      for (let r = 0; r <= h; r++) {
        const ph = b[r];
        for (let c = 0; c < w; c++)
          if ((c + ph) % 2 === 0)
            out.push({
              pts: [
                [c, r],
                [c + 1, r],
              ],
              horiz: true,
            });
      }
      for (let c = 0; c <= w; c++) {
        const ph = b[h + 1 + c];
        for (let r = 0; r < h; r++)
          if ((r + ph) % 2 === 0)
            out.push({
              pts: [
                [c, r],
                [c, r + 1],
              ],
              horiz: false,
            });
      }
      return out;
    },
    brush: { curve: 'corner', r: 0.2, over: 0 },
  },
  /* a row of hooks (勾), each turned left or right by its bit */
  hooks: {
    bits: (w) => w,
    make: (b, w, h) => {
      const out: FigureStroke[] = [];
      for (let i = 0; i < w; i++) {
        const x = i + 0.5,
          f = b[i] ? 1 : -1,
          top = 0.15,
          bot = h - 0.15;
        out.push({
          pts: [
            [x, top],
            [x, bot - 0.45],
            [x + f * 0.42, bot],
            [x + f * 0.5, bot - 0.5],
          ],
          brush: {
            curve: 'corner',
            r: 0.3,
            over: 0.3,
            entry: 'round',
            press0: 0.25,
            exit: 'taper',
            tail: 0.5,
          },
        });
      }
      return out;
    },
    brush: { curve: 'corner' },
  },
};
