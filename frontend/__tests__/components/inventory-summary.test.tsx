import { inventoryProducer } from '@/__tests__/msw/fixtures/asset-producer';
import { InventoryRows } from '@/components/asset/inventory-table';
import { render, screen } from '@/__tests__/utils/test-utils';
import { InventorySummaryNotice, ObjectComposition } from '@/components/asset/inventory-summary';
import type { InventorySummary } from '@/lib/api';
export const summary: InventorySummary = {
  source: 'canonical_asset_indexes',
  anchor: { blockNumber: 123, blockHash: '0x01' },
  counts: { token: 500, object: 200, identity: 4 },
  objectStandards: [{ standard: 'spore', collectionsCount: 100, liveCount: 5000 }],
  objectComposition: [
    { tier: 'pure_ckb', collectionsCount: 90, liveCount: 4000 },
    { tier: 'centralized_mixture', collectionsCount: 110, liveCount: 1000 },
  ],
  fullyOnchainObjectsCount: 4200,
  pendingItemsCount: 17,
};
describe('Inventory global summary', () => {
  // F2 objects (IV-09): the Objects view leads with composition; the totals line keeps the anchor.
  it('keeps global item, collection and pending coverage distinct from one result page', () => {
    render(
      <>
        <ObjectComposition summary={summary} selected={undefined} onSelect={() => {}} />
        <InventorySummaryNotice summary={summary} assetType="object" />
      </>
    );
    const comp = screen.getByRole('region', { name: 'Object composition' });
    expect(comp).toHaveTextContent(
      '90 of 200 collections are fully on-chain · 4,200 live objects are · 17 await DOB decoding'
    );
    expect(screen.getByText(/Canonical asset totals/)).toHaveTextContent('#123');
  });
  it('fails when the composition tiers do not add up to the object count', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <ObjectComposition
          summary={{ ...summary, objectComposition: summary.objectComposition.slice(0, 1) }}
          selected={undefined}
          onSelect={() => {}}
        />
      )
    ).toThrow('composition counts 90 collections, object count is 200');
    log.mockRestore();
  });
});

it('consumes a real cached pending-page and independent canonical summary response', () => {
  render(
    <>
      <ObjectComposition
        summary={inventoryProducer.summary}
        selected={undefined}
        onSelect={() => {}}
      />
      <InventoryRows rows={inventoryProducer.pendingPage.data} assetType="object" />
    </>
  );
  expect(screen.getByRole('table', { name: 'Object collections' })).toHaveTextContent(
    'Decode pending'
  );
  expect(screen.getByRole('table', { name: 'Object collections' })).toHaveTextContent(
    'of 2 minted'
  );
  expect(screen.getByRole('region', { name: 'Object composition' })).toHaveTextContent(
    '2 await DOB decoding'
  );
});
