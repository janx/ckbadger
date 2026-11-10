/**
 * The talisman's lexicon: the figures a compartment can hold (the prototype's `REGION`). A figure
 * is made for a box of w × h lattice units: it says how many bits it reads for that box and turns
 * them into strokes (polylines in the box's units) with its own brush habits.
 *
 * cloud script, thunder (zigzag), stacked bars, cloud scrolls (curls), water, running stitch
 * (dashes), the rosette, the meander; and the talisman's own: the ghost head, the sun and moon
 * (eyes), the well, hooks, the chain of loops, the lock. Ported from the design prototype's
 * `brush.js` with seal2's `quadMirror` (the rosette) and brush.js's `walkRect` (the cloud). The
 * prototype's `key` and `spiral` figures are not in any recipe's lexicon and are not ported.
 */

import {
  chainSegs,
  type Curve,
  type Entry,
  type Exit,
  type Pt,
  type Seg,
} from '@/lib/talisman/brush';

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

export const LEXICON = [
  'cloud',
  'zigzag',
  'bars',
  'curls',
  'water',
  'dashes',
  'rosette',
  'meander',
  'ghost',
  'eyes',
  'well',
  'hooks',
  'loops',
  'lock',
] as const;

export type FigureName = (typeof LEXICON)[number];

/** The figures the core (符胆) may hold. */
export const CORE_LEXICON = [
  'ghost',
  'loops',
  'lock',
  'eyes',
] as const satisfies readonly FigureName[];

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

/** Four-fold symmetry on an n × n lattice (n odd): the top-left quadrant decides (seal2). */
export function quadMirror(b: Bits, n: number): Seg[] {
  const h = (n - 1) / 2,
    m = n - 1,
    s: Seg[] = [];
  let k = 0;
  for (let r = 1; r <= h; r++)
    for (let c = 1; c <= h; c++) {
      const v = b[k++];
      if (v) {
        s.push([c, r, c, r - 1]);
        s.push([m - c, r, m - c, r - 1]);
        s.push([c, m - r, c, m - r + 1]);
        s.push([m - c, m - r, m - c, m - r + 1]);
      } else {
        s.push([c, r, c - 1, r]);
        s.push([m - c, r, m - c + 1, r]);
        s.push([c, m - r, c - 1, m - r]);
        s.push([m - c, m - r, m - c + 1, m - r]);
      }
    }
  return s;
}

/** The odd lattice a rosette fits in a w × h box. */
const rosetteN = (w: number, h: number): number => {
  const m = Math.min(w, h) + 1;
  return m % 2 ? m : m - 1;
};

export const FIGURES: Readonly<Record<FigureName, Figure>> = {
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
  /* stacked bars: every unit of every rule present unless its bit knocks it out */
  bars: {
    bits: (w, h) => w * (h + 1),
    make: (b, w, h) => {
      const segs: Seg[] = [];
      let k = 0;
      for (let r = 0; r <= h; r++)
        for (let c = 0; c < w; c++) if (!b[k++]) segs.push([c, r, c + 1, r]);
      return chainSegs(segs).map((s) => ({ pts: s.pts, horiz: true }));
    },
    brush: { curve: 'corner', r: 0.1, over: 0 },
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
  /* the rosette: four-fold symmetry on the largest odd lattice that fits, centred */
  rosette: {
    bits: (w, h) => {
      const n = rosetteN(w, h);
      return ((n - 1) / 2) ** 2;
    },
    make: (b, w, h) => {
      const n = rosetteN(w, h);
      const ox = (w - (n - 1)) / 2,
        oy = (h - (n - 1)) / 2;
      return chainSegs(quadMirror(b, n)).map((s) => ({
        pts: s.pts.map(([x, y]): Pt => [x + ox, y + oy]),
      }));
    },
    brush: { curve: 'spline', tension: 4 },
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
  /* the ghost head: a spiral with two legs */
  ghost: {
    bits: () => 3,
    make: (b, w, h) => {
      const cx = w / 2,
        R = Math.min(w / 2, h / 2) * 0.62,
        cy = R + 0.1,
        dir = b[0] ? 1 : -1,
        pts: Pt[] = [];
      for (let i = 0; i <= 26; i++) {
        const t = i / 26,
          a = Math.PI / 2 + dir * t * 1.9 * 2 * Math.PI,
          rad = R * (1 - 0.88 * t);
        pts.push([cx + rad * Math.cos(a), cy + rad * Math.sin(a)]);
      }
      const sp = b[1] ? 0.34 : 0.2,
        ly = Math.min(h, cy + R + 1.6);
      const leg: BrushSpec = {
        curve: 'corner',
        entry: 'round',
        press0: 0.2,
        exit: 'taper',
        tail: 0.9,
      };
      return [
        { pts },
        {
          pts: [
            [cx - R * 0.6, cy + R * 0.9],
            [cx - R * 0.6 - sp * w * 0.5, ly],
          ],
          brush: leg,
        },
        {
          pts: [
            [cx + R * 0.6, cy + R * 0.9],
            [cx + R * 0.6 + (b[2] ? sp : sp * 0.5) * w * 0.5, ly],
          ],
          brush: leg,
        },
      ];
    },
    brush: {
      curve: 'spline',
      tension: 6,
      entry: 'round',
      press0: 0.15,
      pulse: 0,
      bulge: 0,
      tail: 1.4,
    },
  },
  /* the sun and moon: two rings, each with its pupil */
  eyes: {
    bits: () => 3,
    make: (b, w, h) => {
      const r = Math.min(w / 4, h / 2) * (b[0] ? 0.78 : 0.62),
        cy = h / 2,
        out: FigureStroke[] = [];
      for (const cx of [w / 4, (3 * w) / 4]) {
        const pts: Pt[] = [];
        for (let i = 0; i <= 12; i++) {
          const a = -Math.PI / 2 + (i / 12) * 2.05 * Math.PI;
          pts.push([cx + r * Math.cos(a), cy + r * Math.sin(a)]);
        }
        out.push({ pts });
        const dx = (b[1] ? 0.22 : -0.22) * r,
          dy = (b[2] ? 0.2 : -0.1) * r;
        out.push({
          pts: [
            [cx + dx - 0.12, cy + dy],
            [cx + dx + 0.12, cy + dy + 0.06],
          ],
          brush: { curve: 'corner', entry: 'round', press0: 0.3, exit: 'blunt' },
        });
      }
      return out;
    },
    brush: {
      curve: 'spline',
      tension: 6,
      entry: 'point',
      exit: 'taper',
      tail: 0.9,
      pulse: 0,
      bulge: 0,
    },
  },
  /* the well (井): two rules and two posts, each a little short where its bit says */
  well: {
    bits: () => 4,
    make: (b, w, h) => {
      const xs = [w / 3, (2 * w) / 3],
        ys = [h / 3, (2 * h) / 3];
      const e = 0.25;
      return [
        {
          pts: [
            [-e + (b[0] ? 0.3 : 0), ys[0]],
            [w + e, ys[0]],
          ],
          horiz: true,
        },
        {
          pts: [
            [-e, ys[1]],
            [w + e - (b[1] ? 0.3 : 0), ys[1]],
          ],
          horiz: true,
        },
        {
          pts: [
            [xs[0], -e + (b[2] ? 0.3 : 0)],
            [xs[0], h + e],
          ],
          horiz: false,
        },
        {
          pts: [
            [xs[1], -e],
            [xs[1], h + e - (b[3] ? 0.3 : 0)],
          ],
          horiz: false,
        },
      ];
    },
    brush: { curve: 'corner', r: 0.1 },
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
  /* the lock: a box with its bar (across or down) and, by a bit, the bolt */
  lock: {
    bits: () => 2,
    make: (b, w, h) => {
      const m = 0.25;
      const out: FigureStroke[] = [
        {
          pts: [
            [m, m],
            [w - m, m],
            [w - m, h - m],
            [m, h - m],
            [m, m + 0.2],
          ],
          brush: {
            curve: 'corner',
            r: 0.15,
            over: 0,
            entry: 'round',
            press0: 0.1,
            exit: 'blunt',
            pulse: 0,
          },
        },
      ];
      if (b[0])
        out.push({
          pts: [
            [-0.2, h / 2],
            [w + 0.2, h / 2],
          ],
          horiz: true,
        });
      else
        out.push({
          pts: [
            [w / 2, -0.2],
            [w / 2, h + 0.2],
          ],
          horiz: false,
        });
      if (b[1])
        out.push({
          pts: [
            [w / 2 - 0.3, h / 2 - 0.3],
            [w / 2 + 0.3, h / 2 + 0.3],
          ],
          brush: { curve: 'corner', entry: 'round', press0: 0.3, exit: 'blunt' },
        });
      return out;
    },
    brush: { curve: 'corner' },
  },
};
