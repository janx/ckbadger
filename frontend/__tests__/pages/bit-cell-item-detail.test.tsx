import { render, screen, fireEvent, waitFor } from '@/__tests__/utils/test-utils';
import Page from '@/app/identities/bit-cell/[identityId]/client-page';
import { api, ApiRequestError, type ItemCellVersions } from '@/lib/api';
import { mockItemReads, identityItem, emptyPage } from '@/__tests__/utils/item-read-fixtures';
import {
  bitCellProtocolProducer as evidence,
  itemActivityFeedProducer,
  itemTransactionsProducer,
} from '@/__tests__/msw/fixtures/item-producer';
vi.mock('@/components/layout/header', () => ({ Header: () => null }));
let query = '';
const replace = vi.fn();
vi.mock('@/src/navigation', () => ({
  useSearchParams: () => new URLSearchParams(query),
  usePathname: () => '/identities/bit-cell/item',
  useRouter: () => ({ replace }),
}));
beforeEach(() => {
  mockItemReads();
  query = '';
  replace.mockClear();
});
afterEach(() => vi.restoreAllMocks());
it('renders exact canonical protocol, whole cell history and raw bytes for bit-cell', async () => {
  render(<Page identityId={evidence.itemId} />);
  expect(await screen.findByRole('heading', { level: 1 })).toHaveClass('text-[26px]');
  expect(await screen.findByLabelText('Raw item evidence')).toHaveTextContent('cell data');
  expect(api.getBitCellItemDetail).toHaveBeenCalledWith(evidence.itemId);
  expect(api.getItemProtocol).toHaveBeenCalledWith('bit-cell', evidence.itemId);
  const link = screen
    .getAllByRole('link')
    .find(
      (a) =>
        a.getAttribute('href') ===
        `/mainnet/cell/${evidence.versions[0].cell.txHash}/${evidence.versions[0].cell.outputIndex}`
    );
  expect(link).toBeDefined();
});
it('keeps recycled identity history while showing no live capacity', async () => {
  vi.mocked(api.getBitCellItemDetail).mockResolvedValue({
    ...identityItem('bit-cell'),
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
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('bit-cell', evidence.itemId, {
      limit: 50,
      cursor: '100:0',
    })
  );
  fireEvent.click(await screen.findByRole('button', { name: 'Next' }));
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('bit-cell', evidence.itemId, {
      limit: 50,
      cursor: '90:0',
    })
  );
  expect(replace).toHaveBeenCalledWith('/identities/bit-cell/item?activity_cursor=90%3A0', {
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
  expect(api.getItemTransactions).toHaveBeenCalledWith('bit-cell', evidence.itemId, {
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
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('bit-cell', evidence.itemId, {
      limit: 50,
      cursor: '90:0',
    })
  );
  const next = '0x' + '66'.repeat(32);
  vi.mocked(api.getBitCellItemDetail).mockResolvedValue({
    ...identityItem('bit-cell'),
    nftId: next,
  });
  query = '';
  rerender(<Page identityId={next} />);
  await waitFor(() =>
    expect(api.getItemActivityFeed).toHaveBeenCalledWith('bit-cell', next, {
      limit: 50,
      cursor: undefined,
    })
  );
});
it('reports missing identity and unavailable raw evidence without inventing a document', async () => {
  vi.mocked(api.getBitCellItemDetail).mockRejectedValue(
    new ApiRequestError(404, 'not_found', 'Identity not found')
  );
  const { unmount } = render(<Page identityId={evidence.itemId} />);
  expect(await screen.findByRole('heading', { name: 'An unanswered name.' })).toBeInTheDocument();
  expect(
    screen.getByText(`/assets/identities/bit-cell/items/${evidence.itemId}`)
  ).toBeInTheDocument();
  unmount();
  vi.mocked(api.getBitCellItemDetail).mockResolvedValue(identityItem('bit-cell'));
  vi.mocked(api.getItemProtocol).mockRejectedValue(new Error('missing local cell bytes'));
  render(<Page identityId={evidence.itemId} />);
  expect(
    (await screen.findAllByRole('alert')).some((a) =>
      a.textContent?.includes('missing local cell bytes')
    )
  ).toBe(true);
});
