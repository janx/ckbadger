import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import BlocksPage from '@/app/blocks/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getBlocks: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

const mockBlocksResponse = {
  data: [
    {
      number: 8_775_639,
      hash: `0x${'22'.repeat(32)}`,
      parentHash: `0x${'11'.repeat(32)}`,
      timestamp: '2026-02-22T00:00:10Z',
      transactionsCount: 2,
      proposalsCount: 0,
      unclesCount: 0,
      difficulty: '0',
      epoch: '5414/1800',
      epochNumber: 5414,
      epochIndex: 8,
      epochLength: 1800,
      nonce: '0x0',
      transactionsRoot: `0x${'33'.repeat(32)}`,
      minerAddress: null,
      minerMessage: null,
      miningReward: null,
      miningRewardTxHash: null,
      hardforkActivation: null,
      compactTarget: '0x0',
      version: 0,
    },
    {
      number: 8_775_638,
      hash: `0x${'44'.repeat(32)}`,
      parentHash: `0x${'22'.repeat(32)}`,
      timestamp: '2026-02-22T00:00:00Z',
      transactionsCount: 1,
      proposalsCount: 0,
      unclesCount: 0,
      difficulty: '0',
      epoch: '5414/1800',
      epochNumber: 5414,
      epochIndex: 7,
      epochLength: 1800,
      nonce: '0x0',
      transactionsRoot: `0x${'55'.repeat(32)}`,
      minerAddress: null,
      minerMessage: null,
      miningReward: null,
      miningRewardTxHash: null,
      hardforkActivation: {
        id: 'mirana-2021',
        name: 'CKB Edition Mirana',
        shortName: 'Mirana',
        activationEpoch: 5414,
        activationDate: '2022-05-10',
      },
      compactTarget: '0x0',
      version: 0,
    },
  ],
  total: 2,
  limit: 50,
  hasMore: false,
  nextCursor: null,
};

describe('BlocksPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getBlocks).mockResolvedValue(mockBlocksResponse);
  });

  it('renders hardfork badge for activation block in block list', async () => {
    render(<BlocksPage />);

    await waitFor(() => {
      expect(api.getBlocks).toHaveBeenCalledWith({ cursor: undefined, limit: 50 });
    });
    await waitFor(() => {
      expect(screen.getAllByText('HF · MIRANA').length).toBeGreaterThan(0);
    });

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Blocks')).toBeInTheDocument();
    expect(screen.getAllByText('#8,775,638').length).toBeGreaterThan(0);
    expect(screen.getAllByText('HF · MIRANA')[0]).toBeInTheDocument();
  });
});
