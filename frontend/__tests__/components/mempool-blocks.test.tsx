import { afterEach, describe, expect, it, vi } from 'vitest';
import { QueryClientProvider } from '@tanstack/react-query';
import { waitFor, render as rtlRender } from '@testing-library/react';
import { act, createTestQueryClient, fireEvent, render, screen, within } from '../utils/test-utils';
import { MempoolBlocks } from '@/components/mempool-blocks';
import { api, Block } from '@/lib/api';

function mockBlock(number: number, txCount: number, hardforkShortName?: string): Block {
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
    hardforkActivation: hardforkShortName
      ? {
          id: `${hardforkShortName.toLowerCase()}-activation`,
          name: `CKB Edition ${hardforkShortName}`,
          shortName: hardforkShortName,
          activationEpoch: 0,
          activationDate: '2026-01-01',
        }
      : null,
    compactTarget: '0x1a000000',
    version: 0,
  };
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('MempoolBlocks', () => {
  it('renders txn circles inside enlarged blocks when tri-metric lens is enabled', async () => {
    vi.spyOn(api, 'getMempoolBlocks').mockResolvedValue({
      pendingBlocks: [
        {
          index: 0,
          transactionCount: 120,
          totalSize: 180000,
          totalFee: 420000000,
          totalCycles: 320000000,
          feeRateRange: { min: 1.1, max: 5.2 },
          medianFeeRate: 2.9,
          estimatedTimeMinutes: 1,
        },
        {
          index: 1,
          transactionCount: 90,
          totalSize: 120000,
          totalFee: 210000000,
          totalCycles: 220000000,
          feeRateRange: { min: 0.9, max: 3.8 },
          medianFeeRate: 2.1,
          estimatedTimeMinutes: 2,
        },
      ],
      totalPendingCount: 210,
      totalProposedCount: 36,
    });

    vi.spyOn(api, 'getBlocks').mockResolvedValue({
      data: [mockBlock(100, 180, 'Mirana'), mockBlock(99, 160)],
      total: 2,
      limit: 10,
      hasMore: false,
      nextCursor: null,
    });

    vi.spyOn(api, 'getBlockFeeStats').mockResolvedValue({
      blockNumber: 100,
      totalSize: 200000,
      totalCycles: 450000000,
      avgFeeRate: 3.3,
      minFeeRate: 1.1,
      maxFeeRate: 6.7,
      transactionCount: 180,
    });

    vi.spyOn(api, 'getMempoolTransactions').mockResolvedValue([
      {
        txHash: '0xmempool-1',
        fee: 1000,
        size: 350,
        cycles: 1200000,
        feeRate: 2.85,
        ancestorsCount: 0,
        timestamp: 1700000000,
        status: 'pending',
      },
      {
        txHash: '0xmempool-2',
        fee: 2000,
        size: 440,
        cycles: 2300000,
        feeRate: 4.11,
        ancestorsCount: 0,
        timestamp: 1700000001,
        status: 'pending',
      },
    ]);

    vi.spyOn(api, 'getPendingProposals').mockResolvedValue({
      proposals: [
        {
          proposalId: '0xproposal-1',
          fullTxHash: '0xproposal-full-1',
          proposedAtBlock: 98,
          proposedAtIndex: 0,
          blocksUntilExpiry: 10,
          fee: 3200,
          size: 480,
          cycles: 2400000,
          feeRate: 6.2,
        },
      ],
      tipBlockNumber: 100,
      totalCount: 1,
    });

    vi.spyOn(api, 'getTransactions').mockImplementation(async (params) => {
      const blockNumber = params?.blockNumber ?? 0;
      return {
        data: [
          {
            hash: `0xcellbase-${blockNumber}`,
            blockNumber,
            blockHash: `0xhash-${blockNumber}`,
            index: 0,
            inputsCount: 1,
            outputsCount: 1,
            fee: '0',
            txSize: 280,
            cycles: undefined,
            isCellbase: true,
            timestamp: '2024-01-15T10:30:00Z',
          },
          {
            hash: `0xblocktx-${blockNumber}`,
            blockNumber,
            blockHash: `0xhash-${blockNumber}`,
            index: 1,
            inputsCount: 1,
            outputsCount: 2,
            fee: '2200',
            txSize: 410,
            cycles: 1800000,
            isCellbase: false,
            timestamp: '2024-01-15T10:30:00Z',
          },
        ],
        total: 2,
        limit: 80,
        hasMore: false,
        nextCursor: null,
      };
    });

    vi.spyOn(api, 'getTransaction').mockImplementation(async (hash) => ({
      hash,
      blockNumber: 0,
      blockHash: '0xpending',
      index: 0,
      inputsCount: 1,
      outputsCount: 2,
      fee: '3200',
      txSize: 480,
      cycles: 2_400_000,
      isCellbase: false,
      timestamp: '2024-01-15T10:30:00Z',
    }));

    render(
      <MempoolBlocks
        latestBlocks={[mockBlock(100, 180, 'Mirana'), mockBlock(99, 160)]}
        showTxnLens
      />
    );

    expect(await screen.findByText('Chain Tip Intelligence')).toBeInTheDocument();
    expect(await screen.findByText('Mempool')).toBeInTheDocument();
    expect(await screen.findByText('Proposals')).toBeInTheDocument();
    expect(await screen.findByText('Next Block')).toBeInTheDocument();
    const summaryRow = screen.getByTestId('pipeline-summary-row');
    expect(within(summaryRow).getByText('Mempool (2)')).toBeInTheDocument();
    expect(screen.getByText('Proposals (1)')).toBeInTheDocument();
    expect(screen.getByText('New Committed (179)')).toBeInTheDocument();
    expect(
      within(summaryRow).getByText(/w -> size \| h -> cycles \| x -> fee \| y -> fee rate/i)
    ).toBeInTheDocument();
    await waitFor(() => {
      const glowLayers = Array.from(
        document.querySelectorAll<HTMLElement>('[data-testid="tx-bubble-layer-glow"]')
      );
      expect(glowLayers.length).toBeGreaterThan(0);
      const glowClasses = glowLayers.map((layer) => layer.className).join(' ');
      expect(glowClasses).toContain('to-amber-400/[0.12]');
      expect(glowClasses).toContain('to-cyan-400/[0.12]');
      expect(glowClasses).toContain('to-terminal-green/10');
    });

    await waitFor(() => {
      expect(document.querySelectorAll('[data-tx-tooltip*="Stage:"]').length).toBeGreaterThan(0);
    });

    const cellbaseBubble = Array.from(
      document.querySelectorAll<HTMLElement>('[data-tx-tooltip]')
    ).find((bubble) => (bubble.getAttribute('data-tx-tooltip') ?? '').includes('Type: Cellbase'));
    expect(cellbaseBubble).toBeTruthy();
    expect(cellbaseBubble?.style.opacity).toBe('0.42');

    const firstBubble = document.querySelector('[data-tx-tooltip]');
    expect(firstBubble).toBeTruthy();
    fireEvent.mouseEnter(firstBubble as Element, { clientX: 140, clientY: 120 });

    const tooltip = await screen.findByTestId('tx-bubble-tooltip');
    expect(tooltip).toHaveClass('fixed');

    const minedBlockLink = document.querySelector('a[href="/blocks/100"]');
    expect(minedBlockLink).toBeTruthy();
    fireEvent.mouseEnter(minedBlockLink as Element, { clientX: 320, clientY: 260 });

    const minedTooltip = await screen.findByTestId('mined-block-tooltip-100');
    expect(minedTooltip).toHaveClass('fixed');
    expect(screen.getByTestId('mempool-mined-hardfork-100')).toHaveTextContent('HF MIRANA');
  });

  it('refreshes txn data queries immediately when new tip block arrives', async () => {
    vi.spyOn(api, 'getMempoolBlocks').mockResolvedValue({
      pendingBlocks: [],
      totalPendingCount: 0,
      totalProposedCount: 0,
    });

    const firstTip = [mockBlock(100, 180), mockBlock(99, 160)];
    const secondTip = [mockBlock(101, 200), mockBlock(100, 180)];
    let getBlocksCalls = 0;

    vi.spyOn(api, 'getBlocks').mockImplementation(async () => {
      getBlocksCalls += 1;
      return {
        data: getBlocksCalls === 1 ? firstTip : secondTip,
        total: 2,
        limit: 10,
        hasMore: false,
        nextCursor: null,
      };
    });

    const getBlockFeeStatsSpy = vi
      .spyOn(api, 'getBlockFeeStats')
      .mockImplementation(async (block) => {
        const blockNumber = typeof block === 'number' ? block : Number(block);
        return {
          blockNumber,
          totalSize: 200000,
          totalCycles: 450000000,
          avgFeeRate: 3.3,
          minFeeRate: 1.1,
          maxFeeRate: 6.7,
          transactionCount: blockNumber === 101 ? 200 : 180,
        };
      });

    vi.spyOn(api, 'getMempoolTransactions').mockResolvedValue([]);
    vi.spyOn(api, 'getPendingProposals').mockResolvedValue({
      proposals: [],
      tipBlockNumber: 100,
      totalCount: 0,
    });

    const getTransactionsSpy = vi
      .spyOn(api, 'getTransactions')
      .mockImplementation(async (params) => {
        const blockNumber = params?.blockNumber ?? 0;
        return {
          data: [
            {
              hash: `0xblocktx-${blockNumber}`,
              blockNumber,
              blockHash: `0xhash-${blockNumber}`,
              index: 0,
              inputsCount: 1,
              outputsCount: 2,
              fee: '2200',
              txSize: 410,
              cycles: 1800000,
              isCellbase: false,
              timestamp: '2024-01-15T10:30:00Z',
            },
          ],
          total: 1,
          limit: 80,
          hasMore: false,
          nextCursor: null,
        };
      });

    vi.spyOn(api, 'getTransaction').mockRejectedValue(new Error('no proposal tx'));

    const queryClient = createTestQueryClient();

    rtlRender(
      <QueryClientProvider client={queryClient}>
        <MempoolBlocks showTxnLens />
      </QueryClientProvider>
    );

    await waitFor(() => {
      expect(getBlocksCalls).toBe(1);
      expect(
        getTransactionsSpy.mock.calls.some(([params]) => params?.blockNumber === 100)
      ).toBeTruthy();
    });

    const txCallsFor100Before = getTransactionsSpy.mock.calls.filter(
      ([params]) => params?.blockNumber === 100
    ).length;
    const feeCallsFor100Before = getBlockFeeStatsSpy.mock.calls.filter(
      ([blockNumber]) => blockNumber === 100
    ).length;

    act(() => {
      queryClient.setQueryData(['latest-blocks'], {
        data: secondTip,
        total: 2,
        limit: 10,
        hasMore: false,
        nextCursor: null,
      });
    });

    await waitFor(() => {
      expect(
        getTransactionsSpy.mock.calls.some(([params]) => params?.blockNumber === 101)
      ).toBeTruthy();
    });

    await waitFor(() => {
      const txCallsFor100After = getTransactionsSpy.mock.calls.filter(
        ([params]) => params?.blockNumber === 100
      ).length;
      const feeCallsFor100After = getBlockFeeStatsSpy.mock.calls.filter(
        ([blockNumber]) => blockNumber === 100
      ).length;

      expect(txCallsFor100After).toBeGreaterThan(txCallsFor100Before);
      expect(feeCallsFor100After).toBeGreaterThan(feeCallsFor100Before);
    });
  });
});
