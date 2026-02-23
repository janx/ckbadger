import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { LatestTransactions } from '@/components/latest-transactions';
import { api, type Transaction } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getTransactions: vi.fn(),
  },
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

  it('renders Block label with improved readable contrast', async () => {
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

    expect(screen.getByText('Block')).toHaveClass('text-slate-500');
  });
});
