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
      status: 'live',
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

// Review 2026-11-03, item 3: a .bit collection is 97 % recycled names, so an 'all' default opened
// on a screen of names with no live owner. Live cells are the state: the list opens on live items.
it('opens on live items, and keeps All and Inactive one choice away', async () => {
  vi.mocked(api.getIdentityCollectionItems).mockResolvedValue(emptyPage([identityItem('dotbit')]));
  render(<CollectionItems id="dotbit" kind="identity" />);
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenCalledWith('dotbit', {
      limit: 24,
      cursor: undefined,
      search: undefined,
      status: 'live',
    })
  );
  const status = screen.getByRole<HTMLSelectElement>('combobox', { name: 'Item status' });
  expect(status.value).toBe('live');
  expect(Array.from(status.options, (o) => o.textContent)).toEqual(['All', 'Live', 'Inactive']);
  fireEvent.change(status, { target: { value: 'all' } });
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenLastCalledWith(
      'dotbit',
      expect.objectContaining({ status: 'all', cursor: undefined })
    )
  );
});

// Round 3 (contracts R-A · R-1, OB-K3): an m-NFT class row reads "<class name> #<token index>".
it('names m-NFT class rows by the class name and the token index', async () => {
  const mnft = (tokenIndex: number, name: string | null) => ({
    nftId: `0x8f67${'00'.repeat(24)}${String(tokenIndex).padStart(8, '0')}`,
    name,
    standard: 'm-nft',
    ownerLockHash: `0x${'12'.repeat(32)}`,
    ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq4s93hhsj',
    tokenIndex,
    isLive: true,
    createdAtBlock: 7_000_000,
  });
  vi.mocked(api.getObjectCollectionItems).mockResolvedValue(
    emptyPage([mnft(0, 'Santa Claus'), mnft(1, null)])
  );
  render(<CollectionItems id="class" kind="object" />);
  expect(await screen.findByRole('link', { name: 'Santa Claus #0' })).toHaveAttribute(
    'href',
    `/mainnet/objects/mnft/${mnft(0, null).nftId}`
  );
  // A class without an on-chain name: the index alone, no invented name.
  expect(screen.getByRole('link', { name: '#1' })).toBeInTheDocument();
});

it('refuses an m-NFT row without its token index', async () => {
  vi.mocked(api.getObjectCollectionItems).mockResolvedValue(
    emptyPage([
      {
        nftId: `0x8f67${'00'.repeat(28)}`,
        name: 'Santa Claus',
        standard: 'm-nft',
        ownerLockHash: null,
        ownerAddress: null,
        isLive: false,
        createdAtBlock: 7_000_000,
      },
    ])
  );
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  render(<CollectionItems id="class" kind="object" />);
  expect(await screen.findByText(/m-NFT item 0x8f67.* has no tokenIndex/)).toBeInTheDocument();
  log.mockRestore();
});
