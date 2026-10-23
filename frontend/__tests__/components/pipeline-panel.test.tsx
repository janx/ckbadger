import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { server } from '@/__tests__/msw/server';
import {
  FIXTURE_TIP,
  blockHash,
  buildPipelineSnapshot,
  committedTxHash,
} from '@/__tests__/msw/fixtures/pipeline-snapshot';
import type { PipelineSnapshot } from '@/lib/api';
import { PipelinePanel } from '@/components/pipeline/pipeline-panel';
import { G, buildScene, chipsPerRow } from '@/lib/pipeline/layout';

const VIEW_W = 1600;
const T = FIXTURE_TIP;

class ResizeObserverStub {
  constructor(private readonly cb: ResizeObserverCallback) {}
  observe() {
    this.cb(
      [{ contentRect: { width: VIEW_W } } as ResizeObserverEntry],
      this as unknown as ResizeObserver
    );
  }
  unobserve() {}
  disconnect() {}
}

function stubMatchMedia(reduced: boolean) {
  vi.stubGlobal(
    'matchMedia',
    vi.fn((query: string) => ({
      matches: reduced && query.includes('reduce'),
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }))
  );
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', ResizeObserverStub);
  stubMatchMedia(false);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

async function renderPanel(snapshot: PipelineSnapshot = buildPipelineSnapshot()) {
  server.use(http.get('/api/:network/v1/pipeline', () => HttpResponse.json(snapshot)));
  render(<PipelinePanel />);
  return screen.findByTestId('pipeline-svg');
}

const card = (svg: Element, n: number) =>
  svg.querySelector(`[data-card="${blockHash(n)}"]`) as SVGGElement;

describe('PipelinePanel static render', () => {
  it('draws one card per visible block, newest first', async () => {
    const svg = await renderPanel();
    const expected = buildScene(buildPipelineSnapshot(), VIEW_W, 'settled').cards.length;
    const cards = [...svg.querySelectorAll('[data-card]')];
    expect(expected).toBeGreaterThanOrEqual(G.minBlocks);
    expect(cards).toHaveLength(expected);
    const numbers = cards.map((c) => Number(c.getAttribute('data-number')));
    expect(numbers[0]).toBe(T);
    expect(numbers).toEqual([...numbers].sort((a, b) => b - a));
  });

  it('draws every proposal as a chip in its state', async () => {
    const svg = await renderPanel();
    const states = (n: number) =>
      [...card(svg, n).querySelectorAll('[data-chip]')].map((c) => c.getAttribute('data-state'));
    expect(states(T)).toEqual(['early', 'early', 'early']);
    expect(states(T - 1)).toEqual(['waiting', 'waiting', 'waiting', 'waiting']);
    expect(states(T - 3).sort()).toEqual(['committed', 'committed', 'waiting']);
    expect(states(T - 12)).toContain('expired');
    // T−7 proposed all 299 txs of T−5: three chip rows, the rest as +N.
    const cap = chipsPerRow(G.maxCols) * G.propRows;
    expect(states(T - 7)).toHaveLength(cap);
    expect(card(svg, T - 7).querySelector('[data-chip-overflow]')).toHaveTextContent(
      `+${299 - cap}`
    );
  });

  it('draws an uncle as a small dashed card holding its own proposal chips', async () => {
    const svg = await renderPanel();
    const uncle = card(svg, T - 1).querySelector('[data-uncle="0"]') as SVGGElement;
    expect(uncle).not.toBeNull();
    expect(uncle).toHaveTextContent(`uncle …${String(T - 2).slice(-4)}`);
    expect(uncle.querySelector('rect')).toHaveAttribute('stroke-dasharray', '2 2');
    expect(card(svg, T - 1).querySelectorAll('[data-chip][data-uncle-index="0"]')).toHaveLength(2);
    // An uncle carries a header and proposals, never transactions.
    expect(uncle.querySelectorAll('[data-sq]')).toHaveLength(0);
  });

  it('draws committed txs as squares after the cellbase and counts the rest as +N', async () => {
    const svg = await renderPanel();
    const full = card(svg, T - 5);
    const squares = full.querySelectorAll('[data-sq]');
    // 14 × 17 cells minus the cellbase hold 237 of the 238 listed txs; 61 more were omitted.
    expect(squares).toHaveLength(238);
    expect(squares[0]).toHaveAttribute('data-stage', 'cellbase');
    expect(full.querySelector('[data-hidden]')).toHaveTextContent('+62');
    expect(full.querySelector('[data-count]')).toHaveTextContent('300 tx');
  });

  it('draws a tx whose cycles are still pending as a dashed square', async () => {
    const svg = await renderPanel();
    const sq = card(svg, T).querySelector(`[data-sq="${committedTxHash(T, 0)}"]`);
    expect(sq).toHaveAttribute('data-style', 'dashed');
    expect(sq).toHaveAttribute('stroke-dasharray', '2 1.5');
  });

  it('splits the pool into pending and proposed, hollow while too early to commit', async () => {
    const svg = await renderPanel();
    const pending = svg.querySelector('[data-pool-grid="pending"]') as SVGGElement;
    const proposed = svg.querySelector('[data-pool-grid="proposed"]') as SVGGElement;
    expect(pending.querySelectorAll('[data-sq]')).toHaveLength(5);
    expect(proposed.querySelectorAll('[data-sq]')).toHaveLength(9);
    expect(proposed.querySelectorAll('[data-style="hollow"]')).toHaveLength(3);
    expect(within(svg as unknown as HTMLElement).getByText('pending · 5')).toBeInTheDocument();
    expect(within(svg as unknown as HTMLElement).getByText('proposed · 9')).toBeInTheDocument();
  });

  it('draws the predicted next block dashed, with ghost txs and ghost chips', async () => {
    const svg = await renderPanel();
    const next = svg.querySelector('[data-next]') as SVGGElement;
    expect(next.querySelector('[data-box]')).toHaveAttribute('stroke-dasharray', '3 2');
    expect(next.querySelectorAll('[data-ghost="tx"]')).toHaveLength(6);
    expect(next.querySelectorAll('[data-ghost="cellbase"]')).toHaveLength(1);
    expect(next.querySelectorAll('[data-ghost-chip]')).toHaveLength(5);
    expect(next.querySelector('[data-count]')).toHaveTextContent('~7 tx');
    expect(next.querySelectorAll('[data-sq]')).toHaveLength(0);
  });

  it('writes "proposal window" exactly once on the canvas and a short subtitle', async () => {
    const svg = (await renderPanel()) as unknown as HTMLElement;
    expect(within(svg).getAllByText('proposal window')).toHaveLength(1);
    expect(within(svg).getByText('newest → older')).toBeInTheDocument();
    // The ruler: two ticks, around the heights NEXT may pack proposals from.
    const ticks = svg.querySelector('[data-ruler-ticks]')?.getAttribute('d') ?? '';
    expect(ticks.match(/M/g)).toHaveLength(2);
  });

  it('shows the header stats from the snapshot', async () => {
    await renderPanel();
    const stats = screen.getByTestId('pipeline-stats');
    expect(stats).toHaveTextContent('Mempool5');
    expect(stats).toHaveTextContent('Proposed9');
    expect(stats).toHaveTextContent('Next~7 tx');
    expect(stats).toHaveTextContent(`Tip#${T.toLocaleString('en-US')}`);
  });

  it('keeps drawing the chain when the pool tip differs from the synced tip', async () => {
    const s = buildPipelineSnapshot();
    const mismatch: PipelineSnapshot = {
      ...s,
      status: 'poolTipMismatch',
      poolTip: { number: T + 1, hash: blockHash(T + 1) },
      pool: null,
      next: null,
    };
    const svg = await renderPanel(mismatch);
    expect(svg.querySelector('[data-pool-status]')).toHaveTextContent('pool ≠ synced tip');
    expect(svg.querySelectorAll('[data-pool-grid]')).toHaveLength(0);
    expect(svg.querySelector('[data-next]')).toBeNull();
    expect(svg.querySelectorAll('[data-card]').length).toBeGreaterThanOrEqual(G.minBlocks);
    expect(screen.getByTestId('pipeline-stats')).toHaveTextContent('Mempool—');
  });

  it('shows an error row when the snapshot request fails', async () => {
    server.use(
      http.get('/api/:network/v1/pipeline', () =>
        HttpResponse.json(
          { error: 'internal_error', message: 'pipeline needs ckb_db_path' },
          { status: 500 }
        )
      )
    );
    render(<PipelinePanel />);
    const row = await screen.findByTestId('pipeline-error');
    expect(row).toHaveTextContent('pipeline needs ckb_db_path');
    expect(screen.queryByTestId('pipeline-svg')).toBeNull();
  });

  it('rejects a snapshot that breaks the contract instead of drawing around it', async () => {
    const s = buildPipelineSnapshot();
    const pool = s.pool as NonNullable<PipelineSnapshot['pool']>;
    const broken: PipelineSnapshot = {
      ...s,
      pool: {
        ...pool,
        proposed: pool.proposed.map((t, i) =>
          i === 0 ? { ...t, proposedIn: { block: T - 2, uncleIndex: null } } : t
        ),
      },
    };
    server.use(http.get('/api/:network/v1/pipeline', () => HttpResponse.json(broken)));
    render(<PipelinePanel />);
    const row = await screen.findByTestId('pipeline-error');
    expect(row).toHaveTextContent(/names #\d+.*no proposal for it/);
  });
});
