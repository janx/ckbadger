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
import {
  assertSealId,
  blockRegions,
  CAPACITY_DIGITS,
  capacityDigits,
  drawSeal,
  drawStone,
  handOf,
  indexLayout,
  PAGE_DIGITS,
  SCRIPT_TEXTURE,
  sealLabel,
  TX_DIGIT_H,
  TX_PAGE_DIGITS,
  txCountRods,
  txPageMeasures,
  type CellClass,
  type SealId,
  type SealOptions,
  type SealState,
} from '@/lib/seal/stones';
import { TEXTURE, TEXTURES, walkRect } from '@/lib/seal/textures';
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

/* ---------- the stones ---------- */

const INKS = { ink: 'var(--c-mark)', ground: 'var(--c-base-bg)', dead: 'var(--c-text-dim)' };
const SHANNONS_PER_CKB = BigInt(100000000);
const ckb = (whole: string) => BigInt(whole) * SHANNONS_PER_CKB;
const PAGE_PX = { block: 104, tx: 94, cell: 112, script: 104 } as const;
const ROW_PX = { block: 26, tx: 24, cell: 30, script: 28 } as const;
const CELL_CLASSES: CellClass[] = ['dao', 'tokens', 'objects', 'identities', 'other'];

const TXH = hashes('tx', 300),
  LOCKH = hashes('lock', 300),
  TYPEH = hashes('type', 300),
  BLOCKH = hashes('block', 300),
  SCRIPTH = hashes('script', 300);

/** One id of each kind from the i-th hashes. */
const idsAt = (i: number) =>
  ({
    block: { kind: 'block', hash: BLOCKH[i], txs: [1, 7, 12, 248, 1820][i % 5] },
    tx: { kind: 'tx', hash: TXH[i], consumed: i % 4, created: (i * 7) % 13 },
    cell: {
      kind: 'cell',
      tx: TXH[i],
      index: i,
      lock: LOCKH[i],
      type: i % 6 === 5 ? null : { class: CELL_CLASSES[i % 5], hash: TYPEH[i] },
      capacity: ckb(String(61 * 10 ** (i % 9))),
    },
    script: { kind: 'script', hash: SCRIPTH[i] },
  }) as const satisfies Record<SealId['kind'], SealId>;

const page = (id: SealId, state: SealState = 'pressed', extra: Partial<SealOptions> = {}) =>
  drawStone(id, { form: 'page', height: PAGE_PX[id.kind], state, ...INKS, ...extra });
const row = (id: SealId, state: SealState = 'pressed', extra: Partial<SealOptions> = {}) =>
  drawStone(id, { form: 'row', height: ROW_PX[id.kind], state, ...INKS, ...extra });

describe('the capacity of a cell', () => {
  it('counts the decimal digits of its whole CKB, from shannons, exactly', () => {
    expect(capacityDigits(ckb('61'))).toBe(2);
    expect(capacityDigits(ckb('1204'))).toBe(4);
    expect(capacityDigits(ckb('33600000000'))).toBe(11); // 33.6 × 10⁹
    expect(capacityDigits(ckb('99999999999'))).toBe(11);
    expect(capacityDigits(ckb('100000000000') - BigInt(1))).toBe(11); // a shannon short of 10¹¹
    expect(capacityDigits(BigInt(6150000000))).toBe(2); // 61.5 CKB: whole CKB
    expect(capacityDigits(BigInt(9999999999))).toBe(2); // 99.99999999 CKB
    expect(capacityDigits(BigInt(0))).toBe(1);
    expect(CAPACITY_DIGITS).toBe(11);
  });

  it('refuses more than eleven digits, a negative capacity and one that is not a BigInt', () => {
    expect(() => capacityDigits(ckb('100000000000'))).toThrow(
      'is 100000000000 CKB, more than 11 digits'
    );
    expect(() => capacityDigits(BigInt(-1))).toThrow('whole number of shannons ≥ 0, got -1');
    expect(() => capacityDigits(61 as unknown as bigint)).toThrow('got 61');
  });
});

describe('a seal’s markup', () => {
  it('is one image whose label names the kind, the id and what it counts', () => {
    for (let i = 0; i < 12; i++)
      for (const id of Object.values(idsAt(i)))
        for (const drawn of [page(id), row(id)]) {
          const { svg } = drawn;
          expect(svg.startsWith('<svg ')).toBe(true);
          expect(svg.endsWith('</svg>')).toBe(true);
          expect(svg.match(/<svg /g)).toHaveLength(1);
          expect(svg.match(/role="img"/g)).toHaveLength(1);
          expect(svg).toContain(`aria-label="${sealLabel(id)}"`);
        }
    const B = BLOCKH[0],
      T = TXH[0],
      L = LOCKH[0],
      Y = TYPEH[0];
    expect(sealLabel({ kind: 'block', hash: B, txs: 248 })).toBe(
      `Block seal ${B.slice(0, 10)}, 248 transactions`
    );
    expect(sealLabel({ kind: 'block', hash: B, txs: 1 })).toBe(
      `Block seal ${B.slice(0, 10)}, 1 transaction`
    );
    expect(sealLabel({ kind: 'tx', hash: T, consumed: 2, created: 3 })).toBe(
      `Transaction seal ${T.slice(0, 10)}, 2 consumed, 3 created`
    );
    expect(sealLabel({ kind: 'tx', hash: T, consumed: null, created: 3 })).toBe(
      `Transaction seal ${T.slice(0, 10)}, consumed not known, 3 created`
    );
    const cell = {
      kind: 'cell',
      tx: T,
      index: 1,
      lock: L,
      type: { class: 'tokens', hash: Y },
      capacity: BigInt(120412345678),
    } as const;
    expect(sealLabel(cell)).toBe(`Cell seal ${T.slice(0, 10)}:1, tokens, 1,204.12345678 CKB`);
    expect(sealLabel({ ...cell, type: null, capacity: ckb('61') })).toBe(
      `Cell seal ${T.slice(0, 10)}:1, ckb, 61.00000000 CKB`
    );
    expect(sealLabel({ kind: 'script', hash: Y })).toBe(`Script seal ${Y.slice(0, 10)}`);
  });

  it('gives each stone the design’s size: square block, long transaction, round cell, tall script', () => {
    const ids = idsAt(0);
    const size = (d: { width: number; height: number }) => [d.width, d.height];
    expect(size(page(ids.block))).toEqual([104, 104]);
    expect(size(page(ids.cell))).toEqual([112, 112]);
    const tx = page(ids.tx);
    expect(tx.height).toBe(94);
    expect(tx.width / tx.height).toBeGreaterThan(1.6);
    expect(tx.width / tx.height).toBeLessThan(1.7); // about 1.66 : 1
    const script = page(ids.script);
    expect(script.height).toBe(104);
    expect(script.width).toBeLessThan(script.height);
    expect(size(row(ids.block))).toEqual([26, 26]);
    expect(size(row(ids.tx))).toEqual([48.5, 24]); // 49 wide, as the plan says
    expect(size(row(ids.cell))).toEqual([30, 30]);
    expect(size(row(ids.script))).toEqual([21, 28]);
    // the size is the kind's, whatever the id
    for (let i = 1; i < 24; i++)
      for (const id of Object.values(idsAt(i))) {
        expect(size(page(id))).toEqual(size(page(idsAt(0)[id.kind])));
        expect(size(row(id))).toEqual(size(row(idsAt(0)[id.kind])));
      }
    const { svg } = drawSeal(ids.tx, { form: 'row', height: 24, state: 'pressed', ...INKS });
    expect(svg).toMatch(/^<svg width="48.5" height="24" viewBox="0 0 /);
  });

  it('presses in three states: the ink; the dead ink; unpressed, dashed and at half strength', () => {
    for (const id of Object.values(idsAt(4))) {
      const pressed = page(id).svg,
        ink = page(id, 'ink').svg,
        unpressed = page(id, 'unpressed').svg;
      expect(pressed).toContain(`stroke="${INKS.ink}"`);
      expect(pressed).not.toContain(INKS.dead);
      expect(pressed).not.toContain('stroke-dasharray');
      expect(pressed).not.toContain('opacity');
      expect(ink).toContain(`stroke="${INKS.dead}"`);
      expect(ink).not.toContain(INKS.ink);
      expect(ink.replaceAll(INKS.dead, INKS.ink)).toBe(pressed);
      expect(unpressed).toContain('stroke-dasharray="1.9 1.4"');
      expect(unpressed).toContain('<g opacity="0.5">');
      expect(unpressed).not.toContain(INKS.dead);
      // a row stone: solid in its ink with the text cut in the ground; unpressed, a dashed hair
      // line with the text in the ink
      const r = row(id).svg,
        rDead = row(id, 'ink').svg,
        rHollow = row(id, 'unpressed').svg;
      expect(r).toMatch(
        new RegExp(`aria-label="[^"]*"><(rect|circle|path)[^>]* fill="${reEscape(INKS.ink)}"`)
      );
      expect(r).toContain(`<g fill="${INKS.ground}">`);
      expect(rDead.replaceAll(INKS.dead, INKS.ink)).toBe(r);
      expect(rHollow).toContain('stroke-dasharray="1.5 1.1"');
      expect(rHollow).toMatch(
        new RegExp(`aria-label="[^"]*"><(rect|circle|path)[^>]* fill="none"`)
      );
      expect(rHollow).not.toContain(INKS.ground);
      expect(rHollow).toContain(`<g fill="${INKS.ink}">`);
    }
  });

  it('turns by the tilt and opens a seam where it is pressed across one', () => {
    const id = idsAt(2).tx;
    expect(page(id, 'pressed', { tilt: -4 }).svg).toContain('style="transform: rotate(-4deg)"');
    expect(page(id).svg).not.toContain('style=');
    const seam = page(id, 'pressed', { seamPx: 3 }).svg;
    expect(seam).toMatch(
      new RegExp(
        `<path d="M[\\d.]+ -1L[\\d.]+ [\\d.]+" stroke="${reEscape(INKS.ground)}" stroke-width="[\\d.]+"/></svg>$`
      )
    );
    expect(page(id, 'pressed', { seamPx: 0 }).svg).toBe(page(id).svg);
  });

  it('writes no colour but the paints it is given', () => {
    for (let i = 0; i < 12; i++)
      for (const id of Object.values(idsAt(i)))
        for (const state of ['pressed', 'ink', 'unpressed'] as const)
          for (const { svg } of [page(id, state), row(id, state)]) {
            expect(svg).not.toMatch(/#[0-9a-f]{3,6}/i);
            const paints = new Set(svg.match(/var\(--c-[a-z-]+\)/g));
            for (const p of paints) expect(Object.values(INKS)).toContain(p);
          }
  });
});

/** RegExp-escaped text. */
function reEscape(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

describe('what a seal says', () => {
  it('makes a transaction and its cells share rim and border (200 transactions)', () => {
    for (let i = 0; i < 200; i++) {
      const tx: SealId = { kind: 'tx', hash: TXH[i], consumed: 1, created: 2 };
      const [, txRing, txRim] = page(tx).chosen;
      expect(row(tx).chosen).toEqual(page(tx).chosen);
      for (const index of [0, 1, 7]) {
        const cell: SealId = {
          ...idsAt(i).cell,
          tx: TXH[i],
          index,
          lock: LOCKH[(i + index) % 300],
        };
        for (const drawn of [page(cell), row(cell)]) {
          const [, rim, ring] = drawn.chosen;
          expect([rim, ring], `tx ${i} cell ${index}`).toEqual([txRim, txRing]);
        }
      }
      // and the cells' rim is drawn where the transaction's is: the same rim path, scaled
      const core = coreOf(TXH[i]);
      expect([core.ring, core.rim]).toEqual([txRing, txRim]);
    }
  });

  it('draws a cell with no type as a bare stone: no window, no glyph', () => {
    for (let i = 0; i < 30; i++) {
      const typed = { ...idsAt(i).cell, type: { class: CELL_CLASSES[i % 5], hash: TYPEH[i] } };
      const bare: SealId = { ...typed, type: null };
      expect(page(bare).chosen[0]).toBe('-');
      expect(row(bare).chosen[0]).toBe('-');
      const glyphStrokes = glyphOf(new BitReader(TYPEH[i])).strokes.length;
      // the page: the same stone but the window and the type's glyph
      const fills = (svg: string) =>
        svg.match(/<path d="[^"]+" fill="var\(--c-mark\)"\/>/g)?.length ?? 0;
      const nones = (svg: string) => svg.match(/fill="none"/g)?.length ?? 0;
      expect(fills(page(typed).svg) - fills(page(bare).svg)).toBe(glyphStrokes);
      expect(nones(page(bare).svg)).toBe(1); // the rim alone
      expect(nones(page(typed).svg)).toBeGreaterThan(1); // the rim and the window
      // the row: the rim, a bead for each digit, the owner's mark; nothing else
      const digits = capacityDigits(bare.capacity);
      const tags = [...row(bare).svg.matchAll(/<(\w+)/g)].map((m) => m[1]).slice(1);
      expect(tags.slice(1)).toEqual([...new Array(digits).fill('circle'), 'rect']);
      expect(row(bare).svg).not.toContain('<g ');
    }
  });

  it('beads a cell’s rim once for each digit of its capacity, starting after its owner', () => {
    for (const [whole, n] of [
      ['61', 2],
      ['1204', 4],
      ['10000', 5],
      ['33600000000', 11],
    ] as const) {
      const cell: SealId = { ...idsAt(5).cell, type: null, capacity: ckb(whole) };
      const beads = page(cell).svg.match(/paint-order="stroke"\/>/g)?.length ?? 0;
      expect(beads - 1, whole).toBe(n); // and the owner's stone
      expect(row(cell).svg.match(/<circle [^>]*r="0.4"/g), whole).toHaveLength(n);
    }
  });

  it('writes a cell’s index exactly, up to four digits, standing on the border at the foot', () => {
    const c = 20,
      RmB = 9,
      bb = 2.2,
      p = 1.58;
    for (const index of [0, 1, 5, 6, 9, 10, 12, 305, 1000, 4096, 9999]) {
      const ix = indexLayout(index, c, RmB, bb, p);
      expect(ix.digits.map((x) => x.d)).toEqual(String(index).split('').map(Number));
      expect(ix.digits[0].box.x).toBeCloseTo(c - ix.total / 2, 12); // centred under the stone
      const last = ix.digits[ix.digits.length - 1].box;
      expect(last.x + last.w).toBeCloseTo(c + ix.total / 2, 12);
      let right = -Infinity;
      for (const { box } of ix.digits) {
        expect(box.x).toBeGreaterThan(right); // left to right, apart
        right = box.x + box.w;
        const xc = box.x + box.w / 2,
          yc = box.y + box.h / 2;
        expect(Math.hypot(xc - c, yc - c)).toBeCloseTo(RmB, 9); // on the border's circle
        expect(yc).toBeGreaterThan(c); // at the foot
      }
    }
    expect(() => indexLayout(10000, c, RmB, bb, p)).toThrow('up to 4 digits, got 10000');
    expect(PAGE_DIGITS).toBe(4);
    // the page seal draws those digits' rods and nothing else differs between two indices but them
    const a = page({ ...idsAt(7).cell, index: 3 }).svg,
      b = page({ ...idsAt(7).cell, index: 4 }).svg;
    expect(a).not.toBe(b);
    // a row seal does not write the index at all
    expect(row({ ...idsAt(7).cell, index: 3 }).svg.replace(':3,', ':4,')).toBe(
      row({ ...idsAt(7).cell, index: 4 }).svg
    );
  });

  it('counts a block’s transactions and a transaction’s cells, a row seal to nine', () => {
    const block = idsAt(0).block;
    expect(row({ ...block, txs: 9 }).svg).not.toContain('width="4.4"');
    expect(row({ ...block, txs: 10 }).svg).toContain('width="4.4" height="1.55"'); // a full block
    expect(page({ ...block, txs: 9999 }).svg).toContain('aria-label');
    const tx = idsAt(0).tx;
    const full = (svg: string) =>
      svg.match(/<rect [^>]*fill="var\(--c-base-bg\)"\/>/g)?.length ?? 0;
    expect(full(row({ ...tx, consumed: 9, created: 9 }).svg)).toBe(0);
    expect(full(row({ ...tx, consumed: 10, created: 9 }).svg)).toBe(1);
    expect(full(row({ ...tx, consumed: 240, created: 12 }).svg)).toBe(2);
    expect(page({ ...tx, consumed: 9999, created: 1000 }).svg).toContain('9999 consumed');
  });

  // A transaction can consume more than 9,999 cells within CKB's 597,000-byte block (an input is
  // 44 bytes: up to 13,568), so its page seal writes five digits; the prototype stopped at four and
  // has no reference for five, so the stack is checked here directly: five rows, each one digit in
  // rods, the greatest on top, inside the zone between the frame and the rim.
  it('writes a transaction’s cells in up to five digits on its page seal, digit under digit in its zone', () => {
    expect(TX_PAGE_DIGITS).toBe(5);
    expect(PAGE_DIGITS).toBe(4); // a block's transactions and a cell's index stay at four
    const hand = handOf('page');
    const m = txPageMeasures(hand);
    /** The digit a row of rods writes: a ring is 0, else its standing rods and 5 for a lying one. */
    const digitOf = (strokes: { brush?: unknown }[]) =>
      strokes.some((s) => s.brush === ARC)
        ? 0
        : strokes.filter((s) => s.brush === ROD).length +
          (strokes.some((s) => s.brush === ROD_FLAT) ? 5 : 0);
    for (const n of [0, 7, 12, 248, 1820, 9999, 10000, 13000, 13568, 40302, 99999])
      for (const end of [0, 1] as const) {
        const { zone, strokes } = txCountRods(n, end, m);
        const nd = String(n).length;
        const dh = TX_DIGIT_H[nd - 1] * m.p,
          step = dh * 1.42,
          top = zone.y + (zone.h - (nd * dh + (nd - 1) * dh * 0.42)) / 2;
        const rows: (typeof strokes)[] = Array.from({ length: nd }, () => []);
        for (const s of strokes) {
          const ys = s.pts.map(([, y]) => y),
            xs = s.pts.map(([x]) => x);
          const k = Math.floor((Math.min(...ys) - top) / step + 1e-9);
          expect(k, `${n} ${end}`).toBeGreaterThanOrEqual(0);
          expect(k, `${n} ${end}`).toBeLessThan(nd);
          // inside its digit's box, and the box inside the zone
          expect(Math.max(...ys)).toBeLessThanOrEqual(top + k * step + dh + 1e-9);
          expect(Math.min(...xs)).toBeGreaterThanOrEqual(zone.x);
          expect(Math.max(...xs)).toBeLessThanOrEqual(zone.x + zone.w);
          rows[k].push(s);
        }
        expect(top, `${n}`).toBeGreaterThan(zone.y);
        expect(top + nd * dh + (nd - 1) * dh * 0.42).toBeLessThan(zone.y + zone.h);
        expect(rows.map(digitOf).join(''), `${n} at end ${end}`).toBe(String(n));
      }
    // the zones: the consumed at the left of the core, the created at its right, in the frame
    const left = txCountRods(13000, 0, m).zone,
      right = txCountRods(13000, 1, m).zone;
    expect(left.x).toBeGreaterThan(m.wf);
    expect(left.x + left.w).toBeLessThan(m.cx - m.Rout);
    expect(right.x).toBeGreaterThan(m.cx + m.Rout);
    expect(right.x + right.w).toBeLessThan(m.W - m.wf);
    expect(left.y).toBeGreaterThan(m.wf);
    expect(left.y + left.h).toBeLessThan(m.H - m.wf);
    // five digits stand where four did: each smaller, the stack no taller than the zone
    expect(TX_DIGIT_H[4]).toBeLessThan(TX_DIGIT_H[3]);
    expect(() => txCountRods(100000, 0, m)).toThrow('in up to 5 digits, got 100000');
    // and the page seal draws exactly those rods, at either size and in every state
    const id: SealId = { kind: 'tx', hash: TXH[3], consumed: 13000, created: 1 };
    for (const height of [94, 64])
      for (const state of ['pressed', 'ink', 'unpressed'] as const) {
        const { svg } = drawSeal(id, { form: 'page', height, state, ...INKS });
        expect(svg).toContain('13000 consumed, 1 created');
        const ink = state === 'ink' ? INKS.dead : INKS.ink;
        for (const [n, end] of [
          [13000, 0],
          [1, 1],
        ] as const)
          expect(svg).toContain(
            PROTOTYPE.ink(
              outlinesOf(
                txCountRods(n, end, m).strokes,
                ROD,
                (x) => x,
                (y) => y,
                1.05,
                hand,
                hand.p
              ),
              ink
            )
          );
      }
  });
});

describe('a seal refuses what it cannot draw, naming it', () => {
  const ids = idsAt(1);
  const P: SealOptions = { form: 'page', height: 104, state: 'pressed', ...INKS };
  const R: SealOptions = { ...P, form: 'row' };
  const refuses = (id: unknown, message: string, o: SealOptions = P) =>
    expect(() => drawSeal(id as SealId, o)).toThrow(message);

  it('a hash that is not 32 lowercase bytes, in any field', () => {
    const bad = ids.block.hash.toUpperCase().replace('0X', '0x');
    refuses(
      { ...ids.block, hash: '0x12' },
      'the block hash is not a lowercase 32-byte hash: "0x12"'
    );
    refuses({ ...ids.block, hash: bad }, `"${bad}"`);
    refuses({ ...ids.tx, hash: ids.tx.hash.slice(2) }, 'the transaction hash is not');
    refuses({ ...ids.cell, tx: '0x' }, 'the out point’s transaction hash is not');
    refuses({ ...ids.cell, lock: `${ids.cell.lock}"/>` }, 'the lock hash is not');
    refuses({ ...ids.cell, type: { class: 'tokens', hash: 'x' } }, 'the type hash is not');
    refuses({ ...ids.script, hash: null }, 'the script hash is not a lowercase 32-byte hash: null');
  });

  it('a count that is not a whole number ≥ 0, or past its digits on a page (four; a transaction’s cells five)', () => {
    refuses(
      { ...ids.block, txs: -1 },
      `block ${ids.block.hash}: the transaction count must be a whole number ≥ 0, got -1`
    );
    refuses({ ...ids.block, txs: 1.5 }, 'got 1.5');
    refuses({ ...ids.block, txs: Number.NaN }, 'got NaN');
    refuses(
      { ...ids.tx, consumed: -2 },
      `transaction ${ids.tx.hash}: the cells consumed must be a whole number ≥ 0, got -2`
    );
    refuses({ ...ids.tx, created: 2.5 }, 'the cells created must be a whole number ≥ 0, got 2.5');
    refuses({ ...ids.tx, created: undefined }, 'got undefined');
    refuses(
      { ...ids.block, txs: 10000 },
      'a page seal writes the transaction count in up to 4 digits, got 10000'
    );
    refuses(
      { ...ids.tx, consumed: 100000 },
      `transaction ${ids.tx.hash}: a page seal writes the cells consumed in up to 5 digits, got 100000`
    );
    refuses(
      { ...ids.tx, created: 100000 },
      'a page seal writes the cells created in up to 5 digits, got 100000'
    );
    // a transaction can consume ten thousand cells or more within a block: its page seal writes them
    expect(() => drawSeal({ ...ids.tx, consumed: 10000 }, P)).not.toThrow();
    expect(() => drawSeal({ ...ids.tx, consumed: 99999, created: 99999 }, P)).not.toThrow();
    // a row seal counts to nine and draws a full block beyond
    expect(() => drawSeal({ ...ids.block, txs: 10000 }, R)).not.toThrow();
    expect(() => drawSeal({ ...ids.tx, consumed: 100000 }, R)).not.toThrow();
  });

  it('an index that is not a whole number ≥ 0, or past four digits on a page', () => {
    refuses(
      { ...ids.cell, index: -1 },
      `cell ${ids.cell.tx}:-1: the index must be a whole number ≥ 0, got -1`
    );
    refuses({ ...ids.cell, index: 0.5 }, 'the index must be a whole number ≥ 0, got 0.5');
    refuses({ ...ids.cell, index: '3' }, 'the index must be a whole number ≥ 0, got 3');
    refuses(
      { ...ids.cell, index: 10000 },
      'a page seal writes the index in up to 4 digits, got 10000'
    );
    expect(() => drawSeal({ ...ids.cell, index: 10000 }, R)).not.toThrow();
  });

  it('a class with no type hash, a type hash with class ckb, a class it does not know', () => {
    const of = `cell ${ids.cell.tx}:${ids.cell.index}`;
    refuses(
      { ...ids.cell, type: { class: 'tokens', hash: null } },
      `${of}: class tokens with no type hash`
    );
    refuses({ ...ids.cell, type: { class: 'dao' } }, 'class dao with no type hash');
    refuses(
      { ...ids.cell, type: { class: 'ckb', hash: TYPEH[1] } },
      `${of}: class ckb is no type's class`
    );
    refuses({ ...ids.cell, type: { class: 'ckb', hash: null } }, "class ckb is no type's class");
    refuses({ ...ids.cell, type: { class: 'nft', hash: TYPEH[1] } }, 'no such class "nft"');
    refuses({ ...ids.cell, type: 'tokens' }, 'the type must be { class, hash } or null');
    refuses(
      { ...ids.cell, capacity: ckb('100000000000') },
      `cell ${ids.cell.tx}:${ids.cell.index}: a capacity of 10000000000000000000 shannons is 100000000000 CKB, more than 11 digits`
    );
    refuses({ ...ids.cell, capacity: 61 }, 'a capacity is a whole number of shannons');
  });

  // assertSealId is what a component checks on every render: exactly drawSeal's checks of an id
  it('assertSealId refuses exactly the ids drawSeal refuses, with the same message', () => {
    const bad: [unknown, SealOptions][] = [
      [{ ...ids.block, hash: '0x12' }, P],
      [{ ...ids.block, txs: 10000 }, P],
      [{ ...ids.tx, consumed: -2 }, R],
      [{ ...ids.tx, created: 100000 }, P],
      [{ ...ids.cell, index: 10000 }, P],
      [{ ...ids.cell, type: { class: 'ckb', hash: TYPEH[1] } }, R],
      [{ ...ids.cell, capacity: 61 }, R],
      [{ ...ids.script, hash: null }, R],
      [{ ...ids.block, kind: 'uncle' }, P],
      [null, R],
    ];
    for (const [i, [id, o]] of bad.entries()) {
      let message = '';
      try {
        drawSeal(id as SealId, o);
      } catch (e) {
        message = (e as Error).message;
      }
      expect(message, `bad id ${i}`).not.toBe('');
      expect(() => assertSealId(id as SealId, o.form)).toThrow(message);
    }
    expect(() => assertSealId(ids.block, 'brush' as 'page')).toThrow('no such form "brush"');
    for (const id of Object.values(ids))
      for (const form of ['page', 'row'] as const)
        expect(() => assertSealId(id, form)).not.toThrow();
    // a row seal takes the counts a page seal refuses
    expect(() => assertSealId({ ...ids.block, txs: 10000 }, 'row')).not.toThrow();
    expect(() => assertSealId({ ...ids.cell, index: 10000 }, 'row')).not.toThrow();
  });

  it('a paint, a height, a tilt, a seam, a kind, a form or a state it does not know', () => {
    refuses(ids.block, 'ink must be a hex colour', { ...P, ink: 'red' });
    refuses(ids.block, 'ground must be', { ...P, ground: 'url(#x)' });
    refuses(ids.block, 'dead must be', { ...P, dead: 'var(--c-mark);x' });
    for (const height of [0, -5, Number.NaN, Infinity])
      refuses(ids.block, `height ${height} px`, { ...P, height });
    refuses(ids.block, 'tilt NaN°', { ...P, tilt: Number.NaN });
    refuses(ids.block, 'seam -1 px', { ...P, seamPx: -1 });
    refuses({ ...ids.block, kind: 'uncle' }, 'no such kind of seal: "uncle"');
    refuses(null, 'no such kind of seal');
    refuses(ids.block, 'no such form "brush"', { ...P, form: 'brush' as 'page' });
    refuses(ids.block, 'no such state "faded"', { ...P, state: 'faded' as 'ink' });
  });
});

describe('a seal reads its hashes within their 256 bits', () => {
  it('never past bit 256: the worst case of every kind and form', () => {
    const variantMax = Math.max(...GLYPHS.map((g) => GLYPH[g].bits));
    const hungriest = (w: number, h: number) =>
      3 + Math.max(...TEXTURES.map((t) => TEXTURE[t].bits(Math.round(w), Math.round(h))));
    // the block page: glyph, right, top, the band count, its heights, a texture in each band
    let block = 0;
    for (const nb of [2, 3])
      for (const heights of variants(nb)) {
        const { regions } = blockRegions(nb, heights, 0);
        const need =
          8 +
          variantMax +
          2 +
          1 +
          nb +
          regions.reduce((a, R) => a + hungriest(R.w - 0.5, R.h - 0.5), 0);
        block = Math.max(block, need);
      }
    // the script page: glyph, top, the eight bits passed over, its texture
    const script =
      8 +
      variantMax +
      1 +
      8 +
      hungriest(
        SCRIPT_TEXTURE.w - 2 * SCRIPT_TEXTURE.margin,
        SCRIPT_TEXTURE.h - 2 * SCRIPT_TEXTURE.margin
      );
    // a transaction's core, a cell's owner and its type's glyph (three hashes)
    const core = 8 + variantMax + 3 + 1 + 1 + 8,
      owner = 8 + variantMax + 8,
      type = 8 + variantMax;
    expect([block, script, core, owner, type]).toEqual([146, 71, 24, 19, 11]);
    for (const need of [block, script, core, owner, type]) expect(need).toBeLessThanOrEqual(256);
    // and in practice: every kind, both forms, 300 ids each, never refused for want of bits
    for (let i = 0; i < 300; i++)
      for (const id of Object.values(idsAt(i))) {
        expect(() => page(id)).not.toThrow();
        expect(() => row(id)).not.toThrow();
      }
  });
});

describe('a row seal is light enough to draw in every row', () => {
  it('keeps its markup within budget: 3 KB for a block or a script, 4 KB for a transaction or a cell', () => {
    const budget = { block: 3072, script: 3072, tx: 4096, cell: 4096 } as const;
    const worst = { block: 0, script: 0, tx: 0, cell: 0 };
    for (let i = 0; i < 300; i++)
      for (const id of Object.values(idsAt(i)))
        for (const state of ['pressed', 'ink', 'unpressed'] as const)
          worst[id.kind] = Math.max(worst[id.kind], row(id, state).svg.length);
    console.log(`row seal markup, the largest of 900 each: ${JSON.stringify(worst)} bytes`);
    for (const kind of ['block', 'script', 'tx', 'cell'] as const)
      expect(worst[kind], kind).toBeLessThanOrEqual(budget[kind]);
  });

  it('draws 300 cell row seals in under 150 ms', () => {
    const cells = Array.from({ length: 300 }, (_, i) => idsAt(i).cell);
    for (const id of cells.slice(0, 30)) row(id); // warm the code, not the ids
    const t0 = performance.now();
    for (const id of cells) drawSeal(id, { form: 'row', height: 30, state: 'pressed', ...INKS });
    const ms = performance.now() - t0;
    console.log(`300 cell row seals: ${ms.toFixed(1)} ms`);
    expect(ms).toBeLessThan(150);
  });

  it('draws byte-identical markup for the same id every time, and another for every other id', () => {
    const seen = new Set<string>();
    for (let i = 0; i < 100; i++)
      for (const id of Object.values(idsAt(i))) {
        for (const draw of [page, row]) {
          const a = draw(id).svg;
          expect(draw({ ...id } as SealId).svg).toBe(a);
          seen.add(a);
        }
      }
    expect(seen.size).toBe(100 * 4 * 2);
    // drawSeal is drawStone without what it chose
    const id = idsAt(3).cell;
    const o: SealOptions = { form: 'row', height: 30, state: 'pressed', ...INKS };
    const { chosen, ...drawn } = drawStone(id, o);
    expect(chosen).toHaveLength(4);
    expect(drawSeal(id, o)).toEqual(drawn);
  });
});
