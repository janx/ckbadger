import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import Page from '@/app/identities/[collectionId]/client-page';
import { api, ApiRequestError } from '@/lib/api';
import { mockItemReads, emptyPage, identityItem } from '@/__tests__/utils/item-read-fixtures';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
beforeEach(mockItemReads);
it('shows a collection 404 with its exact identity endpoint', async () => {
  vi.mocked(api.getIdentityCollection).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'collection absent')
  );
  render(<Page collectionId="dotbit" />);
  expect(await screen.findByText('/assets/identities/dotbit')).toBeInTheDocument();
  expect(
    screen.getByText('This explorer could not open the identity at this address.')
  ).toBeInTheDocument();
});
it('does not turn a collection source failure into 404', async () => {
  vi.mocked(api.getIdentityCollection).mockRejectedValue(
    new ApiRequestError(500, 'internal_error', 'collection mismatch')
  );
  render(<Page collectionId="dotbit" />);
  expect(await screen.findByRole('alert')).toHaveTextContent('collection mismatch');
  expect(
    screen.queryByText('This explorer could not open the identity at this address.')
  ).not.toBeInTheDocument();
});
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
    emptyPage([{ ...item, isLive: false, ownerLockHash: null, ownerAddress: null }])
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

// ── F2 identities (round 2): identity collection pages (ID-COL-1/2/3/5) ──
it('reports a gateway timeout as a status, never the proxy URL, and wraps at 390 (ID-COL-1)', async () => {
  vi.mocked(api.getCollectionInsights).mockRejectedValue(
    new ApiRequestError(
      504,
      'unknown_error',
      "upstream network 'mainnet' timed out: error sending request for url (http://127.0.0.1:8101/api/v1/assets/objects/dotbit/insights)"
    )
  );
  render(<Page collectionId="dotbit" />);
  await screen.findAllByText(/HTTP 504/, {}, { timeout: 4000 });
  expect(document.body).not.toHaveTextContent('127.0.0.1');
  expect(document.body).not.toHaveTextContent('error sending request');
  // The read-state line breaks anywhere, so no long token can widen the page at 390 px.
  const alert = screen.getAllByRole('alert').find((a) => /HTTP 504/.test(a.textContent ?? ''))!;
  expect(alert).toHaveClass('[overflow-wrap:anywhere]');
});
it('names each identity owner by address, a .cell owner by its prefix when unresolved (ID-COL-2/3)', async () => {
  const item = identityItem('dotbit');
  vi.mocked(api.getIdentityCollectionItems).mockResolvedValue(
    emptyPage([
      { ...item, ownerAddress: 'ckb1qzownerofalice' },
      {
        ...item,
        nftId: '0x' + '44'.repeat(20),
        name: 'bob.cell',
        standard: 'dotcell',
        ownerLockHash: null,
        ownerAddress: null,
        ownerHashPrefix: '0x' + '57'.repeat(20),
      },
      {
        ...item,
        nftId: '0x' + '45'.repeat(20),
        name: 'carol.bit',
        ownerLockHash: '0x' + '46'.repeat(32),
        ownerAddress: null,
      },
    ])
  );
  render(<Page collectionId="dotbit" />);
  const region = await screen.findByRole('region', { name: 'Collection items' });
  const alice = await within(region).findByRole('link', { name: /ckb1qzowner/ });
  expect(alice).toHaveAttribute('href', '/mainnet/address/ckb1qzownerofalice');
  // A named prefix with no indexed lock is shown as exactly that, never "No live owner".
  const bob = within(region).getByText('bob.cell').closest('article')!;
  expect(bob).toHaveTextContent('0x' + '57'.repeat(20));
  expect(bob).toHaveTextContent('named owner prefix · no indexed lock');
  expect(bob).not.toHaveTextContent('No live owner');
  // A lock hash whose script is not indexed is not dressed up as an address link.
  const carol = within(region).getByText('carol.bit').closest('article')!;
  expect(within(carol).queryByRole('link', { name: /0x4646/ })).not.toBeInTheDocument();
  expect(carol).toHaveTextContent('address not indexed');
});
it('opens with what the standard is, not its route alias or a placeholder (ID-COL-5)', async () => {
  render(<Page collectionId="dotbit" />);
  // Wait for the collection itself, not the loading shell that shares its title.
  await screen.findByRole('complementary', { name: 'Facts' });
  const header = screen.getByRole('heading', { level: 1 }).closest('header')!;
  expect(header).not.toHaveTextContent('No collection description recorded.');
  expect(header).not.toHaveTextContent('dotbit');
  expect(header).toHaveTextContent('Pure CKB');
  expect(header).toHaveTextContent('Each .bit name and everything about it live in one CKB cell.');
});
