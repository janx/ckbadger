import { render, screen, fireEvent, within } from '@/__tests__/utils/test-utils';
import {
  ObjectGeneticsView,
  ObjectGeneticsEvidence,
  CurrentPatternEvidence,
} from '@/components/asset/genetics';
import { ItemEvidence, ItemTransactionContext } from '@/components/asset/item-evidence';
import {
  objectGeneticsProducer,
  collectionInsightsProducer,
  collectionInsightsPendingProducer,
  didProtocolProducer,
  itemTransactionsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
import type { CurrentPattern, ObjectGenetics, ItemProtocol } from '@/lib/api';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { nervape3xGenetics, nervape3xInsights } from '@/__tests__/fixtures/objects/nervape3x';
describe('Independent DOB evidence', () => {
  // the gels lay themselves out for their measured box; 896 px is the object page's DNA plate
  beforeEach(() => stubChartWidth(896));
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  it('keeps the full object number, VM provenance and actual-byte hover association', () => {
    const data: ObjectGenetics = structuredClone(objectGeneticsProducer);
    data.dnaHex = '0x0102';
    if (data.currentPattern.status !== 'dob') throw new Error('fixture');
    data.currentPattern.steps[0].bytes[0].declaredByteLength = '100';
    data.currentPattern.steps[0].bytes[0].truncated = true;
    const { container } = render(<ObjectGeneticsView evidence={data} subject="Spore" />);
    expect(screen.getByText(/18446744073709551614/)).toBeInTheDocument();
    fireEvent.focus(container.querySelector('[data-gene]')!);
    expect(container.querySelector('[data-nibble="2"]')).toHaveAttribute('opacity', '0.15');
    expect(container.querySelector('[data-nibble="0"]')).toHaveAttribute('opacity', '1');
    render(<ObjectGeneticsEvidence evidence={data} subject="Spore" />);
    expect(screen.getByText(/decoded 2023-11-15 06:13:20 UTC\+8/)).toBeInTheDocument();
    expect(screen.queryByText(/Unix second/)).not.toBeInTheDocument();
    expect(screen.getByText(/original decoder binary hash was not recorded/)).toBeInTheDocument();
    expect(screen.getByText(/Clipped to the actual DNA shown/)).toBeInTheDocument();
  });
  it('does not invent trait values or byte ranges for pending/unsupported evidence', () => {
    const data: ObjectGenetics = structuredClone(objectGeneticsProducer);
    data.cachedVmTraits = {
      ...data.cachedVmTraits,
      status: 'pending',
      decodedAt: null,
      traits: [],
    };
    if (data.currentPattern.status !== 'dob') throw new Error('fixture');
    data.currentPattern.steps[0].description = {
      status: 'unsupported',
      reason: 'Unrecognized binary',
    };
    render(<ObjectGeneticsView evidence={data} subject="Spore" />);
    expect(screen.getByText(/awaiting DOB execution/)).toBeInTheDocument();
    expect(screen.getByText(/Unrecognized binary/)).toBeInTheDocument();
    expect(screen.queryByLabelText('Cached VM traits')).not.toBeInTheDocument();
  });
  it('preserves multiple VM values for one trait name', () => {
    const data: ObjectGenetics = structuredClone(objectGeneticsProducer);
    data.cachedVmTraits.traits.push({ name: 'Face', value: 'Green' });
    const { container } = render(<ObjectGeneticsView evidence={data} subject="Spore" />);
    // F2 objects (OB-8): the gene reads its values plainly ("Cached VM result" was developer wording).
    expect(container.querySelector('[data-gene="Face"]')!.textContent).toContain('Blue · Green');
  });
  // F2 objects (OB-4): the counts are drawn as the collection's allele gel with "this one" on it.
  it('uses complete same-name/value collection counts and never presents partial frequencies', () => {
    const view = render(
      <ObjectGeneticsView
        evidence={objectGeneticsProducer}
        subject="Spore"
        collection={collectionInsightsProducer}
        collectionName="Genes"
      />
    );
    expect(view.container.querySelector('[data-allele="Blue"] title')!.textContent).toBe(
      'Face = Blue: 4 of 8 (50.00%)'
    );
    expect(view.container.querySelector('[data-overlay="me"]')).not.toBeNull();
    view.rerender(
      <ObjectGeneticsView
        evidence={objectGeneticsProducer}
        subject="Spore"
        collection={collectionInsightsPendingProducer}
        collectionName="Genes"
      />
    );
    expect(view.container.querySelector('[data-allele]')).toBeNull();
    expect(
      screen.getByText('Collection variant frequencies await complete decoding.')
    ).toBeInTheDocument();
  });
  it('rejects a byte map that disagrees with immutable DNA', () => {
    const data: ObjectGenetics = structuredClone(objectGeneticsProducer);
    data.dnaHex = '0x02';
    expect(() => render(<ObjectGeneticsView evidence={data} subject="Spore" />)).toThrow(
      'bytes disagree with DNA'
    );
  });
  it('keeps exact declared offsets in the gene table and its row title (C5)', () => {
    const pattern = structuredClone(objectGeneticsProducer.currentPattern);
    pattern.steps[0].description.genes[0].byteOffset = '18446744073709551615';
    render(<CurrentPatternEvidence pattern={pattern} />);
    const table = screen.getByRole('table', { name: 'DOB blueprint' });
    const row = within(table).getByRole('row', { name: /^Face/ });
    expect(row).toHaveTextContent('byte 18446744073709551615 · 1');
    expect(row).toHaveTextContent('options · Red, Blue');
    expect(row).toHaveAttribute(
      'title',
      'Face · byte 18446744073709551615 · 1 · options · Red, Blue'
    );
    // a range past what can be drawn: no ruler, said so
    expect(screen.queryByRole('group', { name: 'DNA byte map' })).toBeNull();
    expect(
      screen.getByText('Its ranges run past what a ruler can draw; the table gives them exactly.')
    ).toBeInTheDocument();
  });
});

// Consistency lane E (C5, Main board "How an object is read"): the decoder steps become a chain
// of three tiles, the DNA ruler, the gene table with what the decoded objects show, one real object
// read through it and the status line; "Mapping unavailable" is gone.
describe('How an object is read (C5)', () => {
  beforeEach(() => stubChartWidth(560));
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  const sample = {
    id: '0x' + 'ab'.repeat(32),
    href: '/objects/0x' + 'ab'.repeat(32),
    dnaHex: nervape3xGenetics.dnaHex,
    traits: nervape3xGenetics.cachedVmTraits.traits,
    decodeStatus: 'decoded',
  };
  it('chains each object’s DNA, the pattern and the decoder in three tiles', () => {
    render(
      <CurrentPatternEvidence
        pattern={nervape3xInsights.currentPattern}
        genetics={nervape3xInsights.genetics}
        sample={sample}
      />
    );
    const section = screen.getByRole('region', { name: 'How an object is read' });
    expect(
      within(section).getByRole('heading', { name: 'How an object is read' })
    ).toBeInTheDocument();
    expect(section).toHaveTextContent('DOB/0 · 90 of 90 decoded');
    const tiles = within(section).getAllByRole('listitem');
    expect(tiles.map((t) => t.querySelector('b')?.textContent)).toEqual([
      'Each object',
      'The pattern',
      'The decoder',
    ]);
    expect(tiles[0]).toHaveTextContent(
      `${(nervape3xGenetics.dnaHex!.length - 2) / 2} bytes of DNA in its cell data.`
    );
    expect(tiles[1]).toHaveTextContent(
      'stored in this cluster’s description: 6 genes, each a byte range and a rule.'
    );
    expect(tiles[2]).toHaveTextContent(
      'by type_id 0xf9e6…8e3d, binary unresolved' +
        'reads the genes · Nervape JSON DOB/0 · clipped to the DNA'
    );
    expect(section).not.toHaveTextContent(/Mapping unavailable|Step 1 ·|Current binary/);
  });

  it('draws one ruler part per gene, noting the genes read inside a wider one', () => {
    render(<CurrentPatternEvidence pattern={nervape3xInsights.currentPattern} sample={sample} />);
    const ruler = screen.getByRole('group', { name: 'DNA byte map' });
    const parts = [...ruler.querySelectorAll('[data-target]')].map((p) =>
      p.getAttribute('data-target')
    );
    const genes =
      nervape3xInsights.currentPattern.status === 'dob' &&
      nervape3xInsights.currentPattern.steps[0].description.status === 'supported'
        ? nervape3xInsights.currentPattern.steps[0].description.genes
        : [];
    // prev.bg owns bytes 0–31; prev.type and relief read byte 0 inside it
    expect(screen.getByText('prev.type and relief read byte 0 too')).toBeInTheDocument();
    expect(new Set(parts).size + 2).toBe(genes.length);
    fireEvent.mouseEnter(ruler.querySelector('[data-target]')!);
    expect(screen.getByRole('status', { name: 'DNA ruler readout' })).toHaveTextContent(
      /prev\.bg · bytes 0–31 · 32/
    );
    expect(screen.getByRole('row', { name: /^prev\.bg/ })).toHaveAttribute('data-active', 'true');
  });

  it('says what the decoded objects show for each gene and reads one real object', () => {
    render(
      <CurrentPatternEvidence
        pattern={nervape3xInsights.currentPattern}
        genetics={nervape3xInsights.genetics}
        sample={sample}
        status={<p>90 of 90 DOB objects decoded · 0 pending</p>}
      />
    );
    const table = screen.getByRole('table', { name: 'DOB blueprint' });
    expect(
      within(table)
        .getAllByRole('columnheader')
        .map((h) => h.textContent)
    ).toEqual(['gene', 'bytes', 'rule', 'found across 90']);
    expect(within(table).getByRole('row', { name: /^relief/ })).toHaveTextContent(
      'NERVAPE 3X, for all'
    );
    const card = screen.getByRole('group', { name: 'One object, read this way' });
    expect(within(card).getByRole('link')).toHaveAttribute('href', `/mainnet${sample.href}`);
    expect(within(card).getByText('relief').nextElementSibling).toHaveTextContent('NERVAPE 3X');
    expect(screen.getByText('90 of 90 DOB objects decoded · 0 pending')).toBeInTheDocument();
  });

  it('replaces "Mapping unavailable" with the ruled sentence, the reason kept in the title', () => {
    const pattern: CurrentPattern = structuredClone(nervape3xInsights.currentPattern);
    if (pattern.status !== 'dob') throw new Error('fixture');
    const reason = 'byte mapping is not defined for decoder binary 0x' + 'da35'.repeat(16);
    pattern.steps.push({
      ...pattern.steps[0],
      step: 1,
      bytes: [],
      description: { status: 'unsupported', reason },
    });
    render(<CurrentPatternEvidence pattern={pattern} />);
    const decoder = screen.getAllByRole('listitem')[2];
    expect(decoder.querySelector('b')).toHaveTextContent('The decoders');
    const line = within(decoder).getByRole('status');
    expect(line).toHaveTextContent(
      'The decoder binary describes no byte mapping; the pattern alone is used.'
    );
    expect(line).toHaveAttribute('title', reason);
    expect(screen.queryByText(/Mapping unavailable/)).toBeNull();
  });

  it('folds a run of numbered one-byte genes into one row and keeps every gene in a fold', () => {
    const pattern = structuredClone(objectGeneticsProducer.currentPattern);
    if (pattern.status !== 'dob' || pattern.steps[0].description.status !== 'supported')
      throw new Error('fixture');
    pattern.steps[0].description.genes = Array.from({ length: 12 }, (_, i) => ({
      ...objectGeneticsProducer.currentPattern.steps[0].description.genes[0],
      index: i,
      name: `_m${i}`,
      byteOffset: String(i),
      ruleArguments: ['0', '1'],
    }));
    render(<CurrentPatternEvidence pattern={pattern} />);
    const table = screen.getByRole('table', { name: 'DOB blueprint' });
    expect(within(table).getAllByRole('row')).toHaveLength(2);
    expect(within(table).getByRole('row', { name: /^_m0 – _m11/ })).toHaveTextContent(
      'bytes 0–11 · 12 × 1'
    );
    const all = screen.getByText('All 12 genes, one row each').closest('details')!;
    expect(within(all).getAllByRole('row')).toHaveLength(13);
    // the ruler still draws one part per gene
    expect(
      screen.getByRole('group', { name: 'DNA byte map' }).querySelectorAll('[data-target]')
    ).toHaveLength(12);
  });
});

describe('Per-version raw protocol and item context', () => {
  it('switches canonical cell evidence while preserving lossless decoded integers', () => {
    const protocol: ItemProtocol = structuredClone(didProtocolProducer);
    const old = structuredClone(protocol.versions[0]);
    old.cell.txHash = '0x' + 'ab'.repeat(32);
    old.cell.blockNumber = 99;
    old.cell.isLive = false;
    protocol.versions.unshift(old);
    render(<ItemEvidence protocol={protocol} />);
    expect(screen.getByRole('combobox')).toHaveValue(`${protocol.versions[1].cell.txHash}:0`);
    fireEvent.change(screen.getByRole('combobox'), { target: { value: `${old.cell.txHash}:0` } });
    expect(screen.getByRole('heading', { name: /version 1/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: new RegExp(old.cell.txHash) })).toHaveAttribute(
      'href',
      `/mainnet/cell/${old.cell.txHash}-0`
    );
    expect(screen.getByText(/18446744073709551615/)).toBeInTheDocument();
  });
  it('uses the exact resulting cell and represents burn without a resulting cell', () => {
    const tx = itemTransactionsProducer.data[0];
    const { rerender } = render(
      <ItemTransactionContext transaction={tx} item={{ kind: 'object' }} />
    );
    // Named like its Activities row (ID-C4), not the raw item action.
    expect(screen.getByText(`Object ${tx.itemContext.action}`)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /cell after/ })).toHaveAttribute(
      'href',
      `/mainnet/cell/${tx.itemContext.createdCell!.txHash}-${tx.itemContext.createdCell!.outputIndex}`
    );
    rerender(
      <ItemTransactionContext
        item={{ kind: 'object' }}
        transaction={{
          ...tx,
          itemContext: { ...tx.itemContext, action: 'burn', createdCell: null },
        }}
      />
    );
    expect(screen.getByText('No resulting item cell')).toBeInTheDocument();
  });
});

describe('The object page DNA panel at phone width (R3, responsive V5)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('fits 358 px: both gels drawn 1:1, the allele gel in two rows of three, nothing to scroll', () => {
    stubChartWidth(358);
    const { container } = render(
      <ObjectGeneticsView
        evidence={nervape3xGenetics}
        subject="Nervape 3x #125"
        collection={nervape3xInsights}
        collectionName="Nervape 3x"
        tier="btc_ckb"
      />
    );
    const section = screen.getByRole('region', { name: 'DNA' });
    // no minimum width and no sideways scroller anywhere in the panel
    for (const el of Array.from(section.querySelectorAll('[class]')))
      expect(el.getAttribute('class')).not.toMatch(/\bmin-w-\[|\boverflow-x-(auto|scroll)\b/);
    const svgs = Array.from(section.querySelectorAll('svg[role="img"]'));
    expect(svgs).toHaveLength(2);
    for (const svg of svgs) {
      expect(svg).toHaveAttribute('width', '358');
      expect(svg.getAttribute('viewBox')).toMatch(/^0 0 358 /);
    }
    // the sequencing gel: the map with the genes as rows under it
    expect(within(section).getByRole('list', { name: 'Genes' })).toBeInTheDocument();
    // the allele gel: lanes wrapped in DNA order, "this one" kept on both rows
    const rows = Array.from(section.querySelectorAll('[data-gel-row]'));
    expect(rows.map((row) => row.querySelectorAll('[data-lane]').length)).toEqual([3, 3]);
    const own = section.querySelector('[data-overlay="me"]')!;
    expect(own.querySelectorAll('[data-tick]')).toHaveLength(6);
    expect(Array.from(own.querySelectorAll('text')).map((t) => t.textContent)).toEqual([
      'this one',
      'this one',
    ]);
  });
});
