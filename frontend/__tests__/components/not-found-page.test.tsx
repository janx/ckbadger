import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '../utils/test-utils';
import { NotFoundPage } from '@/components/not-found-page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getNetworkStats: vi.fn(),
    getBlocks: vi.fn(),
  },
}));

describe('NotFoundPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders poetic 404 copy and live chain telemetry', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue({
      latestBlock: 18888888,
      avgBlockTime: '10.1s',
      hashRate: '1.11 EH/s',
      difficulty: '53.2 EH',
      epoch: '1543(320/1800)',
      tps: '12.3',
      estimatedEpochTime: '2h 12m',
      transactionsPerMinute: '540',
      transactionsPerDay: '777600',
      syncStatus: {
        isSyncing: false,
        syncedBlock: 18888888,
        tipBlock: 18888888,
        progress: 100,
        estimatedTime: null,
        chartDataMayBeIncomplete: false,
        blocksPerSecond: null,
        emaBlocksPerSecond: null,
        txsPerSecond: null,
        emaTxsPerSecond: null,
        syncMode: 'normal',
        startedAt: null,
        elapsedTime: null,
        totalTime: null,
      },
      deepForkStatus: {
        detected: false,
        detectedAt: null,
        depth: null,
        dbTip: 18888888,
        chainTip: 18888888,
        forkPoint: null,
      },
    } as any);

    vi.mocked(api.getBlocks).mockResolvedValue({
      data: [
        {
          number: 18888888,
          hash: '0xabcdef0123456789abcdef0123456789abcdef0123456789abcdef23456789',
        },
      ],
      total: 1,
      limit: 1,
      hasMore: false,
      nextCursor: null,
    } as any);

    render(<NotFoundPage />);

    expect(screen.getByText('404')).toBeInTheDocument();
    expect(
      screen.getByText('The cells you sought have fallen silent in the dark.')
    ).toBeInTheDocument();
    expect(screen.getByText('Elsewhere, unborn cells are gathering light.')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText('#18,888,888')).toBeInTheDocument();
    });
    expect(screen.getByText('0xabcdef01...23456789')).toBeInTheDocument();
    expect(screen.getByText('1.11 EH/s')).toBeInTheDocument();
    expect(screen.queryByText('Track Blocks')).not.toBeInTheDocument();
    expect(screen.queryByText('Ocean Tuning')).not.toBeInTheDocument();
    expect(screen.queryByText('Tip Block Height')).not.toBeInTheDocument();
    expect(screen.queryByText('Tip Hash')).not.toBeInTheDocument();
    expect(screen.queryByText('Energy Absorbing Rate')).not.toBeInTheDocument();
    expect(screen.queryByRole('img', { name: 'CKBadger 404' })).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'DAO' })).toHaveAttribute('href', '/dao');
    expect(screen.getByRole('link', { name: 'Assets' })).toHaveAttribute('href', '/assets');
    expect(screen.getByRole('link', { name: 'Scripts' })).toHaveAttribute('href', '/scripts');
    expect(screen.getByRole('link', { name: 'Charts' })).toHaveAttribute('href', '/charts');

    const telemetryStrip = screen.getByTestId('tip-values-strip');
    expect(telemetryStrip.className).toContain('bg-transparent');
    expect(telemetryStrip.className).not.toContain('bg-slate-');
  });

  it('falls back to placeholder telemetry values when requests fail', async () => {
    vi.mocked(api.getNetworkStats).mockRejectedValue(new Error('network down'));
    vi.mocked(api.getBlocks).mockRejectedValue(new Error('network down'));

    render(<NotFoundPage />);

    await waitFor(() => {
      expect(screen.getAllByText('--').length).toBeGreaterThanOrEqual(3);
    });
  });
});
