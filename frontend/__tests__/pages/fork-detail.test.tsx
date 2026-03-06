import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import ForkDetailPage from '@/app/forks/[id]/client-page';
import { api } from '@/lib/api';

vi.mock('@/src/navigation', () => ({
  notFound: vi.fn(),
  useParams: () => ({ id: '1' }),
}));

vi.mock('@/lib/api', () => ({
  api: {
    getForkDetail: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

describe('ForkDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders fork detail event and orphaned data', async () => {
    vi.mocked(api.getForkDetail).mockResolvedValue({
      event: {
        id: 1,
        eventType: 'reorg',
        depth: 2,
        oldTipNumber: 200,
        oldTipHash: '0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
        newTipNumber: 201,
        newTipHash: '0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
        forkPointNumber: 198,
        forkPointHash: '0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff',
        orphanedBlocksCount: 2,
        orphanedTxsCount: 3,
        detectedAt: '2026-02-20T10:00:00Z',
        resolvedAt: null,
        resolvedBy: null,
        resolutionAction: null,
        resolutionNotes: null,
      },
      orphanedBlocks: [
        {
          number: 199,
          hash: '0x1111111111111111111111111111111111111111111111111111111111111111',
          parentHash: '0x0000000000000000000000000000000000000000000000000000000000000000',
          timestamp: '2026-02-20T09:59:00Z',
          transactionsCount: 10,
          minerLockHash: null,
        },
      ],
      orphanedTransactions: [
        {
          hash: '0x2222222222222222222222222222222222222222222222222222222222222222',
          blockNumber: 199,
          blockHash: '0x1111111111111111111111111111111111111111111111111111111111111111',
          txIndex: 0,
          inputsCount: 1,
          outputsCount: 2,
          totalCapacity: '1000000000',
        },
      ],
    });

    render(<ForkDetailPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();

    await waitFor(() => {
      expect(api.getForkDetail).toHaveBeenCalledWith(1);
      expect(screen.getByText('Fork Event #1')).toBeInTheDocument();
    });

    expect(screen.getByRole('link', { name: '#198' })).toHaveAttribute('href', '/blocks/198');
    expect(screen.getByText('Orphaned Blocks (1)')).toBeInTheDocument();
    expect(screen.getByText('Orphaned Transactions (1)')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '#199' })).toHaveAttribute('href', '/blocks/199');
  });
});
