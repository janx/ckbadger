import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClientProvider } from '@tanstack/react-query';
import { act, createTestQueryClient, render, screen, waitFor } from '../utils/test-utils';
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
  // Material pass v2 (move 4): a vital flares (`.phosphor-flare`, 1.2 s) when its served value
  // changes — never on mount, and only the value that changed.
  it('flares the value that changed, and no other', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <VitalsStrip />
      </QueryClientProvider>
    );
    const block = await screen.findByText('#20,819,143');
    expect(block).not.toHaveClass('phosphor-flare');
    // react-query notifies its observers on the next tick
    act(() => {
      client.setQueryData(['network-stats'], stats({ latestBlock: 20819144 }));
    });
    await waitFor(() => expect(screen.getByText('#20,819,144')).toHaveClass('phosphor-flare'));
    expect(screen.getByText('850.46 P')).not.toHaveClass('phosphor-flare');
    expect(screen.getByText('15,168')).not.toHaveClass('phosphor-flare');
    act(() => {
      client.setQueryData(
        ['network-stats'],
        stats({ latestBlock: 20819144, epoch: '15169(0/1590)', avgBlockTime: '8.50s' })
      );
    });
    await waitFor(() => expect(screen.getByText('15,169')).toHaveClass('phosphor-flare'));
    expect(screen.getByText('8.50 s')).toHaveClass('phosphor-flare');
    expect(screen.getByText('850.46 P')).not.toHaveClass('phosphor-flare');
  });

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

  it('writes the block time with a space before its unit and never scrolls sideways (TX-05)', async () => {
    useRealtimeStore.setState({ socketHeld: true });
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('Block time8.43 s'));
    // Below md the strip is three designed lines (SPEC.md "Vitals strip below md"), not a row
    // that pans: nothing in it scrolls sideways or refuses to wrap onto its next line.
    const scrollClass = /^(?:[a-z]+:)?(?:overflow-x-(?:auto|scroll)|overflow-auto|flex-nowrap)$/;
    for (const el of [strip, ...Array.from(strip.querySelectorAll('*'))]) {
      for (const cls of Array.from(el.classList)) expect(cls).not.toMatch(scrollClass);
    }
    expect(strip.className).not.toMatch(/whitespace-nowrap/);
    // From md up it is the one wrapping row it always was.
    expect(strip).toHaveClass('md:flex', 'md:flex-wrap');
  });

  it('lays the strip out below md as three lines: block and Live, the epoch, the three mining figures', async () => {
    useRealtimeStore.setState({ socketHeld: true, isConnected: true });
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('#20,819,143'));
    // Three equal columns below 640 (repeat(3, minmax(0, 1fr))); at 640–767 the mining figures
    // pack to the left as inline pairs and a fourth track takes the rest.
    expect(strip).toHaveClass('grid', 'grid-cols-3', 'gap-x-4', 'gap-y-2');
    expect(strip).toHaveClass('sm:grid-cols-[max-content_max-content_max-content_1fr]');
    // Line 1: the block on the left, Live at the right end of the same line.
    const block = screen.getByRole('link', { name: /Block #20,819,143/ });
    expect(block).toHaveClass('col-span-2', 'sm:col-span-3');
    const live = screen.getByTestId('vitals-live');
    expect(live).toHaveClass('row-start-1', 'col-start-3', 'sm:col-start-4', 'ml-auto');
    // Line 2: the epoch alone across the full width, its bar taking the free width.
    const epoch = screen.getByTestId('vitals-epoch');
    expect(epoch).toHaveClass('col-span-3', 'sm:col-span-4');
    const bar = screen.getByTestId('vitals-epoch-progress').parentElement as HTMLElement;
    expect(bar).toHaveClass('flex-[1_1_48px]', 'min-w-12', 'md:w-14', 'md:flex-none');
    // Line 3: block time, hash rate, difficulty, one track each, label over value below 640 and
    // label beside value from 640 up.
    const mining = [
      screen.getByText('Block time').parentElement as HTMLElement,
      screen.getByRole('link', { name: /Hash rate/ }),
      screen.getByRole('link', { name: /Difficulty/ }),
    ];
    for (const m of mining) {
      expect(m.className).not.toMatch(/col-span|col-start|row-start/);
      expect(m).toHaveClass('flex-col', 'sm:flex-row', 'sm:items-baseline');
    }
    // Document order is the md-up reading order: Live closes the row there.
    expect(Array.from(strip.children)).toEqual([block, epoch, ...mining, live]);
  });

  it('keeps the error rows on their own lines below md', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(
      stats({ epoch: '100/50/1000', avgBlockTime: '8.43 seconds' })
    );
    render(<VitalsStrip />);
    // The unreadable epoch keeps the epoch's full line; the bad block time keeps its mining track.
    expect(await screen.findByTestId('vitals-epoch-error')).toHaveClass(
      'col-span-3',
      'sm:col-span-4'
    );
    const blockTime = screen.getByTestId('vitals-block-time-error');
    expect(blockTime.className).not.toMatch(/col-span/);
    expect(blockTime).toHaveClass('flex-col', 'sm:flex-row');
  });

  it('gives a failed fetch the whole first line', async () => {
    vi.mocked(api.getNetworkStats).mockRejectedValue(new Error('HTTP 502'));
    render(<VitalsStrip />);
    expect(await screen.findByTestId('vitals-error')).toHaveClass('col-span-3', 'sm:col-span-4');
  });

  it('places the loading labels where the figures will be', () => {
    vi.mocked(api.getNetworkStats).mockImplementation(() => new Promise(() => {}));
    render(<VitalsStrip />);
    const strip = screen.getByRole('region', { name: 'Chain vitals' });
    const [block, epoch, ...mining] = Array.from(strip.children) as HTMLElement[];
    expect(block).toHaveTextContent('Block');
    expect(block).toHaveClass('col-span-2', 'sm:col-span-3');
    expect(epoch).toHaveTextContent('Epoch');
    expect(epoch).toHaveClass('col-span-3', 'sm:col-span-4');
    expect(mining.map((m) => m.textContent)).toEqual(['Block time', 'Hash rate', 'Difficulty']);
    for (const m of mining) expect(m).toHaveClass('flex-col', 'sm:flex-row');
  });

  it('keeps every vital whole: each is one element, and the epoch never stacks', async () => {
    // Below md each vital owns its grid track or line, so nothing is squeezed by a neighbour. The
    // epoch is a plain row whose text never wraps: only its bar gives up width (down to 48 px), so
    // its number, index and time left stay on one line instead of stacking into a column.
    useRealtimeStore.setState({ socketHeld: true });
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('#20,819,143'));
    const items = Array.from(strip.children) as HTMLElement[];
    expect(items).toHaveLength(6);
    const epoch = screen.getByTestId('vitals-epoch');
    expect(epoch).toHaveClass('flex', 'whitespace-nowrap');
    expect(epoch).not.toHaveClass('flex-wrap');
    const bar = screen.getByTestId('vitals-epoch-progress').parentElement as HTMLElement;
    expect(bar).toHaveClass('min-w-12');
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
