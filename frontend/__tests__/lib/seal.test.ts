// @vitest-environment node
import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { CLASS_COLORS } from '@/lib/design-tokens';
import { bitsOf } from '@/lib/night/bits';
import { assertSealHash, BitReader, valueOf } from '@/lib/seal/bits';
import {
  chainSegs,
  orderStrokes,
  sample,
  segmentPaths,
  stroke,
  type Pt,
  type StrokeOptions,
} from '@/lib/seal/brush';
import {
  coreOf,
  cutGaps,
  hourAngle,
  ownerOf,
  RIM_MIN,
  rimPoints,
  rimR,
  RIMS,
  ringStrokes,
  RINGS,
  TAU,
} from '@/lib/seal/core';
import { compactEncoding, outlinesOf, PROTOTYPE } from '@/lib/seal/draw';
import { compactOutline, joinNumbers, simplify, tenths } from '@/lib/seal/encode';
import {
  ANIMALS,
  ARC,
  CUT,
  ELEMENTS,
  GLYPH,
  GLYPH_LABEL,
  glyphOf,
  GLYPHS,
  NOT_ORACLE,
  SIGNS,
} from '@/lib/seal/glyphs';
import { assertPaint } from '@/lib/seal/paint';
import { decimalDigits, ROD, ROD_FLAT, rodCount, rodDigit } from '@/lib/seal/rods';
import { walkRect } from '@/lib/seal/textures';
import { WINDOW_CLASSES, windowOf } from '@/lib/seal/window';
import { decode, distToPolyline } from '@/__tests__/utils/compact-path';

// block #20,914,858's hash, which no transaction has (the night's test hash)
const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';

/** Hashes from fixed seeds: sha256("ckbadger seal <role> <i>"). */
const hashes = (role: string, n: number) =>
  Array.from(
    { length: n },
    (_, i) => '0x' + createHash('sha256').update(`ckbadger seal ${role} ${i}`).digest('hex')
  );

/** A hash whose first bits are `lead`, the rest 0. */
function hashWith(lead: readonly number[]): string {
  const bits = [...lead, ...new Array(256 - lead.length).fill(0)];
  let hex = '';
  for (let i = 0; i < 256; i += 4)
    hex += ((bits[i] << 3) | (bits[i + 1] << 2) | (bits[i + 2] << 1) | bits[i + 3]).toString(16);
  return '0x' + hex;
}

/** The k bits of v, most significant first. */
const bitsOfValue = (v: number, k: number) =>
  Array.from({ length: k }, (_, i) => (v >> (k - 1 - i)) & 1);

/** Every combination of n bits, as arrays. */
const variants = (n: number) => Array.from({ length: 1 << n }, (_, v) => bitsOfValue(v, n));

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

describe('the textures', () => {
  it('walks one line through the lattice (cloud script)', () => {
    expect(walkRect([0, 0, 0], 2, 2, 3)).toEqual([
      [0, 0, 1, 0],
      [1, 0, 1, 1],
      [1, 1, 0, 1],
    ]);
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
});

describe('the paints', () => {
  it('takes a hex colour or a palette reference, and refuses anything else by name', () => {
    expect(() => assertPaint('ink', '#c2452d')).not.toThrow();
    expect(() => assertPaint('ink', 'var(--c-mark)')).not.toThrow();
    expect(() => assertPaint('ink', 'red"/><script>')).toThrow('ink must be');
    expect(() => assertPaint('ground', 'url(#x)')).toThrow('ground must be');
    expect(() => assertPaint('dead', '#c2452')).toThrow('"#c2452"');
  });
});

describe('the glyphs', () => {
  it('are forty: sixteen elements, twelve animals, twelve signs, each named twice', () => {
    expect(ELEMENTS).toHaveLength(16);
    expect(ANIMALS).toHaveLength(12);
    expect(SIGNS).toHaveLength(12);
    expect(GLYPHS).toEqual([...ELEMENTS, ...ANIMALS, ...SIGNS]);
    expect(new Set(GLYPHS).size).toBe(40);
    expect(Object.keys(GLYPH).sort()).toEqual([...GLYPHS].sort());
    expect(Object.keys(GLYPH_LABEL).sort()).toEqual([...GLYPHS].sort());
    for (const name of GLYPHS) {
      const [en, zh] = GLYPH_LABEL[name];
      expect(en, name).toMatch(/^[A-Za-z]+$/);
      expect(zh, name).toMatch(/^\p{Script=Han}{1,2}$/u);
    }
    expect([GLYPH_LABEL.peaks, GLYPH_LABEL.skein, GLYPH_LABEL.archer]).toEqual([
      ['mountain', '山'],
      ['thread', '丝'],
      ['Sagittarius', '射手'],
    ]);
    expect(NOT_ORACLE).toEqual(['metal']);
    // the animals turn by one bit, the signs have no variant; no glyph reads more than three
    for (const a of ANIMALS) expect(GLYPH[a].bits, a).toBe(1);
    for (const s of SIGNS) expect(GLYPH[s].bits, s).toBe(0);
    expect(Math.max(...GLYPHS.map((g) => GLYPH[g].bits))).toBe(3);
  });

  it('draws every glyph inside its unit box, in every variant, each stroke with its brush', () => {
    for (const name of GLYPHS)
      for (const b of variants(GLYPH[name].bits)) {
        const strokes = GLYPH[name].make(b);
        expect(strokes.length, name).toBeGreaterThan(0);
        for (const s of strokes) {
          expect(s.pts.length, name).toBeGreaterThanOrEqual(2);
          expect(s.brush, name).toBeDefined();
          for (const [x, y] of s.pts) {
            expect(x, `${name} ${b} x`).toBeGreaterThanOrEqual(0);
            expect(x, `${name} ${b} x`).toBeLessThanOrEqual(1);
            expect(y, `${name} ${b} y`).toBeGreaterThanOrEqual(0);
            expect(y, `${name} ${b} y`).toBeLessThanOrEqual(1);
          }
        }
      }
  });

  it('draws the same strokes from the same bits, and another drawing from every variant', () => {
    for (const name of GLYPHS) {
      const drawn = variants(GLYPH[name].bits).map((b) => {
        expect(GLYPH[name].make(b), name).toEqual(GLYPH[name].make(b));
        return JSON.stringify(GLYPH[name].make(b));
      });
      if (name !== 'water') expect(new Set(drawn).size, name).toBe(1 << GLYPH[name].bits);
      // water's third bit draws three streams (川), which have no drops: its second bit (two
      // rows of drops or three) is read and not drawn there, as in the prototype — 6 drawings
      else expect(drawn.map((d) => drawn.indexOf(d))).toEqual([0, 1, 2, 1, 4, 5, 6, 5]);
    }
    // and no two glyphs draw alike
    const all = GLYPHS.map((g) => JSON.stringify(GLYPH[g].make(new Array(GLYPH[g].bits).fill(0))));
    expect(new Set(all).size).toBe(40);
  });

  it('reads the glyph from the first byte mod 40, then the glyph’s own variant bits', () => {
    for (let byte = 0; byte < 256; byte++) {
      const name = GLYPHS[byte % 40];
      const n = GLYPH[name].bits;
      const variant = bitsOfValue(byte % (1 << n), n);
      const rd = new BitReader(hashWith([...bitsOfValue(byte, 8), ...variant]));
      const g = glyphOf(rd);
      expect(g.name, `byte ${byte}`).toBe(name);
      expect(g.choice).toEqual({ at: 0, bits: bitsOfValue(byte, 8) });
      expect(g.variant).toEqual({ at: 8, bits: variant });
      expect(g.strokes).toEqual(GLYPH[name].make(variant));
      expect(rd.at).toBe(8 + n);
    }
  });
});

describe('the counting rods', () => {
  const BOX = { x: 1, y: 2, w: 4, h: 6 };
  const PITCH = 0.8;

  it('writes 1–5 as standing rods, 6–9 as a lying rod over 1–4, 0 as a ring', () => {
    const zero = rodDigit(0, BOX, PITCH);
    expect(zero).toHaveLength(1);
    expect(zero[0].brush).toBe(ARC);
    expect(zero[0].pts).toHaveLength(13);
    for (const [x, y] of zero[0].pts) expect(Math.hypot(x - 3, y - 5)).toBeCloseTo(4 * 0.26, 12);
    for (let d = 1; d <= 9; d++) {
      const strokes = rodDigit(d, BOX, PITCH);
      const standing = d > 5 ? d - 5 : d;
      const rods = strokes.filter((s) => s.brush === ROD);
      expect(rods, `${d}`).toHaveLength(standing);
      rods.forEach((s, i) => {
        const [[x0, y0], [x1, y1]] = s.pts;
        expect(x0).toBe(x1); // standing
        expect(x0).toBeCloseTo(3 - ((standing - 1) * PITCH) / 2 + i * PITCH, 12); // centred, a pitch apart
        expect(y0).toBeCloseTo(d > 5 ? 2 + 6 * 0.3 : 2, 12);
        expect(y1).toBe(8);
      });
      const lying = strokes.filter((s) => s.brush === ROD_FLAT);
      expect(lying, `${d}`).toHaveLength(d > 5 ? 1 : 0);
      if (d > 5) {
        const [[x0, y0], [x1, y1]] = lying[0].pts;
        expect(y0).toBe(y1); // lying
        expect(y0).toBeCloseTo(2 + 6 * 0.06, 12);
        expect((x0 + x1) / 2).toBeCloseTo(3, 12);
      }
    }
    for (const bad of [10, -1, 1.5])
      expect(() => rodDigit(bad, BOX, PITCH)).toThrow('is not a digit');
  });

  it('stacks a count’s digits one under the other, the greatest on top, centred in its zone', () => {
    const zone = { x: 0, y: 0, w: 4, h: 20 };
    const r = rodCount(248, zone, { pitch: PITCH, maxDigits: 4, digitH: 4 });
    if (r.full) throw new Error('248 fits four digits');
    const gap = 4 * 0.42,
      y0 = (20 - (3 * 4 + 2 * gap)) / 2;
    expect(r.strokes).toEqual([
      ...rodDigit(2, { x: 0, y: y0, w: 4, h: 4 }, PITCH),
      ...rodDigit(4, { x: 0, y: y0 + 4 + gap, w: 4, h: 4 }, PITCH),
      ...rodDigit(8, { x: 0, y: y0 + 2 * (4 + gap), w: 4, h: 4 }, PITCH),
    ]);
    expect(r.strokes).toHaveLength(2 + 4 + 3 + 1);
  });

  it('counts what its zone cannot take as a full block', () => {
    const zone = { x: 0, y: 0, w: 4, h: 6 };
    expect(rodCount(9, zone, { pitch: 1, maxDigits: 1, digitH: 5 }).full).toBe(false);
    expect(rodCount(10, zone, { pitch: 1, maxDigits: 1, digitH: 5 })).toEqual({ full: true });
    expect(rodCount(9999, zone, { pitch: 1, maxDigits: 4, digitH: 1 }).full).toBe(false);
    expect(rodCount(10000, zone, { pitch: 1, maxDigits: 4, digitH: 1 })).toEqual({ full: true });
  });

  it('refuses a count that is not a whole number ≥ 0, naming it', () => {
    const zone = { x: 0, y: 0, w: 4, h: 6 };
    for (const bad of [-1, 1.5, Number.NaN, Infinity])
      expect(() => rodCount(bad, zone, { pitch: 1, maxDigits: 4, digitH: 1 })).toThrow(
        `whole number ≥ 0, got ${bad}`
      );
    expect(decimalDigits(0)).toEqual([0]);
    expect(decimalDigits(1204)).toEqual([1, 2, 0, 4]);
  });
});

describe('the core of a transaction, and the owner of a cell', () => {
  it('keeps every rim between 0.915 and 1 of its radius, all the way round', () => {
    for (const shape of RIMS) {
      let lo = Infinity,
        hi = -Infinity;
      for (let i = 0; i < 3600; i++) {
        const r = rimR(shape, (i * TAU) / 3600);
        lo = Math.min(lo, r);
        hi = Math.max(hi, r);
      }
      expect(lo, shape).toBeGreaterThanOrEqual(RIM_MIN - 1e-12);
      expect(hi, shape).toBeLessThanOrEqual(1 + 1e-12);
      expect(hi, shape).toBeGreaterThan(0.999);
      if (shape !== 'round') expect(lo, shape).toBeLessThan(0.96); // a shaped mouth
    }
    expect(RIM_MIN).toBe(0.915);
  });

  it('outlines a rim in 180 points, clockwise from the top', () => {
    const pts = rimPoints('round', 10, 10, 5);
    expect(pts).toHaveLength(180);
    expect(pts[0][0]).toBeCloseTo(10, 12);
    expect(pts[0][1]).toBeCloseTo(5, 12);
    expect(pts[45][0]).toBeCloseTo(15, 12); // a quarter round: to the right
    expect(pts[45][1]).toBeCloseTo(10, 12);
    for (const shape of RIMS)
      for (const [i, [x, y]] of rimPoints(shape, 0, 0, 2).entries())
        expect(Math.hypot(x, y)).toBeCloseTo(2 * rimR(shape, (i * TAU) / 180), 12);
  });

  it('writes each of the eight borders on its band, its count of times round', () => {
    const strokesPer = {
      zig: 1,
      wave: 1,
      rope: 2,
      ticks: 0,
      stitch: 0,
      scales: 0,
      key: 0,
      comb: 0,
    };
    for (const ring of RINGS)
      for (const k of [20, 24])
        for (const flip of [0, 1]) {
          const strokes = ringStrokes(ring, 10, 1, k, 0, 0, flip);
          expect(strokes, ring).toHaveLength(strokesPer[ring] || k);
          for (const s of strokes) {
            expect(s.pts.length).toBeGreaterThanOrEqual(2);
            for (const [x, y] of s.pts) {
              expect(Math.hypot(x, y), ring).toBeGreaterThanOrEqual(9 - 1e-9);
              expect(Math.hypot(x, y), ring).toBeLessThanOrEqual(11 + 1e-9);
            }
          }
        }
    // the key's hooks turn with the bit; nothing else does
    expect(ringStrokes('key', 10, 1, 20, 0, 0, 0)).not.toEqual(
      ringStrokes('key', 10, 1, 20, 0, 0, 1)
    );
    expect(ringStrokes('zig', 10, 1, 20, 0, 0, 0)).toEqual(ringStrokes('zig', 10, 1, 20, 0, 0, 1));
  });

  it('reads a core in one order: glyph, border (3 bits), its count, its turn, rim (byte mod 5)', () => {
    // byte 0 is the sun (two variant bits); byte 28 is Aries (none)
    for (const [byte, variant] of [
      [0, [1, 0]],
      [28, []],
    ] as const)
      for (let ring = 0; ring < 8; ring++)
        for (const k of [0, 1])
          for (const flip of [0, 1])
            for (const rimByte of [0, 1, 2, 3, 4, 5, 9, 255]) {
              const hash = hashWith([
                ...bitsOfValue(byte, 8),
                ...variant,
                ...bitsOfValue(ring, 3),
                k,
                flip,
                ...bitsOfValue(rimByte, 8),
              ]);
              const core = coreOf(hash);
              expect(core.glyph.name).toBe(byte ? 'aries' : 'sun');
              expect(core.glyph.variant.bits).toEqual(variant);
              expect(core.ring).toBe(RINGS[ring]);
              expect(core.k).toBe(k ? 24 : 20);
              expect(core.flip).toBe(flip);
              expect(core.rim).toBe(RIMS[rimByte % 5]);
              expect(core.bitsUsed).toBe(8 + variant.length + 3 + 1 + 1 + 8);
            }
  });

  it('gives an owner one of eleven hours after the lock’s glyph, never the sixth (the index’s)', () => {
    const seen = new Set<number>();
    for (let v = 0; v < 256; v++) {
      const owner = ownerOf(hashWith([...bitsOfValue(28, 8), ...bitsOfValue(v, 8)]));
      const h = v % 11;
      expect(owner.hour, `byte ${v}`).toBe(h >= 6 ? h + 1 : h);
      expect(owner.glyph.name).toBe('aries');
      expect(owner.bitsUsed).toBe(16);
      seen.add(owner.hour);
    }
    expect([...seen].sort((a, b) => a - b)).toEqual([0, 1, 2, 3, 4, 5, 7, 8, 9, 10, 11]);
    for (const lock of hashes('lock', 200)) expect(ownerOf(lock).hour).not.toBe(6);
    expect(hourAngle(0)).toBeCloseTo(-Math.PI / 2, 12);
    expect(hourAngle(3)).toBeCloseTo(0, 12);
    expect(hourAngle(6)).toBeCloseTo(Math.PI / 2, 12); // the foot
  });

  it('cuts a stroke where it crosses a gap, keeping runs of two points or more', () => {
    // half a circle from the right (0) through the foot (π/2) to the left (π), y down
    const pts: Pt[] = Array.from({ length: 13 }, (_, i) => [
      Math.cos((i * Math.PI) / 12),
      Math.sin((i * Math.PI) / 12),
    ]);
    const cut = cutGaps([{ pts, brush: CUT }], 0, 0, [[Math.PI / 2, 0.3]]);
    expect(cut).toHaveLength(2);
    for (const s of cut) {
      expect(s.brush).toBe(CUT);
      for (const [x, y] of s.pts)
        expect(Math.abs(Math.atan2(y, x) - Math.PI / 2)).toBeGreaterThanOrEqual(0.3);
    }
    expect(cut[0].pts.length + cut[1].pts.length).toBe(13 - 3); // three points lie in the gap
    // a run left with one point is dropped
    expect(cutGaps([{ pts: pts.slice(0, 2) }], 0, 0, [[Math.PI / 12, 0.1]])).toEqual([]);
  });
});

describe('the windows', () => {
  const at = (cls: Parameters<typeof windowOf>[0]) => windowOf(cls, 10, 10, 4, '#c2452d', 0.4);

  it('has one window for each of the six classes', () => {
    expect([...WINDOW_CLASSES].sort()).toEqual(Object.keys(CLASS_COLORS).sort());
  });

  it('draws none for plain CKB, a ring, a doubled ring, a square, a hexagon, an arch', () => {
    expect(at('ckb')).toBe('');
    expect(at('other')).toBe(
      '<circle cx="10" cy="10" r="4" fill="none" stroke="#c2452d" stroke-width="0.4"/>'
    );
    expect(at('dao').match(/<circle /g)).toHaveLength(2);
    expect(at('dao')).toContain(`r="${Math.round((4 - 0.4 * 2.4) * 100) / 100}"`);
    expect(at('tokens')).toMatch(/^<rect x="[\d.]+" y="[\d.]+" width="[\d.]+" height="[\d.]+" rx/);
    const hex = /<path d="([^"]+)"/.exec(at('objects'))?.[1] ?? '';
    expect(hex.match(/[ML]/g)).toHaveLength(6);
    expect(hex.endsWith('Z')).toBe(true);
    expect(at('identities').match(/A/g)).toHaveLength(1);
    for (const cls of WINDOW_CLASSES.filter((c) => c !== 'ckb'))
      expect(at(cls)).toContain('fill="none" stroke="#c2452d"');
  });

  it('refuses a class it does not know', () => {
    expect(() => windowOf('nft' as 'other', 0, 0, 1, '#c2452d', 0.4)).toThrow(
      'no such class "nft"'
    );
  });
});

describe('the hand', () => {
  const HAND = { w0: 0.58, p: 1.58 };
  const LINE = [
    {
      pts: [
        [0, 0],
        [4, 0],
      ] as Pt[],
    },
  ];

  it('writes a run of outlines in the prototype’s markup, every number cut to one decimal', () => {
    const outs = outlinesOf(
      LINE,
      CUT,
      (x) => x + 1.234,
      (y) => y + 2.345,
      1,
      HAND
    );
    const markup = PROTOTYPE.ink(outs, '#c2452d');
    expect(markup.startsWith('<path d="M')).toBe(true);
    expect(markup).toContain('fill="#c2452d"');
    expect(markup).not.toMatch(/\d\.\d\d/);
    expect(
      PROTOTYPE.ring([
        [1.234, 2.345],
        [3, 4],
      ])
    ).toBe('M1.23 2.35L3 4Z');
  });

  it('writes the same outlines compactly, one fill on the run, within its tolerance', () => {
    const outs = outlinesOf(
      LINE,
      CUT,
      (x) => x,
      (y) => y,
      1,
      HAND
    );
    const tol = 0.05;
    const markup = compactEncoding(tol).ink(outs, 'var(--c-mark)');
    expect(markup.startsWith('<g fill="var(--c-mark)"><path d="M')).toBe(true);
    expect(markup.endsWith('</g>')).toBe(true);
    const d = /<path d="([^"]+)"\/>/.exec(markup)?.[1] ?? '';
    const line = decode(d);
    for (const p of outs[0].polygon)
      expect(distToPolyline(p, [...line, line[0]])).toBeLessThanOrEqual(tol + 0.0708);
    expect(compactEncoding(tol).ink([], 'var(--c-mark)')).toBe('');
    expect(() => compactEncoding(0)).toThrow('tolerance');
  });

  it('lets a stroke’s own brush override the run’s; a horizontal stops blunt, a vertical lifts', () => {
    const [plain] = outlinesOf(
      LINE,
      CUT,
      (x) => x,
      (y) => y,
      1,
      HAND
    );
    expect(plain.caps).toHaveLength(0); // pointed in, lifted out
    const [rod] = outlinesOf(
      [{ ...LINE[0], brush: ROD }],
      CUT,
      (x) => x,
      (y) => y,
      1,
      HAND
    );
    expect(rod.caps).toHaveLength(1); // set down round
    const [flat] = outlinesOf(
      [{ ...LINE[0], horiz: true }],
      CUT,
      (x) => x,
      (y) => y,
      1,
      HAND
    );
    expect(flat.caps).toEqual([[4, 0, expect.any(Number)]]); // a blunt stop at the end
    const [down] = outlinesOf(
      [{ ...LINE[0], horiz: false }],
      CUT,
      (x) => x,
      (y) => y,
      1,
      HAND
    );
    expect(down.caps).toEqual([[0, 0, expect.any(Number)]]); // set down round at the start
    // the stroke's weight and the run's multiply
    const [heavy] = outlinesOf(
      [{ ...LINE[0], weight: 2 }],
      CUT,
      (x) => x,
      (y) => y,
      1.5,
      HAND
    );
    const width = (o: typeof plain) => Math.max(...o.polygon.map(([, y]) => Math.abs(y))) * 2;
    expect(width(heavy) / width(plain)).toBeCloseTo(3, 6);
  });
});
