// @vitest-environment node
import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { bitsOf } from '@/lib/night/bits';
import { assertSealHash, BitReader, valueOf } from '@/lib/talisman/bits';
import {
  chainSegs,
  orderStrokes,
  sample,
  segmentPaths,
  stroke,
  type Pt,
  type StrokeOptions,
} from '@/lib/talisman/brush';
import { compound } from '@/lib/talisman/compound';
import { compactOutline, joinNumbers, simplify, tenths } from '@/lib/talisman/encode';
import {
  CORE_LEXICON,
  FIGURES,
  LEXICON,
  quadMirror,
  walkRect,
  type FigureName,
} from '@/lib/talisman/figures';
import { layTalisman } from '@/lib/talisman/layout';
import { plan } from '@/lib/talisman/plan';
import {
  BRUSH,
  PAGE,
  RECIPES,
  ROW,
  type DrawnRecipe,
  type FoldRecipe,
} from '@/lib/talisman/recipes';

// block #20,914,858's hash, which no transaction has (the night's test hash)
const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';
const PAINT = { ink: 'var(--c-rouge)', ground: 'var(--c-base-bg)' };

/** 50 hashes derived from a fixed seed: sha256("ckbadger hash seal <i>"). */
const HASHES = Array.from(
  { length: 50 },
  (_, i) => '0x' + createHash('sha256').update(`ckbadger hash seal ${i}`).digest('hex')
);

/** A hash whose first bits are `lead`, the rest 0. */
function hashWith(lead: number[]): string {
  const bits = [...lead, ...new Array(256 - lead.length).fill(0)];
  let hex = '';
  for (let i = 0; i < 256; i += 4)
    hex += ((bits[i] << 3) | (bits[i + 1] << 2) | (bits[i + 2] << 1) | bits[i + 3]).toString(16);
  return '0x' + hex;
}

describe('the hash as the seal reads it', () => {
  it('takes only a lowercase 32-byte hash', () => {
    expect(() => assertSealHash(HASH)).not.toThrow();
    expect(() => assertSealHash(HASH.toUpperCase().replace('0X', '0x'))).toThrow(
      'lowercase 32-byte'
    );
    expect(() => assertSealHash(HASH.slice(0, -1))).toThrow('lowercase 32-byte');
    expect(() => assertSealHash(HASH.slice(2))).toThrow('lowercase 32-byte');
    expect(() => assertSealHash(`${HASH}"/><script>`)).toThrow('lowercase 32-byte');
  });

  it('reads the bits most significant first, as the night does, and never past 256', () => {
    const r = new BitReader(HASH);
    // 0xb = 1011, 0xb = 1011
    const first = r.read(8);
    expect(first).toEqual({ at: 0, bits: [1, 0, 1, 1, 1, 0, 1, 1] });
    expect(valueOf(first)).toBe(0xbb);
    expect(r.at).toBe(8);
    expect(r.read(248).bits).toEqual(Array.from(bitsOf(HASH).slice(8)));
    expect(() => r.read(1)).toThrow('reads past bit 256');
    expect(() => new BitReader(HASH).read(-1)).toThrow('cannot read');
  });
});

describe('the brush', () => {
  const LINE: StrokeOptions = {
    curve: 'corner',
    tension: 4,
    spm: 5,
    qm: 6,
    step: 0.4,
    r: 0.48,
    over: 0.3,
    w: 0.5,
    entry: 'point',
    entryLen: 0.64,
    press0: 0.08,
    exit: 'blunt',
    tail: 1.9,
    bulge: 0.14,
    bulgeSigma: undefined,
    hmul: 0.86,
    pulse: 0,
    unit: 1.6,
  };

  it('samples a straight line evenly, with its length and a unit tangent', () => {
    const { pts, cornerS, L } = sample(
      [
        [0, 0],
        [4, 0],
      ],
      LINE
    );
    expect(L).toBeCloseTo(4, 12);
    expect(pts).toHaveLength(11);
    expect(cornerS).toEqual([]);
    for (const p of pts) expect([p.tx, p.ty]).toEqual([1, 0]);
    expect(pts.map((p) => p.x)).toEqual(Array.from({ length: 11 }, (_, i) => (4 * i) / 10));
  });

  it('rounds a corner with a quadratic and presses there', () => {
    const { cornerS } = sample(
      [
        [0, 0],
        [2, 0],
        [2, 2],
      ],
      LINE
    );
    expect(cornerS).toHaveLength(1);
    expect(cornerS[0]).toBeGreaterThan(1.5);
    expect(cornerS[0]).toBeLessThan(2.5);
    expect(() => sample([[0, 0]], LINE)).toThrow('two points');
  });

  it('outlines a stroke left edge out, right edge back, capped where the tip is round', () => {
    const P: Pt[] = [
      [0, 0],
      [4, 0],
    ];
    const blunt = stroke(P, LINE);
    expect(blunt.polygon).toHaveLength(22);
    expect(blunt.caps).toHaveLength(1); // a pointed entry has no cap, a blunt stop has one
    // the outline is symmetric about the centre line of a horizontal stroke
    for (let i = 0; i < 11; i++) {
      const [lx, ly] = blunt.polygon[i],
        [rx, ry] = blunt.polygon[21 - i];
      expect(lx).toBeCloseTo(rx, 12);
      expect(ly).toBeCloseTo(-ry, 12);
    }
    const round = stroke(P, { ...LINE, entry: 'round', exit: 'taper' });
    expect(round.caps).toHaveLength(1);
    expect(round.caps[0][0]).toBe(0);
    // the taper lifts the brush to its 3 % floor at the very end
    expect(Math.abs(round.polygon[10][1])).toBeCloseTo(0.03 * LINE.w * 0.5, 12);
  });

  it('chains unit segments into lines that run down and to the right, straight on where it can', () => {
    // a plus sign: the horizontal and the vertical each come out as one straight line
    const plus = segmentPaths([
      [0, 1, 1, 1],
      [1, 1, 2, 1],
      [1, 0, 1, 1],
      [1, 1, 1, 2],
    ]);
    expect(plus).toEqual([
      [
        [0, 1],
        [1, 1],
        [2, 1],
      ],
      [
        [1, 0],
        [1, 1],
        [1, 2],
      ],
    ]);
    expect(chainSegs([[2, 2, 1, 2]])).toEqual([
      {
        pts: [
          [1, 2],
          [2, 2],
        ],
      },
    ]);
  });

  it('writes the strokes from the top left, the next one down and to the right', () => {
    const s = (x: number, y: number) => ({
      pts: [[x, y] as Pt, [x + 1, y] as Pt],
    });
    const ordered = orderStrokes([s(0, 3), s(2, 0), s(0, 0)]);
    expect(ordered.map((o) => o.pts[0])).toEqual([
      [0, 0],
      [2, 0],
      [0, 3],
    ]);
    expect(orderStrokes([])).toEqual([]);
  });
});

describe('the figures', () => {
  it('walks one line through the lattice (cloud script)', () => {
    expect(walkRect([0, 0, 0], 2, 2, 3)).toEqual([
      [0, 0, 1, 0],
      [1, 0, 1, 1],
      [1, 1, 0, 1],
    ]);
  });

  it('mirrors the top-left quadrant four ways (the rosette)', () => {
    expect(quadMirror([1], 3)).toEqual([
      [1, 1, 1, 0],
      [1, 1, 1, 0],
      [1, 1, 1, 2],
      [1, 1, 1, 2],
    ]);
    expect(quadMirror([0], 3)).toEqual([
      [1, 1, 0, 1],
      [1, 1, 2, 1],
      [1, 1, 0, 1],
      [1, 1, 2, 1],
    ]);
  });

  it('has the fourteen figures of the lexicon and the four of the core', () => {
    expect(LEXICON).toHaveLength(14);
    expect(Object.keys(FIGURES).sort()).toEqual([...LEXICON].sort());
    expect(CORE_LEXICON).toEqual(['ghost', 'loops', 'lock', 'eyes']);
  });

  it('draws every figure in its box from its own bits, the same bits the same strokes', () => {
    for (const name of LEXICON)
      for (const [w, h] of [
        [5, 1],
        [5, 2],
        [5, 3],
      ]) {
        const fig = FIGURES[name];
        const n = fig.bits(w, h);
        expect(Number.isInteger(n) && n >= 0, `${name} ${w}×${h}`).toBe(true);
        for (const fill of [0, 1]) {
          const b = new Array(n).fill(fill);
          const strokes = fig.make(b, w, h);
          expect(fig.make(b, w, h), name).toEqual(strokes);
          for (const s of strokes) {
            expect(s.pts.length, `${name} ${w}×${h}`).toBeGreaterThanOrEqual(2);
            for (const [x, y] of s.pts) {
              expect(x, `${name} x`).toBeGreaterThanOrEqual(-0.6);
              expect(x, `${name} x`).toBeLessThanOrEqual(w + 0.6);
              expect(y, `${name} y`).toBeGreaterThanOrEqual(-0.6);
              expect(y, `${name} y`).toBeLessThanOrEqual(h + 0.6);
            }
          }
        }
      }
  });
});

describe('the plan', () => {
  it('reads the layout bits first, then each compartment: its choice, then its figure', () => {
    for (const recipe of [PAGE, BRUSH])
      for (const hash of HASHES.slice(0, 10)) {
        const p = plan(hash, recipe);
        const nb = recipe.layout.bands;
        expect(p.heights.at).toBe(0);
        expect(p.heights.bits).toHaveLength(nb);
        expect(p.foot.bit.at).toBe(nb);
        let at = nb + 1;
        let above: FigureName | null = null;
        p.compartments.forEach((c, i) => {
          const choose: readonly FigureName[] =
            recipe.layout.core && i === Math.floor(nb / 2) ? CORE_LEXICON : LEXICON;
          expect(c.choice.at).toBe(at);
          expect(c.choice.bits).toHaveLength(choose.length === 14 ? 4 : 2);
          let idx = valueOf(c.choice) % choose.length;
          if (choose[idx] === above) idx = (idx + 1) % choose.length;
          expect(c.figure).toBe(choose[idx]);
          expect(c.figure).not.toBe(above); // a figure never follows itself
          at += c.choice.bits.length;
          expect(c.bits.at).toBe(at);
          expect(c.bits.bits).toHaveLength(FIGURES[c.figure].bits(c.rw, c.rh));
          expect(c.bits.bits).toEqual(Array.from(bitsOf(hash).slice(at, at + c.bits.bits.length)));
          at += c.bits.bits.length;
          above = c.figure;
        });
        expect(p.bitsUsed).toBe(at);
      }
  });

  it('puts the heavier core in the middle of the page seal only', () => {
    for (const hash of HASHES) {
      const page = plan(hash, PAGE);
      expect(page.compartments.map((c) => c.weight)).toEqual([1, 1, 1.35, 1, 1]);
      expect(CORE_LEXICON).toContain(page.compartments[2].figure);
      expect(plan(hash, BRUSH).compartments.map((c) => c.weight)).toEqual([1, 1, 1]);
    }
  });

  it('fills the field: the foot ends where the body ends, the sword reaches the stone foot', () => {
    for (const recipe of [PAGE, BRUSH]) {
      const p = plan(HASH, recipe);
      const last = p.compartments.length - 1;
      const bodyEnd = p.slots[last].y + p.compartments[last].h + 0.45;
      expect(bodyEnd).toBeCloseTo(recipe.layout.H - 3.4 + 0.45, 9);
      const sword = p.foot.strokes[p.foot.strokes.length - 1];
      expect(sword.pts[1][1]).toBeCloseTo(recipe.layout.H - 0.3, 9);
    }
  });

  it('never reads past bit 256, whatever the hash (the worst case of every recipe)', () => {
    // every combination of the height bits, and the hungriest figure in every compartment
    for (const recipe of [PAGE, BRUSH]) {
      const nb = recipe.layout.bands;
      let worst = 0;
      for (let combo = 0; combo < 1 << nb; combo++) {
        const lead = Array.from({ length: nb }, (_, i) => (combo >> (nb - 1 - i)) & 1);
        const laid = layTalisman(recipe.layout, new BitReader(hashWith(lead)));
        let need = nb + 1;
        for (const box of laid.boxes) {
          const choose: readonly FigureName[] = box.core ? CORE_LEXICON : LEXICON;
          need += Math.ceil(Math.log2(choose.length));
          need += Math.max(
            ...choose.map((f) => FIGURES[f].bits(Math.round(box.w), Math.round(box.h)))
          );
        }
        worst = Math.max(worst, need);
      }
      expect(worst, recipe.name).toBeLessThanOrEqual(256);
      // page: 6 layout bits, four cloud scripts of 5 × 2 at 4 + 36, the core 2 + 3; brush: 4, three of 5 × 3 at 4 + 48
      expect(worst).toBe(recipe === PAGE ? 171 : 160);
    }
  });

  it('refuses a layout whose compartments would be too short or whose sword has no room', () => {
    const squeezed: DrawnRecipe = { ...PAGE, layout: { ...PAGE.layout, H: 12 } };
    expect(() => plan(HASH, squeezed)).toThrow('units tall');
    const noKeep: FoldRecipe = { ...ROW, keep: 0 };
    expect(() => plan(HASH, noKeep)).toThrow('keeps 1–5 compartments');
    const narrow: FoldRecipe = { ...ROW, W: 5 };
    expect(() => plan(HASH, narrow)).toThrow('cannot hold');
    const short: FoldRecipe = { ...ROW, H: 8 };
    expect(() => plan(HASH, short)).toThrow('no room for the sword');
  });
});

describe('the fold rule: the row seal is the page seal folded', () => {
  it('takes the page seal’s head, first two compartments (figures, bits, sizes) and foot', () => {
    for (const hash of HASHES) {
      const page = plan(hash, PAGE);
      const row = plan(hash, ROW);
      expect(row.compartments).toEqual(page.compartments.slice(0, 2));
      expect(row.compartments.map((c) => [c.figure, c.bits])).toEqual(
        page.compartments.slice(0, 2).map((c) => [c.figure, c.bits])
      );
      // laid in the 7 × 11 white stone: the same rows, moved half a unit right to centre them
      expect([row.W, row.H]).toEqual([7, 11]);
      expect(row.slots).toEqual(page.slots.slice(0, 2).map(({ x, y }) => ({ x: x + 0.5, y })));
      expect(row.head.name).toBe(page.head.name);
      expect(row.dividers).toHaveLength(1);
      expect(row.dividers[0].pts).toEqual(page.dividers[0].pts.map(([x, y]) => [x + 0.5, y]));
      expect(row.foot.name).toBe(page.foot.name);
      expect(row.foot.bit).toEqual(page.foot.bit);
      expect(row.heights).toEqual(page.heights);
      // the row reads no bit the page does not, and no bit after its second compartment
      expect(row.bitsUsed).toBe(
        page.compartments[1].bits.at + page.compartments[1].bits.bits.length
      );
      expect(row.bitsUsed).toBeLessThanOrEqual(page.bitsUsed);
    }
  });

  it('draws byte-identical SVG for the same hash, every recipe, every time', () => {
    for (const hash of HASHES)
      for (const [recipe, size] of [
        [PAGE, 150],
        [ROW, 28],
        [BRUSH, 150],
      ] as const) {
        const a = compound(hash, recipe, size, PAINT);
        const b = compound(hash, recipe, size, PAINT);
        expect(b.svg).toBe(a.svg);
        expect(b.chosen).toEqual(a.chosen);
      }
  });

  it('gives different hashes different seals', () => {
    const seen = new Set(HASHES.map((h) => compound(h, ROW, 28, PAINT).svg));
    expect(seen.size).toBe(HASHES.length);
  });
});

describe('the compact encoding (the row seal)', () => {
  it('writes tenths in their shortest form and only the separators SVG needs', () => {
    expect([14, 4, -4, 30, 0, -30, -14, 105].map(tenths)).toEqual([
      '1.4',
      '.4',
      '-.4',
      '3',
      '0',
      '-3',
      '-1.4',
      '10.5',
    ]);
    expect(() => tenths(0.5)).toThrow('whole number');
    expect(joinNumbers(['1.4', '.3', '-.2', '5', '.1', '2'])).toBe('1.4.3-.2 5 .1 2');
  });

  it('keeps the ends and the corners and drops what lies within the tolerance', () => {
    const line: Pt[] = Array.from({ length: 11 }, (_, i) => [i, i % 2 ? 0.01 : 0]);
    expect(simplify(line, 0.05)).toEqual([
      [0, 0],
      [10, 0],
    ]);
    const corner: Pt[] = [
      [0, 0],
      [1, 0],
      [2, 0],
      [2, 1],
      [2, 2],
    ];
    expect(simplify(corner, 0.05)).toEqual([
      [0, 0],
      [2, 0],
      [2, 2],
    ]);
    expect(() => simplify(corner, -1)).toThrow('tolerance');
  });

  /** Absolute points of a compact outline `M x y l dx dy … z`. */
  function decode(d: string): Pt[] {
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

  const distToPolyline = ([px, py]: Pt, line: Pt[]): number => {
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

  it('decodes to the outline it was given, within the tolerance and the rounding', () => {
    const ring: Pt[] = Array.from({ length: 60 }, (_, i) => [
      3 + 2 * Math.cos((i / 60) * 2 * Math.PI),
      4 + 1.5 * Math.sin((i / 60) * 2 * Math.PI),
    ]);
    const tol = 0.15;
    const pts = decode(compactOutline(ring, tol));
    expect(pts.length).toBeLessThan(ring.length);
    for (const p of ring) expect(distToPolyline(p, pts)).toBeLessThanOrEqual(tol + 0.0708);
  });

  it('draws the row seal as the prototype encoding of the same plan draws it, at a fifth of the bytes', () => {
    const full: FoldRecipe = { ...ROW, encoding: { kind: 'prototype' } };
    for (const hash of HASHES.slice(0, 10)) {
      const compact = compound(hash, ROW, 28, PAINT).svg;
      const proto = compound(hash, full, 28, PAINT).svg;
      expect(compact.length).toBeLessThan(proto.length / 4);
      const cPaths = [...compact.matchAll(/<path d="([^"]+)"\/>/g)].map((m) => decode(m[1]));
      const pPaths = [...proto.matchAll(/<path d="([^"]+)" fill="[^"]+"\/>/g)].map((m) =>
        (m[1].match(/-?\d+(\.\d+)?/g) ?? []).map(Number)
      );
      expect(cPaths.length).toBe(pPaths.length);
      // 28 px over the 21.12-unit stone: 0.2 px is 0.151 units; the prototype's truncation 0.1
      const tol = 0.2 / (28 / 21.12) + 0.1 + 0.0708;
      cPaths.forEach((line, i) => {
        const nums = pPaths[i];
        for (let k = 0; k < nums.length; k += 2)
          expect(distToPolyline([nums[k], nums[k + 1]], line)).toBeLessThanOrEqual(tol);
      });
      expect(compact.match(/<circle /g)?.length).toBe(proto.match(/<circle /g)?.length);
    }
  });
});

describe('the seal markup', () => {
  it('is an image named by the hash, sized on its longer side', () => {
    for (const [name, size] of [
      ['page', 150],
      ['row', 28],
      ['brush', 150],
    ] as const) {
      const { svg } = compound(HASH, RECIPES[name], size, PAINT);
      expect(svg.startsWith('<svg ')).toBe(true);
      expect(svg).toContain('role="img" aria-label="Hash seal 0xbbb655ae"');
      const [, w, h] = /^<svg width="(\d+)" height="(\d+)"/.exec(svg) ?? [];
      expect(Math.max(Number(w), Number(h))).toBe(size);
      expect(Number(w)).toBeLessThan(Number(h)); // a talisman is tall
      expect(svg).not.toMatch(/#[0-9a-f]{6}/i);
    }
  });

  it('presses red text in a frame, cuts white text out of the stone, lays the brush bare', () => {
    expect(compound(HASH, PAGE, 150, PAINT).svg).toContain('fill="none" stroke="var(--c-rouge)"');
    const row = compound(HASH, ROW, 28, PAINT).svg;
    expect(row).toContain(
      '<rect x="0" y="0" width="14.48" height="21.12" rx="0.4" fill="var(--c-rouge)"/>'
    );
    expect(row).toContain('<g fill="var(--c-base-bg)">');
    const brush = compound(HASH, BRUSH, 150, PAINT).svg;
    expect(brush).not.toContain('<rect');
    expect(brush).not.toContain('var(--c-base-bg)');
  });

  it('keeps a 28 px row seal under 4 KB', () => {
    for (const hash of HASHES) expect(compound(hash, ROW, 28, PAINT).svg.length).toBeLessThan(4096);
  });

  it('turns by the tilt, and refuses what is not a number, a hash or a paint', () => {
    expect(compound(HASH, PAGE, 150, { ...PAINT, tilt: -4 }).svg).toContain(
      'style="transform: rotate(-4deg)"'
    );
    expect(compound(HASH, PAGE, 150, PAINT).svg).not.toContain('style=');
    expect(() => compound(HASH, PAGE, 150, { ...PAINT, ink: 'red"/><script>' })).toThrow(
      'ink must be'
    );
    expect(() => compound(HASH, PAGE, 150, { ...PAINT, ground: 'url(#x)' })).toThrow(
      'ground must be'
    );
    expect(() => compound(HASH, PAGE, 0, PAINT)).toThrow('size 0');
    expect(() => compound(HASH, PAGE, 150, { ...PAINT, tilt: Number.NaN })).toThrow('tilt');
    expect(() => compound('0x12', PAGE, 150, PAINT)).toThrow('lowercase 32-byte');
  });
});
