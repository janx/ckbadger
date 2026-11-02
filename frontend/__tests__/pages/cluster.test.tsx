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
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import {
  clusterDetailProducer,
  objectCollectionDetailProducer,
  collectionGalleryFilteredProducer,
  collectionInsightsPendingProducer,
  collectionInsightsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));
const id = clusterDetailProducer.clusterId;
beforeEach(() => {
  mockItemReads();
  // the allele gel lays itself out for its measured box (V2); 1248 px is the 1280 column
  stubChartWidth(1248);
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
it('renders collection identity, capacity history, genes, gallery, holders and both feed layers', async () => {
  render(<ClusterPage clusterId={id} />);
  expect(await screen.findByRole('heading', { name: 'Genes' })).toHaveClass('text-entity');
  expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent(
    'Inventory/Objects'
  );
  // C7: the CKB the objects' cells hold is the hero's companion figure, not a rail fact.
  expect(document.querySelector('[data-role="hero"]')).toHaveTextContent('4,000');
  expect(screen.queryByRole('complementary', { name: 'Facts' })).toBeNull();
  // F2 objects (OB-C1/C2/C3): the genome list and the 24-card gallery became one Objects section
  // (allele gel + Newest list).
  const objects = await screen.findByRole('region', { name: 'Objects' });
  expect(within(objects).getByRole('img', { name: /allele gel/ })).toBeInTheDocument();
  expect(await within(objects).findByRole('table', { name: 'Newest objects' })).toBeInTheDocument();
  expect(screen.getByRole('group', { name: 'History time range' })).toBeInTheDocument();
  // Main.dc.html: "Recent activity" — its link to the transactions opens the full feed there.
  const recent = screen.getByRole('region', { name: 'Recent activity' });
  fireEvent.click(within(recent).getByRole('button', { name: /transactions →$/ }));
  expect(screen.getByRole('tab', { name: 'Transactions' })).toHaveAttribute(
    'aria-selected',
    'true'
  );
  expect(await screen.findByRole('table', { name: 'Asset transactions' })).toBeInTheDocument();
  expect(api.getCollectionTransactions).toHaveBeenCalledWith('spore', id, {
    limit: 50,
    cursor: undefined,
  });
});
it('keeps live, created and burned counts separate and never adds the cluster cell to item capacity', async () => {
  render(<ClusterPage clusterId={id} />);
  await screen.findByRole('heading', { name: 'Genes' });
  // C7 (Main.dc.html): the hero's figure — live objects, "all live · none burned" — and the CKB
  // their cells hold beside it (consistency plan C12: the feed's word "burned").
  const hero = document.querySelector('[data-role="hero"]')!;
  expect(hero.querySelector('[data-role="figure"]')).toHaveTextContent(
    'objects live8 all live · none burned'
  );
  expect(hero).toHaveTextContent('4,000.00000000 CKB held in their cells');
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
      limit: 20,
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
      limit: 20,
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
  const facts = screen.getByRole('region', { name: 'Metrics' });
  expect(within(facts).getByText('Created').nextElementSibling).toHaveTextContent('Not recorded');
  expect(facts.querySelector('a[href$="/blocks/null"]')).toBeNull();
});
it('retains the stored description and exact current decoder version without inventing DOB/0', async () => {
  vi.mocked(api.getCollectionInsights).mockResolvedValue({
    ...collectionInsightsProducer,
    currentPattern: { ...collectionInsightsProducer.currentPattern, status: 'dob', version: null },
  });
  render(<ClusterPage clusterId={id} />);
  await screen.findByRole('heading', { name: 'How an object is read' });
  expect(screen.getByText(/DOB version not stated/)).toBeInTheDocument();
  // Lane E (C5): the stored description is folded, pretty-printed and tinted, nothing changed.
  const stored = screen
    .getByText('Description, as stored · pretty JSON')
    .closest('details')!
    .querySelector('pre')!;
  expect(JSON.parse(stored.textContent!)).toEqual(JSON.parse(clusterDetailProducer.description!));
});
it('handles empty gallery and holder sets without using collection totals as page lengths', async () => {
  vi.mocked(api.getCollectionGallery).mockResolvedValue(emptyPage());
  vi.mocked(api.getSporeClusterHolders).mockResolvedValue(emptyPage());
  render(<ClusterPage clusterId={id} />);
  expect(await screen.findByText('No matching objects.')).toBeInTheDocument();
  expect(await screen.findByText('No current holders.')).toBeInTheDocument();
  expect(document.querySelector('[data-role="figure"]')).toHaveTextContent('objects live8');
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
  // C11 (Main.dc.html): the row names the genes that vary; its DNA is in the row's title.
  const row = gallery
    .querySelector(`a[href='/mainnet/objects/${item.sporeId}']`)!
    .closest('[role="row"]')!;
  expect(row.getAttribute('title')).toContain(`DNA ${item.dnaHex}`);
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
  await within(gallery).findAllByText(/^#[\d,]+$/);
  vi.mocked(api.getCollectionGallery).mockRejectedValue(new Error('view failed'));
  await act(async () => {
    await client.invalidateQueries({ queryKey: ['collection-gallery'] });
  });
  expect(await within(gallery).findByRole('alert')).toHaveTextContent(
    'Showing previously loaded data.'
  );
  expect(within(gallery).getAllByText(/^#[\d,]+$/).length).toBeGreaterThan(0);
});

// Round 3 (contracts L2, coordinator ruling): while the indexer has not derived the collection
// activity count the cluster detail serves `activitiesCount: null` and the feed `total: null`.
it('says nothing of an activity count the indexer has not derived (L2)', async () => {
  vi.mocked(api.getSporeCluster).mockResolvedValue({
    ...clusterDetailProducer,
    activitiesCount: null,
  });
  // The collection view of the same cluster (`/assets/objects/{id}`) is null too.
  vi.mocked(api.getObjectCollection).mockResolvedValue({
    ...objectCollectionDetailProducer,
    activitiesCount: null,
  });
  render(<ClusterPage clusterId={id} />);
  expect(await screen.findByRole('heading', { name: 'Genes' })).toBeInTheDocument();
  const facts = screen.getByRole('region', { name: 'Metrics' });
  // Only the insights' exact mint / transfer / burn counts remain; no count stands in for the
  // unknown.
  expect(facts).toHaveTextContent('Mints · transfers · burns0 mints · 0 transfers · 0 burns');
  expect(facts).not.toHaveTextContent('Transactions');
  expect(screen.queryByText('Activities', { selector: 'dt' })).toBeNull();
  expect(screen.queryByRole('alert')).toBeNull();
});
