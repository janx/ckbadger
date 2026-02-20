import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '../utils/test-utils';
import { PipelinePreview } from '@/components/chain-wave/pipeline-preview';
import { api, type Block } from '@/lib/api';

vi.mock('@/components/mempool-blocks', () => ({
  MempoolBlocks: ({
    showTxnLens,
    showHeader,
    legendMode,
  }: {
    showTxnLens?: boolean;
    showHeader?: boolean;
    legendMode?: 'row' | 'none';
  }) => (
    <div data-testid="mempool-blocks">
      lens:{String(showTxnLens)} header:{String(showHeader)} legend:{String(legendMode)}
    </div>
  ),
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

describe('PipelinePreview', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders compact chain tip visualization with integrated txn lens', async () => {
    vi.spyOn(api, 'getMempoolBlocks').mockResolvedValue({
      pendingBlocks: [],
      totalPendingCount: 128,
      totalProposedCount: 0,
    });
    vi.spyOn(api, 'getMempoolTransactions').mockResolvedValue(
      Array.from({ length: 128 }, (_, index) => ({
        txHash: `0xmempool-${index}`,
        fee: 1000 + index,
        size: 300 + index,
        cycles: 1_000_000 + index,
        feeRate: 2.5,
        ancestorsCount: 0,
        timestamp: 1700000000 + index,
        status: 'pending',
      }))
    );
    vi.spyOn(api, 'getPendingProposals').mockResolvedValue({
      proposals: [],
      tipBlockNumber: 100,
      totalCount: 41,
    });

    vi.spyOn(api, 'getBlocks').mockResolvedValue({
      data: [mockBlock(100, 200)],
      total: 1,
      limit: 10,
      hasMore: false,
      nextCursor: null,
    });
    render(<PipelinePreview initialBlocks={[mockBlock(100, 200)]} />);

    expect(await screen.findByText('Transaction Pipeline')).toBeInTheDocument();
    expect(await screen.findByText(/Mempool \(128\)/i)).toBeInTheDocument();
    expect(screen.getByText(/Proposals \(41\)/i)).toBeInTheDocument();
    expect(screen.getByText(/New Committed \(199\)/i)).toBeInTheDocument();
    const summaryRow = screen.getByTestId('pipeline-preview-summary-row');
    expect(within(summaryRow).getByText(/Mempool \(128\)/i)).toBeInTheDocument();
    expect(
      within(summaryRow).getByText(/w -> size \| h -> cycles \| x -> fee \| y -> fee rate/i)
    ).toBeInTheDocument();
    expect(screen.getByTestId('mempool-blocks')).toHaveTextContent('lens:true');
    expect(screen.getByTestId('mempool-blocks')).toHaveTextContent('header:false');
    expect(screen.getByTestId('mempool-blocks')).toHaveTextContent('legend:none');
  });
});
