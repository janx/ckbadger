import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import { CollectionGallery, CollectionItems } from '@/components/asset/collection-gallery';
import { api } from '@/lib/api';
import { mockItemReads, emptyPage, identityItem } from '@/__tests__/utils/item-read-fixtures';
import { collectionGalleryFilteredProducer } from '@/__tests__/msw/fixtures/item-producer';
beforeEach(mockItemReads);
it('keeps multiple values of one trait without React identity collisions', async () => {
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
  render(<CollectionGallery id="collection" traits={{}} />);
  const region = screen.getByRole('region', { name: 'Collection objects' });
  expect(await within(region).findByText('Blue')).toBeInTheDocument();
  expect(within(region).getByText('Red')).toBeInTheDocument();
  expect(log.mock.calls.flat().join(' ')).not.toMatch(/same key|unique.*key/);
});
it('passes the whole-collection trait predicate before cursor pagination', async () => {
  vi.mocked(api.getCollectionGallery).mockResolvedValue({
    ...collectionGalleryFilteredProducer,
    hasMore: true,
    nextCursor: 'older',
  });
  render(<CollectionGallery id="collection" traits={{ Face: 'Blue' }} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getCollectionGallery).toHaveBeenLastCalledWith('collection', {
      limit: 24,
      cursor: 'older',
      traits: { Face: 'Blue' },
    })
  );
});
it('resets server pagination when search or status changes and preserves item-specific links', async () => {
  vi.mocked(api.getIdentityCollectionItems).mockResolvedValue({
    ...emptyPage([identityItem('bit-cell')]),
    hasMore: true,
    nextCursor: 'later',
  });
  render(<CollectionItems id="bit-cell" kind="identity" />);
  const item = identityItem('bit-cell');
  expect(await screen.findByRole('link', { name: item.name! })).toHaveAttribute(
    'href',
    `/mainnet/identities/bit-cell/${item.nftId}`
  );
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenLastCalledWith(
      'bit-cell',
      expect.objectContaining({ cursor: 'later' })
    )
  );
  fireEvent.change(screen.getByRole('textbox', { name: 'Search collection' }), {
    target: { value: 'alice' },
  });
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenLastCalledWith('bit-cell', {
      limit: 24,
      cursor: undefined,
      search: 'alice',
      status: 'all',
    })
  );
  fireEvent.change(screen.getByRole('combobox', { name: 'Item status' }), {
    target: { value: 'recycled' },
  });
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenLastCalledWith('bit-cell', {
      limit: 24,
      cursor: undefined,
      search: 'alice',
      status: 'recycled',
    })
  );
});
