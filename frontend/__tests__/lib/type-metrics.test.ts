import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { brotliDecompressSync } from 'node:zlib';
import { describe, expect, it } from 'vitest';
import tailwindConfig from '@/tailwind.config';
import { TYPE_SCALE, TYPE_STEPS } from '@/lib/type-scale';
import {
  LABEL_LINE_PX,
  MONO_ADVANCE_EM,
  MONO_DESCENT_EM,
  ROW_LINE_PX,
  advancePx,
  charPx,
  columnPx,
  descentPx,
  fontPx,
  lineHeightPx,
  textPx,
  tightLinePx,
} from '@/lib/type-metrics';

/**
 * The metrics every text budget is computed from (readability plan 2026-11-18, lane F3, step 1):
 * the face's advance read from the font files the site serves, and each size and line read from
 * the type scale, so a change of the scale moves every budget with it.
 */

const FONTS = join(__dirname, '..', '..', 'public', 'fonts');

/**
 * The WOFF2 known-table tags, by the index a table directory entry's flags carry (WOFF2 §5.1),
 * through `glyf` and `loca`, whose transform the version bits mark the other way round.
 */
const KNOWN_TAGS = [
  'cmap',
  'head',
  'hhea',
  'hmtx',
  'maxp',
  'name',
  'OS/2',
  'post',
  'cvt ',
  'fpgm',
  'glyf',
  'loca',
  'prep',
  'CFF ',
  'VORG',
  'EBDT',
  'EBLC',
  'gasp',
  'hdmx',
  'kern',
  'LTSH',
  'PCLT',
  'VDMX',
  'vhea',
  'vmtx',
  'BASE',
  'GDEF',
  'GPOS',
  'GSUB',
];

/**
 * The untransformed tables of a WOFF2 file, by tag (WOFF2 §5–6): the table directory, then one
 * Brotli stream holding every table end to end in directory order. `head`, `hhea`, `OS/2` and
 * `post` are never transformed, so they are read as stored.
 */
function woff2Tables(path: string): Map<string, Buffer> {
  const b = readFileSync(path);
  expect(b.toString('latin1', 0, 4), path).toBe('wOF2');
  const numTables = b.readUInt16BE(12);
  const compressedLength = b.readUInt32BE(20);
  let p = 48;
  const base128 = () => {
    let v = 0;
    for (let i = 0; i < 5; i++) {
      const c = b[p++];
      v = v * 128 + (c & 0x7f);
      if (!(c & 0x80)) return v;
    }
    throw new Error(`${path}: a UIntBase128 longer than 5 bytes`);
  };
  const entries: { tag: string; length: number }[] = [];
  for (let i = 0; i < numTables; i++) {
    const flags = b[p++];
    let tag: string;
    if ((flags & 0x3f) === 0x3f) {
      tag = b.toString('latin1', p, p + 4);
      p += 4;
    } else tag = KNOWN_TAGS[flags & 0x3f] ?? `#${flags & 0x3f}`;
    const version = (flags >> 6) & 3;
    const origLength = base128();
    const transformed = tag === 'glyf' || tag === 'loca' ? version !== 3 : version !== 0;
    entries.push({ tag, length: transformed ? base128() : origLength });
  }
  const data = brotliDecompressSync(b.subarray(p, p + compressedLength));
  const tables = new Map<string, Buffer>();
  let offset = 0;
  for (const { tag, length } of entries) {
    tables.set(tag, data.subarray(offset, offset + length));
    offset += length;
  }
  return tables;
}

describe('the face (JetBrains Mono, public/fonts)', () => {
  const faces = readdirSync(FONTS).filter((name) => /^jetbrains-mono-.*\.woff2$/.test(name));

  it('serves the five JetBrains Mono faces the stylesheet declares', () => {
    expect(faces.sort()).toEqual([
      'jetbrains-mono-italic.woff2',
      'jetbrains-mono-light.woff2',
      'jetbrains-mono-medium.woff2',
      'jetbrains-mono-regular.woff2',
      'jetbrains-mono-semibold.woff2',
    ]);
  });

  it.each(faces)('%s advances every glyph 0.6 em: MONO_ADVANCE_EM is the font’s', (face) => {
    const t = woff2Tables(join(FONTS, face));
    const unitsPerEm = t.get('head')!.readUInt16BE(18);
    const advanceWidthMax = t.get('hhea')!.readUInt16BE(10);
    const xAvgCharWidth = t.get('OS/2')!.readInt16BE(2);
    const isFixedPitch = t.get('post')!.readUInt32BE(12);
    expect(isFixedPitch, 'post.isFixedPitch').not.toBe(0);
    expect(xAvgCharWidth, 'OS/2.xAvgCharWidth').toBe(advanceWidthMax);
    expect(advanceWidthMax / unitsPerEm).toBe(MONO_ADVANCE_EM);
    // and reaches 0.3 em under its baseline: hhea.descender
    expect(-t.get('hhea')!.readInt16BE(6) / unitsPerEm).toBe(MONO_DESCENT_EM);
  });

  it('gives a line’s descent at a size: 0.3 em', () => {
    expect([10, 12, 14].map(descentPx)).toEqual([3, 3.6, 4.2]);
  });
});

describe('sizes and advances', () => {
  it('reads every step’s size from the type scale', () => {
    for (const step of TYPE_STEPS) expect(fontPx(step), step).toBe(parseFloat(TYPE_SCALE[step][0]));
    expect([fontPx('meta'), fontPx('small'), fontPx('body')]).toEqual([12, 13, 14]);
  });

  it('gives a glyph’s advance at a size as the nearest double of 0.6 em', () => {
    // 0.6 × 12 is 7.199999999999999 in binary floating point; 600 × 12 / 1000 is 7.2
    expect([10, 10.5, 11, 11.5, 12, 12.5, 13].map(advancePx)).toEqual([
      6, 6.3, 6.6, 6.9, 7.2, 7.5, 7.8,
    ]);
  });

  it('gives a step’s advance as the advance at its size', () => {
    for (const step of TYPE_STEPS) expect(charPx(step), step).toBe(advancePx(fontPx(step)));
    expect([charPx('meta'), charPx('small'), charPx('body')]).toEqual([7.2, 7.8, 8.4]);
  });

  it('gives a run of glyphs as one product, exact for a whole count', () => {
    expect(textPx(13, 12)).toBe(93.6); // "3,500,000,000" at text-small (13 × 7.2 adds to 93.60000000000001)
    expect(textPx(11, 13)).toBe(85.8);
    expect(textPx(34, 13)).toBe(265.2);
    expect(textPx(0, 11)).toBe(0);
    for (let n = 1; n <= 80; n++) expect(textPx(n, 12)).toBeCloseTo(n * charPx('meta'), 10);
  });
});

describe('lines', () => {
  it('reads every step’s line from the type scale', () => {
    for (const step of TYPE_STEPS)
      expect(lineHeightPx(step), step).toBeCloseTo(
        fontPx(step) * parseFloat(TYPE_SCALE[step][1]),
        10
      );
    expect([lineHeightPx('meta'), lineHeightPx('small'), lineHeightPx('body')]).toEqual([
      18, 19.5, 22.4,
    ]);
  });

  it('stands a list row on the small step’s line, in whole pixels: 20 px', () => {
    expect(ROW_LINE_PX).toBe(Math.ceil(lineHeightPx('small')));
    expect(Number.isInteger(ROW_LINE_PX)).toBe(true);
    expect(ROW_LINE_PX).toBe(20); // 13 × 1.5 = 19.5, up to a whole pixel
  });

  it('sets a line tight with 3 px of leading, and a label at the meta step so: 15 px', () => {
    expect([11, 12, 13].map(tightLinePx)).toEqual([14, 15, 16]);
    expect(LABEL_LINE_PX).toBe(tightLinePx(fontPx('meta')));
    expect(LABEL_LINE_PX).toBe(15);
  });

  it('emits both lines as Tailwind line heights: leading-row and leading-label', () => {
    const theme = tailwindConfig.theme as { extend: { lineHeight?: Record<string, string> } };
    expect(theme.extend.lineHeight).toEqual({
      row: `${ROW_LINE_PX}px`,
      label: `${LABEL_LINE_PX}px`,
    });
  });
});

describe('column budgets', () => {
  it('rounds what a column holds up to a whole pixel and adds its spare', () => {
    expect(columnPx(textPx(13, 12))).toBe(94); // 93.6
    expect(columnPx(textPx(20, 12) + 16)).toBe(160); // exactly 160 stays 160
    expect(columnPx(56.5 + textPx(17, 13) + 16, 2)).toBe(208); // 205.1 → 206, and 2 spare
    expect(columnPx(160.00000000000003)).toBe(160); // a sum a hair over a whole pixel is that pixel
  });
});
