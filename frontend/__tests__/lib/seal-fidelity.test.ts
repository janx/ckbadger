// @vitest-environment node
import { createRequire } from 'node:module';
import { describe, expect, it } from 'vitest';
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
import { TEXTURE, TEXTURES } from '@/lib/seal/textures';
import { WINDOW_CLASSES } from '@/lib/seal/window';

/**
 * The port against the prototype. `fixtures/seal-prototype/` holds verbatim copies of the design
 * prototype (docs/superpowers/design/2026-11-10-rubbing-song/src/{kinds5,brush,seal2,seal3}.js,
 * gitignored at its source): `kinds5.js` is the generator the round 16 and 17 boards drew with
 * (`seal(kind, id, size, o)`). Its parts first: the glyphs in every variant, the textures, the
 * rims, the borders and the classes must be the prototype's own.
 */

type ProtoStroke = { pts: number[][]; brush?: object; weight?: number };
interface Prototype {
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

  it('keeps the eight textures, the rims, the borders and the classes', () => {
    expect([...TEXTURES]).toEqual(proto.TEXTURES);
    expect([...RIMS]).toEqual(proto.RIMS);
    expect(RIM_LABEL).toEqual(proto.RIM_LABEL);
    expect([...RINGS]).toEqual(proto.RINGS);
    expect([...WINDOW_CLASSES]).toEqual(proto.CLASSES);
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
