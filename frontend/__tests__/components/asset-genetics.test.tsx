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
import type { ObjectGenetics, ItemProtocol } from '@/lib/api';
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
  it('shows current binary and declared rules without converting exact offsets', () => {
    const pattern = structuredClone(objectGeneticsProducer.currentPattern);
    pattern.steps[0].description.genes[0].byteOffset = '18446744073709551615';
    render(<CurrentPatternEvidence pattern={pattern} />);
    // Table-rows L2-5: the exact offset is in the gene's one-line span and in the row title.
    expect(screen.getByText('byte 18446744073709551615 · 1')).toBeInTheDocument();
    expect(screen.getByRole('row', { name: /Face/ })).toHaveAttribute(
      'title',
      'Face · byte offset 18446744073709551615 · length 1 · byte 18446744073709551615 · options · String · 2 options: Red, Blue'
    );
    // F2 objects (OB-C12): the blueprint table names the rule and its value domain.
    expect(screen.getAllByText(/options/).length).toBeGreaterThan(0);
    expect(screen.getByText('2 options: Red, Blue')).toBeInTheDocument();
  });
});
// Table-rows round, lane L2-5: the blueprint's genes on one line each — the gene bright at 13 px,
// "bytes 0–39 · 40" (span · length) in one cell, rule and values one line; the declared offset and
// length in words go to the row title.
describe('DOB blueprint table on the row rules (table-rows L2-5)', () => {
  it('writes each gene on one row-density line with the byte span and length together', () => {
    const pattern = structuredClone(objectGeneticsProducer.currentPattern);
    if (pattern.status !== 'dob' || pattern.steps[0].description.status !== 'supported')
      throw new Error('fixture');
    pattern.steps[0].description.genes.push({
      ...pattern.steps[0].description.genes[0],
      index: 1,
      name: 'name',
      byteOffset: '0',
      byteLength: '40',
      rule: 'utf8',
      ruleArguments: [],
    });
    render(<CurrentPatternEvidence pattern={pattern} />);
    const table = screen.getByRole('table', { name: 'DOB blueprint' });
    for (const td of table.querySelectorAll('tbody td'))
      expect(td).toHaveClass('py-[11px]', 'align-middle');
    const row = within(table).getByRole('row', { name: /^name/ });
    expect(within(row).getByText('name')).toHaveClass(
      'text-body',
      'text-text-bright',
      'leading-[18px]',
      'truncate'
    );
    expect(row).toHaveTextContent('bytes 0–39 · 40');
    expect(row).not.toHaveTextContent('byte offset');
    expect(row).toHaveAttribute(
      'title',
      'name · byte offset 0 · length 40 · bytes 0–39 · utf8 · String · 0 arguments'
    );
    expect(within(row).getByText('0 arguments')).toHaveClass('block', 'truncate');
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
