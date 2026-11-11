// @vitest-environment node
import { describe, expect, it } from 'vitest';
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
import { compactOutline, joinNumbers, simplify, tenths } from '@/lib/seal/encode';
import { assertPaint } from '@/lib/seal/paint';
import { walkRect } from '@/lib/seal/textures';
import { decode, distToPolyline } from '@/__tests__/utils/compact-path';

// block #20,914,858's hash, which no transaction has (the night's test hash)
const HASH = '0xbbb655aefa778c0856e702030e9f8e9f6171daa392c90b3d9abe3d69c74b386b';

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
