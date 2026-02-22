import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import BlockDetailPage from '@/app/blocks/[id]/page';
import { api } from '@/lib/api';

const BLOCK_ID = '8775638';

vi.mock('@/lib/api', () => ({
  api: {
    getBlock: vi.fn(),
    getTransactions: vi.fn(),
    getBlockFeeStats: vi.fn(),
    getBlockProposals: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('next/navigation', () => ({
  useParams: () => ({ id: BLOCK_ID }),
}));

const mockBlock = {
  number: 8_775_638,
  hash: `0x${'aa'.repeat(32)}`,
  parentHash: `0x${'bb'.repeat(32)}`,
  timestamp: '2026-02-22T00:00:00Z',
  transactionsCount: 1,
  proposalsCount: 0,
  unclesCount: 0,
  difficulty: '123456',
  epoch: '5414(0/1800)',
  epochNumber: 5414,
  epochIndex: 0,
  epochLength: 1800,
  nonce: '0x1',
  transactionsRoot: `0x${'cc'.repeat(32)}`,
  minerAddress: 'ckb1qyqz683v8g0kz6r2gz7g3q6v3c4x4cv8j7ysf30a0g',
  minerMessage: null,
  miningReward: '10000000000',
  miningRewardTxHash: `0x${'dd'.repeat(32)}`,
  compactTarget: '0x1',
  version: 0,
  hardforkActivation: null,
};

describe('BlockDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getBlock).mockResolvedValue(mockBlock);
    vi.mocked(api.getTransactions).mockResolvedValue({
      data: [
        {
          hash: `0x${'11'.repeat(32)}`,
          blockNumber: mockBlock.number,
          blockHash: mockBlock.hash,
          index: 0,
          inputsCount: 1,
          outputsCount: 1,
          fee: '1000',
          isCellbase: false,
          timestamp: mockBlock.timestamp,
        },
      ],
      limit: 100,
      hasMore: false,
      nextCursor: null,
    });
    vi.mocked(api.getBlockFeeStats).mockResolvedValue({
      blockNumber: mockBlock.number,
      totalSize: 1200,
      totalCycles: 5_000_000,
      avgFeeRate: 1000,
      minFeeRate: 900,
      maxFeeRate: 1100,
      transactionCount: 1,
    });
    vi.mocked(api.getBlockProposals).mockResolvedValue([]);
  });

  it('shows hardfork activation badge on activation block', async () => {
    vi.mocked(api.getBlock).mockResolvedValue({
      ...mockBlock,
      hardforkActivation: {
        id: 'mirana-2021',
        name: 'CKB Edition Mirana',
        shortName: 'Mirana',
        activationEpoch: 5414,
        activationDate: '2022-05-10',
      },
    });

    render(<BlockDetailPage />);

    await waitFor(() => {
      expect(api.getBlock).toHaveBeenCalledWith(BLOCK_ID);
    });

    await waitFor(() => {
      expect(screen.getByText(/HARDFORK ACTIVATION/i)).toBeInTheDocument();
      expect(screen.getByText(/MIRANA/i)).toBeInTheDocument();
    });
  });

  it('does not show activation badge when block is not a hardfork block', async () => {
    vi.mocked(api.getBlock).mockResolvedValue({
      ...mockBlock,
      hardforkActivation: null,
    });

    render(<BlockDetailPage />);

    await waitFor(() => {
      expect(api.getBlock).toHaveBeenCalledWith(BLOCK_ID);
    });

    expect(screen.queryByText(/HARDFORK ACTIVATION/i)).not.toBeInTheDocument();
  });
});
