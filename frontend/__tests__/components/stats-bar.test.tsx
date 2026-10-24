import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '../utils/test-utils';
import { api, type NetworkStats } from '@/lib/api';
import { VitalsStrip } from '@/components/stats-bar';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';

vi.mock('@/lib/api', () => ({
  api: {
    getNetworkStats: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

function stats(overrides: Partial<NetworkStats> = {}): NetworkStats {
  return {
    latestBlock: 20819143,
    avgBlockTime: '8.43s',
    hashRate: '93.91 PH/s',
    difficulty: '850.46 P',
    epoch: '15168(246/1590)',
    tps: '0.2',
    estimatedEpochTime: '3h 8m',
    transactionsPerMinute: '1',
    transactionsPerDay: '1',
    syncStatus: {
      isSyncing: false,
      syncedBlock: 20819143,
      tipBlock: 20819143,
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
    ...overrides,
  };
}

describe('VitalsStrip', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useRealtimeStore.setState({ isConnected: false });
  });
  afterEach(() => act(() => useRealtimeStore.getState().reset()));

  it('shows block, epoch progress, block time, hash rate and difficulty in one labelled strip', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);

    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('#20,819,143'));

    expect(screen.getByRole('link', { name: /Block #20,819,143/ })).toHaveAttribute(
      'href',
      '/mainnet/blocks/20819143'
    );
    const epoch = screen.getByRole('link', {
      name: 'Epoch 15,168, block 246 of 1,590, about 3h 8m left',
    });
    expect(epoch).toHaveAttribute('href', '/mainnet/charts/epoch-time-length');
    expect(epoch).toHaveTextContent('15,168');
    expect(epoch).toHaveTextContent('246 / 1,590');
    expect(epoch).toHaveTextContent('≈ 3h 8m left');
    expect(screen.getByTestId('vitals-epoch-progress').style.width).toBe(`${(246 / 1590) * 100}%`);
    expect(strip).toHaveTextContent('Block time8.43s');
    expect(screen.getByRole('link', { name: /Hash rate 93.91 PH\/s/ })).toHaveAttribute(
      'href',
      '/mainnet/charts/hash-rate'
    );
    expect(screen.getByRole('link', { name: /Difficulty 850.46 P/ })).toHaveAttribute(
      'href',
      '/mainnet/charts/difficulty'
    );
  });

  it('renders an explicit error for a malformed epoch instead of inventing 0 / 1800', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats({ epoch: '100/50/1000' }));
    render(<VitalsStrip />);

    const error = await screen.findByTestId('vitals-epoch-error');
    expect(error).toHaveAttribute('role', 'alert');
    expect(error).toHaveTextContent('epoch "100/50/1000" is not number(index/length)');
    expect(screen.queryByText('1,800')).not.toBeInTheDocument();
    expect(screen.queryByTestId('vitals-epoch-progress')).not.toBeInTheDocument();
  });

  it('shows Live only while the realtime connection is up', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    await screen.findByText('#20,819,143');
    expect(screen.queryByTestId('vitals-live')).not.toBeInTheDocument();

    act(() => useRealtimeStore.setState({ isConnected: true }));
    expect(screen.getByTestId('vitals-live')).toHaveTextContent('Live');

    act(() => useRealtimeStore.setState({ isConnected: false }));
    expect(screen.queryByTestId('vitals-live')).not.toBeInTheDocument();
  });

  it('keeps the labels in place while the stats load', () => {
    vi.mocked(api.getNetworkStats).mockImplementation(() => new Promise(() => {}));
    render(<VitalsStrip />);
    const strip = screen.getByRole('region', { name: 'Chain vitals' });
    for (const label of ['Block', 'Epoch', 'Block time', 'Hash rate', 'Difficulty']) {
      expect(strip).toHaveTextContent(label);
    }
  });
});
