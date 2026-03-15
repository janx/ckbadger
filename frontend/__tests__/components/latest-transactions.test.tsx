import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { LatestTransactions } from '@/components/latest-transactions';
import { api, type Transaction } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getTransactions: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

function mockTx(hash: string): Transaction {
  return {
    hash,
    blockNumber: 8_775_638,
    blockHash: '0xblock',
    index: 0,
    inputsCount: 2,
    outputsCount: 3,
    fee: '10000',
    isCellbase: false,
    timestamp: '2026-02-22T00:00:00Z',
  };
}

describe('LatestTransactions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders latest transaction details with transaction and block links', async () => {
    const txs: Transaction[] = [mockTx('0xtx1')];

    vi.mocked(api.getTransactions).mockResolvedValue({
      data: txs,
      total: txs.length,
      limit: 10,
      hasMore: false,
      nextCursor: null,
    });

    render(<LatestTransactions initialTransactions={txs} />);

    await waitFor(() => {
      expect(screen.getByText('Block')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: '#8,775,638' })).toHaveAttribute(
      'href',
      '/blocks/8775638'
    );
    expect(screen.getByTitle('Click to copy: 0xtx1').closest('a')).toHaveAttribute(
      'href',
      '/tx/0xtx1'
    );
    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.getByText('3')).toBeInTheDocument();
  });
});
