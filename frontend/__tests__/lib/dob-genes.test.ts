import {
  alleleLanes,
  geneOrder,
  genotypePoints,
  isReadableTrait,
  sequencingGenes,
  traitDisplay,
} from '@/lib/dob-genes';
import { BITCOIN_COLORS, GENE_COLORS } from '@/lib/design-tokens';
import { nervape3xGenetics, nervape3xInsights } from '@/__tests__/fixtures/objects/nervape3x';

describe('dob-genes (F2 objects, OB-4/5/7/8, OB-C2)', () => {
  it('orders genes by the DOB pattern (DNA order), not by the API trait list (OB-C2)', () => {
    // The API lists traits by name; the gel must follow the pattern's own gene order.
    expect(nervape3xInsights.genetics.traits.map((t) => t.name)).toEqual([
      'Color',
      'Note',
      'Serial',
      'prev.bg',
      'prev.type',
      'relief',
    ]);
    expect(geneOrder(nervape3xInsights.currentPattern)).toEqual([
      'prev.type',
      'relief',
      'prev.bg',
      'Color',
      'Serial',
      'Note',
    ]);
    expect(geneOrder({ status: 'notDob', steps: [] })).toBeNull();
  });

  it('gives each byte to its widest gene and colours genes copper / cyan / violet in DNA order (OB-7)', () => {
    const { genes, owners, bytes } = sequencingGenes(nervape3xGenetics, 'Nervape 3x');
    expect(bytes).toHaveLength(53);
    const byName = Object.fromEntries(genes.map((g) => [g.name, g]));
    // prev.type and relief read byte 0, which the 32-byte Bitcoin pointer owns.
    expect(byName['prev.type'].owns).toBe(false);
    expect(byName.relief.owns).toBe(false);
    expect(byName['prev.bg'].owns).toBe(true);
    expect(owners[0]).toBe(byName['prev.bg'].key);
    expect(owners[32]).toBe(byName.Color.key);
    expect(owners[52]).toBe(byName.Note.key);
    // The Bitcoin pointer is copper (Bitcoin = copper site-wide); then cyan, violet, slate.
    expect(byName['prev.bg'].color).toBe(BITCOIN_COLORS.copper);
    expect(byName.Color.color).toBe(GENE_COLORS.cycle[0]);
    expect(byName.Serial.color).toBe(GENE_COLORS.cycle[1]);
    expect(byName.Note.color).toBe(GENE_COLORS.cycle[2]);
    expect(byName.relief.color).toBe(GENE_COLORS.shared);
    // Board wording: "bytes 0–31 · btcfs", "1 option"; the raw bytes, then the trait.
    expect(byName['prev.bg'].rule).toBe('btcfs');
    expect(byName.relief.rule).toBe('1 option');
    expect(byName.Color.raw).toBe('03');
    expect(byName.Color.value).toBe('Nature Green');
    expect(byName['prev.bg'].value).toBe('Bitcoin inscription …79e0i0');
    expect(byName.Note.value).toBe('empty');
  });

  it('writes a trait value for people: empty, Bitcoin pointers and SVG source are named (OB-8, OB-16)', () => {
    expect(traitDisplay('')).toBe('empty');
    expect(traitDisplay('\u0000\u0002\u00003X0125CLNVAP014')).toBe('3X0125CLNVAP014');
    expect(traitDisplay('<svg xmlns="http://www.w3.org/2000/svg"></svg>')).toBe(
      'SVG image · 46 characters'
    );
    expect(isReadableTrait('Color', 'Nature Green')).toBe(true);
    expect(isReadableTrait('Note', '')).toBe(true);
    expect(isReadableTrait('prev.bg', 'btcfs://aa')).toBe(false);
    expect(isReadableTrait('IMAGE', "<svg xmlns='http://www.w3.org/2000/svg'/>")).toBe(false);
  });

  it('stacks alleles common to rare, dims conserved genes and draws colour alleles in their colour', () => {
    const order = geneOrder(nervape3xInsights.currentPattern)!;
    const lanes = alleleLanes(nervape3xInsights.genetics, order, 'Nervape 3x');
    expect(lanes.map((l) => l.gene)).toEqual(order);
    const color = lanes.find((l) => l.gene === 'Color')!;
    expect(color.variants).toBe(8);
    expect(color.bands[0]).toMatchObject({ value: 'Water Veins', count: 20 });
    expect(color.bands.reduce((s, b) => s + b.share, 0)).toBeCloseTo(1, 10);
    expect(lanes.find((l) => l.gene === 'relief')!.conserved).toBe(true);
    expect(lanes.find((l) => l.gene === 'Serial')!.variants).toBe(73);
    const hex = alleleLanes(
      {
        ...nervape3xInsights.genetics,
        traits: [
          {
            name: 'prev.bgcolor',
            values: [
              { value: '#AFE7F9', count: 60 },
              { value: '#FFFFFF', count: 30 },
            ],
          },
        ],
      },
      ['prev.bgcolor'],
      'x'
    );
    expect(hex[0].bands[0].colour).toBe('#AFE7F9');
    expect(hex[0].bands[0].isColour).toBe(true);
  });

  it('places an object on every lane by its own values (genotype overlay, OB-4)', () => {
    const order = geneOrder(nervape3xInsights.currentPattern)!;
    const lanes = alleleLanes(nervape3xInsights.genetics, order, 'Nervape 3x');
    const { points, absent } = genotypePoints(lanes, nervape3xGenetics.cachedVmTraits.traits);
    expect(points).toHaveLength(6);
    expect(absent).toEqual([]);
    const missing = genotypePoints(lanes, [{ name: 'Color', value: 'Not a colour' }]);
    expect(missing.absent).toEqual(['Color']);
  });

  it('fails on a gene map that disagrees with the DNA', () => {
    const broken = structuredClone(nervape3xGenetics);
    if (broken.currentPattern.status !== 'dob') throw new Error('fixture');
    broken.currentPattern.steps[0].bytes[3].rawHex = 'ff';
    expect(() => sequencingGenes(broken, 'Nervape 3x')).toThrow(
      'Nervape 3x: gene Color bytes disagree with DNA'
    );
  });
});
