import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import { ledeRestatements } from '@/__tests__/utils/lede-rule';
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
  expect(await screen.findByRole('heading', { level: 1 })).toHaveClass('text-entity');
  expect(await screen.findByLabelText('Raw item evidence')).toHaveTextContent(
    /[Cc]ell data · \d+ bytes/
  );
  // Lane E (C5): the cell data is a byte map whose detail box gives each field its meaning.
  expect(
    within(screen.getByLabelText('Raw item evidence')).getByRole('complementary', {
      name: /field detail$/,
    })
  ).toHaveTextContent(
    'accountDataHash[0..32) · 32 BHash of the account’s data, which the transaction’s witness carries. 32 bytes.'
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
    bitCells: [],
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
      limit: 20,
      cursor: '100:0',
    })
  );
  fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('dotbit', evidence.itemId, {
      limit: 20,
      cursor: '90:0',
    })
  );
  expect(replace).toHaveBeenCalledWith('/identities/dotbit/item?activity_cursor=90%3A0', {
    scroll: false,
  });
});
it('uses the independent Transactions layer, without a Cycles column (ID-C8)', async () => {
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
  // ID-C8: the identity boards carry no Cycles (or Output capacity) column, so a pending cycles
  // count is not shown at all rather than as "Not computed".
  const table = screen.getByRole('table', { name: 'Asset transactions' });
  expect(table).not.toHaveTextContent('Not computed');
  expect(Array.from(table.querySelectorAll('th')).map((th) => th.textContent)).not.toContain(
    'Cycles'
  );
  expect(screen.getByRole('table', { name: 'Asset transactions' })).toBeInTheDocument();
  expect(api.getItemTransactions).toHaveBeenCalledWith('dotbit', evidence.itemId, {
    limit: 20,
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
      limit: 20,
      cursor: '90:0',
    })
  );
  const next = '0x' + '66'.repeat(32);
  vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
    ...identityItem('dotbit'),
    bitCells: [],
    nftId: next,
  });
  query = '';
  rerender(<Page identityId={next} />);
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('dotbit', next, {
      limit: 20,
      cursor: undefined,
    })
  );
});
// Visual finish V8: the page loads as the shared detail skeleton, not the asset shell with a
// "Loading identity…" line.
it('shows the detail skeleton while the identity loads', () => {
  vi.mocked(api.getDotbitItemDetail).mockReturnValue(new Promise(() => {}));
  render(<Page identityId={evidence.itemId} />);
  const skeleton = screen.getByRole('status', { name: 'Loading …' });
  expect(skeleton.children).toHaveLength(3);
  expect(screen.queryByText('Loading identity…')).toBeNull();
  expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
});

it('reports missing identity and unavailable raw evidence without inventing a document', async () => {
  vi.mocked(api.getDotbitItemDetail).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Identity not found')
  );
  const { unmount } = render(<Page identityId={evidence.itemId} />);
  expect(
    await screen.findByRole('heading', {
      level: 1,
      name: "no identity at this address. the local index was asked; it holds none. honey badger don't care.",
    })
  ).toBeInTheDocument();
  expect(
    screen.getByText(`/assets/identities/dotbit/items/${evidence.itemId}`)
  ).toBeInTheDocument();
  unmount();
  vi.mocked(api.getDotbitItemDetail).mockResolvedValue({ ...identityItem('dotbit'), bitCells: [] });
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
  expect(screen.queryByText('the local index was asked; it holds none.')).not.toBeInTheDocument();
});

// ── F2 identities (round 2): the IdentityBit board's tier 1 (ID-C5/C6/C7/C10, ID-B1/B2/B4) ──
it('reads like the .bit board: badge, summary, held-until bar, domain facts, internals in tier 2', async () => {
  const { container } = render(<Page identityId={evidence.itemId} />);
  const facts = await screen.findByRole('region', { name: 'Metrics' }, { timeout: 4000 });
  expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('alice.bit');
  // Consistency plan C1: the tab names the page.
  expect(document.title).toBe('alice.bit · identity · CKBadger');
  expect(
    screen.getByRole('heading', { level: 1 }).querySelector('.text-identity')
  ).toHaveTextContent('.bit');
  expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toHaveTextContent(
    'Inventory/Identities/.bit'
  );
  expect(screen.getByText('Live')).toHaveAttribute('data-identity-state', 'live');
  expect(screen.getByText('Pure CKB')).toBeInTheDocument();
  // C7: the summary restated the metrics row; it is tier 2's "in words" now, and the lede (the
  // standard's sentence) restates none of the row.
  const summary = screen.getByTestId('identity-summary');
  expect(summary).toHaveTextContent('Its owner has never changed. 1 record is set.');
  expect(summary.closest('[data-tier]')).toHaveAttribute('data-tier', '2');
  expect(screen.getByRole('heading', { name: 'This identity, in words' })).toBeInTheDocument();
  expect(ledeRestatements(container)).toEqual([]);
  expect(screen.queryByRole('complementary', { name: 'Facts' })).toBeNull();
  expect(screen.getByRole('heading', { name: 'Term held until 2030-03-18' })).toBeInTheDocument();
  expect(Array.from(facts.querySelectorAll('dt')).map((dt) => dt.textContent)).toEqual([
    'Owner',
    'Expires',
    'Registered',
    'Records',
    // Round 3 (L3): the reverse link to the .bit Cells naming the account.
    '.bit Cells',
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

it('puts the metrics row right after the header in reading order (ID-C11, C7)', async () => {
  render(<Page identityId={evidence.itemId} />);
  const facts = await screen.findByRole('region', { name: 'Metrics' }, { timeout: 4000 });
  const term = screen.getByRole('heading', { name: /^Term held until/ });
  expect(facts.compareDocumentPosition(term) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

// Round 3 (contracts L3, ID-B-N1): the .bit page links back to the .bit Cells naming it.
const bitCell = (n: number, over: Record<string, unknown> = {}) => ({
  id: `0x${n.toString(16).padStart(2, '0').repeat(32)}`,
  name: 'alice.bit',
  ownerLockHash: `0x${'12'.repeat(32)}`,
  ownerAddress: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq4s93hhsj',
  // 2030-03-17 17:46:40 UTC = 2030-03-18 01:46:40 UTC+8
  expiredAt: 1900000000,
  ...over,
});
it('lists every live .bit Cell that names the account, linked to its page (L3)', async () => {
  vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
    ...identityItem('dotbit'),
    bitCells: [bitCell(0xb1), bitCell(0xb2, { name: null })],
  });
  render(<Page identityId={evidence.itemId} />);
  const fact = await screen.findByText('.bit Cells', { selector: 'dt' });
  const value = fact.parentElement!.querySelector('dd')!;
  const links = Array.from(value.querySelectorAll('a'));
  expect(links.map((a) => a.getAttribute('href'))).toEqual([
    `/mainnet/identities/bit-cell/0x${'b1'.repeat(32)}`,
    `/mainnet/identities/bit-cell/0x${'b2'.repeat(32)}`,
  ]);
  expect(value).toHaveTextContent('alice.bit · expires 2030-03-18');
  // A .bit Cell whose data names no account text is named by its id.
  expect(value).toHaveTextContent('0xb2b2b2b2...b2b2');
});
it('says no .bit Cell names the account when there is none', async () => {
  render(<Page identityId={evidence.itemId} />);
  const fact = await screen.findByText('.bit Cells', { selector: 'dt' });
  expect(fact.parentElement!.querySelector('dd')).toHaveTextContent('none');
});
it('says nothing of .bit Cells while the index is not derived (L3: bitCells null)', async () => {
  vi.mocked(api.getDotbitItemDetail).mockResolvedValue({
    ...identityItem('dotbit'),
    bitCells: null,
  });
  render(<Page identityId={evidence.itemId} />);
  expect(await screen.findByText('Registered', { selector: 'dt' })).toBeInTheDocument();
  expect(screen.queryByText('.bit Cells', { selector: 'dt' })).toBeNull();
  expect(screen.queryByRole('alert')).toBeNull();
});

// ID-C8 (IdentityBit board): the Transactions tab reads Transaction · Block · date · .bit action ·
// Cells · Fee · Size · This name's cell after it.
it('lays the .bit Transactions out in the board columns (ID-C8)', async () => {
  render(<Page identityId={evidence.itemId} />);
  fireEvent.click(await screen.findByRole('tab', { name: 'Transactions' }));
  const table = await screen.findByRole('table', { name: 'Asset transactions' });
  const headers = Array.from(table.querySelectorAll('th')).map((th) =>
    th.textContent!.replace(/0[\d,]+$/, '')
  );
  expect(headers).toEqual([
    'Transaction',
    'Block · date',
    '.bit action',
    'Cells',
    'Fee',
    'Size, bytes',
    "This name's cell after it",
  ]);
  const row = table.querySelector('tbody tr')!;
  expect(row).toHaveTextContent(/cell after/);
});
