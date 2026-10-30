// F2 group `objects` (round 2): the Collection board's Objects section (allele gel + Newest).
// The two tests of the retired 24-card gallery are kept here against the new list.
import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import { CollectionObjects, DecodeCoverage } from '@/components/asset/collection-objects';
import { api } from '@/lib/api';
import { mockItemReads, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import {
  collectionGalleryFilteredProducer,
  collectionInsightsPendingProducer,
  collectionInsightsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
beforeEach(mockItemReads);

it('keeps multiple values of one trait on one row without React identity collisions', async () => {
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  vi.mocked(api.getCollectionGallery).mockResolvedValue(
    emptyPage([
      {
        ...collectionGalleryFilteredProducer.data[0],
        traits: [
          { name: 'Face', value: 'Blue' },
          { name: 'Face', value: 'Red' },
        ],
      },
    ])
  );
  render(<CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />);
  const table = await screen.findByRole('table', { name: 'Newest objects' });
  expect(within(table).getAllByRole('row')).toHaveLength(2);
  expect(log.mock.calls.flat().join(' ')).not.toMatch(/same key|unique.*key/);
});

it('passes the whole-collection trait predicate before cursor pagination', async () => {
  vi.mocked(api.getCollectionGallery).mockResolvedValue({
    ...collectionGalleryFilteredProducer,
    hasMore: true,
    nextCursor: 'older',
  });
  const { container } = render(
    <CollectionObjects id="collection" title="Genes" insights={collectionInsightsProducer} />
  );
  await screen.findByRole('table', { name: 'Newest objects' });
  fireEvent.click(container.querySelector('[data-allele="Blue"]')!);
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith('collection', {
      limit: 8,
      cursor: undefined,
      traits: { Face: 'Blue' },
    })
  );
  fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith('collection', {
      limit: 8,
      cursor: 'older',
      traits: { Face: 'Blue' },
    })
  );
});

it('offers no gel or filters while decoding is incomplete, and says how far it is', async () => {
  const { container } = render(
    <CollectionObjects id="collection" title="Genes" insights={collectionInsightsPendingProducer} />
  );
  await screen.findByRole('table', { name: 'Newest objects' });
  expect(container.querySelector('[data-lane]')).toBeNull();
  expect(screen.getByRole('status')).toHaveTextContent(
    'Allele frequencies and filtering await complete decoding'
  );
});

it('writes decode coverage with thousands separators (OB-C11)', () => {
  render(
    <DecodeCoverage
      genetics={{
        ...collectionInsightsProducer.genetics,
        liveItems: 2777,
        eligibleItems: 2777,
        decodedItems: 2777,
      }}
    />
  );
  expect(
    screen.getByText(/2,777 of 2,777 DOB objects decoded · 0 pending · 0 failed/)
  ).toBeInTheDocument();
});
