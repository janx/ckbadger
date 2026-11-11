import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { http, HttpResponse } from 'msw';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '../utils/test-utils';
import { server } from '../msw/server';
import { StatusClock, StatusLine } from '@/components/layout/status-line';
import { useRealtimeStore } from '@/hooks/useRealtimeStore';

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

describe('StatusClock', () => {
  afterEach(() => vi.useRealTimers());

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
