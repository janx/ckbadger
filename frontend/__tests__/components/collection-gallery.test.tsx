import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import { CollectionItems } from '@/components/asset/collection-gallery';
import { api } from '@/lib/api';
import { mockItemReads, emptyPage, identityItem } from '@/__tests__/utils/item-read-fixtures';
import { collectionGalleryFilteredProducer } from '@/__tests__/msw/fixtures/item-producer';
beforeEach(mockItemReads);
// F2 objects (round 2): the 24-card CollectionGallery was replaced by the Newest list; its two
// tests moved to __tests__/components/collection-objects.test.tsx.
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
