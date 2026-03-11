import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { LatestBlocks } from '@/components/latest-blocks';
import { api, type Block } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getBlocks: vi.fn(),
  },
}));

function mockBlock(number: number): Block {
  return {
    number,
    hash: `0xblock${number}`,
    parentHash: `0xblock${number - 1}`,
    timestamp: '2026-02-22T00:00:00Z',
    transactionsCount: 5,
    proposalsCount: 0,
    unclesCount: 0,
    difficulty: '0',
    epoch: '5414/1800',
    epochNumber: 5414,
    epochIndex: 0,
    epochLength: 1800,
    nonce: '0x0',
    transactionsRoot: '0xroot',
    minerAddress: null,
    minerMessage: null,
    miningReward: null,
    miningRewardTxHash: null,
    compactTarget: '0x0',
    version: 0,
  };
}

describe('LatestBlocks', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders hardfork badge for activation block', async () => {
    const blocks: Block[] = [
      {
        ...mockBlock(8_775_638),
        hardforkActivation: {
          id: 'mirana-2021',
          name: 'CKB Edition Mirana',
          shortName: 'Mirana',
          activationEpoch: 5414,
          activationDate: '2022-05-10',
        },
      },
      mockBlock(8_775_639),
    ];

    vi.mocked(api.getBlocks).mockResolvedValue({
      data: blocks,
      total: blocks.length,
      limit: 10,
      hasMore: false,
      nextCursor: null,
    });

    render(<LatestBlocks initialBlocks={blocks} />);

    await waitFor(() => {
      expect(screen.getByTestId('latest-block-hardfork-8775638')).toBeInTheDocument();
    });
    expect(screen.getByText('HF · MIRANA')).toBeInTheDocument();
    expect(screen.queryByTestId('latest-block-hardfork-8775639')).not.toBeInTheDocument();
    expect(screen.getAllByText('#')[0]).toHaveClass('text-text-dim');
  });
});
