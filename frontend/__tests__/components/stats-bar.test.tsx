import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { api } from '@/lib/api';
import { GlobalStatsBar } from '@/components/stats-bar';

vi.mock('@/lib/api', () => ({
  api: {
    getNetworkStats: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

describe('GlobalStatsBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      latestBlock: 18823834,
      avgBlockTime: '5.00s',
      hashRate: '334.18 PH/s',
      difficulty: '1',
      epoch: '13814(828/947)',
      tps: '1',
      estimatedEpochTime: '1',
      transactionsPerMinute: '1',
      transactionsPerDay: '1',
      syncStatus: {
        isSyncing: false,
        syncedBlock: 18823834,
        tipBlock: 18823834,
        progress: 100,
        estimatedTime: null,
        chartDataMayBeIncomplete: false,
        blocksPerSecond: null,
        emaBlocksPerSecond: null,
        syncMode: 'synced',
        startedAt: null,
        elapsedTime: null,
        totalTime: null,
      },
      deepForkStatus: {
        detected: false,
        detectedAt: null,
        depth: null,
        dbTip: null,
        chainTip: null,
        forkPoint: null,
      },
      knowledgeSize: null,
      circulatingSupply: null,
      daoLocked: null,
    });
  });

  it('renders fetched stats with chart and block links', async () => {
    render(<GlobalStatsBar />);

    await waitFor(() => {
      expect(screen.getByText('block')).toBeInTheDocument();
    });

    expect(api.getNetworkStats).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('link', { name: /block/i })).toHaveAttribute(
      'href',
      '/blocks/18823834'
    );
    expect(screen.getByRole('link', { name: /epoch/i })).toHaveAttribute(
      'href',
      '/charts/epoch-time-length'
    );
    expect(screen.getByRole('link', { name: /hash/i })).toHaveAttribute(
      'href',
      '/charts/hash-rate'
    );
    expect(screen.getByText('13,814')).toBeInTheDocument();
    expect(screen.getByText('828/947 87.4%')).toBeInTheDocument();
    expect(screen.getByText('334.18 PH/s')).toBeInTheDocument();
    expect(screen.getByText('5.00s')).toBeInTheDocument();
  });
});
