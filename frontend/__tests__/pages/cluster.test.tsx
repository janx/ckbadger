import { QueryClientProvider } from '@tanstack/react-query';
import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
  act,
  createTestQueryClient,
} from '@/__tests__/utils/test-utils';
import ClusterPage from '@/app/clusters/[clusterId]/client-page';
import { api } from '@/lib/api';
import { mockItemReads, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import {
  clusterDetailProducer,
  collectionGalleryFilteredProducer,
  collectionInsightsPendingProducer,
  collectionInsightsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
const id = clusterDetailProducer.clusterId;
beforeEach(mockItemReads);
it('renders collection identity, capacity history, genes, gallery, holders and both feed layers', async () => {
  render(<ClusterPage clusterId={id} />);
  expect(await screen.findByRole('heading', { name: 'Genes' })).toHaveClass('text-[26px]');
  expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent(
    'Inventory/Objects'
  );
  expect(screen.getByRole('complementary', { name: 'Facts' })).toHaveTextContent('4,000');
  // F2 objects (OB-C1/C2/C3): the genome list and the 24-card gallery became one Objects section
  // (allele gel + Newest list).
  const objects = await screen.findByRole('region', { name: 'Objects' });
  expect(within(objects).getByRole('img', { name: /allele gel/ })).toBeInTheDocument();
  expect(await within(objects).findByRole('table', { name: 'Newest objects' })).toBeInTheDocument();
  expect(screen.getByRole('group', { name: 'History time range' })).toBeInTheDocument();
  expect(screen.getByRole('tab', { name: 'Activities' })).toHaveAttribute('aria-selected', 'true');
  fireEvent.click(screen.getByRole('tab', { name: 'Transactions' }));
  expect(await screen.findByRole('table', { name: 'Asset transactions' })).toBeInTheDocument();
  expect(api.getCollectionTransactions).toHaveBeenCalledWith('spore', id, {
    limit: 50,
    cursor: undefined,
  });
});
it('keeps live, created and burned counts separate and never adds the cluster cell to item capacity', async () => {
  render(<ClusterPage clusterId={id} />);
  await screen.findByRole('heading', { name: 'Genes' });
  const facts = screen.getByRole('complementary', { name: 'Facts' });
  // F2 objects (OB-C8): the board rail — live objects, "all live · none melted", CKB held.
  expect(within(facts).getByText('Objects').nextElementSibling).toHaveTextContent(
    '8all live · none melted'
  );
  expect(within(facts).getByText('CKB held in their cells').nextElementSibling).toHaveTextContent(
    '4,000'
  );
});
it('applies exact whole-collection trait filtering before pagination and resets the cursor', async () => {
  vi.mocked(api.getCollectionGallery).mockResolvedValue({
    ...collectionGalleryFilteredProducer,
    hasMore: true,
    nextCursor: 'older',
  });
  const { container } = render(<ClusterPage clusterId={id} />);
  const objects = await screen.findByRole('region', { name: 'Objects' });
  fireEvent.click(await within(objects).findByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith(
      id,
      expect.objectContaining({ cursor: 'older' })
    )
  );
  // F2 objects (OB-C2): a band of the allele gel is the filter.
  fireEvent.click(container.querySelector('[data-allele="Blue"]')!);
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith(id, {
      limit: 8,
      cursor: undefined,
      traits: { Face: 'Blue' },
    })
  );
  expect(within(objects).getByRole('button', { name: 'Previous' })).toBeDisabled();
  fireEvent.click(within(objects).getByRole('button', { name: 'Clear Face = Blue' }));
  await waitFor(() =>
    expect(container.querySelector('[data-allele="Blue"]')).not.toHaveAttribute('stroke')
  );
});
it('does not present partial frequencies or offer filtering while decoding is incomplete', async () => {
  vi.mocked(api.getCollectionInsights).mockResolvedValue(collectionInsightsPendingProducer);
  const { container } = render(<ClusterPage clusterId={id} />);
  const objects = await screen.findByRole('region', { name: 'Objects' });
  expect(objects).toHaveTextContent('Allele frequencies and filtering await complete decoding');
  expect(container.querySelector('[data-allele]')).toBeNull();
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenCalledWith(id, {
      limit: 8,
      cursor: undefined,
      traits: {},
    })
  );
});
it('shows unknown metadata without synthesizing a name, owner, or minted block', async () => {
  vi.mocked(api.getSporeCluster).mockResolvedValue({
    ...clusterDetailProducer,
    name: null,
    createdAtBlock: null,
    ownerLockHash: null,
    ownerAddress: null,
  });
  render(<ClusterPage clusterId={id} />);
  expect(await screen.findByRole('heading', { name: 'Unnamed collection' })).toBeInTheDocument();
  expect(await screen.findByText('No current cluster cell')).toBeInTheDocument();
  const facts = screen.getByRole('complementary', { name: 'Facts' });
  expect(within(facts).getByText('Created').nextElementSibling).toHaveTextContent('Not recorded');
  expect(facts.querySelector('a[href$="/blocks/null"]')).toBeNull();
});
it('retains the stored description and exact current decoder version without inventing DOB/0', async () => {
  vi.mocked(api.getCollectionInsights).mockResolvedValue({
    ...collectionInsightsProducer,
    currentPattern: { ...collectionInsightsProducer.currentPattern, status: 'dob', version: null },
  });
  render(<ClusterPage clusterId={id} />);
  await screen.findByRole('heading', { name: 'Current decoder blueprint' });
  expect(screen.getByText(/DOB version not stated/)).toBeInTheDocument();
  expect(screen.getByText(clusterDetailProducer.description!)).toBeInTheDocument();
});
it('handles empty gallery and holder sets without using collection totals as page lengths', async () => {
  vi.mocked(api.getCollectionGallery).mockResolvedValue(emptyPage());
  vi.mocked(api.getSporeClusterHolders).mockResolvedValue(emptyPage());
  render(<ClusterPage clusterId={id} />);
  expect(await screen.findByText('No matching objects.')).toBeInTheDocument();
  expect(await screen.findByText('No current holders.')).toBeInTheDocument();
  expect(screen.getByRole('complementary', { name: 'Facts' })).toHaveTextContent('Objects8');
});
it('links each gallery item and preserves DNA, metadata and explicit no-owner state', async () => {
  const item = collectionGalleryFilteredProducer.data[0];
  vi.mocked(api.getCollectionGallery).mockResolvedValue(
    emptyPage([{ ...item, ownerLockHash: null, ownerAddress: null }])
  );
  render(<ClusterPage clusterId={id} />);
  const gallery = await screen.findByRole('region', { name: 'Objects' });
  await within(gallery).findByText('No live owner');
  expect(gallery.querySelector(`a[href='/mainnet/objects/${item.sporeId}']`)).not.toBeNull();
  expect(
    within(gallery).getByRole('img', { name: new RegExp(`${item.sporeId} DNA:`) })
  ).toBeInTheDocument();
});
it('reports failed reads without substituting a fake collection', async () => {
  vi.mocked(api.getObjectCollection).mockRejectedValue(new Error('Not found'));
  render(<ClusterPage clusterId={id} />);
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Could not load collection: Not found'
  );
  expect(screen.queryByRole('region', { name: 'Objects' })).not.toBeInTheDocument();
});
it('preserves prior gallery data while reporting a refetch error', async () => {
  const client = createTestQueryClient();
  render(
    <QueryClientProvider client={client}>
      <ClusterPage clusterId={id} />
    </QueryClientProvider>
  );
  const gallery = await screen.findByRole('region', { name: 'Objects' });
  await within(gallery).findAllByText(/Genes #/);
  vi.mocked(api.getCollectionGallery).mockRejectedValue(new Error('view failed'));
  await act(async () => {
    await client.invalidateQueries({ queryKey: ['collection-gallery'] });
  });
  expect(await within(gallery).findByRole('alert')).toHaveTextContent(
    'Showing previously loaded data.'
  );
  expect(within(gallery).getAllByText(/Genes #/).length).toBeGreaterThan(0);
});
