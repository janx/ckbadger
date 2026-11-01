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
  it('shows unavailable block time and epoch estimate explicitly', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(
      stats({ avgBlockTime: null, estimatedEpochTime: null })
    );
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('Block time—'));
    expect(screen.getByTestId('vitals-epoch')).toHaveTextContent('—');
    expect(screen.getByTestId('vitals-epoch')).toHaveAttribute(
      'aria-label',
      expect.stringContaining('remaining time unavailable')
    );
  });

  it('shows undefined network hash rate as an em dash', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats({ hashRate: null }));
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('Hash rate'));
    expect(strip).toHaveTextContent('—');
  });
  beforeEach(() => {
    vi.clearAllMocks();
    useRealtimeStore.setState({ isConnected: false, socketHeld: false });
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
    expect(strip).toHaveTextContent('Block time8.43 s');
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

  // TX-01 (Main.dc.html vitals): "● Live" closes the strip, pushed to its right end, and only on
  // a page that holds the realtime socket; elsewhere nothing claims or denies a live feed.
  it('says nothing about live updates on a page that never opens the realtime socket (TX-01)', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('#20,819,143'));
    expect(screen.queryByTestId('vitals-live')).not.toBeInTheDocument();
    expect(screen.queryByTestId('vitals-live-spacer')).not.toBeInTheDocument();
    expect(strip).not.toHaveTextContent(/Live/);
  });

  it('ends the strip with "● Live" on the page holding the socket, "Not live" while it is down (TX-01)', async () => {
    useRealtimeStore.setState({ socketHeld: true });
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('#20,819,143'));
    const live = screen.getByTestId('vitals-live');
    expect(strip.lastElementChild).toBe(live);
    expect(live).toHaveClass('ml-auto');
    expect(live).toHaveTextContent('Not live');
    expect(live).toHaveAttribute('data-live', 'false');
    act(() => useRealtimeStore.setState({ isConnected: true }));
    expect(live).toHaveTextContent(/^Live$/);
    expect(live).toHaveAttribute('data-live', 'true');
    expect(live).toHaveAttribute('title', 'Live: realtime updates connected');
    // Nothing leads the Block item any more.
    expect(strip.firstElementChild).toBe(screen.getByRole('link', { name: /Block #20,819,143/ }));
  });

  it('writes the block time with a space before its unit and keeps one row on a phone (TX-05)', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('Block time8.43 s'));
    // Below md the strip is one row that scrolls inside itself; from md up it wraps as before.
    expect(strip).toHaveClass('flex-nowrap', 'overflow-x-auto', 'md:flex-wrap');
  });

  it('keeps every vital whole when the row overflows: nothing shrinks, the epoch never stacks', async () => {
    // A non-wrapping row that overflows shrinks its items to their min-content width. Text that
    // cannot wrap has none to give, so a wrapping epoch item would take all the squeeze and stack
    // its number, bar, index and time left into a column (seen at ~680 px). Every vital refuses to
    // shrink and the epoch is a plain row, so the strip scrolls as designed instead.
    useRealtimeStore.setState({ socketHeld: true });
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('#20,819,143'));
    const items = Array.from(strip.children) as HTMLElement[];
    expect(items).toHaveLength(6);
    for (const item of items) expect(item).toHaveClass('shrink-0');
    const epoch = screen.getByTestId('vitals-epoch');
    expect(epoch).toHaveClass('flex');
    expect(epoch).not.toHaveClass('flex-wrap');
  });

  it('refuses a block time that is not "<seconds>s" (TX-05)', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats({ avgBlockTime: '8.43 seconds' }));
    render(<VitalsStrip />);
    expect(await screen.findByTestId('vitals-block-time-error')).toHaveTextContent(
      'block time "8.43 seconds" is not <seconds>s'
    );
  });

  it('spaces the items 24 px apart from md up, with a 56 px epoch bar, so one row fits at 1280', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    expect(strip).toHaveClass('md:gap-x-6');
    const bar = (await screen.findByTestId('vitals-epoch-progress')).parentElement as HTMLElement;
    expect(bar).toHaveClass('md:w-14');
  });

  it('shows an explicit error, not endless skeletons, when the stats fail to load', async () => {
    vi.mocked(api.getNetworkStats).mockRejectedValue(new Error('HTTP 502'));
    render(<VitalsStrip />);
    const strip = screen.getByRole('region', { name: 'Chain vitals' });
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Chain vitals unavailable: HTTP 502');
    expect(alert).toHaveClass('text-rouge');
    expect(strip.querySelector('.animate-pulse')).toBeNull();
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
