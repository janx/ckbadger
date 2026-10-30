import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
import {
  ObjectGeneticsView,
  CollectionGenes,
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
  it('wraps a full BTCFS trait value beside its exact count without changing the filter value', () => {
    const value = 'btcfs://11b6303eb7d887d7ade459ac27959754cd55f9f9e50345ced8e1e8f47f4581fai0';
    const select = vi.fn();
    render(
      <CollectionGenes
        evidence={{
          ...collectionInsightsProducer.genetics,
          liveItems: 2777,
          eligibleItems: 2777,
          nonDobItems: 0,
          decodedItems: 2777,
          traits: [{ name: 'preview', values: [{ value, count: 199 }] }],
        }}
        selected={{}}
        onSelect={select}
      />
    );
    const button = screen.getByRole('button', { name: `${value} 199 / 2777` });
    // break-words alone cannot shrink a flex item's automatic minimum width.
    // Both constraints are needed to keep the full identifier and adjacent count visible.
    expect(screen.getByText(value)).toHaveClass('min-w-0', 'break-words');
    expect(button).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(button);
    expect(select).toHaveBeenLastCalledWith('preview', value);
    fireEvent.focus(button);
    expect(button.closest('[data-allele]')).not.toHaveClass('opacity-35');
  });

  it('keeps every variant of a long trait selectable behind "+N more" (OB-C1)', () => {
    const values = Array.from({ length: 12 }, (_, i) => ({ value: `Colour ${i}`, count: 20 - i }));
    const select = vi.fn();
    render(
      <CollectionGenes
        evidence={{
          ...collectionInsightsProducer.genetics,
          liveItems: 300,
          eligibleItems: 300,
          nonDobItems: 0,
          decodedItems: 300,
          traits: [{ name: 'Background', values }],
        }}
        selected={{}}
        onSelect={select}
      />
    );
    expect(screen.getAllByRole('button', { pressed: false })).toHaveLength(8);
    expect(screen.queryByText(/filter from the Objects list/)).not.toBeInTheDocument();
    const more = screen.getByRole('button', { name: '+4 more Background variants' });
    expect(more).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(more);
    expect(screen.getAllByRole('button', { pressed: false })).toHaveLength(12);
    fireEvent.click(screen.getByRole('button', { name: 'Colour 11 9 / 300' }));
    expect(select).toHaveBeenLastCalledWith('Background', 'Colour 11');
    fireEvent.click(screen.getByRole('button', { name: 'Show the top 8 Background variants' }));
    expect(screen.queryByRole('button', { name: 'Colour 11 9 / 300' })).not.toBeInTheDocument();
  });

  it('keeps the full object number, VM provenance and actual-byte hover association', () => {
    const data: ObjectGenetics = structuredClone(objectGeneticsProducer);
    data.dnaHex = '0x0102';
    if (data.currentPattern.status !== 'dob') throw new Error('fixture');
    data.currentPattern.steps[0].bytes[0].declaredByteLength = '100';
    data.currentPattern.steps[0].bytes[0].truncated = true;
    const { container } = render(<ObjectGeneticsView evidence={data} subject="Spore" />);
    expect(screen.getByText(/18446744073709551614/)).toBeInTheDocument();
    expect(screen.getByText(/decoded 2023-11-15 06:13:20 UTC\+8/)).toBeInTheDocument();
    expect(screen.queryByText(/Unix second/)).not.toBeInTheDocument();
    expect(screen.getByText(/original decoder binary hash was not recorded/)).toBeInTheDocument();
    expect(screen.getByText(/Clipped to the actual DNA shown/)).toBeInTheDocument();
    fireEvent.focus(container.querySelector('[data-gene]')!);
    expect(container.querySelector('[data-nibble="2"]')).toHaveAttribute('opacity', '0.15');
    expect(container.querySelector('[data-nibble="0"]')).toHaveAttribute('opacity', '1');
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
    render(<ObjectGeneticsView evidence={data} subject="Spore" />);
    expect(screen.getByText('Cached VM result: Blue · Green')).toBeInTheDocument();
  });
  it('uses complete same-name/value collection counts and never presents partial frequencies', () => {
    const view = render(
      <ObjectGeneticsView
        evidence={objectGeneticsProducer}
        subject="Spore"
        collection={collectionInsightsProducer.genetics}
      />
    );
    expect(screen.getByText('4 of 8 live DOB items')).toBeInTheDocument();
    view.rerender(
      <ObjectGeneticsView
        evidence={objectGeneticsProducer}
        subject="Spore"
        collection={collectionInsightsPendingProducer.genetics}
      />
    );
    expect(screen.queryByText(/of 8 live DOB items/)).not.toBeInTheDocument();
  });
  it('rejects a byte map that disagrees with immutable DNA', () => {
    const data: ObjectGenetics = structuredClone(objectGeneticsProducer);
    data.dnaHex = '0x02';
    expect(() => render(<ObjectGeneticsView evidence={data} subject="Spore" />)).toThrow(
      'bytes disagree with DNA'
    );
  });
  it('filters by complete collection frequencies and shows pending coverage explicitly', () => {
    const select = vi.fn();
    const { rerender } = render(
      <CollectionGenes
        evidence={collectionInsightsProducer.genetics}
        selected={{}}
        onSelect={select}
      />
    );
    fireEvent.click(screen.getByRole('button', { name: 'Blue 4 / 8' }));
    expect(select).toHaveBeenCalledWith('Face', 'Blue');
    rerender(
      <CollectionGenes
        evidence={collectionInsightsPendingProducer.genetics}
        selected={{}}
        onSelect={select}
      />
    );
    expect(screen.getByText(/await complete decoding/)).toBeInTheDocument();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
  it('shows current binary and declared rules without converting exact offsets', () => {
    const pattern = structuredClone(objectGeneticsProducer.currentPattern);
    pattern.steps[0].description.genes[0].byteOffset = '18446744073709551615';
    render(<CurrentPatternEvidence pattern={pattern} />);
    expect(screen.getByText(/byte offset 18446744073709551615/)).toBeInTheDocument();
    expect(screen.getByText(/options/)).toBeInTheDocument();
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
