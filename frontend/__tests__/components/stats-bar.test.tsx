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
}));

/** The page the strip is drawn on (the setup's stub answers '/', the home page). */
const nav = vi.hoisted(() => ({ pathname: '/' }));
vi.mock('@/src/navigation', () => ({
  useRouter: () => ({
    push: vi.fn(),
    replace: vi.fn(),
    back: vi.fn(),
    forward: vi.fn(),
    refresh: vi.fn(),
    prefetch: vi.fn(),
  }),
  useSearchParams: () => new URLSearchParams(),
  usePathname: () => nav.pathname,
  useParams: () => ({}),
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
    // rubbing × Song print, phase 2: the progress in block characters, 246 / 1590 of the cells
    // filled (whole cells only): one of ten from 640 px, none of five on a phone
    const bar = screen.getByTestId('vitals-epoch-bar');
    expect(bar).toHaveAttribute('aria-hidden', 'true');
    expect(bar.querySelector('[data-cells="10"]')).toHaveTextContent(/^█░{9}$/);
    expect(bar.querySelector('[data-cells="5"]')).toHaveTextContent(/^░{5}$/);
    expect(bar.querySelector('[data-cells="10"] [data-part="full"]')).toHaveClass('text-jade');
    expect(bar.querySelector('[data-cells="10"] [data-part="empty"]')).toHaveClass('text-text-dim');
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
    expect(screen.queryByTestId('vitals-epoch-bar')).not.toBeInTheDocument();
  });

  // Regression: with the tip at genesis (a fresh local chain that has mined nothing) the tip
  // header's epoch is the zero epoch, `0(0/0)`: CKB builds the genesis header with it. The strip
  // says what the header says, draws no bar for a fraction the header does not state, and says why.
  it('reads the genesis tip’s zero epoch as what its header states, with no bar', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(
      stats({
        latestBlock: 0,
        epoch: '0(0/0)',
        avgBlockTime: null,
        hashRate: null,
        estimatedEpochTime: null,
      })
    );
    render(<VitalsStrip />);
    const epoch = await screen.findByRole('link', {
      name: 'Epoch 0, the genesis block: its header states no epoch length, remaining time unavailable',
    });
    expect(epoch).toHaveAttribute('data-testid', 'vitals-epoch');
    expect(epoch).toHaveTextContent(/^Epoch0genesis: no length0 \/ 0—$/);
    expect(screen.getByTestId('vitals-epoch-genesis')).toHaveClass('text-text-dim');
    expect(screen.queryByTestId('vitals-epoch-bar')).not.toBeInTheDocument();
    expect(screen.queryByTestId('vitals-epoch-error')).not.toBeInTheDocument();
  });

  it('refuses the zero epoch on a tip after genesis', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats({ latestBlock: 5, epoch: '0(0/0)' }));
    render(<VitalsStrip />);
    expect(await screen.findByTestId('vitals-epoch-error')).toHaveTextContent(
      'unreadable: block 5: epoch 0(0/0) is the genesis header’s alone'
    );
    expect(screen.queryByTestId('vitals-epoch-bar')).not.toBeInTheDocument();
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
    // Line 2: the epoch alone across the full width. Its bar is block characters (rubbing × Song
    // print, phase 2): five cells below 640 px, ten from 640 px, never shrunk or clipped.
    const epoch = screen.getByTestId('vitals-epoch');
    expect(epoch).toHaveClass('col-span-3', 'sm:col-span-4');
    const bar = screen.getByTestId('vitals-epoch-bar');
    expect(bar).toHaveClass('shrink-0');
    expect(bar.querySelector('[data-cells="5"]')).toHaveClass('sm:hidden');
    expect(bar.querySelector('[data-cells="10"]')).toHaveClass('hidden', 'sm:inline');
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
    // epoch is two groups whose text never wraps, its bar a fixed run of block characters (five
    // cells on a phone): its number and bar stay on one line, its index and time left on one line,
    // beside them where the line holds both and under them where it does not (readability F3-f;
    // re-pinned from one nowrap row, which ran past the line at 360 px at the 12 / 14 px scale).
    useRealtimeStore.setState({ socketHeld: true });
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('#20,819,143'));
    const items = Array.from(strip.children) as HTMLElement[];
    expect(items).toHaveLength(6);
    const epoch = screen.getByTestId('vitals-epoch');
    expect(epoch).toHaveClass('flex', 'flex-wrap');
    const groups = Array.from(epoch.children) as HTMLElement[];
    expect(groups).toHaveLength(2);
    for (const group of groups) expect(group).toHaveClass('flex', 'whitespace-nowrap');
    expect(screen.getByTestId('vitals-epoch-bar')).toHaveClass('shrink-0');
  });

  it('refuses a block time that is not "<seconds>s" (TX-05)', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats({ avgBlockTime: '8.43 seconds' }));
    render(<VitalsStrip />);
    expect(await screen.findByTestId('vitals-block-time-error')).toHaveTextContent(
      'block time "8.43 seconds" is not <seconds>s'
    );
  });

  it('spaces the items 24 px apart from md up, with a ten-cell epoch bar, so one row fits at 1280', async () => {
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    expect(strip).toHaveClass('md:gap-x-6');
    const bar = await screen.findByTestId('vitals-epoch-bar');
    expect([...(bar.querySelector('[data-cells="10"]')!.textContent ?? '')]).toHaveLength(10);
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

// PA-14 (UI review 2026-11-14): below md the three lines, with the 52 px header and the 26 px
// status line, took 212 of an 844 px first screen on every page. Ruling: below md the home page
// keeps the three lines; every other page has one line, the block number with the live mark and
// the epoch with its bar, the three mining figures being the home page's. From md the strip is
// the one wrapping row on every page. CSS decides the width (a layout CSS can do stays in CSS,
// hooks/useFromMd), so these read the classes that do it.
describe('VitalsStrip below md: three lines on the home page, one line elsewhere (PA-14)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useRealtimeStore.setState({ isConnected: true, socketHeld: true });
  });
  afterEach(() => {
    nav.pathname = '/';
    act(() => useRealtimeStore.getState().reset());
  });

  /**
   * The three mining figures, and the epoch's position with its time left (one group since
   * readability F3-f, so the home page's line can carry it under the bar; the group is what is
   * the home page's below md, as each of the two was).
   */
  function homeOnly() {
    const position = screen.getByText('246 / 1,590').parentElement as HTMLElement;
    expect(position).toContainElement(screen.getByText('≈ 3h 8m left'));
    return [
      screen.getByText('Block time').parentElement as HTMLElement,
      screen.getByRole('link', { name: /Hash rate/ }),
      screen.getByRole('link', { name: /Difficulty/ }),
      position,
    ];
  }

  it('keeps the three lines on the home page', async () => {
    nav.pathname = '/';
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('#20,819,143'));
    expect(strip).toHaveClass('grid', 'grid-cols-3');
    for (const el of homeOnly()) expect(el.className, el.textContent!).not.toMatch(/max-md:hidden/);
    expect(screen.getByTestId('vitals-live').lastElementChild).not.toHaveClass('max-sm:sr-only');
  });

  it('draws one line on any other page: the block, the epoch and its bar, the live mark', async () => {
    nav.pathname = '/blocks';
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('#20,819,143'));
    // one wrapping flex line below md, 12 px apart; from md the row every page has
    expect(strip).not.toHaveClass('grid');
    expect(strip).toHaveClass('flex', 'flex-wrap', 'items-baseline', 'gap-x-3', 'py-2.5');
    expect(strip).toHaveClass('md:gap-x-6', 'md:gap-y-2.5', 'md:py-3');
    // what the line holds, drawn at every width, placed by no grid track
    const block = screen.getByRole('link', { name: /Block #20,819,143/ });
    const epoch = screen.getByTestId('vitals-epoch');
    const live = screen.getByTestId('vitals-live');
    for (const el of [block, epoch, screen.getByTestId('vitals-epoch-bar'), live]) {
      expect(el.className).not.toMatch(/max-md:hidden|col-span|col-start|row-start/);
    }
    expect(epoch).toHaveTextContent('15,168');
    // what is the home page's below md, drawn from md
    for (const el of homeOnly()) expect(el, el.textContent!).toHaveClass('max-md:hidden');
    // the live mark closes the line; below 640 it is its dot, the word kept for a screen reader
    expect(live).toHaveClass('ml-auto');
    expect(live).toHaveTextContent(/^Live$/);
    expect(live.lastElementChild).toHaveTextContent('Live');
    expect(live.lastElementChild).toHaveClass('max-sm:sr-only');
    // the same items in the same order as the row from md
    expect(Array.from(strip.children)).toEqual([block, epoch, ...homeOnly().slice(0, 3), live]);
  });

  // Readability F3-f (the scale at 12 / 13 / 14 px): the home page's epoch line below md holds the
  // number, the five-cell bar, `index / length` and the time left, 6 px apart. At its widest real
  // content (`99,999`, `1,799 / 1,800`, `≈ 23h 59m left`) that is 340.8 px, and already 333.6 px
  // with today's `≈ 3h 59m left`: past the 328 px line at 360 and the 288 px one at 320, where the
  // nowrap epoch ran past the page. It breaks between its two groups, the epoch with its bar and
  // its position with the time left, never inside either (each holds at 320: 134.4 and 200.4 px).
  it('lets the epoch break between its bar and its position below md, never inside either (F3-f)', async () => {
    nav.pathname = '/';
    vi.mocked(api.getNetworkStats).mockResolvedValue(stats());
    render(<VitalsStrip />);
    const strip = await screen.findByRole('region', { name: 'Chain vitals' });
    await waitFor(() => expect(strip).toHaveTextContent('#20,819,143'));
    const epoch = screen.getByTestId('vitals-epoch');
    expect(epoch).toHaveClass('flex', 'flex-wrap');
    expect(epoch).not.toHaveClass('whitespace-nowrap');
    const groups = Array.from(epoch.children) as HTMLElement[];
    expect(groups).toHaveLength(2);
    const [barGroup, positionGroup] = groups;
    for (const group of groups) expect(group).toHaveClass('flex', 'whitespace-nowrap');
    expect(barGroup).toHaveTextContent(/^Epoch15,168/);
    expect(barGroup).toContainElement(screen.getByTestId('vitals-epoch-bar'));
    expect(positionGroup).toHaveTextContent(/^246 \/ 1,590≈ 3h 8m left$/);
    // the home page's: drawn below md here, as before
    expect(positionGroup.className).not.toMatch(/max-md:hidden/);
  });

  it('holds the one line while the figures load', () => {
    nav.pathname = '/tx/0xabc';
    useRealtimeStore.setState({ socketHeld: false });
    vi.mocked(api.getNetworkStats).mockImplementation(() => new Promise(() => {}));
    render(<VitalsStrip />);
    const strip = screen.getByRole('region', { name: 'Chain vitals' });
    const [block, epoch, ...mining] = Array.from(strip.children) as HTMLElement[];
    expect(block).toHaveTextContent('Block');
    expect(epoch).toHaveTextContent('Epoch');
    for (const el of [block, epoch]) expect(el.className).not.toMatch(/max-md:hidden|col-span/);
    expect(mining.map((m) => m.textContent)).toEqual(['Block time', 'Hash rate', 'Difficulty']);
    for (const m of mining) expect(m).toHaveClass('max-md:hidden');
  });
});
