import { render, screen, fireEvent, waitFor, within } from '@/__tests__/utils/test-utils';
import { expectHeadId } from '@/__tests__/utils/head-id';
import Page from '@/app/identities/did/[identityId]/client-page';
import { api, ApiRequestError, type ItemCellVersion, type ItemCellVersions } from '@/lib/api';
import { mockItemReads, identityItem, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import {
  didProtocolProducer as evidence,
  itemActivityFeedProducer,
  itemTransactionsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
import { pagedRead } from '@/__tests__/utils/paged-read';
// re-pinned (lane c2-assets): the feed's tab row is the controls library's Tabs, a command line
// whose names are lowercase ('activities', 'transactions'; were 'Activities', 'Transactions'), and
// its pager is the board's, a list in time ('older →', '← newer'; were 'Next', 'Previous')
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
let query = '';
const replace = vi.fn();
vi.mock('@/src/navigation', () => ({
  useSearchParams: () => new URLSearchParams(query),
  usePathname: () => '/identities/did/item',
  useRouter: () => ({ replace }),
}));
beforeEach(() => {
  mockItemReads();
  query = '';
  replace.mockClear();
});
afterEach(() => vi.restoreAllMocks());
it('renders exact canonical protocol, whole cell history and raw bytes for did', async () => {
  render(<Page identityId={evidence.itemId} />);
  expect(await screen.findByRole('heading', { level: 1 })).toHaveClass('text-entity');
  expect(await screen.findByLabelText('Raw item evidence')).toHaveTextContent(
    /[Cc]ell data · \d+ bytes/
  );
  expect(api.getDidCkbItemDetail).toHaveBeenCalledWith(evidence.itemId);
  expect(api.getItemProtocol).toHaveBeenCalledWith('did', evidence.itemId);
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
  vi.mocked(api.getDidCkbItemDetail).mockResolvedValue({
    ...identityItem('did'),
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
  expect(await screen.findByText('Inactive')).toBeInTheDocument();
  expect(await screen.findByText('no live cell')).toBeInTheDocument();
  expect(screen.getAllByText('0').length).toBeGreaterThan(0);
  expect(screen.getByLabelText('Raw item evidence')).toBeInTheDocument();
});
it('hydrates the explicit activity cursor and advances full-record pagination', async () => {
  query = 'activity_cursor=100:0';
  vi.mocked(api.getItemActivityFeed).mockResolvedValue(
    pagedRead({
      ...itemActivityFeedProducer,
      total: 100,
      hasMore: true,
      nextCursor: '90:0',
    })
  );
  render(<Page identityId={evidence.itemId} />);
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('did', evidence.itemId, {
      limit: 20,
      cursor: '100:0',
    })
  );
  fireEvent.click(await screen.findByRole('button', { name: 'older →' }));
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('did', evidence.itemId, {
      limit: 20,
      cursor: '90:0',
    })
  );
  expect(replace).toHaveBeenCalledWith('/identities/did/item?activity_cursor=90%3A0', {
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
  fireEvent.click(await screen.findByRole('tab', { name: 'transactions' }));
  await screen.findByRole('table', { name: 'Asset transactions' });
  // ID-C8: the identity boards carry no Cycles (or Output capacity) column, so a pending cycles
  // count is not shown at all rather than as "Not computed".
  const table = screen.getByRole('table', { name: 'Asset transactions' });
  expect(table).not.toHaveTextContent('Not computed');
  expect(Array.from(table.querySelectorAll('th')).map((th) => th.textContent)).not.toContain(
    'Cycles'
  );
  expect(screen.getByRole('table', { name: 'Asset transactions' })).toBeInTheDocument();
  expect(api.getItemTransactions).toHaveBeenCalledWith('did', evidence.itemId, {
    limit: 20,
    cursor: undefined,
  });
});
it('starts a new identity on its own first cursor page', async () => {
  vi.mocked(api.getItemActivityFeed).mockResolvedValue(
    pagedRead({
      // a full page of one captured activity with more after it (lane cd-fixes-controls,
      // SHOULD-5: an empty page that said more followed, a page the API cannot send)
      ...itemActivityFeedProducer,
      total: 100,
      hasMore: true,
      nextCursor: '90:0',
    })
  );
  const { rerender } = render(<Page identityId={evidence.itemId} />);
  fireEvent.click(await screen.findByRole('button', { name: 'older →' }));
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('did', evidence.itemId, {
      limit: 20,
      cursor: '90:0',
    })
  );
  const next = '0x' + '66'.repeat(32);
  vi.mocked(api.getDidCkbItemDetail).mockResolvedValue({ ...identityItem('did'), nftId: next });
  query = '';
  rerender(<Page identityId={next} />);
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('did', next, {
      limit: 20,
      cursor: undefined,
    })
  );
});
// Visual finish V8: the page loads as the shared detail skeleton, not the asset shell with a
// "Loading identity…" line.
it('shows the detail skeleton while the identity loads', () => {
  vi.mocked(api.getDidCkbItemDetail).mockReturnValue(new Promise(() => {}));
  render(<Page identityId={evidence.itemId} />);
  const skeleton = screen.getByRole('status', { name: 'Loading …' });
  expect(skeleton.children).toHaveLength(3);
  expect(screen.queryByText('Loading identity…')).toBeNull();
  expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
});

it('reports missing identity and unavailable raw evidence without inventing a document', async () => {
  vi.mocked(api.getDidCkbItemDetail).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Identity not found')
  );
  const { unmount } = render(<Page identityId={evidence.itemId} />);
  expect(
    await screen.findByRole('heading', {
      level: 1,
      name: "no identity with this id. the local index was asked; it holds none. honey badger don't care.",
    })
  ).toBeInTheDocument();
  // U-18: the facts name the checked endpoint with the id shortened, once
  expect(document.querySelector('.error-facts [data-endpoint]')).toHaveTextContent(
    `/assets/identities/did/items/${evidence.itemId.slice(0, 10)}…${evidence.itemId.slice(-6)}`
  );
  unmount();
  vi.mocked(api.getDidCkbItemDetail).mockResolvedValue(identityItem('did'));
  vi.mocked(api.getItemProtocol).mockRejectedValue(new Error('missing local cell bytes'));
  render(<Page identityId={evidence.itemId} />);
  expect(
    (await screen.findAllByRole('alert')).some((a) =>
      a.textContent?.includes('missing local cell bytes')
    )
  ).toBe(true);
});

it('keeps a failed identity source distinct from an absent identity', async () => {
  vi.mocked(api.getDidCkbItemDetail).mockRejectedValue(
    new ApiRequestError(500, 'internal_error', 'identity index mismatch')
  );
  render(<Page identityId={evidence.itemId} />);
  expect(await screen.findByRole('alert')).toHaveTextContent('identity index mismatch');
  expect(screen.queryByText('the local index was asked; it holds none.')).not.toBeInTheDocument();
});

// ── F2 identities (round 2): IdentityDid — titled by its handle, the document read (ID-D1/D2/D3) ──
it('titles a did:ckb by its handle, reads its document and places it among did:ckb', async () => {
  render(<Page identityId={evidence.itemId} />);
  await waitFor(() =>
    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('alice.test')
  );
  // re-pinned on purpose (lane P2's third round, item 6): the handle is the title whole, in the
  // title's ink; its personal data server's domain is no standard's suffix and is not drawn in the
  // rose a .bit's or a .cell's suffix takes (the review: it read as a top-level domain)
  expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent(/^alice\.test$/);
  expect(screen.getByRole('heading', { level: 1 }).querySelector('.text-identity')).toBeNull();
  // The DID string is the identity line under the title (with copy), not the title.
  expect(screen.getByRole('heading', { level: 1 }).closest('header')).toHaveTextContent(
    'did:ckb:ceirceirceirceirceirceirceirceir'
  );
  // Lane c2-finish, item 4: the DID is the head's id as every head's is (HeadId: one line or
  // equal lines, never a scroller; it was HexCode's own-box scroller, copied by a click on it),
  // the copy button after it
  expectHeadId('did:ckb:ceirceirceirceirceirceirceirceir');
  expect(screen.getByRole('heading', { level: 1 }).closest('header')).toContainElement(
    screen.getByRole('button', { name: 'copy DID' })
  );
  const doc = screen.getByRole('region', { name: 'DID document' });
  expect(doc).toHaveTextContent('at://alice.test');
  expect(doc).toHaveTextContent('Personal data server');
  expect(doc).not.toHaveTextContent('Document version 1');
  const among = await screen.findByRole('region', { name: 'Among did:ckb' });
  expect(among).toHaveTextContent('this one');
  expect(screen.getByRole('region', { name: 'Metrics' })).toHaveTextContent('Expiry');
  expect(screen.getByRole('region', { name: 'Metrics' })).toHaveTextContent('none');
});

/**
 * Versions of one cell, oldest first, the last live: the same capacity, byte counts and lock
 * address throughout, each given its own data hash (and its own type, where `types` says so), as a
 * name's cell is recreated under one lock by a transfer, a renewal or an edit of the same length.
 */
function sameShapeVersions(
  base: ItemCellVersion,
  dataHashes: readonly string[],
  types: readonly (ItemCellVersion['type'] | undefined)[] = []
): ItemCellVersions {
  const data = dataHashes.map((dataHash, i): ItemCellVersion => {
    const last = i === dataHashes.length - 1;
    return {
      ...base,
      txHash: `0x${String(i + 1)
        .padStart(2, '0')
        .repeat(32)}`,
      outputIndex: 0,
      blockNumber: 100 + i,
      txIndex: 0,
      dataHash,
      type: types[i] ?? base.type,
      isLive: last,
      consumedByTx: last
        ? null
        : `0x${String(i + 2)
            .padStart(2, '0')
            .repeat(32)}`,
      consumedAtBlock: last ? null : 101 + i,
    };
  });
  return { data, total: data.length };
}
const hashOf = (b: string) => `0x${b.repeat(32)}`;

// Lane P2's fifth round (item 1, MUST): a did:ckb document edited to the same length keeps the
// cell's capacity, byte counts and lock, and changes its data: never folded as "the same bytes".
it('never folds a document edited to the same length', async () => {
  const base = evidence.versions[0].cell as ItemCellVersion;
  vi.mocked(api.getItemCellVersions).mockResolvedValue(
    sameShapeVersions(base, [hashOf('d1'), hashOf('d2'), hashOf('d3'), hashOf('d4')])
  );
  render(<Page identityId={evidence.itemId} />);
  const list = await screen.findByLabelText(/cell versions$/);
  expect(within(list).getAllByRole('article')).toHaveLength(4);
  expect(list.querySelector('[data-versions-fold]')).toBeNull();
});

// Lane P2's fifth round (NOTE 8): an activity feed opened at a URL cursor on its oldest page (no
// more after it) drew no pager, so it had no way back to the newest page; "← newer" stands there,
// taking the feed to its newest page, and "older →" is an end that cannot be taken.
it('keeps the way to the newest page on a feed opened at a URL cursor on its oldest page', async () => {
  query = 'activity_cursor=10:0';
  vi.mocked(api.getItemActivityFeed).mockResolvedValue(
    pagedRead({
      ...itemActivityFeedProducer,
      total: 100,
      hasMore: false,
      nextCursor: null,
    })
  );
  render(<Page identityId={evidence.itemId} />);
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('did', evidence.itemId, {
      limit: 20,
      cursor: '10:0',
    })
  );
  fireEvent.click(await screen.findByRole('button', { name: '← newer' }));
  expect(screen.queryByRole('button', { name: 'older →' })).toBeNull();
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('did', evidence.itemId, {
      limit: 20,
      cursor: undefined,
    })
  );
});
