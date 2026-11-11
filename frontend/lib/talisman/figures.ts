/**
 * The talisman's lexicon: the figures a compartment can hold (the prototype's `REGION`). A figure
 * is made for a box of w × h lattice units: it says how many bits it reads for that box and turns
 * them into strokes (polylines in the box's units) with its own brush habits.
 *
 * cloud script, thunder (zigzag), stacked bars, cloud scrolls (curls), water, running stitch
 * (dashes), the rosette, the meander; and the talisman's own: the ghost head, the sun and moon
 * (eyes), the well, hooks, the chain of loops, the lock. Ported from the design prototype's
 * `brush.js` with seal2's `quadMirror` (the rosette). The eight textures the seals keep (cloud,
 * zigzag, water, curls, loops, meander, dashes, hooks) live in `lib/seal/textures.ts` and are
 * taken from there; the six here are the talisman's alone. The prototype's `key` and `spiral`
 * figures are not in any recipe's lexicon and are not ported.
 */

import { chainSegs, type Pt, type Seg } from '@/lib/seal/brush';
import {
  TEXTURE,
  type Bits,
  type BrushSpec,
  type Figure,
  type FigureStroke,
} from '@/lib/seal/textures';

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
  cloud: TEXTURE.cloud,
  zigzag: TEXTURE.zigzag,
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
  curls: TEXTURE.curls,
  water: TEXTURE.water,
  dashes: TEXTURE.dashes,
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
  meander: TEXTURE.meander,
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
  hooks: TEXTURE.hooks,
  loops: TEXTURE.loops,
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
