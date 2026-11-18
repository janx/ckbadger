import { readFileSync } from 'node:fs';
import { brotliDecompressSync } from 'node:zlib';
import { expect } from 'vitest';

/**
 * The font files the site serves, read in tests (readability plan 2026-11-18, lane F3): the
 * tables of a WOFF2 file and the code points its `cmap` maps, so a metric or a character range
 * the type modules state is checked against the faces themselves.
 */

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
 * Brotli stream holding every table end to end in directory order. `head`, `hhea`, `OS/2`, `post`
 * and `cmap` are never transformed, so they are read as stored.
 */
export function woff2Tables(path: string): Map<string, Buffer> {
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

/**
 * Every code point a face's `cmap` maps to a glyph (OpenType `cmap`, formats 4 and 12, from all
 * of its subtables); a format the faces do not use is refused.
 */
export function cmapCodePoints(tables: Map<string, Buffer>): Set<number> {
  const c = tables.get('cmap');
  if (!c) throw new Error('woff2: no cmap table');
  const points = new Set<number>();
  const count = c.readUInt16BE(2);
  for (let i = 0; i < count; i++) {
    const at = c.readUInt32BE(8 + i * 8);
    const format = c.readUInt16BE(at);
    if (format === 4) {
      const segX2 = c.readUInt16BE(at + 6);
      const ends = at + 14;
      const starts = ends + segX2 + 2;
      const deltas = starts + segX2;
      const offsets = deltas + segX2;
      for (let s = 0; s < segX2 / 2; s++) {
        const end = c.readUInt16BE(ends + 2 * s);
        const start = c.readUInt16BE(starts + 2 * s);
        const delta = c.readInt16BE(deltas + 2 * s);
        const offset = c.readUInt16BE(offsets + 2 * s);
        for (let cp = start; cp <= end && cp !== 0xffff; cp++) {
          let glyph: number;
          if (offset === 0) glyph = (cp + delta) & 0xffff;
          else {
            glyph = c.readUInt16BE(offsets + 2 * s + offset + 2 * (cp - start));
            if (glyph !== 0) glyph = (glyph + delta) & 0xffff;
          }
          if (glyph !== 0) points.add(cp);
        }
      }
    } else if (format === 12) {
      const groups = c.readUInt32BE(at + 12);
      for (let g = 0; g < groups; g++) {
        const start = c.readUInt32BE(at + 16 + g * 12);
        const end = c.readUInt32BE(at + 20 + g * 12);
        for (let cp = start; cp <= end; cp++) points.add(cp);
      }
    } else throw new Error(`woff2: cmap subtable format ${format} is not read`);
  }
  return points;
}
