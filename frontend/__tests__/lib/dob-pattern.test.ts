import { describe, expect, it } from 'vitest';
import type { CollectionInsights, GeneDefinition } from '@/lib/api';
import { dnaRulerParts, foundAcross, foundAcrossRun, geneRows, ruleWords } from '@/lib/dob-pattern';

const gene = (
  index: number,
  name: string,
  offset: number,
  length: number,
  rule = 'options',
  ruleArguments: unknown = ['0', '1']
): GeneDefinition => ({
  index,
  traitIndex: index,
  valueIndex: 0,
  name,
  typeTag: 'String',
  byteOffset: String(offset),
  byteLength: String(length),
  rule,
  ruleArguments,
});

// Nervape Headwear (Main board): prev.type reads byte 0, inside prev.bg's 74 bytes.
const NERVAPE = [
  gene(0, 'prev.type', 0, 1, 'options', ['image']),
  gene(1, 'prev.bg', 0, 74, 'utf8', []),
  gene(2, 'prev.view', 74, 1, 'range', [0, 255]),
  gene(3, 'Entity', 75, 8, 'raw', null),
];

const genetics = (traits: { name: string; values: [string, number][] }[]) =>
  ({
    liveItems: 1298,
    eligibleItems: 1298,
    nonDobItems: 0,
    decodedItems: 1298,
    pendingItems: 0,
    failedItems: 0,
    complete: true,
    traits: traits.map((t) => ({
      name: t.name,
      values: t.values.map(([value, count]) => ({ value, count })),
    })),
  }) satisfies CollectionInsights['genetics'];

describe('the DNA ruler (lane E, C5)', () => {
  it('gives each byte to the widest gene reading it and notes the genes left inside others', () => {
    const { parts, inside } = dnaRulerParts(
      NERVAPE.map((g) => ({ key: `0:${g.index}`, gene: g })),
      83
    );
    expect(parts.map((p) => [p.key, p.start, p.end])).toEqual([
      ['0:1', 0, 74],
      ['0:2', 74, 75],
      ['0:3', 75, 83],
    ]);
    expect(inside).toEqual([{ key: '0:0', name: 'prev.type', start: 0, end: 1 }]);
    // one segment per pattern entry, counting the ones that read bytes another owns
    expect(parts.length + inside.length).toBe(NERVAPE.length);
  });

  it('marks bytes no gene reads and clips ranges to the DNA it is drawn over', () => {
    const { parts } = dnaRulerParts(
      [
        { key: 'a', gene: gene(0, 'a', 0, 2) },
        { key: 'b', gene: gene(1, 'b', 4, 10) },
      ],
      8
    );
    expect(parts.map((p) => [p.key, p.start, p.end])).toEqual([
      ['a', 0, 2],
      [null, 2, 4],
      ['b', 4, 8],
    ]);
  });
});

describe('gene rows', () => {
  it('folds a run of numbered one-byte genes with one rule into one row', () => {
    const genes = [
      gene(0, 'name', 0, 40, 'utf8', []),
      ...Array.from({ length: 66 }, (_, i) => gene(i + 1, `_m${i}`, 64 + i, 1)),
      gene(67, 'palette', 63, 1, 'options', ['lime', 'amber']),
    ];
    const rows = geneRows(genes.map((g) => ({ key: `0:${g.index}`, gene: g })));
    expect(rows.map((r) => r.label)).toEqual(['name', '_m0 – _m65', 'palette']);
    expect(rows[1].genes).toHaveLength(66);
    expect(rows[1].span).toBe('bytes 64–129 · 66 × 1');
    expect(rows[0].span).toBe('bytes 0–39 · 40');
  });

  it('keeps genes apart when their rules or bytes differ', () => {
    const rows = geneRows(
      [gene(0, '_m0', 0, 1), gene(1, '_m1', 1, 1, 'options', ['a', 'b']), gene(2, '_m2', 3, 1)].map(
        (g) => ({ key: String(g.index), gene: g })
      )
    );
    expect(rows.map((r) => r.label)).toEqual(['_m0', '_m1', '_m2']);
  });

  it('words a rule with its domain', () => {
    expect(ruleWords(NERVAPE[0])).toBe('options · image');
    expect(ruleWords(gene(0, 'p', 0, 1, 'options', ['lime', 'lime', 'amber', 'mint']))).toBe(
      'options · lime, lime, amber, …'
    );
    expect(ruleWords(NERVAPE[1])).toBe('utf8 · String');
    expect(ruleWords(NERVAPE[2])).toBe('range 0 – 255');
  });
});

describe('found across N', () => {
  it('says what the decoded objects show for a gene', () => {
    const g = genetics([
      { name: 'prev.type', values: [['image', 1298]] },
      {
        name: 'prev.view',
        values: [
          ['0', 1139],
          ['1', 159],
        ],
      },
      {
        name: 'Entity',
        values: Array.from({ length: 1298 }, (_, i) => [String(i), 1] as [string, number]),
      },
      {
        name: 'prev.bg',
        values: Array.from(
          { length: 52 },
          (_, i) => [`btcfs://x${i}`, i ? 25 : 23] as [string, number]
        ),
      },
    ]);
    expect(foundAcross('prev.type', g)).toBe('image, for all');
    expect(foundAcross('prev.view', g)).toBe('0 for 1,139 · 1 for 159');
    expect(foundAcross('Entity', g)).toBe('unique to each object');
    expect(foundAcross('prev.bg', g)).toBe('52 values');
    expect(foundAcross('missing', g)).toBe('not among the decoded traits');
  });

  it('sums up a folded run by its one reading, or by how many values each gene takes', () => {
    const g = genetics([
      {
        name: '_m0',
        values: [
          ['1', 4],
          ['0', 2],
        ],
      },
      { name: '_m1', values: [['0', 6]] },
      {
        name: '_m2',
        values: [
          ['1', 3],
          ['0', 3],
        ],
      },
    ]);
    expect(foundAcrossRun(['_m0', '_m1', '_m2'], { ...g, eligibleItems: 6 })).toBe(
      '1–2 values each'
    );
    expect(foundAcrossRun(['_m1', '_m1'], { ...g, eligibleItems: 6 })).toBe('0, for all');
  });

  it('waits for complete decoding before counting', () => {
    expect(foundAcross('prev.type', { ...genetics([]), complete: false })).toBe(
      'awaiting decoding'
    );
  });
});
