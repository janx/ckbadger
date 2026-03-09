import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import TransactionsPage from '@/app/transactions/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getTransactions: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

describe('TransactionsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders transactions with accent hash and block links', async () => {
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
          isCellbase: false,
          timestamp: '2026-02-20T00:00:00Z',
        },
      ],
      total: 1,
      limit: 25,
      hasMore: false,
      nextCursor: null,
    });

    render(<TransactionsPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Transactions')).toBeInTheDocument();

    await waitFor(() => {
      expect(api.getTransactions).toHaveBeenCalledWith({ cursor: undefined, limit: 25 });
    });

    await waitFor(() => {
      expect(document.querySelector('a[href="/blocks/123456"]')).toBeTruthy();
    });
    expect(document.querySelector('a[href="/blocks/123456"]')).toHaveClass('text-emphasis');
    expect(screen.getByText('→')).toHaveClass('text-text-muted');
    expect(
      document.querySelector(
        '[title="Click to copy: 0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"] .text-emphasis'
      )
    ).toBeTruthy();
  });
});
