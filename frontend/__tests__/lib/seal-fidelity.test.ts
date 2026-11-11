// @vitest-environment node
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { describe, expect, it } from 'vitest';
import type { Pt } from '@/lib/seal/brush';
import { RIM_LABEL, RIMS, RINGS } from '@/lib/seal/core';
import {
  ANIMALS,
  ELEMENTS,
  GLYPH,
  GLYPH_LABEL,
  GLYPHS,
  NOT_ORACLE,
  SIGNS,
} from '@/lib/seal/glyphs';
import {
  CAPACITY_DIGITS,
  drawStone,
  sealLabel,
  type CellClass,
  type SealId,
  type SealOptions,
  type SealState,
} from '@/lib/seal/stones';
import { TEXTURE, TEXTURES } from '@/lib/seal/textures';
import { WINDOW_CLASSES } from '@/lib/seal/window';
import { decode, distToPolyline } from '@/__tests__/utils/compact-path';

/**
 * The port against the prototype. `fixtures/seal-prototype/` holds verbatim copies of the design
 * prototype (docs/superpowers/design/2026-11-10-rubbing-song/src/{kinds5,brush,seal2,seal3}.js,
 * gitignored at its source): `kinds5.js` is the generator the round 16 and 17 boards drew with
 * (`seal(kind, id, size, o)`, `size` the height in px). A page seal must be the prototype's byte
 * for byte (but for its label, which the port words itself); a row seal must draw the same plan,
 * byte for byte in the prototype's encoding and within the compact encoding's tolerance in its own.
 */

type ProtoStroke = { pts: number[][]; brush?: object; weight?: number };
interface ProtoSealMeta {
  svg: string;
  chosen: string[];
  pxW: number;
  pxH: number;
}
interface Prototype {
  seal(kind: string, id: unknown, size: number, o: Record<string, unknown>): ProtoSealMeta;
  GLYPH: Record<string, { bits: number; make(b: number[]): ProtoStroke[] }>;
  GLYPHS: string[];
  ELEMENTS: string[];
  ANIMALS: string[];
  SIGNS: string[];
  GLYPH_LABEL: Record<string, [string, string]>;
  NOT_ORACLE: string[];
  TEXTURES: string[];
  RINGS: string[];
  RIMS: string[];
  RIM_LABEL: Record<string, [string, string]>;
  CLASSES: string[];
  GAUGE: number;
}
interface ProtoBrush {
  REGION: Record<
    string,
    {
      bits(w: number, h: number): number;
      make(b: number[], w: number, h: number): ProtoStroke[];
      brush: object;
    }
  >;
}

const req = createRequire(import.meta.url);
const proto = req('../fixtures/seal-prototype/kinds5.js') as Prototype;
const protoBrush = req('../fixtures/seal-prototype/brush.js') as ProtoBrush;

// The board's own inks (sealworld.js passes d.cinnabar, d.bg, d.dim); here only as test data.
const PAINT = { ink: '#c2452d', ground: '#15120f', dead: '#8a8073' };
const STATES: SealState[] = ['pressed', 'ink', 'unpressed'];
const CLASSES: CellClass[] = ['dao', 'tokens', 'objects', 'identities', 'other'];
const SHANNONS = BigInt(100000000);

const sha = (seed: string) => '0x' + createHash('sha256').update(seed).digest('hex');
/** 60 hashes of each role from fixed seeds. */
const H = (role: string, n = 60) =>
  Array.from({ length: n }, (_, i) => sha(`seal kinds ${role} ${i}`));
const TX = H('tx'),
  LOCK = H('lock'),
  TYPE = H('type'),
  BLOCK = H('block'),
  SCRIPT = H('script');

const TXS = [1, 7, 12, 248, 1820, 3, 9999, 0, 10, 100];
const COUNTS = [0, 1, 2, 3, 9, 12, 240, 1000, 9999, 5, 6, 8];
const INDICES = [0, 1, 2, 12, 305, 9999, 7, 40, 6, 1000];
const CKB = [61, 142, 1204, 10000, 1200000, 33600000000, 99999999999, 203, 318, 506];

/** The spread of ids: every kind, every class and none, counts of one to four digits. */
function ids(i: number): SealId[] {
  const cls = i % 6 === 5 ? null : CLASSES[i % 5];
  return [
    { kind: 'block', hash: BLOCK[i], txs: TXS[i % TXS.length] },
    {
      kind: 'tx',
      hash: TX[i],
      consumed: COUNTS[i % COUNTS.length],
      created: COUNTS[(i * 7 + 3) % COUNTS.length],
    },
    {
      kind: 'cell',
      tx: TX[i],
      index: INDICES[i % INDICES.length],
      lock: LOCK[i],
      type: cls ? { class: cls, hash: TYPE[i] } : null,
      capacity: BigInt(CKB[i % CKB.length]) * SHANNONS,
    },
    { kind: 'script', hash: SCRIPT[i] },
  ];
}

/** The prototype's id for one of the port's. */
function protoId(id: SealId): unknown {
  switch (id.kind) {
    case 'block':
      return { hash: id.hash, txs: id.txs };
    case 'tx':
      return { hash: id.hash, inputs: id.consumed, outputs: id.created };
    case 'cell':
      if (id.capacity % SHANNONS !== BigInt(0)) throw new Error('the prototype takes whole CKB');
      return {
        tx: id.tx,
        index: id.index,
        lock: id.lock,
        type: id.type ? { class: id.type.class, hash: id.type.hash } : { class: 'ckb', hash: null },
        capacity: Number(id.capacity / SHANNONS),
      };
    case 'script':
      return id.hash;
  }
}

const HEIGHT = { block: 104, tx: 94, cell: 112, script: 104 } as const;
const ROW_HEIGHT = { block: 26, tx: 24, cell: 30, script: 28 } as const;

const prototypeSeal = (id: SealId, o: SealOptions): ProtoSealMeta =>
  proto.seal(id.kind, protoId(id), o.height, {
    form: o.form,
    state: o.state,
    ink: o.ink,
    ground: o.ground,
    dead: o.dead,
    tilt: o.tilt ?? 0,
    seamPx: o.seamPx,
    meta: true,
  });

/** The markup with its label taken out: the one place the port words itself. */
const unlabel = (svg: string) => svg.replace(/ aria-label="[^"]*"/, ' aria-label=""');
const labelOf = (svg: string) => /aria-label="([^"]*)"/.exec(svg)?.[1];

describe('the glyphs, textures, rims, borders and classes are the prototype’s', () => {
  it('has the forty glyphs in the prototype’s order, named as it names them', () => {
    expect([...GLYPHS]).toEqual(proto.GLYPHS);
    expect([...ELEMENTS]).toEqual(proto.ELEMENTS);
    expect([...ANIMALS]).toEqual(proto.ANIMALS);
    expect([...SIGNS]).toEqual(proto.SIGNS);
    expect(GLYPH_LABEL).toEqual(proto.GLYPH_LABEL);
    expect([...NOT_ORACLE]).toEqual(proto.NOT_ORACLE);
  });

  it('draws every glyph in every variant with the prototype’s strokes', () => {
    for (const name of GLYPHS) {
      expect(GLYPH[name].bits, name).toBe(proto.GLYPH[name].bits);
      const n = GLYPH[name].bits;
      for (let v = 0; v < 1 << n; v++) {
        const b = Array.from({ length: n }, (_, i) => (v >> (n - 1 - i)) & 1);
        expect(GLYPH[name].make(b), `${name} ${b}`).toEqual(proto.GLYPH[name].make(b));
      }
    }
  });

  it('keeps the eight textures, rims, borders, classes and the eleven-digit gauge', () => {
    expect([...TEXTURES]).toEqual(proto.TEXTURES);
    expect([...RIMS]).toEqual(proto.RIMS);
    expect(RIM_LABEL).toEqual(proto.RIM_LABEL);
    expect([...RINGS]).toEqual(proto.RINGS);
    expect([...WINDOW_CLASSES]).toEqual(proto.CLASSES);
    expect(CAPACITY_DIGITS).toBe(proto.GAUGE);
  });

  it('fills a box with each texture exactly as the prototype’s REGION does', () => {
    for (const name of TEXTURES)
      for (const [w, h] of [
        [4, 2],
        [4, 3],
        [4, 4],
        [4, 6],
        [5, 3],
      ]) {
        const n = TEXTURE[name].bits(w, h);
        expect(n, `${name} ${w}×${h}`).toBe(protoBrush.REGION[name].bits(w, h));
        for (const seed of [0, 1, 2]) {
          const b = Array.from({ length: n }, (_, i) => ((i * 7 + seed * 3) % 5 < 2 ? 1 : 0));
          expect(TEXTURE[name].make(b, w, h), `${name} ${w}×${h}`).toEqual(
            protoBrush.REGION[name].make(b, w, h)
          );
        }
        expect(TEXTURE[name].brush).toEqual(protoBrush.REGION[name].brush);
      }
  });
});

describe('a page seal is the prototype’s, byte for byte', () => {
  for (const kind of ['block', 'tx', 'cell', 'script'] as const)
    it(`${kind}: 60 ids, three states, the same markup, plan and size`, () => {
      for (let i = 0; i < 60; i++) {
        const id = ids(i).find((x) => x.kind === kind) as SealId;
        for (const state of i < 20 ? STATES : (['pressed'] as SealState[])) {
          const o: SealOptions = { form: 'page', height: HEIGHT[kind], state, ...PAINT };
          const want = prototypeSeal(id, o);
          const got = drawStone(id, o);
          expect(unlabel(got.svg), `${kind} ${i} ${state}`).toBe(unlabel(want.svg));
          expect(labelOf(got.svg)).toBe(sealLabel(id));
          expect(got.chosen).toEqual(want.chosen);
          expect([got.width, got.height]).toEqual([want.pxW, want.pxH]);
        }
      }
    });

  it('covers every glyph, rim, border and texture in that spread', () => {
    const seen = new Set<string>();
    for (let i = 0; i < 60; i++)
      for (const id of ids(i))
        for (const c of drawStone(id, {
          form: 'page',
          height: HEIGHT[id.kind],
          state: 'pressed',
          ...PAINT,
        }).chosen)
          seen.add(c);
    for (const name of [...GLYPHS, ...RIMS, ...RINGS, ...TEXTURES])
      expect(seen, name).toContain(name);
  });

  it('is pressed across a seam and turned as the prototype presses and turns it', () => {
    for (let i = 0; i < 6; i++)
      for (const id of ids(i)) {
        const o: SealOptions = {
          form: 'page',
          height: 90,
          state: 'pressed',
          ...PAINT,
          tilt: -4,
          seamPx: 3,
        };
        expect(unlabel(drawStone(id, o).svg)).toBe(unlabel(prototypeSeal(id, o).svg));
      }
  });

  it('draws with palette references as with hex colours', () => {
    const o: SealOptions = {
      form: 'page',
      height: 112,
      state: 'unpressed',
      ink: 'var(--c-mark)',
      ground: 'var(--c-base-bg)',
      dead: 'var(--c-text-dim)',
    };
    for (const id of ids(3))
      expect(unlabel(drawStone(id, o).svg)).toBe(unlabel(prototypeSeal(id, o).svg));
  });
});

/* ---------- the row seal ---------- */

interface El {
  tag: string;
  attrs: Record<string, string>;
}

/**
 * The drawn elements of a seal in order, a group's fill given to what it holds (the compact
 * encoding writes a run of strokes as one filled group).
 */
function drawn(svg: string): El[] {
  const out: El[] = [];
  const fills: (string | undefined)[] = [];
  for (const [, close, tag, rest] of svg.matchAll(/<(\/?)(\w+)([^>]*)>/g)) {
    if (tag === 'svg') continue;
    if (tag === 'g') {
      if (close) fills.pop();
      else fills.push(/fill="([^"]*)"/.exec(rest)?.[1]);
      continue;
    }
    const attrs: Record<string, string> = {};
    for (const [, k, v] of rest.matchAll(/([\w-]+)="([^"]*)"/g)) attrs[k] = v;
    const fill = fills[fills.length - 1];
    if (attrs.fill === undefined && fill !== undefined) attrs.fill = fill;
    out.push({ tag, attrs });
  }
  return out;
}

const pointsOf = (d: string): Pt[] => {
  const nums = (d.match(/-?\d+(\.\d+)?/g) ?? []).map(Number);
  const pts: Pt[] = [];
  for (let k = 0; k < nums.length; k += 2) pts.push([nums[k], nums[k + 1]]);
  return pts;
};

/**
 * The compact row seal against the same seal in the prototype's encoding: the same elements in
 * the same order with the same paints, every outline point of the prototype's within `tol` units
 * of the compact outline, every cap and circle within the two roundings.
 */
function expectWithin(compact: string, full: string, tol: number) {
  const a = drawn(compact),
    b = drawn(full);
  expect(a.map((e) => e.tag)).toEqual(b.map((e) => e.tag));
  a.forEach((e, i) => {
    const f = b[i];
    const rest = (x: El, drop: string[]) =>
      Object.fromEntries(Object.entries(x.attrs).filter(([k]) => !drop.includes(k)));
    if (e.tag === 'path') {
      expect(rest(e, ['d'])).toEqual(rest(f, ['d']));
      if (e.attrs.d === f.attrs.d) return; /* a rule written alike in both */
      const line = decode(e.attrs.d);
      line.push(line[0]); /* closed */
      for (const p of pointsOf(f.attrs.d)) expect(distToPolyline(p, line)).toBeLessThanOrEqual(tol);
    } else if (e.tag === 'circle') {
      expect(rest(e, ['cx', 'cy', 'r'])).toEqual(rest(f, ['cx', 'cy', 'r']));
      for (const k of ['cx', 'cy', 'r'])
        expect(Math.abs(Number(e.attrs[k]) - Number(f.attrs[k]))).toBeLessThanOrEqual(0.1501);
    } else expect(e.attrs).toEqual(f.attrs);
  });
}

describe('a row seal draws the prototype’s plan', () => {
  for (const kind of ['block', 'tx', 'cell', 'script'] as const)
    it(`${kind}: in the prototype’s encoding byte for byte; compact within 0.2 px`, () => {
      for (let i = 0; i < 60; i++) {
        const id = ids(i).find((x) => x.kind === kind) as SealId;
        for (const state of i < 20 ? STATES : (['pressed'] as SealState[])) {
          const o: SealOptions = { form: 'row', height: ROW_HEIGHT[kind], state, ...PAINT };
          const want = prototypeSeal(id, o);
          const full = drawStone(id, o, 'prototype');
          expect(unlabel(full.svg), `${kind} ${i} ${state}`).toBe(unlabel(want.svg));
          expect(full.chosen).toEqual(want.chosen);
          const compact = drawStone(id, o);
          expect(compact.chosen).toEqual(want.chosen);
          expect([compact.width, compact.height]).toEqual([want.pxW, want.pxH]);
          // 0.2 px in units at this size; the prototype cuts its outlines to one decimal (0.1);
          // the compact encoding rounds to tenths (half a tenth each way: 0.0708 diagonally)
          const units =
            0.2 / (o.height / Number(/viewBox="0 0 [\d.]+ ([\d.]+)"/.exec(want.svg)?.[1]));
          expectWithin(compact.svg, full.svg, units + 0.1 + 0.0708 + 1e-9);
        }
      }
    });

  it('counts past nine as a full block, as the prototype does', () => {
    const block: SealId = { kind: 'block', hash: BLOCK[0], txs: 248 };
    const tx: SealId = { kind: 'tx', hash: TX[0], consumed: 12, created: 240 };
    for (const id of [block, tx]) {
      const o: SealOptions = {
        form: 'row',
        height: ROW_HEIGHT[id.kind],
        state: 'pressed',
        ...PAINT,
      };
      expect(unlabel(drawStone(id, o, 'prototype').svg)).toBe(unlabel(prototypeSeal(id, o).svg));
    }
  });
});

describe('a count that is not known leaves its end of the stone empty, as in the prototype', () => {
  it('draws a transaction with either count or both null as the prototype draws it', () => {
    for (const [consumed, created] of [
      [null, 3],
      [2, null],
      [null, null],
    ] as const)
      for (const form of ['page', 'row'] as const) {
        const id: SealId = { kind: 'tx', hash: TX[1], consumed, created };
        const o: SealOptions = {
          form,
          height: form === 'page' ? HEIGHT.tx : ROW_HEIGHT.tx,
          state: 'pressed',
          ...PAINT,
        };
        expect(unlabel(drawStone(id, o, 'prototype').svg)).toBe(unlabel(prototypeSeal(id, o).svg));
        // and fewer strokes than with both counts known
        const known = drawStone({ ...id, consumed: 2, created: 3 }, o).svg;
        expect(drawStone(id, o).svg.length).toBeLessThan(known.length);
      }
  });
});
