import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
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
describe('Independent DOB evidence', () => {
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
    expect(screen.getByText(/byte offset 18446744073709551615/)).toBeInTheDocument();
    // F2 objects (OB-C12): the blueprint table names the rule and its value domain.
    expect(screen.getAllByText(/options/).length).toBeGreaterThan(0);
    expect(screen.getByText('2 options: Red, Blue')).toBeInTheDocument();
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
