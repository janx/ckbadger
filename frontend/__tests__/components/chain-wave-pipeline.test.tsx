import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { ChainWave } from '@/components/chain-wave';
import { api, Block, Transaction } from '@/lib/api';

function mockBlock(number: number): Block {
  return {
    number,
    hash: `0xblock${number}`,
    parentHash: `0xblock${number - 1}`,
    timestamp: '2024-01-15T10:30:00Z',
    transactionsCount: 2,
    proposalsCount: 0,
    unclesCount: 0,
    difficulty: '0x1000000',
    epoch: '0x1',
    epochNumber: 100,
    epochIndex: 10,
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

function mockTx(hash: string, blockNumber: number): Transaction {
  return {
    hash,
    blockNumber,
    blockHash: `0xblock${blockNumber}`,
    index: 0,
    inputsCount: 1,
    outputsCount: 2,
    fee: '1000',
    txSize: 400,
    cycles: 2_000_000,
    isCellbase: false,
    timestamp: '2024-01-15T10:30:00Z',
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('ChainWave pipeline view', () => {
  it('renders pipeline and tri-metric legend', async () => {
    const blocks = [mockBlock(100), mockBlock(99)];

    vi.spyOn(api, 'getMempoolTransactions').mockResolvedValue([
      {
        txHash: '0xmempool1',
        fee: 1000,
        size: 420,
        cycles: 1_500_000,
        feeRate: 2.38,
        ancestorsCount: 0,
        timestamp: 1_700_000_000,
        status: 'pending',
      },
    ]);

    vi.spyOn(api, 'getPendingProposals').mockResolvedValue({
      proposals: [
        {
          proposalId: '0xproposal',
          fullTxHash: '0xproposal-full',
          proposedAtBlock: 98,
          proposedAtIndex: 1,
          blocksUntilExpiry: 10,
          fee: 3000,
          size: 500,
          cycles: 2_000_000,
          feeRate: 6,
        },
      ],
      tipBlockNumber: 100,
      totalCount: 1,
    });

    vi.spyOn(api, 'getBlocks').mockResolvedValue({
      data: blocks,
      total: 2,
      limit: 4,
      hasMore: false,
      nextCursor: null,
    });

    vi.spyOn(api, 'getTransactions').mockImplementation(async (params) => {
      const blockNumber = params?.blockNumber ?? 0;
      return {
        data: [mockTx(`0xtx-${blockNumber}`, blockNumber)],
        total: 1,
        limit: 80,
        hasMore: false,
        nextCursor: null,
      };
    });

    render(<ChainWave initialBlocks={blocks} />);

    expect(await screen.findByText('Transaction Flow Pipeline')).toBeInTheDocument();
    expect(screen.getByText('Recent Committed Blocks')).toBeInTheDocument();
    expect(screen.getByText('Tri-metric encoding')).toBeInTheDocument();
    expect(screen.getByText('Bubble size = txn size (bytes)')).toBeInTheDocument();
  });
});
