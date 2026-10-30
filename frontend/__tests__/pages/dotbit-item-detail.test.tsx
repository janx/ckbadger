import { render, screen, fireEvent, waitFor } from '@/__tests__/utils/test-utils';
import Page from '@/app/identities/dotbit/[identityId]/client-page';
import { api, ApiRequestError, type ItemCellVersions } from '@/lib/api';
import { mockItemReads, identityItem, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import {
  dotbitV4ProtocolProducer as evidence,
  itemActivityFeedProducer,
  itemTransactionsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
let query = '';
const replace = vi.fn();
vi.mock('@/src/navigation', () => ({
  useSearchParams: () => new URLSearchParams(query),
  usePathname: () => '/identities/dotbit/item',
  useRouter: () => ({ replace }),
}));
beforeEach(() => {
  mockItemReads();
  query = '';
  replace.mockClear();
});
afterEach(() => vi.restoreAllMocks());
it('renders exact canonical protocol, whole cell history and raw bytes for dotbit', async () => {
  render(<Page identityId={evidence.itemId} />);
  expect(await screen.findByRole('heading', { level: 1 })).toHaveClass('text-[26px]');
  expect(await screen.findByLabelText('Raw item evidence')).toHaveTextContent(
    /[Cc]ell data · \d+ bytes/
  );
  expect(api.getDotbitItemDetail).toHaveBeenCalledWith(evidence.itemId);
  expect(api.getItemProtocol).toHaveBeenCalledWith('dotbit', evidence.itemId);
  const link = screen
    .getAllByRole('link')
    .find(
      (a) =>
        a.getAttribute('href') ===
        `/mainnet/cell/${evidence.versions[0].cell.txHash}-${evidence.versions[0].cell.outputIndex}`
    );
  expect(link).toBeDefined();
});
it('keeps recycled identity history while showing no live capacity', async () => {
  vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
    ...identityItem('dotbit'),
    isLive: false,
    ownerLockHash: null,
    txHash: null,
    outputIndex: null,
  });
  const cells: ItemCellVersions = {
    data: evidence.versions.map((v) => ({
      ...v.cell,
      isLive: false,
      consumedByTx: '0x' + 'ab'.repeat(32),
      consumedAtBlock: 101,
    })),
    total: evidence.versions.length,
  };
  vi.mocked(api.getItemCellVersions).mockResolvedValue(cells);
  render(<Page identityId={evidence.itemId} />);
  expect(await screen.findByText('Recycled')).toBeInTheDocument();
  expect(await screen.findByText('no live cell')).toBeInTheDocument();
  expect(screen.getAllByText('0').length).toBeGreaterThan(0);
  expect(screen.getByLabelText('Raw item evidence')).toBeInTheDocument();
});
it('hydrates the explicit activity cursor and advances full-record pagination', async () => {
  query = 'activity_cursor=100:0';
  vi.mocked(api.getItemActivityFeed).mockResolvedValue({
    ...itemActivityFeedProducer,
    total: 100,
    hasMore: true,
    nextCursor: '90:0',
  });
  render(<Page identityId={evidence.itemId} />);
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('dotbit', evidence.itemId, {
      limit: 50,
      cursor: '100:0',
    })
  );
  fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('dotbit', evidence.itemId, {
      limit: 50,
      cursor: '90:0',
    })
  );
  expect(replace).toHaveBeenCalledWith('/identities/dotbit/item?activity_cursor=90%3A0', {
    scroll: false,
  });
});
it('uses the independent Transactions layer and preserves unavailable cycles', async () => {
  vi.mocked(api.getItemTransactions).mockResolvedValue({
    ...itemTransactionsProducer,
    data: itemTransactionsProducer.data.map((t) => ({
      ...t,
      cycles: null,
      cyclesStatus: 'pending',
    })),
  });
  render(<Page identityId={evidence.itemId} />);
  fireEvent.click(await screen.findByRole('tab', { name: 'Transactions' }));
  await screen.findByRole('table', { name: 'Asset transactions' });
  expect(screen.getByRole('table', { name: 'Asset transactions' })).toHaveTextContent(
    'Not computed'
  );
  expect(screen.getByRole('table', { name: 'Asset transactions' })).toBeInTheDocument();
  expect(api.getItemTransactions).toHaveBeenCalledWith('dotbit', evidence.itemId, {
    limit: 50,
    cursor: undefined,
  });
});
it('starts a new identity on its own first cursor page', async () => {
  vi.mocked(api.getItemActivityFeed).mockResolvedValue({
    ...emptyPage(),
    total: 100,
    hasMore: true,
    nextCursor: '90:0',
  });
  const { rerender } = render(<Page identityId={evidence.itemId} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('dotbit', evidence.itemId, {
      limit: 50,
      cursor: '90:0',
    })
  );
  const next = '0x' + '66'.repeat(32);
  vi.mocked(api.getDotbitItemDetail).mockResolvedValue({ ...identityItem('dotbit'), nftId: next });
  query = '';
  rerender(<Page identityId={next} />);
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('dotbit', next, {
      limit: 50,
      cursor: undefined,
    })
  );
});
it('reports missing identity and unavailable raw evidence without inventing a document', async () => {
  vi.mocked(api.getDotbitItemDetail).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Identity not found')
  );
  const { unmount } = render(<Page identityId={evidence.itemId} />);
  expect(await screen.findByRole('heading', { name: 'An unanswered name.' })).toBeInTheDocument();
  expect(screen.getByText('checked').nextElementSibling).toHaveTextContent(
    `/assets/identities/dotbit/items/${evidence.itemId}`
  );
  unmount();
  vi.mocked(api.getDotbitItemDetail).mockResolvedValue(identityItem('dotbit'));
  vi.mocked(api.getItemProtocol).mockRejectedValue(new Error('missing local cell bytes'));
  render(<Page identityId={evidence.itemId} />);
  expect(
    (await screen.findAllByRole('alert')).some((a) =>
      a.textContent?.includes('missing local cell bytes')
    )
  ).toBe(true);
});

it('keeps a failed identity source distinct from an absent identity', async () => {
  vi.mocked(api.getDotbitItemDetail).mockRejectedValue(
    new ApiRequestError(500, 'internal_error', 'identity index mismatch')
  );
  render(<Page identityId={evidence.itemId} />);
  expect(await screen.findByRole('alert')).toHaveTextContent('identity index mismatch');
  expect(screen.queryByText('An unanswered name.')).not.toBeInTheDocument();
});

// ── F2 identities (round 2): the IdentityBit board's tier 1 (ID-C5/C6/C7/C10, ID-B1/B2/B4) ──
it('reads like the .bit board: badge, summary, held-until bar, domain facts, internals in tier 2', async () => {
  render(<Page identityId={evidence.itemId} />);
  const facts = await screen.findByRole('complementary', { name: 'Facts' }, { timeout: 4000 });
  expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('alice.bit');
  expect(
    screen.getByRole('heading', { level: 1 }).querySelector('.text-identity')
  ).toHaveTextContent('.bit');
  expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent(
    'Inventory/Identities/.bit'
  );
  expect(screen.getByText('Live')).toHaveAttribute('data-identity-state', 'live');
  expect(screen.getByText('Pure CKB')).toBeInTheDocument();
  expect(screen.getByTestId('identity-summary')).toHaveTextContent(
    'Its owner has never changed. 1 record is set.'
  );
  expect(screen.getByRole('heading', { name: 'Held until 2030-03-18' })).toBeInTheDocument();
  expect(Array.from(facts.querySelectorAll('dt')).map((dt) => dt.textContent)).toEqual([
    'Owner',
    'Expires',
    'Registered',
    'Records',
    'CKB held in its cell',
  ]);
  // The record is named by its coin (ID-DC1 rule, shared by .bit).
  expect(screen.getByRole('region', { name: 'What alice.bit points to' })).toHaveTextContent(
    'Ethereum'
  );
  // Raw status / witness version / lock args are tier 2 only (ID-B1, ID-B2).
  const tier1 = document.querySelector('[data-tier="1"]')!;
  expect(tier1).not.toHaveTextContent(/witness version|account status|algorithm \d/);
  expect(
    screen.getByRole('region', { name: 'Account data, from the witness' }).closest('[data-tier]')
  ).toHaveAttribute('data-tier', '2');
  // No second heading repeating the name (ID-B4); no raw Unix seconds in tier 1 (ID-C7) — the
  // decoded JSON in tier 2 keeps them as the evidence they are.
  expect(screen.queryByRole('heading', { level: 2, name: 'alice.bit' })).not.toBeInTheDocument();
  expect(tier1).not.toHaveTextContent(/Unix|1900000000/);
});
