import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { PipelinePreview } from '@/components/chain-wave/pipeline-preview';
import { api } from '@/lib/api';
import type { Block } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getMempoolInfo: vi.fn(),
    getMempoolTransactions: vi.fn(),
    getPendingProposals: vi.fn(),
    getBlocks: vi.fn(),
    getTransactions: vi.fn(),
  },
}));

function mockBlock(number: number, txCount: number): Block {
  return {
    number,
    hash: `0xblock${number}`,
    parentHash: `0xblock${number - 1}`,
    timestamp: '2024-01-15T10:30:00Z',
    transactionsCount: txCount,
    proposalsCount: 0,
    unclesCount: 0,
    difficulty: '0x1000000',
    epoch: '0x1',
    epochNumber: 100,
    epochIndex: 1,
    epochLength: 1000,
    nonce: '0x0',
    transactionsRoot: '0xroot',
    minerAddress: null,
    minerMessage: null,
    miningReward: null,
    miningRewardTxHash: null,
    compactTarget: '0x1a000000',
    version: 0,
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('PipelinePreview', () => {
  it('renders compact tri-metric pipeline snapshot and link', async () => {
    vi.spyOn(api, 'getMempoolInfo').mockResolvedValue({
      pendingCount: 1200,
      proposedCount: 320,
      orphanCount: 10,
      totalSize: 1_048_576,
      totalCycles: 123_456_789,
      minFeeRate: 1,
      tipNumber: 1_000_000,
      tipHash: '0xtip',
      lastUpdatedAt: 1_700_000_000,
    });

    vi.spyOn(api, 'getMempoolTransactions').mockResolvedValue([
      {
        txHash: '0xmempool-tx-1',
        fee: 1000,
        size: 320,
        cycles: 1_200_000,
        feeRate: 3.12,
        ancestorsCount: 0,
        timestamp: 1_700_000_000,
        status: 'pending',
      },
    ]);

    vi.spyOn(api, 'getPendingProposals').mockResolvedValue({
      proposals: [
        {
          proposalId: '0xproposal111',
          fullTxHash: '0xproposal-full-hash-111',
          proposedAtBlock: 99,
          proposedAtIndex: 1,
          blocksUntilExpiry: 10,
          fee: 2000,
          size: 450,
          cycles: 2_300_000,
          feeRate: 4.44,
        },
      ],
      tipBlockNumber: 100,
      totalCount: 1,
    });

    vi.spyOn(api, 'getBlocks').mockResolvedValue({
      data: [mockBlock(100, 200), mockBlock(99, 180), mockBlock(98, 160)],
      total: 3,
      limit: 3,
      hasMore: false,
      nextCursor: null,
    });

    vi.spyOn(api, 'getTransactions').mockResolvedValue({
      data: [
        {
          hash: '0xblocktx1',
          blockNumber: 100,
          blockHash: '0xblock100',
          index: 0,
          inputsCount: 1,
          outputsCount: 2,
          fee: '3200',
          txSize: 600,
          cycles: 1_800_000,
          isCellbase: false,
          timestamp: '2024-01-15T10:30:00Z',
        },
      ],
      total: 1,
      limit: 40,
      hasMore: false,
      nextCursor: null,
    });

    render(<PipelinePreview initialBlocks={[mockBlock(100, 200)]} />);

    expect(await screen.findByText('Pipeline Snapshot')).toBeInTheDocument();
    await waitFor(() => expect(api.getMempoolInfo).toHaveBeenCalled());
    expect(api.getMempoolTransactions).toHaveBeenCalled();
    expect(api.getPendingProposals).toHaveBeenCalled();
    expect(api.getTransactions).toHaveBeenCalled();
    expect(screen.getAllByText('Mempool').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Proposed').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Committed').length).toBeGreaterThan(0);
    expect(screen.getByText('200')).toBeInTheDocument();
    expect(screen.getByText('Bubble size = txn size')).toBeInTheDocument();
    expect(screen.getByText('X = fee rate')).toBeInTheDocument();
    expect(screen.getByText('Y = cycles')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'View full pipeline' })).toHaveAttribute(
      'href',
      '/pipeline'
    );
  });
});
