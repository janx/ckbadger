import { inventoryProducer } from '@/__tests__/msw/fixtures/asset-producer';
import { InventoryRows } from '@/components/asset/inventory-table';
import { render, screen } from '@/__tests__/utils/test-utils';
import { InventorySummaryNotice } from '@/components/asset/inventory-summary';
import type { InventorySummary } from '@/lib/api';
export const summary: InventorySummary = {
  source: 'canonical_asset_indexes',
  anchor: { blockNumber: 123, blockHash: '0x01' },
  counts: { token: 500, object: 200, identity: 4 },
  objectStandards: [{ standard: 'spore', collectionsCount: 100, liveCount: 5000 }],
  objectComposition: [{ tier: 'pure_ckb', collectionsCount: 90, liveCount: 4000 }],
  fullyOnchainObjectsCount: 4200,
  pendingItemsCount: 17,
};
describe('Inventory global summary', () => {
  it('keeps global item, collection and pending coverage distinct from one result page', () => {
    render(<InventorySummaryNotice summary={summary} assetType="object" />);
    expect(screen.getByLabelText('Inventory totals')).toHaveTextContent('4,200 live objects');
    expect(screen.getByLabelText('Inventory totals')).toHaveTextContent(
      '17 live objects await DOB'
    );
    expect(screen.getByLabelText('Inventory totals')).toHaveTextContent(
      '100 collections · 5,000 live items'
    );
    expect(screen.getByText(/Items are grouped/)).toHaveTextContent('collection’s composition');
    expect(screen.getByText(/Canonical asset totals/)).toHaveTextContent('#123');
  });
});

it('consumes a real cached pending-page and independent canonical summary response', () => {
  render(
    <>
      <InventorySummaryNotice summary={inventoryProducer.summary} assetType="object" />
      <InventoryRows rows={inventoryProducer.pendingPage.data} assetType="object" />
    </>
  );
  expect(screen.getByRole('table', { name: 'Object collections' })).toHaveTextContent(
    'Decode pending'
  );
  expect(screen.getByRole('table', { name: 'Object collections' })).toHaveTextContent(
    'of 2 minted'
  );
  expect(screen.getByLabelText('Inventory totals')).toHaveTextContent('2 live objects await DOB');
});
