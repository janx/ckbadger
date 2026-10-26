import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import TransactionsPage from '@/app/transactions/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getTransactions: vi.fn(),
    getPoolTransactions: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

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
    expect(screen.getByText('0 pending · 1 proposed · 0 awaiting index')).toBeInTheDocument();
  });
});
