import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { PipelineDashboard } from '@/components/pipeline/pipeline-dashboard';
import type { Block } from '@/lib/api';

vi.mock('@/components/chain-wave', () => ({
  ChainWave: () => <div data-testid="chain-wave">ChainWave</div>,
}));

vi.mock('@/components/mempool-blocks', () => ({
  MempoolBlocks: () => <div data-testid="mempool-blocks">MempoolBlocks</div>,
}));

function mockBlock(number: number): Block {
  return {
    number,
    hash: `0xblock${number}`,
    parentHash: `0xblock${number - 1}`,
    timestamp: '2024-01-15T10:30:00Z',
    transactionsCount: 100,
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

describe('PipelineDashboard', () => {
  it('renders enhanced layout and operational insights', async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
          staleTime: Infinity,
          gcTime: Infinity,
        },
      },
    });

    queryClient.setQueryData(['pipeline-dashboard-mempool-info'], {
      pendingCount: 1200,
      proposedCount: 320,
      orphanCount: 10,
      totalSize: 2_048_000,
      totalCycles: 222_333_444,
      minFeeRate: 1.2,
      tipNumber: 1_000_000,
      tipHash: '0xtip',
      lastUpdatedAt: 1_700_000_000,
    });

    queryClient.setQueryData(['pipeline-dashboard-recommended-fees'], {
      fastestFee: 12,
      halfHourFee: 10,
      hourFee: 8,
      economyFee: 5,
      minimumFee: 1,
    });

    queryClient.setQueryData(['pipeline-dashboard-pending-proposals'], {
      proposals: [
        {
          proposalId: '0xproposal1111111111111111111111111111111111111111111111111111111111',
          fullTxHash: '0xtx111111111111111111111111111111111111111111111111111111111111',
          proposedAtBlock: 999_999,
          proposedAtIndex: 1,
          blocksUntilExpiry: 1,
          fee: 1000,
          size: 500,
          cycles: 2_000_000,
          feeRate: 6,
        },
        {
          proposalId: '0xproposal2222222222222222222222222222222222222222222222222222222222',
          fullTxHash: '0xtx222222222222222222222222222222222222222222222222222222222222',
          proposedAtBlock: 999_998,
          proposedAtIndex: 2,
          blocksUntilExpiry: 6,
          fee: 2000,
          size: 800,
          cycles: 3_000_000,
          feeRate: 9,
        },
      ],
      tipBlockNumber: 1_000_000,
      totalCount: 2,
    });

    render(
      <QueryClientProvider client={queryClient}>
        <PipelineDashboard initialBlocks={[mockBlock(100), mockBlock(99)]} />
      </QueryClientProvider>
    );

    expect(screen.getByTestId('chain-wave')).toBeInTheDocument();
    expect(screen.getByTestId('mempool-blocks')).toBeInTheDocument();
    expect(screen.queryByText('Flow Trend')).not.toBeInTheDocument();
    expect(screen.getByText('Recommended Fee Rates')).toBeInTheDocument();
    expect(screen.getByText('Fastest')).toBeInTheDocument();
    expect(screen.getByText('Half Hour')).toBeInTheDocument();
    expect(screen.getByText('1 Hour')).toBeInTheDocument();
    expect(screen.getByText('F')).toBeInTheDocument();
    expect(screen.getAllByText(/x min/i).length).toBeGreaterThan(0);
    expect(screen.getByText('Mempool Health')).toBeInTheDocument();
    expect(screen.getByText('Health Score')).toBeInTheDocument();
    expect(screen.getByText(/until .* reaches warning/i)).toBeInTheDocument();
    expect(screen.getByText(/Adaptive thresholds from recent/i)).toBeInTheDocument();
    expect(screen.getByText('Queue pressure')).toBeInTheDocument();
    expect(screen.getByText('Proposal Pressure')).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getAllByText('1,200').length).toBeGreaterThan(0);
      expect(screen.getByText('12.00 sh/B')).toBeInTheDocument();
    });
    expect(screen.getByText('Near expiry queue')).toBeInTheDocument();
    expect(screen.getByText('Open charts for broader context')).toHaveAttribute('href', '/charts');
  });
});
