import { describe, expect, it } from 'vitest';
import {
  asciiPreview,
  bytePieces,
  fieldShape,
  hexWords,
  moleculeTableHeader,
  scaleLayout,
  scaleRulerTicks,
} from '@/lib/byte-map';
import { hexBytes } from '@/lib/hex-bytes';

const bytesOf = (hex: string) => hexBytes(hex, 'test');
const text = (s: string) =>
  Array.from(new TextEncoder().encode(s), (b) => b.toString(16).padStart(2, '0'));
const u32 = (n: number) =>
  [n & 255, (n >> 8) & 255, (n >> 16) & 255, (n >> 24) & 255].map((b) =>
    b.toString(16).padStart(2, '0')
  );

describe('bytePieces (lane E, C5)', () => {
  it('partitions the bytes into described runs and the gaps no field describes', () => {
    expect(
      bytePieces(
        20,
        [
          { id: 'name', start: 4, end: 10 },
          { id: 'tail', start: 12, end: 20 },
        ],
        'cell'
      )
    ).toEqual([
      { start: 0, end: 4, ids: [] },
      { start: 4, end: 10, ids: ['name'] },
      { start: 10, end: 12, ids: [] },
      { start: 12, end: 20, ids: ['tail'] },
    ]);
  });

  it('splits overlapping fields at their edges and keeps an empty field as a sliver at its offset', () => {
    expect(
      bytePieces(
        3,
        [
          { id: 'full', start: 0, end: 3 },
          { id: 'tail', start: 1, end: 3 },
          { id: 'none', start: 3, end: 3 },
        ],
        'cell'
      )
    ).toEqual([
      { start: 0, end: 1, ids: ['full'] },
      { start: 1, end: 3, ids: ['full', 'tail'] },
      { start: 3, end: 3, ids: ['none'] },
    ]);
  });

  it('fails fast on a range outside the bytes', () => {
    expect(() => bytePieces(4, [{ id: 'x', start: 2, end: 5 }], 'cell 0xabc:0')).toThrow(
      'cell 0xabc:0: byte range x [2, 5) outside 4 bytes'
    );
  });
});

describe('scaleLayout', () => {
  it('draws parts in proportion to their bytes, keeping a floor and fixed slivers', () => {
    const parts = scaleLayout(
      100,
      [
        { start: 0, end: 90 },
        { start: 90, end: 91 },
        { start: 91, end: 91 },
      ],
      { gap: 2, floor: 10, sliver: 4 }
    );
    expect(parts.map((p) => Math.round(p.w))).toEqual([82, 10, 4]);
    expect(parts.map((p) => Math.round(p.x))).toEqual([0, 84, 96]);
    expect(parts.at(-1)!.x + parts.at(-1)!.w).toBeCloseTo(100);
  });

  it('keeps a part as wide as its own minimum while all minimums fit, else the floor', () => {
    const spans = [
      { start: 0, end: 16, min: 21 },
      { start: 16, end: 31, min: 21 },
      { start: 31, end: 13890 },
    ];
    expect(scaleLayout(300, spans).map((p) => Math.round(p.w))).toEqual([21, 21, 254]);
    // 40 px cannot hold 21 + 21 + 3 and the gaps: every part falls back to the 3 px floor
    expect(scaleLayout(40, spans).map((p) => Math.round(p.w))).toEqual([3, 3, 30]);
  });

  it('places a ruler label at each boundary that has room, always the first and the last', () => {
    const layout = scaleLayout(
      300,
      [
        { start: 0, end: 16 },
        { start: 16, end: 31 },
        { start: 31, end: 13890 },
      ],
      { gap: 2, floor: 8, sliver: 4 }
    );
    const ticks = scaleRulerTicks(layout, [0, 16, 31, 13890], 300, (n) =>
      n.toLocaleString('en-US')
    );
    expect(ticks.map((t) => t.value)).toEqual([0, 16, 31, 13890]);
    // 0, 16 and 31 sit 10 px apart: left to right, 16 has no room after 0's label, 31 has
    expect(ticks.map((t) => t.x)).toEqual([0, 10, 20, 300]);
    expect(ticks.filter((t) => t.label).map((t) => t.label)).toEqual(['0', '31', '13,890']);
    expect(ticks.at(-1)!.anchor).toBe('end');
  });
});

describe('fieldShape', () => {
  it('reads a molecule Bytes field as its 4-byte length and the text after it', () => {
    expect(fieldShape([...u32(11), ...text('.cell deeds')])).toBe(
      'a 4-byte length (11), then 11 bytes of UTF-8 text'
    );
    expect(fieldShape([...u32(9), ...text('{"a":[1]}')])).toBe(
      'a 4-byte length (9), then 9 bytes of JSON'
    );
  });

  it('names empty fields, plain text and opaque bytes without guessing a type', () => {
    expect(fieldShape([])).toBe('empty: no bytes');
    expect(fieldShape(text('21523.bit'))).toBe('9 bytes of UTF-8 text');
    expect(fieldShape(bytesOf('0xb01b906500000000'))).toBe('8 bytes');
  });
});

describe('moleculeTableHeader', () => {
  it('recognises a header only when its total size and offsets match the bytes', () => {
    const bytes = [
      ...u32(31),
      ...u32(16),
      ...u32(20),
      ...u32(31),
      ...u32(0),
      ...u32(7),
      ...text('cluster'),
    ];
    expect(bytes).toHaveLength(31);
    expect(moleculeTableHeader(bytes, 16, [16, 20, 31])).toEqual({
      total: 31,
      offsets: [16, 20, 31],
    });
    // the offsets do not name the fields that follow
    expect(moleculeTableHeader(bytes, 16, [16, 24, 31])).toBeNull();
    // the first word is not the data's size
    expect(moleculeTableHeader(bytes.slice(0, 30), 16, [16, 20, 30])).toBeNull();
  });
});

describe('hexWords and asciiPreview', () => {
  it('groups bytes into 4-byte words and shows printable ASCII beside them', () => {
    const bytes = [...u32(11), ...text('.cell dee')];
    expect(hexWords(bytes)).toEqual(['0b000000', '2e63656c', '6c206465', '65']);
    expect(asciiPreview(bytes)).toBe('.....cell dee');
  });
});
