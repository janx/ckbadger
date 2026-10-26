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

  it('renders transaction and block links from fetched transactions', async () => {
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

    expect(screen.getAllByTitle(/Click to copy: 0xaaaaaaaa/i)).toHaveLength(2);
    expect(
      screen
        .getAllByTitle(/Click to copy: 0xaaaaaaaa/i)
        .every(
          (hashDisplay) =>
            hashDisplay.closest('a')?.getAttribute('href') ===
            '/mainnet/tx/0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
        )
    ).toBe(true);
    expect(screen.getAllByText('2')).toHaveLength(2);
    expect(screen.getAllByText('3')).toHaveLength(2);
  });
});

describe('Transactions pool view', () => {
  it('reads the paged pool and keeps partial interpretation explicit', async () => {
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
    fireEvent.click(screen.getByRole('button', { name: 'In the pool' }));
    expect(
      await screen.findByRole('table', { name: 'Transactions in the pool' })
    ).toHaveTextContent('Interpretation partial');
    expect(api.getPoolTransactions).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
    expect(screen.getByTestId('pool-banner')).toHaveTextContent(
      '0 pending · 1 proposed · 0 awaiting index'
    );
    expect(screen.getByLabelText('Pool summary')).toHaveTextContent('226 bytes');
    expect(screen.getByLabelText('Pool summary')).toHaveTextContent('1,709,221');
    expect(screen.getByLabelText('Pool summary')).toHaveTextContent('54,623–54,623 shannon/KB');
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
  expect(screen.getByRole('button', { name: 'DAO' })).toHaveTextContent('3 actions');
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
