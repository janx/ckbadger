import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { act, fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth, stubPointerEvents } from '@/__tests__/utils/chart-width';
import { server } from '@/__tests__/msw/server';
import { daoStatistics } from '@/__tests__/msw/fixtures/home';
import { CkbytesSection, allocationOf } from '@/components/home/ckbytes-section';
import { MINUS } from '@/lib/chart-ticks';
import type { NetworkStats } from '@/lib/api';

const API = '/api/:network/v1';

/** Whole CKB → shannon string. */
const ckb = (whole: number | string) => `${whole}00000000`;

function networkStats(overrides: Partial<NetworkStats> = {}): NetworkStats {
  return {
    latestBlock: 20819348,
    avgBlockTime: '8.83s',
    hashRate: '93.54 PH/s',
    difficulty: '850.46 P',
    epoch: '15168(451/1590)',
    tps: '0.20',
    estimatedEpochTime: '2h 47m',
    transactionsPerMinute: '11.8',
    transactionsPerDay: '16960',
    syncStatus: {
      isSyncing: false,
      syncedBlock: 20819348,
      tipBlock: 20819348,
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
    knowledgeSize: '15113424500000000',
    circulatingSupply: '4986878303450933125',
    daoLocked: '812889561643799700',
    ...overrides,
  };
}

function serveStats(stats: NetworkStats) {
  server.use(http.get(`${API}/statistics/network`, () => HttpResponse.json(stats)));
}

describe('CkbytesSection', () => {
  beforeEach(() => {
    stubChartWidth(280);
    stubPointerEvents();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('prints circulation exactly from shannons, the decimals and unit dim', async () => {
    serveStats(networkStats());
    render(<CkbytesSection />);
    const total = await screen.findByTestId('ckbytes-circulating');
    expect(total).toHaveTextContent('49,868,783,034.50933125 CKB');
    expect(within(total).getByText('.50933125')).toHaveClass('text-text-dim');
    expect(screen.getByRole('link', { name: 'CKBytes in circulation' })).toHaveAttribute(
      'href',
      '/mainnet/charts/total-supply'
    );
  });

  it('splits it into knowledge, free and Nervos DAO with exact two-decimal shares', async () => {
    serveStats(networkStats());
    render(<CkbytesSection />);
    const bar = await screen.findByRole('img', {
      name: 'Knowledge 0.30 percent, free 83.40 percent, Nervos DAO 16.30 percent',
    });
    expect(bar.querySelector('[data-segment="knowledge"]')).toHaveClass('bg-jade', 'min-w-[3px]');
    expect(bar.querySelector('[data-segment="dao"]')).toHaveClass('bg-gold');
    expect(document.querySelector('[data-legend="Knowledge"]')).toHaveTextContent(
      'Knowledge151.13M CKB151.1 MB · 0.30%'
    );
    expect(document.querySelector('[data-legend="Free"]')).toHaveTextContent(
      'Free41.59B CKB83.40%'
    );
    expect(document.querySelector('[data-legend="Nervos DAO"]')).toHaveTextContent(
      'Nervos DAO8.13B CKB16.30%'
    );
  });

  it('keeps free as the exact remainder of circulation', () => {
    const a = allocationOf(
      networkStats({
        knowledgeSize: ckb(159_690_777),
        circulatingSupply: ckb(49_345_845_077),
        daoLocked: ckb(8_371_195_163),
      })
    );
    expect(a.state).toBe('ready');
    if (a.state === 'ready') expect(a.view.free).toBe(BigInt(ckb(40_814_959_137)));
  });

  it('surfaces an impossible allocation instead of clamping free to zero', async () => {
    serveStats(
      networkStats({
        knowledgeSize: ckb(40_000_000_000),
        daoLocked: ckb(20_000_000_000),
        circulatingSupply: ckb(49_345_845_077),
      })
    );
    render(<CkbytesSection />);
    const error = await screen.findByTestId('ckbytes-allocation-error');
    expect(error).toHaveAttribute('role', 'alert');
    expect(error.textContent).toContain('40,000,000,000');
    expect(error.textContent).toContain('20,000,000,000');
    expect(error.textContent).toContain('49,345,845,077');
    expect(screen.queryByTestId('ckbytes-circulating')).not.toBeInTheDocument();
  });

  it('says so while the supply figures are not available', async () => {
    serveStats(networkStats({ circulatingSupply: null }));
    render(<CkbytesSection />);
    expect(await screen.findByText('Supply figures are not available yet.')).toBeInTheDocument();
  });

  it('charts the last 30 days of knowledge-size change, clipping the outlier', async () => {
    serveStats(networkStats());
    render(<CkbytesSection />);
    const fig = screen.getByTestId('home-knowledge-change');
    await waitFor(() =>
      expect(screen.getByTestId('home-knowledge-change-headline')).toHaveTextContent('+82,991')
    );
    expect(fig).toHaveTextContent('daily change, CKB');
    const svg = within(fig).getByRole('img');
    expect(svg.querySelectorAll('[data-bar]')).toHaveLength(30);
    expect([...svg.querySelectorAll('[data-axis="y"] text')].map((t) => t.textContent)).toEqual([
      '2M',
      '0',
      `${MINUS}2M`,
    ]);
    const clipped = svg.querySelectorAll('[data-clipped="true"]');
    expect(clipped).toHaveLength(1);
    expect(clipped[0]).toHaveTextContent(`${MINUS}8.94M`);
    expect(svg).toHaveAccessibleName(/drawn clipped: −8\.94M on 09\/27/);
  });

  it('charts DAO deposit changes exactly: the headline is the last day, truncated to whole CKB', async () => {
    serveStats(networkStats());
    render(<CkbytesSection />);
    const fig = screen.getByTestId('home-dao-change');
    await waitFor(() =>
      expect(screen.getByTestId('home-dao-change-headline')).toHaveTextContent('+168,366')
    );
    const svg = within(fig).getByRole('img');
    // Hovering the last bar (centre at x ≈ 268) reads its exact change, decimals dim.
    fireEvent.pointerMove(svg, { clientX: 268 });
    const headline = screen.getByTestId('home-dao-change-headline');
    expect(headline).toHaveTextContent('+168,366.47983386');
    expect(within(headline).getByText('.47983386')).toHaveClass('text-text-dim', 'text-[10px]');
    expect(screen.getByTestId('home-dao-change-subtitle')).toHaveTextContent('10/23');
    fireEvent.pointerLeave(svg);
    expect(headline).toHaveTextContent(/^\+168,366$/);
    expect(screen.getByTestId('home-dao-change-subtitle')).toHaveTextContent('daily change, CKB');
    expect(svg.querySelector('[data-clipped="true"]')).toHaveTextContent('+28.12M');
  });

  it('shows the DAO block: exact deposit, APC, depositors, active deposits and the signed 24 h change', async () => {
    serveStats(networkStats());
    render(<CkbytesSection />);
    const dao = screen.getByTestId('home-dao');
    const deposited = await within(dao).findByTestId('home-dao-deposited');
    expect(deposited).toHaveTextContent('8,128,895,616.43799700 CKB');
    const terms = [...dao.querySelectorAll('dt')].map((t) => t.textContent);
    expect(terms).toEqual(['Estimated APC', 'Depositors', 'Active deposits', 'Change in 24 h']);
    expect(dao).toHaveTextContent('1.99%');
    expect(dao).toHaveTextContent(`16,673 ${MINUS}3 in 24 h`);
    expect(dao).toHaveTextContent('22,379');
    const change = within(dao).getByTestId('home-dao-change-24h');
    expect(change).toHaveTextContent(`${MINUS}2,578,825 CKB`);
    expect(change).toHaveClass('text-negative');
    expect(change).toHaveAttribute('title', `${MINUS}2,578,825.48689593 CKB`);
    expect(dao).not.toHaveTextContent(/Avg deposit/i);
    expect(within(dao).getByRole('link', { name: 'DAO details →' })).toHaveAttribute(
      'href',
      '/mainnet/dao'
    );
  });

  it('shows a dash, not a zero, when there is no previous day to compare with', async () => {
    serveStats(networkStats());
    const { depositChange24h: _a, depositorsChange24h: _b, ...first } = daoStatistics;
    server.use(http.get(`${API}/dao/statistics`, () => HttpResponse.json(first)));
    render(<CkbytesSection />);
    const dao = screen.getByTestId('home-dao');
    await within(dao).findByTestId('home-dao-deposited');
    expect(within(dao).queryByTestId('home-dao-change-24h')).not.toBeInTheDocument();
    expect(dao).toHaveTextContent('Change in 24 h—');
    // No depositor change either.
    expect(dao).toHaveTextContent('Depositors16,673Active deposits');
  });
});

describe('CkbytesSection allocation highlight', () => {
  beforeEach(() => {
    stubChartWidth(280);
    stubPointerEvents();
    serveStats(networkStats());
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  async function renderBar() {
    render(<CkbytesSection />);
    const bar = await screen.findByRole('img', { name: /^Knowledge 0\.30 percent/ });
    const seg = (part: string) => bar.querySelector(`[data-segment="${part}"]`) as HTMLElement;
    return { bar, seg };
  }
  const legend = (name: RegExp) => screen.getByRole('button', { name });

  function expectHighlighted(seg: (p: string) => HTMLElement, on: string) {
    for (const part of ['knowledge', 'free', 'dao']) {
      if (part === on) {
        expect(seg(part)).toHaveAttribute('data-active', 'true');
        expect(seg(part)).not.toHaveClass('opacity-35');
      } else {
        expect(seg(part)).not.toHaveAttribute('data-active');
        expect(seg(part)).toHaveClass('opacity-35');
      }
    }
  }
  function expectNoHighlight(seg: (p: string) => HTMLElement) {
    for (const part of ['knowledge', 'free', 'dao']) {
      expect(seg(part)).not.toHaveAttribute('data-active');
      expect(seg(part)).not.toHaveClass('opacity-35');
    }
  }

  it('highlights a hovered segment and reads it out exactly in its legend item', async () => {
    const { seg } = await renderBar();
    fireEvent.pointerEnter(seg('free'));
    expectHighlighted(seg, 'free');
    const free = legend(/^Free/);
    expect(free).toHaveTextContent('Free41,588,753,173.07133425 CKB83.40%');
    expect(within(free).getByText('Free')).toHaveClass('text-text-bright');

    fireEvent.pointerLeave(seg('free'));
    expectNoHighlight(seg);
    expect(free).toHaveTextContent('Free41.59B CKB83.40%');
    expect(within(free).getByText('Free')).toHaveClass('text-text');
  });

  it('highlights the segment of a hovered legend item', async () => {
    const { seg } = await renderBar();
    const dao = legend(/^Nervos DAO/);
    fireEvent.pointerEnter(dao);
    expectHighlighted(seg, 'dao');
    expect(dao).toHaveTextContent('Nervos DAO8,128,895,616.43799700 CKB16.30%');
    fireEvent.pointerLeave(dao);
    expectNoHighlight(seg);
  });

  it('highlights on keyboard focus of a legend item', async () => {
    const { seg } = await renderBar();
    const knowledge = legend(/^Knowledge/);
    act(() => knowledge.focus());
    expect(knowledge).toHaveFocus();
    expectHighlighted(seg, 'knowledge');
    expect(knowledge).toHaveTextContent('Knowledge151,134,245.00000000 CKB151.1 MB · 0.30%');
    act(() => knowledge.blur());
    expectNoHighlight(seg);
  });

  it('toggles the highlight with a tap on touch', async () => {
    const { seg } = await renderBar();
    const free = legend(/^Free/);
    const tap = (el: HTMLElement) => {
      fireEvent.pointerDown(el, { pointerType: 'touch' });
      fireEvent.pointerEnter(el, { pointerType: 'touch' });
      fireEvent.click(el);
      fireEvent.pointerLeave(el, { pointerType: 'touch' });
    };
    tap(free);
    expectHighlighted(seg, 'free');
    tap(free);
    expectNoHighlight(seg);
    tap(seg('dao'));
    expectHighlighted(seg, 'dao');
  });

  it('does not pin on a mouse click', async () => {
    const { seg } = await renderBar();
    const free = legend(/^Free/);
    fireEvent.pointerDown(free, { pointerType: 'mouse' });
    fireEvent.pointerEnter(free, { pointerType: 'mouse' });
    fireEvent.click(free);
    fireEvent.pointerLeave(free, { pointerType: 'mouse' });
    expectNoHighlight(seg);
  });
});
