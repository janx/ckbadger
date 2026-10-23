import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { act, fireEvent, render as renderPlain } from '@testing-library/react';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { server } from '@/__tests__/msw/server';
import {
  FIXTURE_TIP,
  blockHash,
  buildPipelineSnapshot,
  committedTxHash,
  mineNext,
  poolTxHash,
  shortIdOf,
} from '@/__tests__/msw/fixtures/pipeline-snapshot';
import type { PipelineSnapshot } from '@/lib/api';
import { PipelineCanvas, PipelinePanel } from '@/components/pipeline/pipeline-panel';
import { chipPaint } from '@/lib/pipeline/encoding';
import { G, buildScene, windowRuler, type Scene } from '@/lib/pipeline/layout';
import { usePipelineHoverStore } from '@/hooks/usePipelineHoverStore';
import { diffSnapshots } from '@/lib/pipeline/transitions';

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
    // T−7 proposed all 299 txs of T−5. The fixed card holds three rows of 12 chips (9 columns of
    // 8 px at a 6 px pitch); "+267" at the end of the last row gives up one slot per glyph.
    expect(states(T - 7)).toHaveLength(36 - 4);
    expect(card(svg, T - 7).querySelector('[data-chip-overflow]')).toHaveTextContent('+267');
  });

  it('draws an uncle as a dashed mini-card labelled "uncle" inside the fixed-width card', async () => {
    const svg = await renderPanel();
    const host = card(svg, T - 1);
    const uncle = host.querySelector('[data-uncle="0"]') as SVGGElement;
    expect(uncle).not.toBeNull();
    // Just the word: the uncle's number is in its tooltip.
    expect([...uncle.querySelectorAll('text')].map((t) => t.textContent)).toEqual(['uncle']);
    const box = uncle.querySelector('rect') as SVGRectElement;
    expect(box).toHaveAttribute('stroke-dasharray', '2 2');
    const hostW = Number(host.querySelector('[data-box]')?.getAttribute('width')) + 1;
    expect(hostW).toBe(81);
    expect(Number(box.getAttribute('x'))).toBe(G.padX);
    expect(Number(box.getAttribute('x')) + Number(box.getAttribute('width'))).toBe(hostW - G.padX);
    expect(host.querySelectorAll('[data-chip][data-uncle-index="0"]')).toHaveLength(2);
    // An uncle carries a header and proposals, never transactions.
    expect(uncle.querySelectorAll('[data-sq]')).toHaveLength(0);
  });

  it('draws committed txs as squares after the cellbase and counts the rest as +N', async () => {
    const svg = await renderPanel();
    const full = card(svg, T - 5);
    const squares = full.querySelectorAll('[data-sq]');
    // 9 × 14 cells hold the cellbase and 125 of the 238 listed txs; 113 more do not fit and 61
    // were omitted by the backend.
    expect(squares).toHaveLength(126);
    expect(squares[0]).toHaveAttribute('data-stage', 'cellbase');
    expect(full.querySelector('[data-hidden]')).toHaveTextContent('+174');
    expect(full.querySelector('[data-count]')).toHaveTextContent('300 tx');
  });

  it('draws no "none" text in an empty proposal row, on a block card or on NEXT', async () => {
    const s = buildPipelineSnapshot();
    const next = s.next as NonNullable<PipelineSnapshot['next']>;
    const svg = await renderPanel({ ...s, next: { ...next, proposals: 0 } });
    // T−8 has no proposals at all; T−6 has only the ones its uncle carries.
    for (const n of [T - 8, T - 6]) {
      expect(card(svg, n).querySelectorAll('[data-chip]:not([data-uncle-index])')).toHaveLength(0);
      expect(within(card(svg, n) as unknown as HTMLElement).queryByText('none')).toBeNull();
    }
    const nextCard = svg.querySelector('[data-next]') as SVGGElement;
    expect(nextCard.querySelectorAll('[data-ghost-chip]')).toHaveLength(0);
    expect(within(nextCard as unknown as HTMLElement).queryByText('none')).toBeNull();
  });

  it('writes the block and NEXT numbers in the card headers at 11 px', async () => {
    const svg = await renderPanel();
    expect(card(svg, T).querySelector('[data-block-number]')).toHaveClass('text-[11px]');
    const nextNumber = within(svg.querySelector('[data-next]') as unknown as HTMLElement).getByText(
      `…${String(T + 1).slice(-4)}`
    );
    expect(nextNumber).toHaveClass('text-[11px]');
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

  it('counts the predicted txs the backend omitted as +N on the NEXT card', async () => {
    const s = buildPipelineSnapshot();
    const next = s.next as NonNullable<PipelineSnapshot['next']>;
    const svg = await renderPanel({ ...s, next: { ...next, omittedTxs: 40 } });
    const card = svg.querySelector('[data-next]') as SVGGElement;
    expect(card.querySelectorAll('[data-ghost="tx"]')).toHaveLength(next.txs.length);
    expect(card.querySelector('[data-hidden]')).toHaveTextContent('+40');
    expect(card.querySelector('[data-count]')).toHaveTextContent(`~${next.txs.length + 40 + 1} tx`);
    expect(screen.getByTestId('pipeline-stats')).toHaveTextContent(
      `Next~${next.txs.length + 40 + 1} tx`
    );
  });

  it('writes "proposal window" exactly once on the canvas and no idle subtitles', async () => {
    const svg = (await renderPanel()) as unknown as HTMLElement;
    expect(within(svg).getAllByText('proposal window')).toHaveLength(1);
    // The ruler: two ticks, around the heights NEXT may pack proposals from.
    const ticks = svg.querySelector('[data-ruler-ticks]')?.getAttribute('d') ?? '';
    expect(ticks.match(/M/g)).toHaveLength(2);
    // Zone titles only: no subtitles under them and no row labels beside NEXT.
    for (const word of ['Mempool', 'Next', 'Chain']) {
      expect(within(svg).getByText(word)).toBeInTheDocument();
    }
    for (const gone of [
      'newest → older',
      'transaction pool',
      'being assembled',
      'just mined',
      'unavailable',
      'block',
      'proposals',
      'txs',
    ]) {
      expect(within(svg).queryByText(gone)).toBeNull();
    }
    expect(svg.querySelector('[data-chain-sub]')).toBeNull();
  });

  it('keys the canvas in one legend line: the stages, then cellbase, cycles and unknown', async () => {
    await renderPanel();
    const legend = screen.getByTestId('pipeline-legend');
    // The empty entry is the divider between the stage colours and the rest.
    expect([...legend.children].map((el) => el.textContent)).toEqual([
      'pending',
      'proposed',
      'too early',
      'committed',
      'expired',
      '',
      'cellbase',
      'cycles',
      'unknown',
    ]);
    // No usage hint under the canvas while nothing is pinned.
    expect(screen.queryByTestId('pipeline-selection')).toBeNull();
    expect(screen.getByTestId('pipeline-panel')).not.toHaveTextContent(/hover a/i);
  });

  it('puts what the panel shows and how to use it in the "?" title', async () => {
    await renderPanel();
    const title = screen.getByText('?').getAttribute('title') ?? '';
    expect(title).toContain('proposed in one block and committed in a later block');
    expect(title).toContain('Square area = bytes; shade = cycles.');
    expect(title).toMatch(/Hover a tx or a proposal.*click a tx to pin it/);
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
    expect(svg.querySelector('[data-next-column]')?.textContent).toBe('Nextunavailable');
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

describe('PipelineCanvas motion', () => {
  const s1 = buildPipelineSnapshot();
  const w0 = poolTxHash('w0');
  const q0 = poolTxHash('q0');
  const s2 = mineNext(s1, { commit: [w0], propose: [q0] });
  const t = diffSnapshots(s1, s2);
  const newHash = blockHash(T + 1);

  beforeEach(() => {
    vi.useFakeTimers({
      toFake: [
        'setTimeout',
        'clearTimeout',
        'requestAnimationFrame',
        'cancelAnimationFrame',
        'performance',
      ],
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  const at = (ms: number) =>
    act(() => {
      vi.advanceTimersByTime(ms);
    });

  it('boots without animating and measures before drawing', () => {
    const { getByTestId } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    expect(svg.getAttribute('viewBox')).toBe(`0 0 ${VIEW_W} ${G.H}`);
    expect(vi.getTimerCount()).toBe(0);
    expect(svg.querySelector('[data-layer="exit"]')?.childElementCount).toBe(0);
  });

  it('lands a mined block at NEXT, flies its commit in from the pool, then joins the chain', () => {
    expect(t.mode).toBe('mined');
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    const poolX = Number(svg.querySelector(`[data-pool] [data-sq="${w0}"]`)?.getAttribute('x'));
    rerender(<PipelineCanvas snapshot={s2} />);

    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    expect(landing).toHaveAttribute('data-slot', 'landing');
    expect(landing.getAttribute('transform')).toBe(`translate(${G.nextX.toFixed(2)},0.00)`);
    expect(svg.querySelector('[data-next]')).toBeNull();
    // Dashed until its commits land.
    expect(landing.querySelector('[data-box]')).toHaveAttribute('stroke-dasharray', '3 2');
    // The commit starts where it sat in the pool, expressed in the landing card's coordinates.
    const sq = landing.querySelector(`[data-sq="${w0}"]`) as SVGRectElement;
    expect(Number(sq.getAttribute('x'))).toBeCloseTo(poolX - G.nextX, 1);

    at(t.contact + 20);
    const target = buildScene(s2, VIEW_W, 'landing').cards[0].squares.find((q) => q.id === w0);
    expect(Number(sq.getAttribute('x'))).toBeCloseTo(target?.x as number, 1);

    // React flushes the phase step when act() ends; the 700 ms move runs after that.
    at(t.settleAt - t.contact);
    at(800);
    const joined = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    expect(joined).toHaveAttribute('data-slot', 'chain');
    expect(joined.getAttribute('transform')).toBe(`translate(${G.chainX0.toFixed(2)},0.00)`);
    expect(joined.querySelector('[data-box]')).not.toHaveAttribute('stroke-dasharray');
    expect(svg.querySelector('[data-next]')).not.toBeNull();
    expect(svg.querySelectorAll('[data-card]')).toHaveLength(
      buildScene(s2, VIEW_W, 'settled').cards.length
    );
  });

  it('retires the block that leaves the screen through the exit layer', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    const before = buildScene(s1, VIEW_W, 'settled').cards.length;
    rerender(<PipelineCanvas snapshot={s2} />);
    at(t.settleAt + 10);
    const exitLayer = svg.querySelector('[data-layer="exit"]') as SVGGElement;
    const visibleAfter = buildScene(s2, VIEW_W, 'settled').cards.length;
    // Every card that no longer fits leaves as a clone (none of them still claims a block).
    expect(exitLayer.childElementCount).toBeGreaterThanOrEqual(before + 1 - visibleAfter);
    expect(exitLayer.querySelectorAll('[data-card]')).toHaveLength(0);
    at(700);
    expect(exitLayer.querySelectorAll('g').length).toBe(0);
  });

  it('resolves a proposal chip when its commit lands, not before', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    const chip = svg.querySelector(
      `[data-card="${blockHash(T - 1)}"] [data-chip="${shortIdOf(w0)}|"]`
    ) as SVGRectElement;
    expect(chip).toHaveAttribute('data-state', 'committed');
    expect(chip.style.getPropertyValue('fill')).toBe(chipPaint('waiting').fill);
    at(t.contact + 20);
    expect(chip.style.getPropertyValue('fill')).toBe(chipPaint('committed').fill);
  });

  it('shows the final state at once with reduced motion', () => {
    stubMatchMedia(true);
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    const card = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    expect(card).toHaveAttribute('data-slot', 'chain');
    expect(card.getAttribute('transform')).toBe(`translate(${G.chainX0.toFixed(2)},0.00)`);
    expect(svg.querySelector('[data-next]')).not.toBeNull();
    expect(svg.querySelector('[data-layer="exit"]')?.childElementCount).toBe(0);
    expect(svg.querySelector('[data-layer="fx"]')?.childElementCount).toBe(0);
    const target = buildScene(s2, VIEW_W, 'settled').cards[0].squares.find((q) => q.id === w0);
    const sq = card.querySelector(`[data-sq="${w0}"]`) as SVGRectElement;
    expect(Number(sq.getAttribute('x'))).toBeCloseTo(target?.x as number, 1);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('PipelinePanel interactions', () => {
  const snapshot = buildPipelineSnapshot();
  const scene: Scene = buildScene(snapshot, VIEW_W, 'settled');
  const cardOf = (n: number) => {
    const c = scene.cards.find((k) => k.block.number === n);
    if (!c) throw new Error(`card ${n} not on screen`);
    return c;
  };
  const squareCentre = (n: number, id: string) => {
    const c = cardOf(n);
    const q = c.squares.find((k) => k.id === id);
    if (!q) throw new Error(`square ${id} not in card ${n}`);
    return { clientX: c.x + q.x + q.s / 2, clientY: q.y + q.s / 2 };
  };
  const chipCentre = (n: number, key: string) => {
    const c = cardOf(n);
    const k = c.chips.find((x) => x.key === key);
    if (!k) throw new Error(`chip ${key} not in card ${n}`);
    return { clientX: c.x + k.x + G.chip / 2, clientY: k.y + G.chip / 2 };
  };

  beforeEach(() => {
    usePipelineHoverStore.getState().clear();
    // jsdom has neither PointerEvent nor layout: a MouseEvent-based PointerEvent and 1:1 boxes.
    Object.defineProperty(window, 'PointerEvent', {
      configurable: true,
      writable: true,
      value: class PointerEventStub extends MouseEvent {},
    });
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      width: VIEW_W,
      height: G.H,
      right: VIEW_W,
      bottom: G.H,
      toJSON: () => ({}),
    } as DOMRect);
  });

  afterEach(() => {
    Reflect.deleteProperty(window, 'PointerEvent');
    vi.restoreAllMocks();
  });

  const tooltip = () => screen.getByTestId('pipeline-tooltip');
  const dimOfCard = (svg: Element, n: number) =>
    card(svg, n).querySelector(':scope > [data-dim]')?.getAttribute('data-dim');

  it('rings the proposal of a hovered committed tx, links them in gold and steps the rest back', async () => {
    const svg = await renderPanel(snapshot);
    const id = committedTxHash(T, 2);
    fireEvent.pointerMove(svg, squareCentre(T, id));

    expect(tooltip()).toHaveTextContent('committed in');
    expect(tooltip()).toHaveTextContent(`#${T.toLocaleString('en-US')}`);
    expect(tooltip()).toHaveTextContent(`#${(T - 2).toLocaleString('en-US')} (2 blocks earlier)`);
    expect(svg).toHaveClass('is-focus');
    expect(svg.querySelector(`[data-sq="${id}"]`)).toHaveClass('is-on');
    const ring = svg.querySelector('[data-hover="chip-ring"]') as SVGRectElement;
    const chip = chipCentre(T - 2, `${shortIdOf(id)}|`);
    expect(Number(ring.getAttribute('x')) + Number(ring.getAttribute('width')) / 2).toBeCloseTo(
      chip.clientX,
      1
    );
    expect(svg.querySelector('[data-hover="link"]')).toHaveAttribute('stroke-dasharray', '2 2');
    // The window now belongs to the block that committed it.
    expect(dimOfCard(svg, T)).toBe('false');
    expect(dimOfCard(svg, T - 5)).toBe('false');
    expect(dimOfCard(svg, T - 1)).toBe('true');
    expect(usePipelineHoverStore.getState()).toMatchObject({
      hoverTx: id,
      hoverBlock: T,
      windowBlock: T,
    });
  });

  it('names the uncle that carried a hovered proposal chip', async () => {
    const svg = await renderPanel(snapshot);
    fireEvent.pointerMove(svg, chipCentre(T - 1, `${shortIdOf(poolTxHash('u0'))}|0`));
    expect(tooltip()).toHaveTextContent('Proposal');
    expect(tooltip()).toHaveTextContent(
      new RegExp(`carried by\\s*uncle #${(T - 2).toLocaleString('en-US')}`)
    );
    expect(tooltip()).toHaveTextContent('in the pool, proposed');
  });

  it('explains an uncle box without moving the window', async () => {
    const svg = await renderPanel(snapshot);
    const c = cardOf(T - 1);
    const u = c.uncles[0];
    const box = card(svg, T - 1).querySelector('[data-uncle="0"] rect') as SVGRectElement;
    fireEvent.pointerMove(box, { clientX: c.x + G.padX + 30, clientY: u.y + 4 });
    expect(tooltip()).toHaveTextContent('not on the chain');
    // The mini-card only says "uncle"; its number is here.
    expect(tooltip()).toHaveTextContent(`height#${(T - 2).toLocaleString('en-US')}`);
    expect(tooltip()).toHaveTextContent(`included by#${(T - 1).toLocaleString('en-US')}`);
    expect(usePipelineHoverStore.getState()).toMatchObject({
      hoverBlock: T - 1,
      windowBlock: null,
    });
    expect(dimOfCard(svg, T - 3)).toBe('false');
  });

  it('explains the window lane with a help cursor and steps back pool txs outside it', async () => {
    const svg = await renderPanel(snapshot);
    const slots = [
      {
        number: T + 1,
        x: (scene.next as NonNullable<Scene['next']>).x,
        w: (scene.next as NonNullable<Scene['next']>).w,
        mined: false,
      },
      ...scene.cards.map((c) => ({ number: c.block.number, x: c.x, w: c.w, mined: true })),
    ];
    const ruler = windowRuler(slots, T + 1, snapshot.consensus, VIEW_W);
    fireEvent.pointerMove(svg, {
      clientX: (ruler.ticks[0] as number) + 10,
      clientY: G.cardTop - 8,
    });
    expect(tooltip()).toHaveTextContent('Proposal window');
    expect(tooltip()).toHaveTextContent('pool txs it can pack6 of 9');
    expect(svg.style.cursor).toBe('help');
    const dimOf = (name: string) =>
      svg.querySelector(`[data-sq="${poolTxHash(name)}"]`)?.parentElement?.getAttribute('data-dim');
    expect(dimOf('w0')).toBe('false');
    expect(dimOf('e0')).toBe('true');
    expect(dimOf('q0')).toBe('true');
  });

  it('anchors the window on a hovered block and steps back the blocks outside it', async () => {
    const svg = await renderPanel(snapshot);
    const c = cardOf(T - 3);
    const box = card(svg, T - 3).querySelector('[data-box]') as SVGRectElement;
    fireEvent.pointerMove(box, { clientX: c.x + c.w / 2, clientY: G.cardTop + 5 });
    expect(tooltip()).toHaveTextContent(`Block #${(T - 3).toLocaleString('en-US')}`);
    expect(box).toHaveAttribute('stroke-opacity', '1');
    expect(dimOfCard(svg, T - 3)).toBe('false');
    expect(dimOfCard(svg, T - 5)).toBe('false');
    expect(dimOfCard(svg, T)).toBe('true');
    expect(dimOfCard(svg, T - 2)).toBe('true');
    expect(svg.querySelector(`[data-sq="${poolTxHash('q0')}"]`)?.parentElement).toHaveAttribute(
      'data-dim',
      'soft'
    );
  });

  it('draws the reward link of a hovered cellbase and says what it pays', async () => {
    const svg = await renderPanel(snapshot);
    fireEvent.pointerMove(svg, squareCentre(T, snapshot.blocks[0].cellbase.hash));
    expect(svg.querySelector('[data-hover="reward"]')).not.toBeNull();
    expect(svg.querySelector('[data-chain-sub]')).toHaveTextContent(
      `the cellbase of …${String(T).slice(-4)} pays the reward of …${String(T - 11).slice(-4)}, 11 blocks back`
    );
    expect(svg.querySelector('[data-ruler-ticks]')).toHaveAttribute('opacity', '0.15');
    expect(tooltip()).toHaveTextContent('Cellbase');
  });

  it('pins a tx on click and lets Escape unpin it', async () => {
    const svg = await renderPanel(snapshot);
    const id = poolTxHash('q1');
    const q = scene.pool?.pending.squares.find((k) => k.id === id);
    const at = { clientX: (q?.x as number) + 3, clientY: (q?.y as number) + 3 };
    fireEvent.click(svg, at);
    expect(usePipelineHoverStore.getState().pinned).toBe(id);
    expect(screen.getByTestId('pipeline-selection')).toHaveTextContent('Selected');
    expect(screen.getByTestId('pipeline-selection')).toHaveTextContent('waiting to be proposed');
    expect(svg.querySelector('[data-hover="pin-ring"]')).not.toBeNull();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    expect(svg.querySelector('[data-hover="pin-ring"]')).toBeNull();
  });

  it('replaces the legend with the selection line while a tx is pinned', async () => {
    const svg = await renderPanel(snapshot);
    expect(screen.getByTestId('pipeline-legend')).toBeInTheDocument();
    expect(screen.queryByTestId('pipeline-selection')).toBeNull();

    const q = scene.pool?.pending.squares.find((k) => k.id === poolTxHash('q1'));
    fireEvent.click(svg, { clientX: (q?.x as number) + 3, clientY: (q?.y as number) + 3 });
    expect(screen.getByTestId('pipeline-selection')).toHaveTextContent('Selected');
    expect(screen.queryByTestId('pipeline-legend')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Clear selection' }));
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    expect(screen.getByTestId('pipeline-legend')).toBeInTheDocument();
    expect(screen.queryByTestId('pipeline-selection')).toBeNull();
  });

  it('follows hovers coming from the Latest Blocks and Latest Transactions rows', async () => {
    const svg = await renderPanel(snapshot);
    act(() => usePipelineHoverStore.getState().setHover({ block: T - 3, tx: null, window: T - 3 }));
    expect(card(svg, T - 3).querySelector('[data-box]')).toHaveAttribute('stroke-opacity', '1');
    expect(dimOfCard(svg, T)).toBe('true');
    const id = committedTxHash(T, 1);
    act(() => usePipelineHoverStore.getState().setHover({ block: T, tx: id, window: T }));
    expect(svg.querySelector(`[data-sq="${id}"]`)).toHaveClass('is-on');
    expect(svg.querySelector('[data-hover="ring"]')).not.toBeNull();
    act(() => usePipelineHoverStore.getState().clearHover());
    expect(svg).not.toHaveClass('is-focus');
  });

  const ghostCentre = (hash: string) => {
    const next = scene.next as NonNullable<Scene['next']>;
    const g = next.ghosts.find((k) => k.id === hash);
    if (!g) throw new Error(`no ghost for ${hash}`);
    return { clientX: next.x + g.x + g.s / 2, clientY: g.y + g.s / 2 };
  };

  it('rings the predicted ghost of a hovered pool tx and the pool square of a hovered ghost', async () => {
    const svg = await renderPanel(snapshot);
    const id = poolTxHash('w2');
    const q = scene.pool?.proposed.squares.find((k) => k.id === id);
    fireEvent.pointerMove(svg, { clientX: (q?.x as number) + 2, clientY: (q?.y as number) + 2 });
    expect(svg.querySelector('[data-hover="ghost-ring"]')).not.toBeNull();
    expect(svg.querySelector(`[data-next] [data-ghost-tx="${id}"]`)).not.toBeNull();

    fireEvent.pointerMove(svg, { clientX: 5, clientY: 5 });
    expect(svg.querySelector('[data-hover="ghost-ring"]')).toBeNull();
    fireEvent.pointerMove(svg, ghostCentre(id));
    expect(usePipelineHoverStore.getState().hoverTx).toBe(id);
    expect(svg.querySelector('[data-hover="ring"]')).not.toBeNull();
    expect(svg.querySelector('[data-hover="ghost-ring"]')).not.toBeNull();
    expect(tooltip()).toHaveTextContent('predicted');
    expect(tooltip()).toHaveTextContent(`#${(T + 1).toLocaleString('en-US')}`);
    // Clicking the ghost pins the pool tx it predicts.
    fireEvent.click(svg, ghostCentre(id));
    expect(usePipelineHoverStore.getState().pinned).toBe(id);
  });

  it('explains a ghost whose tx is not in the listed pool without failing', async () => {
    const outside = `0x${'ab'.repeat(32)}`;
    const next = snapshot.next as NonNullable<PipelineSnapshot['next']>;
    const s: PipelineSnapshot = {
      ...snapshot,
      next: { ...next, txs: [{ hash: outside, size: 900 }, ...next.txs] },
    };
    const svg = await renderPanel(s);
    const sc = buildScene(s, VIEW_W, 'settled').next as NonNullable<Scene['next']>;
    const g = sc.ghosts.find((k) => k.id === outside) as NonNullable<
      Scene['next']
    >['ghosts'][number];
    fireEvent.pointerMove(svg, { clientX: sc.x + g.x + g.s / 2, clientY: g.y + g.s / 2 });
    expect(screen.queryByTestId('pipeline-error')).toBeNull();
    expect(tooltip()).toHaveTextContent('predicted');
    expect(tooltip()).toHaveTextContent('not in the listed pool txs');
    expect(svg.querySelector('[data-hover="ring"]')).toBeNull();
    expect(svg.querySelector('[data-hover="ghost-ring"]')).not.toBeNull();
  });

  it('clears the hover when the pointer leaves', async () => {
    const svg = await renderPanel(snapshot);
    fireEvent.pointerMove(svg, squareCentre(T, committedTxHash(T, 2)));
    fireEvent.pointerLeave(svg);
    expect(screen.queryByTestId('pipeline-tooltip')).toBeNull();
    expect(usePipelineHoverStore.getState()).toMatchObject({ hoverTx: null, hoverBlock: null });
  });
});
