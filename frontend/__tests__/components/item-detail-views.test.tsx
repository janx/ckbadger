import { QueryClientProvider } from '@tanstack/react-query';
import { render, screen, within, act, createTestQueryClient } from '@/__tests__/utils/test-utils';
import { SporeDetailView } from '@/components/asset/spore-detail';
import { CollectionDetail } from '@/components/asset/collection-detail';
import { CanonicalIdentityDetail } from '@/components/asset/identity-detail';
import { api } from '@/lib/api';
import { mockItemReads, dotcellItem } from '@/__tests__/utils/item-read-fixtures';
import {
  sporeDetailProducer,
  clusterDetailProducer,
  clusterProtocolProducer,
  dotcellProtocolProducer,
  mnftCollectionInsightsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
beforeEach(mockItemReads);
it('keeps object capacity primary, one versions heading and exact occupied values', async () => {
  render(<SporeDetailView object={sporeDetailProducer} error={null} />);
  // F2 objects (OB-9): the versions are the board's "CKB held in its cell" — one heading for both.
  expect(await screen.findByLabelText(/cell versions$/)).toBeInTheDocument();
  expect(screen.queryByRole('heading', { name: 'Cell versions' })).not.toBeInTheDocument();
  expect(screen.getAllByRole('heading', { name: 'CKB held in its cell' })).toHaveLength(1);
  expect(
    screen.getByLabelText(`Object ${sporeDetailProducer.sporeId} capacity partition`)
  ).toHaveTextContent('229');
  expect(
    screen.getByRole('region', { name: 'Raw item evidence' }).closest('[data-tier]')
  ).toHaveAttribute('data-tier', '2');
});
it('rejects impossible object capacity rather than clamping it', () => {
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  expect(() =>
    render(<SporeDetailView object={{ ...sporeDetailProducer, ownedCapacity: '1' }} error={null} />)
  ).toThrow('occupied capacity exceeds');
  log.mockRestore();
});
it('does not identify cluster metadata ownership as a current cell without canonical proof', async () => {
  render(<CollectionDetail id={clusterDetailProducer.clusterId} kind="spore" />);
  await screen.findByRole('heading', { name: 'Genes' });
  expect(await screen.findByText('No current cluster cell')).toBeInTheDocument();
  expect(screen.getByRole('region', { name: 'Cluster cell evidence' })).toHaveTextContent(
    'No current cell carries this cluster type.'
  );
});
it('links a canonically resolved cluster cell through the one-segment outpoint route', async () => {
  vi.mocked(api.getSporeClusterProtocol).mockResolvedValue(clusterProtocolProducer);
  render(<CollectionDetail id={clusterDetailProducer.clusterId} kind="spore" />);
  const link = await screen.findByRole('link', { name: 'Current cluster cell →' });
  expect(link).toHaveAttribute(
    'href',
    `/mainnet/cell/${clusterProtocolProducer.current.cell.txHash}-${clusterProtocolProducer.current.cell.outputIndex}`
  );
});
it('retains collection evidence and reports a failed background refresh', async () => {
  const client = createTestQueryClient();
  render(
    <QueryClientProvider client={client}>
      <CollectionDetail id={clusterDetailProducer.clusterId} kind="spore" />
    </QueryClientProvider>
  );
  await screen.findByRole('heading', { name: 'Genes' });
  vi.mocked(api.getObjectCollection).mockRejectedValue(new Error('secondary failed'));
  await act(async () => {
    await client.invalidateQueries({
      queryKey: ['object-collection', clusterDetailProducer.clusterId],
    });
  });
  expect(
    await screen.findByText(/Could not refresh collection: secondary failed/)
  ).toBeInTheDocument();
  expect(
    within(screen.getByRole('complementary', { name: 'Facts' })).getByText('4,000')
  ).toBeInTheDocument();
});
it('shows an m-NFT class its real holder buckets and no DOB blueprint line (#3)', async () => {
  vi.mocked(api.getCollectionInsights).mockResolvedValue(mnftCollectionInsightsProducer);
  render(<CollectionDetail id={'0x' + '8f'.repeat(24)} kind="object" />);
  expect(await screen.findByText('Full collection: 2 holders · 8 items')).toBeInTheDocument();
  expect(api.getCollectionInsights).toHaveBeenCalledWith('0x' + '8f'.repeat(24), 'object');
  expect(screen.queryByText('No DOB blueprint in this collection.')).not.toBeInTheDocument();
  expect(screen.queryByText('No current holders.')).not.toBeInTheDocument();
});
it('reports failed m-NFT insights instead of drawing an empty weekly panel (#17)', async () => {
  vi.mocked(api.getCollectionInsights).mockRejectedValue(new Error('insights unavailable'));
  render(<CollectionDetail id={'0x' + '8f'.repeat(24)} kind="object" />);
  expect(
    await screen.findByText(/Could not load collection insights: insights unavailable/)
  ).toBeInTheDocument();
  expect(
    await screen.findByText('Weekly transfers unavailable: insights unavailable')
  ).toBeInTheDocument();
});
it('draws an identity collection its weekly transfers from the insights it is served (#9)', async () => {
  vi.mocked(api.getCollectionInsights).mockResolvedValue({
    ...mnftCollectionInsightsProducer,
    weeklyActivity: [
      {
        weekStart: '2023-11-08',
        mintTransactions: 0,
        transferTransactions: 7,
        burnTransactions: 0,
      },
    ],
  });
  render(<CollectionDetail id="dotbit" kind="identity" />);
  expect(
    await screen.findByText(/Week of 2023-11-08: 7 transfers/, {}, { timeout: 3000 })
  ).toBeInTheDocument();
  expect(api.getCollectionInsights).toHaveBeenCalledWith('dotbit', 'identity');
  expect(screen.queryByText(/not indexed for identity collections/)).not.toBeInTheDocument();
});
it('separates physical cell ownership from protocol-named .cell parties', async () => {
  const p = dotcellProtocolProducer;
  render(
    <CanonicalIdentityDetail
      kind="dotcell"
      id={p.itemId}
      title="alice.cell"
      isLive
      createdAtBlock={p.versions[0].cell.blockNumber}
      error={null}
      dotcell={dotcellItem()}
    />
  );
  const facts = await screen.findByRole('complementary', { name: 'Facts' });
  await screen.findByRole('heading', { name: 'Cell versions' });
  // F2 identities (round 2, ID-DC4 / ID-DC7): the rail names the protocol owner and manager;
  // the physical lock every name shares is tier-2 evidence, explained there.
  expect(facts).toHaveTextContent(p.versions[0].decoded.owner.hashPrefix);
  expect(facts).toHaveTextContent(p.versions[0].decoded.manager.hashPrefix);
  expect(
    facts.querySelector(`a[href='/mainnet/address/${p.versions[0].cell.ownerAddress}']`)
  ).toBeNull();
  const cell = screen.getByRole('region', { name: 'Name cell' });
  expect(
    cell.querySelector(`a[href='/mainnet/address/${p.versions[0].cell.ownerAddress}']`)
  ).not.toBeNull();
  expect(cell).toHaveTextContent('ownership is the owner field in the data');
});
it("titles collection feed rows from the API's collectionContext, never the tx's first item (OB-C13)", async () => {
  render(<CollectionDetail id={clusterDetailProducer.clusterId} kind="spore" />);
  const feed = await screen.findByRole('region', { name: 'What happened' });
  expect(await within(feed).findByText('Transferred 1')).toBeInTheDocument();
  expect(within(feed).queryByText(/^Object (mint|transfer|burn)$/)).toBeNull();
  // The Transactions tab says the same thing per row.
  act(() => within(feed).getByRole('tab', { name: 'Transactions' }).click());
  // Table and compact layouts both carry it.
  expect((await within(feed).findAllByText('Minted 1')).length).toBeGreaterThan(0);
});
