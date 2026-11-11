// @vitest-environment node
import { createRequire } from 'node:module';
import { describe, expect, it } from 'vitest';
import { compound } from '@/lib/talisman/compound';
import { plan } from '@/lib/talisman/plan';
import { BRUSH, PAGE, ROW, type DrawnRecipe } from '@/lib/talisman/recipes';

/**
 * The port against the prototype. `fixtures/talisman-prototype/` holds verbatim copies of the
 * design prototype (docs/superpowers/design/2026-11-10-rubbing-song/src/{brush,seal2,seal3}.js,
 * gitignored at its source); the board that chose the seals (`boards/sealfu.js`) calls
 * `compound(hash, size, { layout: 'talisman2', tal, ...cfg })` with the recipes below.
 */

interface ProtoSeal {
  svg: string;
  bitsUsed: number;
  chosen: string[];
}
interface Prototype {
  compound(hex: string, size: number, o: Record<string, unknown>): ProtoSeal;
}

const proto = createRequire(import.meta.url)(
  '../fixtures/talisman-prototype/brush.js'
) as Prototype;

/** The board's three reference hashes (sealfu.js HASHES[0..2]). */
const HASHES = [
  '0x9c4e1b7a2d0f83e6b5a74c1d2e9f0a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f',
  '0xcf78a19e4d0b3c62e5d2a7f49b1c6e3d8a0f5b2c7d4e9a1f6c3b8d5e2a7f4c91',
  '0x5e17d2a9c3f04b86a3f17c4e9b2d6085c7e1f3a9d4b8062e5c1a7f3d9e6b2c48',
];

/** The board's recipes, as the prototype takes them (sealfu.js P[2], P[5]). */
const PROTO: Record<'page' | 'brush', Record<string, unknown>> = {
  page: {
    tal: { head: 'dots3', foot: 'doubleKnot', core: true, bands: [5, 5], bandH: [2, 2, 3, 2, 2] },
    style: 'zhu',
    framew: 2,
    pad: 1.3,
    ratio: 0.58,
    w: 0.82,
  },
  brush: {
    tal: { head: 'bars3', foot: 'knotSword', bands: [3, 3], bandH: [3, 3, 3], W: 6, H: 16 },
    style: 'ink',
    pad: 0.8,
    ratio: 0.62,
    w: 1.25,
  },
};

// The board's own inks (sealfu.js passes d.cinnabar and d.bg); here only as test data.
const PAINT = { ink: '#c2452d', ground: '#15120f' };

const prototypeSeal = (hash: string, name: 'page' | 'brush', size: number): ProtoSeal =>
  proto.compound(hash, size, {
    layout: 'talisman2',
    ink: PAINT.ink,
    ground: PAINT.ground,
    id: name,
    meta: true,
    ...PROTO[name],
  });

/** The SVG as a list of elements: tag, the non-numeric attributes, and every number in order. */
function elements(svg: string): { tag: string; text: string; nums: number[] }[] {
  return [...svg.matchAll(/<(\w+)([^>]*)>/g)].map(([, tag, attrs]) => ({
    tag,
    text: attrs.replace(/-?\d+(\.\d+)?/g, '#'),
    nums: (attrs.match(/-?\d+(\.\d+)?/g) ?? []).map(Number),
  }));
}

/** The largest coordinate difference between two SVGs of the same structure. */
function maxDeviation(a: string, b: string): number {
  const ea = elements(a),
    eb = elements(b);
  expect(ea.map((e) => e.tag)).toEqual(eb.map((e) => e.tag));
  let worst = 0;
  ea.forEach((e, i) => {
    expect(e.text, `element ${i} <${e.tag}>`).toBe(eb[i].text);
    expect(e.nums.length, `element ${i} <${e.tag}>`).toBe(eb[i].nums.length);
    e.nums.forEach((n, j) => (worst = Math.max(worst, Math.abs(n - eb[i].nums[j]))));
  });
  return worst;
}

const TOLERANCE = 0.1;

describe('the port draws what the prototype draws', () => {
  const cases: [DrawnRecipe, 'page' | 'brush', number][] = [
    [PAGE, 'page', 150],
    [BRUSH, 'brush', 150],
  ];
  for (const [recipe, name, size] of cases)
    for (const hash of HASHES)
      it(`${name} seal of ${hash.slice(0, 10)}: same figures, same bits, paths within ${TOLERANCE}`, () => {
        const want = prototypeSeal(hash, name, size);
        const got = compound(hash, recipe, size, PAINT);
        expect(got.chosen).toEqual(want.chosen);
        expect(got.bitsUsed).toBe(want.bitsUsed);
        expect(elements(got.svg).length).toBeGreaterThan(20);
        expect(maxDeviation(got.svg, want.svg)).toBeLessThanOrEqual(TOLERANCE);
      });

  it('matches the prototype byte for byte (the measured deviation is 0)', () => {
    for (const hash of HASHES) {
      expect(compound(hash, PAGE, 150, PAINT).svg).toBe(prototypeSeal(hash, 'page', 150).svg);
      expect(compound(hash, BRUSH, 150, PAINT).svg).toBe(prototypeSeal(hash, 'brush', 150).svg);
    }
  });
});

describe('the row seal is the page seal folded (it differs from design 8 by design)', () => {
  for (const hash of HASHES)
    it(`row seal of ${hash.slice(0, 10)} carries the page's head, first two figures and foot`, () => {
      const page = prototypeSeal(hash, 'page', 150);
      const row = plan(hash, ROW);
      const pagePlan = plan(hash, PAGE);
      expect(row.compartments.map((c) => c.figure)).toEqual(page.chosen.slice(0, 2));
      // the head: the page's strokes, moved half a unit right to centre them in the 7-unit stone
      expect(row.head.name).toBe('dots3');
      expect(row.head.strokes.map((s) => s.pts)).toEqual(
        pagePlan.head.strokes.map((s) => s.pts.map(([x, y]) => [x + 0.5, y]))
      );
      // the foot: the page's two knots, moved with the body's end; the sword hooked by the same bit
      expect(row.foot.name).toBe('doubleKnot');
      expect(row.foot.bit).toEqual(pagePlan.foot.bit);
      const [rk1, rk2, rSword] = row.foot.strokes;
      const [pk1, pk2, pSword] = pagePlan.foot.strokes;
      const dy = rk1.pts[0][1] - pk1.pts[0][1];
      expect(dy).toBeLessThan(0);
      for (const [r, p] of [
        [rk1, pk1],
        [rk2, pk2],
      ]) {
        expect(r.pts).toHaveLength(p.pts.length);
        r.pts.forEach(([x, y], i) => {
          expect(x).toBeCloseTo(p.pts[i][0] + 0.5, 9);
          expect(y).toBeCloseTo(p.pts[i][1] + dy, 9);
        });
      }
      expect(rSword.pts).toHaveLength(pSword.pts.length);
      expect(rSword.pts[0][0]).toBeCloseTo(pSword.pts[0][0] + 0.5, 9);
      expect(rSword.pts[0][1]).toBeCloseTo(pSword.pts[0][1] + dy, 9);
      expect(rSword.pts[1][1]).toBeCloseTo(ROW.H - 0.3, 9);
    });
});
