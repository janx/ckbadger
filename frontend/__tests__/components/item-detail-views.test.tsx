import { QueryClientProvider } from '@tanstack/react-query';
import { render, screen, within, act, createTestQueryClient } from '@/__tests__/utils/test-utils';
import { SporeDetailView } from '@/components/asset/spore-detail';
import { CollectionDetail } from '@/components/asset/collection-detail';
import { CanonicalIdentityDetail } from '@/components/asset/identity-detail';
import { api } from '@/lib/api';
import { mockItemReads } from '@/__tests__/utils/item-read-fixtures';
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
  expect(await screen.findByRole('heading', { name: 'Cell versions' })).toBeInTheDocument();
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
    />
  );
  const facts = await screen.findByRole('complementary', { name: 'Facts' });
  await screen.findByRole('heading', { name: 'Cell versions' });
  expect(within(facts).getByText('Physical cell owner')).toBeInTheDocument();
  expect(
    facts.querySelector(`a[href='/mainnet/address/${p.versions[0].cell.ownerAddress}']`)
  ).not.toBeNull();
  expect(screen.getByText('Named owner')).toBeInTheDocument();
  expect(screen.getByText('Named manager')).toBeInTheDocument();
});
