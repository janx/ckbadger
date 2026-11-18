import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { http, HttpResponse } from 'msw';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '../utils/test-utils';
import { server } from '../msw/server';
import { stubChartWidth } from '../utils/chart-width';
import { drawnStatusLinePx } from '../utils/shell-budget';
import { atViewportWidth } from '../utils/viewport';
import { StatusClock, StatusLine } from '@/components/layout/status-line';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';
import { formatBlockNumber } from '@/lib/format-block';

// Rubbing × Song print, phase 2 (lib.js `status()`): the page is a terminal session and the status
// line is its last row — fixed at the foot of the window, 26 px, the elevated ground under a 1 px
// line; the mode block, the network, the tip, the release and sync state, the key hints, a clock.

const API_BASE = '/api/:network/v1';

function mount(path = '/mainnet') {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/:network/*" element={<StatusLine />} />
      </Routes>
    </MemoryRouter>
  );
}

const line = () => screen.getByRole('status', { name: 'Status line' });
const mode = () => screen.getByTestId('status-mode');

beforeEach(() => {
  window.__CKBADGER_RUNTIME_CONFIG__ = {
    networks: [{ name: 'mainnet' }, { name: 'testnet' }],
    defaultNetwork: 'mainnet',
    buildVersion: '0.1.0+feature/foo@abcdef123456',
  };
  useRealtimeStore.setState({ socketHeld: false, isConnected: false });
});
afterEach(() => {
  delete window.__CKBADGER_RUNTIME_CONFIG__;
  act(() => {
    useRealtimeStore.getState().reset();
    useRealtimeStore.setState({ socketHeld: false });
  });
});

describe('StatusLine', () => {
  it('is one status row fixed at the foot of the window: 26 px, elevated, a line above', () => {
    mount();
    const bar = line();
    expect(bar).toHaveClass(
      'fixed',
      'inset-x-0',
      'bottom-0',
      'h-[26px]',
      'bg-base-elevated',
      'border-t',
      'border-base-border',
      'text-meta',
      'font-mono',
      'whitespace-nowrap',
      'overflow-hidden'
    );
    // the page ends above it
    const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');
    expect(css).toMatch(/\nbody\s*\{[^}]*padding-bottom: 26px;/);
  });

  it('reads every segment from the vitals: LIVE, the network, the tip, the release, synced', async () => {
    mount('/mainnet/blocks');
    await waitFor(() => expect(mode()).toHaveTextContent('LIVE'));
    const bar = line();
    // `LIVE` in literal capitals on a celadon block in the ground's ink — no case class
    expect(mode()).toHaveTextContent(/^LIVE$/);
    expect(mode()).toHaveClass('bg-jade', 'text-base-bg', 'font-semibold');
    expect(mode().className).not.toMatch(/uppercase|tracking/);
    expect(mode()).toHaveAttribute('data-mode', 'LIVE');
    const segments = Array.from(bar.children) as HTMLElement[];
    expect(segments[0]).toBe(mode());
    expect(segments[1]).toHaveTextContent(/^mainnet$/);
    expect(segments[2]).toHaveTextContent('tip#1,000,000');
    expect(segments[3]).toHaveTextContent(/^ckbadger 0\.1\.0 · synced$/);
    expect(segments[3]).toHaveAttribute('title', 'ckbadger 0.1.0+feature/foo@abcdef123456');
    // no node version: the vitals payload carries none, and none is made up
    expect(bar).not.toHaveTextContent(/node/);
  });

  it('names the network of the route it is under', async () => {
    mount('/testnet/tx/0xabc');
    await waitFor(() => expect(mode()).toHaveTextContent('LIVE'));
    expect(line().children[1]).toHaveTextContent(/^testnet$/);
  });

  it('closes on the help’s key hints as <kbd> and the clock, at its right end', async () => {
    mount();
    const right = line().lastElementChild as HTMLElement;
    expect(right).toHaveClass('ml-auto');
    const cells = Array.from(right.children) as HTMLElement[];
    expect(cells.map((c) => c.textContent)).toEqual([
      '?help',
      '/search',
      'g bblocks',
      expect.stringMatching(/^\d{2}:\d{2}:\d{2}$/),
    ]);
    expect(Array.from(right.querySelectorAll('kbd')).map((k) => k.textContent)).toEqual([
      '?',
      '/',
      'g b',
    ]);
    expect(cells[3]).toContainElement(screen.getByTestId('status-clock'));
    for (const c of cells) expect(c).toHaveClass('border-l');
  });

  it('says SYNCING on gold while the indexer is behind the node', async () => {
    server.use(
      http.get(`${API_BASE}/statistics/network`, () =>
        HttpResponse.json({
          latestBlock: 1000000,
          avgBlockTime: '8.50s',
          hashRate: '1.5 PH/s',
          difficulty: '0x1000000000',
          epoch: '100(50/1000)',
          tps: '2.5',
          estimatedEpochTime: '4h 30m',
          transactionsPerMinute: '150',
          transactionsPerDay: '216000',
          syncStatus: {
            isSyncing: true,
            syncedBlock: 400000,
            tipBlock: 1000000,
            progress: 40,
            estimatedTime: '1h',
            chartDataMayBeIncomplete: true,
            blocksPerSecond: 1000,
            emaBlocksPerSecond: 1000,
            syncMode: 'bulk',
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
        })
      )
    );
    mount();
    await waitFor(() => expect(mode()).toHaveTextContent(/^SYNCING$/));
    expect(mode()).toHaveClass('bg-gold', 'text-base-bg');
    expect(mode()).toHaveAttribute('title', 'Syncing: indexed #400,000 of #1,000,000');
    expect(line().children[3]).toHaveTextContent(/^ckbadger 0\.1\.0 · syncing$/);
  });

  it('says OFFLINE on cinnabar when the vitals cannot be read, with no tip made up', async () => {
    server.use(
      http.get(`${API_BASE}/statistics/network`, () =>
        HttpResponse.json({ error: 'upstream down' }, { status: 502 })
      )
    );
    mount();
    await waitFor(() => expect(mode()).toHaveTextContent(/^OFFLINE$/));
    expect(mode()).toHaveClass('bg-rouge', 'text-base-bg');
    expect(mode().getAttribute('title')).toMatch(/^Offline: chain vitals unavailable: /);
    expect(line().children[2]).toHaveTextContent(/^tip—$/);
    expect(line().children[3]).toHaveTextContent(/^ckbadger 0\.1\.0$/);
  });

  it('follows the realtime socket on a page that holds it, as the vitals strip’s Live mark does', async () => {
    useRealtimeStore.setState({ socketHeld: true, isConnected: false });
    mount();
    await waitFor(() => expect(mode()).toHaveTextContent(/^OFFLINE$/));
    expect(mode()).toHaveAttribute('title', 'Offline: realtime updates not connected');
    act(() => useRealtimeStore.setState({ isConnected: true }));
    expect(mode()).toHaveTextContent(/^LIVE$/);
  });

  it('claims no mode before the vitals arrive', () => {
    server.use(http.get(`${API_BASE}/statistics/network`, () => new Promise<never>(() => {})));
    mount();
    expect(mode()).toHaveTextContent('—');
    expect(mode()).toHaveAttribute('data-mode', 'unknown');
    expect(line().children[2]).toHaveTextContent(/^tip—$/);
  });

  // The designed narrow form: one row of the mode, the network, the tip and the clock. The release
  // and the key hints show from md up only (a touch screen has no keys; the release is in the help).
  it('keeps the mode, the network, the tip and the clock below md; the release and keys from md', async () => {
    mount();
    await waitFor(() => expect(mode()).toHaveTextContent('LIVE'));
    const bar = line();
    const [modeCell, network, tip, release] = Array.from(bar.children) as HTMLElement[];
    for (const el of [modeCell, network, tip]) expect(el.className).not.toMatch(/\bhidden\b/);
    expect(release).toHaveClass('hidden', 'md:flex');
    const right = bar.lastElementChild as HTMLElement;
    const cells = Array.from(right.children) as HTMLElement[];
    for (const key of cells.slice(0, 3)) expect(key).toHaveClass('hidden', 'md:flex');
    expect(cells[3].className).not.toMatch(/\bhidden\b/);
    expect(right.className).not.toMatch(/\bhidden\b/);
  });

  it('is a polite region for its mode, with the tip and the clock kept quiet', async () => {
    mount();
    await waitFor(() => expect(mode()).toHaveTextContent('LIVE'));
    expect(line()).toHaveAttribute('role', 'status');
    expect(line().children[2].querySelector('[aria-live="off"]')).toHaveTextContent('#1,000,000');
    expect(screen.getByTestId('status-clock')).toHaveAttribute('aria-live', 'off');
  });
});

/** The vitals as the API serves them, mainnet's tip of 2026-11-18 by default (eight digits). */
function vitals({ latestBlock = 21_045_159, isSyncing = false } = {}) {
  return {
    latestBlock,
    avgBlockTime: '10.14s',
    hashRate: '73.56 PH/s',
    difficulty: '850.46 P',
    epoch: '15317(674/1644)',
    tps: '0.2',
    estimatedEpochTime: '2h 43m',
    transactionsPerMinute: '1',
    transactionsPerDay: '1',
    syncStatus: {
      isSyncing,
      syncedBlock: isSyncing ? latestBlock - 1_000 : latestBlock,
      tipBlock: latestBlock,
      progress: isSyncing ? 99 : 100,
      estimatedTime: null,
      chartDataMayBeIncomplete: isSyncing,
      blocksPerSecond: null,
      emaBlocksPerSecond: null,
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
  };
}

// Readability F3-f (the type scale at 12 / 13 / 14 px): the line's text is the meta step, 12 px,
// 7.2 px a glyph. Measured in Chrome on the new scale its parts ran 773 px in a 768 px line (the key
// hints and the clock cut 5 px short) and 338.6 px in a 320 px one (the clock cut at 269…327): the
// breakpoint that let the release and the hints in from md, and the narrow form's four parts below
// it, had fitted at 11 px. A part is drawn at the widths where the line holds it, and leaves in
// one order (docs/RESPONSIVE_LAYOUT.md "Status line"): the key hints from the last, then the tip's
// word "tip" (its `#` says it is a block), then the tip, the network, the release; the mode and the
// clock last of all.
describe('StatusLine holds its parts at its width (readability F3-f)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  /** The line in a window `width` px wide, the vitals it reads `vitals(…)`; the line measures itself. */
  async function at(width: number, served: ReturnType<typeof vitals>, path = '/mainnet') {
    server.use(http.get(`${API_BASE}/statistics/network`, () => HttpResponse.json(served)));
    atViewportWidth(width);
    stubChartWidth(width);
    mount(path);
    const tip = formatBlockNumber(served.latestBlock);
    await waitFor(() => expect(line()).toHaveTextContent(tip));
    return line();
  }

  const rightCells = (bar: HTMLElement) =>
    Array.from((bar.lastElementChild as HTMLElement).children).map((c) => c.textContent);

  it('holds every part but the last key hint at 768 px, where it ran 5 px past the line', async () => {
    const bar = await at(768, vitals());
    expect(drawnStatusLinePx(bar, true)).toBeLessThanOrEqual(768);
    expect(mode()).toHaveTextContent(/^LIVE$/);
    expect(bar.children[1]).toHaveTextContent(/^mainnet$/);
    expect(bar.children[2]).toHaveTextContent(/^tip#21,045,159$/);
    expect(bar.children[3]).toHaveTextContent(/^ckbadger 0\.1\.0 · synced$/);
    expect(rightCells(bar)).toEqual([
      '?help',
      '/search',
      expect.stringMatching(/^\d\d:\d\d:\d\d$/),
    ]);
  });

  it('holds all of them from 1024 px, syncing and with a nine-digit tip', async () => {
    const served = vitals({ latestBlock: 999_999_999, isSyncing: true });
    const bar = await at(1024, served);
    expect(drawnStatusLinePx(bar, true)).toBeLessThanOrEqual(1024);
    expect(mode()).toHaveTextContent(/^SYNCING$/);
    expect(bar.children[3]).toHaveTextContent(/^ckbadger 0\.1\.0 · syncing$/);
    expect(rightCells(bar).slice(0, 3)).toEqual(['?help', '/search', 'g bblocks']);
  });

  it('keeps the clock whole at 320 px: the tip says its number without its word', async () => {
    const bar = await at(320, vitals());
    expect(drawnStatusLinePx(bar, false)).toBeLessThanOrEqual(320);
    expect(mode()).toHaveTextContent(/^LIVE$/);
    expect(bar.children[1]).toHaveTextContent(/^mainnet$/);
    const tip = bar.children[2] as HTMLElement;
    expect(tip.querySelector('[aria-live="off"]')).toHaveTextContent(/^#21,045,159$/);
    // the word stays for a screen reader
    expect(tip).toHaveTextContent(/^tip ?#21,045,159$/);
    expect(tip.querySelector('.sr-only')).toHaveTextContent(/^tip$/);
    expect(screen.getByTestId('status-clock')).toHaveTextContent(/^\d\d:\d\d:\d\d$/);
  });

  it('holds the mode, the network, the tip and the clock at 360 px while syncing, the tip without its word', async () => {
    const bar = await at(360, vitals({ isSyncing: true }));
    expect(drawnStatusLinePx(bar, false)).toBeLessThanOrEqual(360);
    expect(mode()).toHaveTextContent(/^SYNCING$/);
    expect(bar.children[1]).toHaveTextContent(/^mainnet$/);
    expect((bar.children[2] as HTMLElement).querySelector('.sr-only')).toHaveTextContent(/^tip$/);
    expect(screen.getByTestId('status-clock')).toBeInTheDocument();
  });

  it('keeps the tip with its word where the line holds it: 390 px, live', async () => {
    const bar = await at(390, vitals());
    expect(drawnStatusLinePx(bar, false)).toBeLessThanOrEqual(390);
    expect(bar.children[2]).toHaveTextContent(/^tip#21,045,159$/);
    expect((bar.children[2] as HTMLElement).querySelector('.sr-only')).toBeNull();
  });
});

describe('StatusClock', () => {
  afterEach(() => vi.useRealTimers());

  // A4 review 5: where Intl gives no time zone the title names none ("Local time, undefined").
  it('says only "Local time" where Intl gives no time zone', () => {
    const resolved = Intl.DateTimeFormat().resolvedOptions();
    const spy = vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({
      ...resolved,
      timeZone: undefined as unknown as string,
    });
    try {
      render(<StatusClock />);
      expect(screen.getByTestId('status-clock')).toHaveAttribute('title', 'Local time');
    } finally {
      spy.mockRestore();
    }
  });

  it('tells local time and ticks once a second, on the second', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 10, 11, 14, 2, 31, 400));
    render(<StatusClock />);
    const clock = screen.getByTestId('status-clock');
    expect(clock).toHaveTextContent('14:02:31');
    expect(clock.tagName).toBe('TIME');
    expect(clock).toHaveAttribute('title', expect.stringMatching(/^Local time, \S+/));
    // the first tick waits for the next whole second (600 ms here), then one per second
    act(() => vi.advanceTimersByTime(599));
    expect(clock).toHaveTextContent('14:02:31');
    act(() => vi.advanceTimersByTime(1));
    expect(clock).toHaveTextContent('14:02:32');
    act(() => vi.advanceTimersByTime(1000));
    expect(clock).toHaveTextContent('14:02:33');
    act(() => vi.advanceTimersByTime(60_000));
    expect(clock).toHaveTextContent('14:03:33');
  });

  it('stops ticking when it unmounts', () => {
    vi.useFakeTimers();
    const { unmount } = render(<StatusClock />);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});
