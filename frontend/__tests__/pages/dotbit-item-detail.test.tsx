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
  expect(await screen.findByLabelText('Raw item evidence')).toHaveTextContent('cell data');
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
  expect(await screen.findByText('Recycled / inactive')).toBeInTheDocument();
  expect(await screen.findByText('No live cell')).toBeInTheDocument();
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
