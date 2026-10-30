import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { dailyActivities, feedSamples } from '@/__tests__/msw/fixtures/home';
import TransactionsPage from '@/app/transactions/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', async () => ({
  ...(await vi.importActual('@/lib/api')),
  api: {
    getTransactions: vi.fn(),
    getPoolTransactions: vi.fn(),
    getDailyActivityStats: vi.fn(),
    getTxStats: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const emptyPool = {
  summary: { size: '0', cycles: '0', minFeeRate: null, maxFeeRate: null, newestSeenAt: null },
  data: [],
  total: 0,
  limit: 50,
  nextCursor: null,
  totals: { pending: 0, proposed: 0, awaitingIndex: 0 },
  truncated: false,
  lastPolledAt: null,
};
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getPoolTransactions).mockResolvedValue(emptyPool);
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([]);
  vi.mocked(api.getTxStats).mockResolvedValue({
    currentHour: 0,
    currentDay: 0,
    hourlyData: [],
    dailyData: [],
  });
});

describe('TransactionsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders neutral transaction hashes and block links from fetched transactions', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [
        {
          hash: '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          blockNumber: 123456,
          blockHash: '0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          index: 0,
          inputsCount: 2,
          outputsCount: 3,
          fee: '100000000',
          value: '123456789012345678',
          txSize: 226,
          cycles: 1000000,
          cyclesStatus: 'complete',
          activity: feedSamples.eightParties,
          isCellbase: false,
          timestamp: '2026-02-20T00:00:00Z',
        },
      ],
      total: 1,
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });

    render(<TransactionsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Transactions')).toBeInTheDocument();
    expect(
      screen.queryByText('Browse all transactions on the CKB network')
    ).not.toBeInTheDocument();

    await waitFor(() => {
      expect(api.getTransactions).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
    });

    await waitFor(() => {
      expect(screen.getAllByRole('link', { name: '#123,456' })).toHaveLength(2);
    });
    expect(
      screen
        .getAllByRole('link', { name: '#123,456' })
        .every((link) => link.getAttribute('href') === '/mainnet/blocks/123456')
    ).toBe(true);

    const hash = '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
    expect(screen.getAllByTitle(hash)).toHaveLength(2);
    for (const hashDisplay of screen.getAllByTitle(hash)) {
      expect(hashDisplay).toHaveClass('text-text', 'text-xs');
      expect(hashDisplay.children).toHaveLength(0);
      expect(hashDisplay.closest('a')).toHaveAttribute('href', `/mainnet/tx/${hash}`);
    }
    expect(screen.queryByTitle(/Click to copy/)).not.toBeInTheDocument();
    expect(screen.getAllByText('2 → 3')).toHaveLength(2);
    // F2-tx (TX-25 / TX-26): the board's columns, newest first with cellbase rows set back.
    const table = screen.getByRole('table', { name: 'Committed transactions' });
    expect(table).toHaveTextContent('What it did');
    expect(screen.getByText('newest first · cellbase rows set back')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Older →' })).toBeDisabled();
  });
});

describe('Transactions pool view', () => {
  it('keeps full pool evidence inside the desktop width budget without shortening exact amounts', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      ...emptyPool,
      summary: {
        size: '226',
        cycles: '3500000000',
        minFeeRate: '54623',
        maxFeeRate: '54623',
        newestSeenAt: '1792886400000',
      },
      total: 1,
      totals: { pending: 0, proposed: 1, awaitingIndex: 0 },
      data: [
        {
          hash: `0x${'ab'.repeat(32)}`,
          status: 'proposed',
          value: '18446744073709551615',
          fee: '12345',
          feeRate: '54623',
          txSize: 226,
          cycles: '3500000000',
          inputsCount: 2,
          outputsCount: 3,
          timestamp: '1792886400000',
          interpretation: { status: 'complete', reasons: [] },
          activity: feedSamples.eightParties,
        },
      ],
    });
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
    const table = await screen.findByRole('table', { name: 'Transactions in the pool' });
    expect(Number.parseFloat(table.style.minWidth)).toBeLessThanOrEqual(1248);
    expect(table).toHaveTextContent('184,467,440,737.09551615 CKB');
    expect(table).toHaveTextContent('0.00012345 CKB');
    expect(table).toHaveTextContent('3,500,000,000');
    expect(table.querySelectorAll('[data-ckb-amount]')).toHaveLength(2);
    for (const amount of table.querySelectorAll('[data-ckb-amount]'))
      expect(amount).toHaveClass('flex-wrap');
  });
  it('reads the paged pool with neutral hashes and keeps partial interpretation explicit', async () => {
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [],
      limit: 50,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getPoolTransactions).mockResolvedValue({
      summary: {
        size: '226',
        cycles: '1709221',
        minFeeRate: '54623',
        maxFeeRate: '54623',
        newestSeenAt: '1792886400000',
      },
      data: [
        {
          hash: `0x${'ab'.repeat(32)}`,
          status: 'proposed',
          value: null,
          fee: '12345',
          feeRate: '54623',
          txSize: 226,
          cycles: '1709221',
          inputsCount: 2,
          outputsCount: 3,
          timestamp: '1792886400000',
          interpretation: { status: 'partial', reasons: [{ code: 'unresolved_input' }] },
          activity: null,
        },
      ],
      total: 1,
      limit: 50,
      nextCursor: null,
      totals: { pending: 0, proposed: 1, awaitingIndex: 0 },
      truncated: false,
      lastPolledAt: '2026-10-25T00:00:00Z',
    });
    render(<TransactionsPage />);
    fireEvent.click(screen.getByRole('button', { name: /^In the pool/ }));
    expect(
      await screen.findByRole('table', { name: 'Transactions in the pool' })
    ).toHaveTextContent('Interpretation partial');
    expect(api.getPoolTransactions).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
    expect(screen.getByTestId('pool-banner')).toHaveTextContent(
      '0 pending · 1 proposed · 0 awaiting index'
    );
    const summary = screen.getByLabelText('Pool summary');
    expect(summary).toHaveTextContent('Pending · proposed0 · 1');
    expect(summary).toHaveTextContent('Total size226 B');
    expect(summary).toHaveTextContent('Total cycles1,709,221');
    expect(summary).toHaveTextContent('Fee rate, shannon/KB54,623 – 54,623');
    expect(
      screen.getByText(
        'Newest first, 50 a page. A proposed transaction enters a block 2–10 blocks after its proposal.'
      )
    ).toBeInTheDocument();
    expect(screen.getByText('not yet in a block · newest first')).toBeInTheDocument();
    const pool = screen.getByRole('table', { name: 'Transactions in the pool' });
    expect(pool).toHaveTextContent('Status · waiting');
    expect(pool).not.toHaveTextContent('Seen');
    for (const hash of screen.getAllByTitle(`0x${'ab'.repeat(32)}`)) {
      expect(hash).toHaveClass('text-text', 'text-xs');
      expect(hash).toHaveTextContent('0xababababab...ababab');
      expect(hash.children).toHaveLength(0);
      expect(hash.closest('a')).toHaveAttribute('href', `/mainnet/tx/0x${'ab'.repeat(32)}`);
    }
  });
});

it('keeps the pool banner above tabs and uses exact day counts even when kinds overlap', async () => {
  const yesterday = new Date(Date.now() + 8 * 3600000 - 86400000).toISOString().slice(0, 10);
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([
    {
      ...dailyActivities[0],
      date: yesterday,
      transferCount: 12,
      tokenCount: 11,
      coinbaseCount: 3,
      daoDepositCount: 2,
      daoWithdrawRequestCount: 1,
      daoWithdrawCompleteCount: 0,
    },
  ]);
  vi.mocked(api.getTxStats).mockResolvedValue({
    currentHour: 0,
    currentDay: 500,
    hourlyData: [],
    dailyData: [{ label: yesterday.slice(5).replace('-', '/'), value: 20, complete: true }],
  });
  vi.mocked(api.getTransactions).mockResolvedValue({
    data: [],
    limit: 50,
    nextCursor: null,
    hasMore: false,
    total: 0,
  });
  render(<TransactionsPage />);
  expect(await screen.findByRole('button', { name: 'All' })).toBeInTheDocument();
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'All' })).toHaveTextContent('All20')
  );
  expect(screen.getByRole('button', { name: 'Nervos DAO' })).toHaveTextContent(/^Nervos DAO3$/);
  expect(screen.getByText(/DAO counts actions/)).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Tokens' })).toHaveTextContent('11');
  expect(screen.getByRole('button', { name: 'Cellbase' })).toHaveTextContent('3');
  expect(
    screen
      .getByTestId('pool-banner')
      .compareDocumentPosition(screen.getByRole('group', { name: 'Transaction views' })) &
      Node.DOCUMENT_POSITION_FOLLOWING
  ).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: 'Tokens' }));
  await waitFor(() =>
    expect(api.getTransactions).toHaveBeenLastCalledWith({
      cursor: undefined,
      limit: 50,
      activityType: 'token',
    })
  );
  fireEvent.click(screen.getByRole('button', { name: 'Open the pool →' }));
  await waitFor(() =>
    expect(api.getPoolTransactions).toHaveBeenCalledWith({
      cursor: undefined,
      limit: 50,
      activityType: 'token',
    })
  );
  expect(screen.getByRole('link', { name: 'Activities →' })).toHaveAttribute(
    'href',
    '/mainnet/activities'
  );
});
it('does not infer a transaction total from kind counts when yesterday is incomplete', async () => {
  const yesterday = new Date(Date.now() + 8 * 3600000 - 86400000).toISOString().slice(0, 10);
  vi.mocked(api.getDailyActivityStats).mockResolvedValue([
    { ...dailyActivities[0], date: yesterday },
  ]);
  vi.mocked(api.getTxStats).mockResolvedValue({
    currentHour: 0,
    currentDay: 500,
    hourlyData: [],
    dailyData: [{ label: yesterday.slice(5).replace('-', '/'), value: 999, complete: false }],
  });
  vi.mocked(api.getTransactions).mockResolvedValue({
    data: [],
    limit: 50,
    nextCursor: null,
    hasMore: false,
  });
  render(<TransactionsPage />);
  expect(
    await screen.findByText(`No complete transaction count for ${yesterday} (UTC+8).`)
  ).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'All' })).toHaveTextContent(/^All$/);
});

it('draws awaiting-index pool squares hollow, apart from pending (cyan) and proposed (gold) (TX-38)', async () => {
  vi.mocked(api.getTransactions).mockResolvedValue({
    data: [],
    limit: 50,
    hasMore: false,
    nextCursor: null,
  });
  const row = (n: number, status: 'pending' | 'proposed' | 'committed_awaiting_index') => ({
    hash: `0x${String(n).repeat(64)}`,
    status,
    value: '0',
    fee: '1000',
    feeRate: '1000',
    txSize: 400,
    cycles: '1000',
    inputsCount: 1,
    outputsCount: 1,
    timestamp: '1792886400000',
    interpretation: { status: 'complete' as const, reasons: [] },
    activity: null,
  });
  vi.mocked(api.getPoolTransactions).mockResolvedValue({
    ...emptyPool,
    data: [row(1, 'pending'), row(2, 'proposed'), row(3, 'committed_awaiting_index')],
    total: 3,
    totals: { pending: 1, proposed: 1, awaitingIndex: 1 },
  });
  render(<TransactionsPage />);
  const banner = await screen.findByTestId('pool-banner');
  await waitFor(() => expect(banner.querySelectorAll('[data-pool-status]')).toHaveLength(3));
  expect(banner.querySelector('[data-pool-status="pending"]')).toHaveClass('bg-info');
  expect(banner.querySelector('[data-pool-status="proposed"]')).toHaveClass('bg-gold');
  const awaiting = banner.querySelector('[data-pool-status="committed_awaiting_index"]');
  expect(awaiting).toHaveClass('border', 'border-text-dim');
  expect(awaiting).not.toHaveClass('bg-info');
  expect(screen.getByRole('button', { name: /^In the pool/ })).toHaveTextContent('In the pool2');
});
