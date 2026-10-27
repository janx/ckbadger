import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import Page from '@/app/identities/[collectionId]/client-page';
import { api } from '@/lib/api';
import { mockItemReads, emptyPage, identityItem } from '@/__tests__/utils/item-read-fixtures';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
beforeEach(mockItemReads);
it('shows identities, exact created/live counts, capacity history and parallel record layers', async () => {
  render(<Page collectionId="dotbit" />);
  await screen.findByRole('heading', { name: '.bit' });
  expect(await screen.findByRole('heading', { name: 'Identities' })).toBeInTheDocument();
  expect(screen.getByRole('complementary', { name: 'Facts' })).toHaveTextContent(
    'Minted / created2'
  );
  expect(screen.getByRole('tab', { name: 'Activities' })).toHaveAttribute('aria-selected', 'true');
  fireEvent.click(screen.getByRole('tab', { name: 'Transactions' }));
  expect(await screen.findByRole('table', { name: 'Asset transactions' })).toBeInTheDocument();
  expect(api.getCollectionTransactions).toHaveBeenCalledWith(
    'identity',
    'dotbit',
    expect.anything()
  );
});
it('searches the entire identity collection and applies status before pagination', async () => {
  render(<Page collectionId="dotbit" />);
  fireEvent.change(await screen.findByRole('textbox', { name: 'Search collection' }), {
    target: { value: 'alice' },
  });
  fireEvent.change(screen.getByRole('combobox', { name: 'Item status' }), {
    target: { value: 'live' },
  });
  await waitFor(() =>
    expect(api.getIdentityCollectionItems).toHaveBeenLastCalledWith('dotbit', {
      limit: 24,
      cursor: undefined,
      search: 'alice',
      status: 'live',
    })
  );
});
it.each([
  ['dotbit', 'dotbit'],
  ['bit-cell', 'bit-cell'],
  ['did', 'did'],
] as const)('links %s identities to the canonical detail route', async (kind, path) => {
  const item = identityItem(kind);
  vi.mocked(api.getIdentityCollectionItems).mockResolvedValue(emptyPage([item]));
  render(<Page collectionId={kind} />);
  expect(await screen.findByRole('link', { name: item.name! })).toHaveAttribute(
    'href',
    `/mainnet/identities/${path}/${item.nftId}`
  );
});
it('shows inactive identity state without manufacturing a live owner', async () => {
  const item = identityItem('dotbit');
  vi.mocked(api.getIdentityCollectionItems).mockResolvedValue(
    emptyPage([{ ...item, isLive: false, ownerLockHash: null }])
  );
  render(<Page collectionId="dotbit" />);
  const region = await screen.findByRole('region', { name: 'Collection items' });
  expect(await within(region).findByText(/Inactive/)).toBeInTheDocument();
  expect(within(region).getByText('No live owner')).toBeInTheDocument();
});
it('keeps holders visible independently of the records tabs and handles empty pages', async () => {
  vi.mocked(api.getIdentityCollectionHolders).mockResolvedValue(emptyPage());
  render(<Page collectionId="dotbit" />);
  expect(await screen.findByText('No current holders.')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('tab', { name: 'Transactions' }));
  expect(screen.getByText('No current holders.')).toBeInTheDocument();
});
it('uses whole-scope current DID documents for PDS grouping with explicit overlapping endpoints', async () => {
  vi.mocked(api.getIdentityCollection).mockResolvedValue({
    collectionId: 'did:ckb',
    standard: 'did_ckb',
    name: 'did:ckb',
    totalCount: 2,
    liveCount: 2,
    holdersCount: 1,
    activitiesCount: 3,
    ownedCapacity: '100000000000',
    ownedKnowledge: '20400000000',
  });
  render(<Page collectionId="did:ckb" />);
  const pds = await screen.findByRole('region', { name: 'PDS distribution' });
  expect(
    await within(pds).findByText(/An identity can name more than one endpoint/)
  ).toBeInTheDocument();
  expect(api.getDidPdsDistribution).toHaveBeenCalledOnce();
});
