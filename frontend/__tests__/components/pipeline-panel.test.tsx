import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { act, fireEvent, render as renderPlain } from '@testing-library/react';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { server } from '@/__tests__/msw/server';
import {
  FIXTURE_TIP,
  blockHash,
  buildPipelineSnapshot,
  cellbaseHash,
  committedTxHash,
  mineNext,
  poolTxHash,
  shortIdOf,
} from '@/__tests__/msw/fixtures/pipeline-snapshot';
import type { PipelineSnapshot } from '@/lib/api';
import { PipelineCanvas, PipelinePanel } from '@/components/pipeline/pipeline-panel';
import {
  TILE_FILL,
  WELL_FILL,
  ageOpacity,
  chipPaint,
  diamondPath,
  tok,
} from '@/lib/pipeline/encoding';
import {
  BLOCK_FIELD_CAPACITY,
  CARD_H,
  G,
  buildScene,
  inWindow,
  labelWidth,
  rulerPath,
  tail4,
  windowLabel,
  windowMarksPath,
  windowRuler,
  type RulerSlot,
  type Scene,
} from '@/lib/pipeline/layout';
import { FRESH_MS } from '@/lib/pipeline/motion';
import { usePipelineHoverStore } from '@/hooks/usePipelineHoverStore';
import { diffSnapshots } from '@/lib/pipeline/transitions';

const VIEW_W = 1600;
/** The page container is 1280 px wide; the canvas inside its padding is 1246 px. */
const CONTAINER_VIEW_W = 1246;
const T = FIXTURE_TIP;
const fmt = (n: number) => n.toLocaleString('en-US');
/** The width the stubbed ResizeObserver reports; a test may change it before rendering. */
let viewW = VIEW_W;

class ResizeObserverStub {
  constructor(private readonly cb: ResizeObserverCallback) {}
  observe() {
    this.cb(
      [{ contentRect: { width: viewW } } as ResizeObserverEntry],
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
  viewW = VIEW_W;
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
const tile = (svg: Element, n: number) => card(svg, n).querySelector('[data-box]') as SVGElement;
const path = (svg: Element, name: string) =>
  svg.querySelector(`[data-path="${name}"]`) as SVGPathElement;

/** The ruler slots of a scene: NEXT (when shown), then the chain. */
const rulerSlots = (scene: Scene): RulerSlot[] => [
  ...(scene.next
    ? [{ number: scene.next.number, x: scene.next.x, w: scene.next.w, mined: false }]
    : []),
  ...scene.cards.map((c) => ({ number: c.block.number, x: c.x, w: c.w, mined: true })),
];

/** A path made only of horizontal segments on the chain line (or the empty placeholder). */
const CHAIN_Y = String(G.chainY + 0.5);
const isPlainLine = (d: string | null) =>
  d === 'M0 0' || new RegExp(`^(M[\\d.]+ ${CHAIN_Y}L[\\d.]+ ${CHAIN_Y})+$`).test(d ?? '');

/**
 * The drawn window label is fully inside [0, viewW], clear of every tick, and the dimension line
 * never runs under it. Reads only the DOM, so it checks the static render and the motion layer alike.
 */
function expectLabelClear(svg: Element, width: number) {
  const label = svg.querySelector('[data-window-label]') as SVGTextElement;
  expect(label).not.toBeNull();
  expect(label).toHaveAttribute('text-anchor', 'middle');
  const half = labelWidth(label.textContent ?? '') / 2;
  const x = Number(label.getAttribute('x'));
  const [l, r] = [x - half, x + half];
  expect(l).toBeGreaterThanOrEqual(0);
  expect(r).toBeLessThanOrEqual(width);
  const d = svg.querySelector('[data-ruler-ticks]')?.getAttribute('d') ?? '';
  const ticks = [...d.matchAll(/M([\d.]+) [\d.]+V/g)].map((m) => Number(m[1]));
  expect(ticks.length).toBeGreaterThan(0);
  for (const tx of ticks) expect(tx < l || tx > r).toBe(true);
  const line = [...d.matchAll(/M([\d.]+) [\d.]+H([\d.]+)/g)].map((m) => [
    Number(m[1]),
    Number(m[2]),
  ]);
  expect(line.length).toBeGreaterThan(0);
  for (const [a, b] of line) expect(b <= l || a >= r).toBe(true);
}

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

  it('draws every proposal as a dot in its state', async () => {
    viewW = 2000; // wide enough to show T−12 and its expired proposals
    const svg = await renderPanel();
    const states = (n: number) =>
      [...card(svg, n).querySelectorAll('[data-chip]')].map((c) => c.getAttribute('data-state'));
    expect(states(T)).toEqual(['early', 'early', 'early']);
    expect(states(T - 1)).toEqual(['waiting', 'waiting', 'waiting', 'waiting']);
    expect(states(T - 3).sort()).toEqual(['committed', 'committed', 'waiting']);
    expect(states(T - 12)).toContain('expired');
    // A proposal short id is a dot, never a square.
    const dots = [...svg.querySelectorAll('[data-chip]')];
    expect(dots.length).toBeGreaterThan(0);
    expect(new Set(dots.map((d) => d.tagName))).toEqual(new Set(['circle']));
    // T−7 proposed all 299 txs of T−5. The fixed card holds two rows of 10 dots (8 columns of
    // 10 px at an 8 px pitch); "+283" at the end of the last row gives up one slot per glyph.
    expect(states(T - 7)).toHaveLength(20 - 4);
    expect(card(svg, T - 7).querySelector('[data-chip-overflow]')).toHaveTextContent('+283');
  });

  it('draws an uncle as an inset mini-card labelled "uncle", without an outline', async () => {
    const svg = await renderPanel();
    const host = card(svg, T - 1);
    const uncle = host.querySelector('[data-uncle="0"]') as SVGGElement;
    expect(uncle).not.toBeNull();
    // Just the word: the uncle's number is in its tooltip.
    expect([...uncle.querySelectorAll('text')].map((t) => t.textContent)).toEqual(['uncle']);
    const box = uncle.querySelector('rect') as SVGRectElement;
    expect(box).not.toHaveAttribute('stroke-dasharray');
    expect(box.style.getPropertyValue('stroke')).toBe('');
    expect(box.style.getPropertyValue('fill')).toBe(tok('surface'));
    const hostW = Number(host.querySelector('[data-box]')?.getAttribute('width')) + 1;
    expect(hostW).toBe(89);
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
    // 8 × 5 cells hold the cellbase and all 39 listed txs; the other 260 were omitted by the
    // backend, which never lists more than a card can draw.
    expect(squares).toHaveLength(BLOCK_FIELD_CAPACITY);
    expect(squares[0]).toHaveAttribute('data-stage', 'cellbase');
    const count = full.querySelector('[data-count]') as SVGTextElement;
    expect(count.textContent).toBe('+260');
    // Right-aligned on the header row.
    expect(count).toHaveAttribute('text-anchor', 'end');
    expect(Number(count.getAttribute('x'))).toBe(G.cardW - G.padX);
    expect(Number(count.getAttribute('y'))).toBe(G.chainY + 3.5);
  });

  it('shows only the number in a block header, and "+N" only when txs are hidden', async () => {
    const svg = await renderPanel();
    const scene = buildScene(buildPipelineSnapshot(), VIEW_W, 'settled');
    expect(scene.cards.some((c) => c.hidden > 0)).toBe(true);
    expect(scene.cards.some((c) => c.hidden === 0)).toBe(true);
    for (const c of scene.cards) {
      const el = card(svg, c.block.number);
      const header = [...el.querySelectorAll(':scope > [data-age] > [data-dim] > text')].map(
        (t) => t.textContent
      );
      expect(header).toEqual(
        c.hidden > 0 ? [tail4(c.block.number), `+${c.hidden}`] : [tail4(c.block.number)]
      );
    }
    // No "N tx" count anywhere on the canvas.
    expect(svg.textContent).not.toMatch(/\d tx\b/);
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

  it('draws a mined block as a solid tile, without an outline at rest', async () => {
    const svg = await renderPanel();
    for (const n of [T, T - 5, T - 10]) {
      const box = tile(svg, n);
      expect(box.style.getPropertyValue('fill')).toBe(TILE_FILL);
      expect(box).toHaveAttribute('stroke-opacity', '0');
      expect(box).toHaveAttribute('fill-opacity', '1');
      expect(box).not.toHaveAttribute('stroke-dasharray');
    }
  });

  it('fades older blocks on an inner group: 1 at the tip, 0.4 from tip − 7 on', async () => {
    const svg = await renderPanel();
    const cards = [...svg.querySelectorAll('[data-card]')] as SVGGElement[];
    expect(cards.length).toBeGreaterThan(8);
    cards.forEach((c, i) => {
      const age = c.querySelector(':scope > [data-age]') as SVGGElement;
      expect(age).toHaveAttribute('data-age', String(i));
      expect(Number(age.style.opacity)).toBeCloseTo(ageOpacity(i), 6);
      // The outer group's opacity attribute belongs to the motion layer (fade in / retire).
      expect(c.getAttribute('opacity')).toBe('1.000');
    });
    expect(Number((cards[0].querySelector('[data-age]') as SVGGElement).style.opacity)).toBe(1);
    expect(Number((cards[7].querySelector('[data-age]') as SVGGElement).style.opacity)).toBeCloseTo(
      0.4,
      6
    );
  });

  it('draws the pool as two wells: no stroke, the stage name in its colour and its total', async () => {
    const svg = await renderPanel();
    const wells = [
      { id: 'pending', title: 'mempool', tone: 'fill-aqua', total: '5' },
      { id: 'proposed', title: 'proposed', tone: 'fill-gold', total: '9' },
    ];
    for (const w of wells) {
      const well = svg.querySelector(`[data-pool-card="${w.id}"]`) as SVGGElement;
      const tray = well.querySelector('[data-tray]') as SVGPathElement;
      expect(tray).toHaveAttribute('stroke', 'none');
      expect(tray.style.getPropertyValue('fill')).toBe(WELL_FILL);
      expect(within(well as unknown as HTMLElement).getByText(w.title)).toHaveClass(w.tone);
      expect(well.querySelector('[data-count]')?.textContent).toBe(w.total);
    }
    // mempool ⇢ proposed ⇢ NEXT: dashed links through the header row.
    expect(svg.querySelectorAll('[data-flow]')).toHaveLength(2);
  });

  it('splits the pool into pending and proposed, hollow while too early to commit', async () => {
    const svg = await renderPanel();
    const pending = svg.querySelector('[data-pool-grid="pending"]') as SVGGElement;
    const proposed = svg.querySelector('[data-pool-grid="proposed"]') as SVGGElement;
    expect(pending.closest('[data-pool-card="pending"]')).not.toBeNull();
    expect(proposed.closest('[data-pool-card="proposed"]')).not.toBeNull();
    expect(pending.querySelectorAll('[data-sq]')).toHaveLength(5);
    expect(proposed.querySelectorAll('[data-sq]')).toHaveLength(9);
    expect(proposed.querySelectorAll('[data-style="hollow"]')).toHaveLength(3);
  });

  it('shows the pool totals in the wells even when the backend truncated the lists', async () => {
    const s = buildPipelineSnapshot();
    const pool = s.pool as NonNullable<PipelineSnapshot['pool']>;
    const svg = await renderPanel({
      ...s,
      pool: { ...pool, pendingTotal: 1234, proposedTotal: 5678 },
    });
    const total = (id: string) =>
      svg.querySelector(`[data-pool-card="${id}"] [data-count]`)?.textContent;
    expect(total('pending')).toBe('1,234');
    expect(total('proposed')).toBe('5,678');
  });

  it('draws NEXT as the only dashed outline, with dashed squares and hollow dots', async () => {
    const svg = await renderPanel();
    const next = svg.querySelector('[data-next]') as SVGGElement;
    const nextBox = next.querySelector('[data-box]') as SVGRectElement;
    expect(nextBox).toHaveAttribute('stroke-dasharray', '3 2');
    expect(nextBox.style.getPropertyValue('stroke')).toBe(tok('jade'));
    expect(next.querySelectorAll('[data-ghost="tx"]')).toHaveLength(6);
    for (const g of next.querySelectorAll('[data-ghost="tx"]')) {
      expect(g.tagName).toBe('rect');
      expect(g).toHaveAttribute('stroke-dasharray', '2 1.5');
    }
    expect(next.querySelectorAll('[data-ghost="cellbase"]')).toHaveLength(1);
    const dots = [...next.querySelectorAll('[data-ghost-chip]')];
    expect(dots).toHaveLength(5);
    for (const d of dots) {
      expect(d.tagName).toBe('circle');
      expect(d).toHaveAttribute('fill', 'none');
      expect((d as SVGElement).style.getPropertyValue('stroke')).toBe(tok('gold'));
    }
    // Nothing is hidden, so the header shows only the number.
    expect(next.querySelector('[data-count]')).toBeNull();
    expect(next.querySelectorAll('[data-sq]')).toHaveLength(0);
    // Tiles, wells and uncle mini-cards have no dashed outline.
    const outlines = [...svg.querySelectorAll('[data-box], [data-tray], [data-uncle] > rect')];
    expect(outlines.length).toBeGreaterThan(10);
    expect(outlines.filter((el) => el.hasAttribute('stroke-dasharray'))).toEqual([nextBox]);
  });

  it('counts the predicted txs the backend omitted as +N on the NEXT card', async () => {
    const s = buildPipelineSnapshot();
    const next = s.next as NonNullable<PipelineSnapshot['next']>;
    const svg = await renderPanel({ ...s, next: { ...next, omittedTxs: 40 } });
    const card = svg.querySelector('[data-next]') as SVGGElement;
    expect(card.querySelectorAll('[data-ghost="tx"]')).toHaveLength(next.txs.length);
    const count = card.querySelector('[data-count]') as SVGTextElement;
    expect(count.textContent).toBe('+40');
    expect(count).toHaveAttribute('text-anchor', 'end');
    expect(Number(count.getAttribute('y'))).toBe(G.chainY + 3.5);
    expect(screen.getByTestId('pipeline-stats')).not.toHaveTextContent('Next');
  });

  it('draws the window at rest as gold links and marks only: no label, ticks or gold numbers', async () => {
    const s = buildPipelineSnapshot();
    const svg = await renderPanel(s);
    expect(svg.querySelector('[data-window-label]')).toBeNull();
    expect(svg.querySelector('[data-ruler-ticks]')).toBeNull();
    const scene = buildScene(s, VIEW_W, 'settled');
    const ruler = windowRuler(
      rulerSlots(scene),
      T + 1,
      s.consensus,
      VIEW_W,
      windowLabel(T + 1, T, s.consensus)
    );
    expect(ruler.ticks).toHaveLength(2);
    const marks = svg.querySelector('[data-window-marks]') as SVGPathElement;
    expect(marks).toHaveAttribute('d', windowMarksPath(ruler));
    expect(marks.style.getPropertyValue('stroke')).toBe(tok('gold'));
    // The window of NEXT is T−9 … T−1: the eight links between them are gold, 1.5 px.
    const win = path(svg, 'win');
    expect(win.getAttribute('d')?.match(/M/g)).toHaveLength(8);
    expect(win.style.getPropertyValue('stroke')).toBe(tok('gold'));
    expect(win).toHaveAttribute('stroke-width', '1.5');
    for (const el of svg.querySelectorAll('[data-block-number]')) {
      expect(el).not.toHaveClass('fill-gold');
    }
  });

  it('draws the chain as a plain line, no arrowheads: dashed before the tip, solid after', async () => {
    const svg = await renderPanel();
    const chain = svg.querySelector('[data-chain]') as SVGGElement;
    expect([...chain.querySelectorAll('path')].map((p) => p.getAttribute('data-path'))).toEqual([
      'solid',
      'pend',
      'win',
      'tail',
    ]);
    for (const p of [...chain.querySelectorAll('path'), ...svg.querySelectorAll('[data-flow]')]) {
      expect(isPlainLine(p.getAttribute('d'))).toBe(true);
    }
    // mempool ⇢ proposed ⇢ NEXT ⇢ tip is dashed; the mined chain is solid.
    for (const el of [...svg.querySelectorAll('[data-flow]'), path(svg, 'pend')]) {
      expect(el).toHaveAttribute('stroke-dasharray', '3 3');
    }
    expect(path(svg, 'pend').getAttribute('d')).not.toBe('M0 0');
    expect(path(svg, 'solid')).not.toHaveAttribute('stroke-dasharray');
    // The window of NEXT ends on screen, so the dotted tail is quiet jade.
    expect(path(svg, 'tail').style.getPropertyValue('stroke')).toBe(tok('jade'));
    expect(path(svg, 'tail')).toHaveAttribute('stroke-opacity', '0.25');
  });

  it('writes no zone titles, subtitles or window text on the canvas at rest', async () => {
    const svg = (await renderPanel()) as unknown as HTMLElement;
    for (const gone of [
      'Mempool',
      'Next',
      'Chain',
      'proposal window',
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
    expect(svg.querySelector('[data-next-column]')).toBeNull();
  });

  it('keys the canvas in one legend line: the shapes, the stages, then cellbase, cycles, unknown', async () => {
    await renderPanel();
    const legend = screen.getByTestId('pipeline-legend');
    const keys = [...legend.children];
    // The empty entries are the dividers between the shapes, the stage colours and the rest.
    expect(keys.map((el) => el.textContent)).toEqual([
      'tx',
      'proposal',
      '',
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
    // ■ tx, ● proposal; the stage swatches are shape-neutral bars (both shapes use them).
    expect(keys[0].querySelector('rect')).not.toBeNull();
    expect(keys[0].querySelector('circle')).toBeNull();
    expect(keys[1].querySelector('circle')).not.toBeNull();
    for (const k of keys.slice(3, 8)) {
      const bar = k.querySelector('rect') as SVGRectElement;
      expect([bar.getAttribute('width'), bar.getAttribute('height')]).toEqual(['10', '4']);
    }
    // "too early" is the hollow one.
    expect((keys[5].querySelector('rect') as SVGRectElement).style.getPropertyValue('stroke')).toBe(
      tok('gold')
    );
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

  it('keeps only the tip in the header stats: the pool counts are on the canvas', async () => {
    await renderPanel();
    const stats = screen.getByTestId('pipeline-stats');
    expect(stats).toHaveTextContent(`Tip#${fmt(T)}`);
    for (const gone of ['Mempool', 'Proposed', 'Next']) expect(stats).not.toHaveTextContent(gone);
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
    const status = svg.querySelector('[data-pool-status]') as SVGTextElement;
    expect(status).toHaveTextContent(/unavailable$/);
    expect(status.querySelector('title')).toHaveTextContent(
      `The node's tx pool is at #${fmt(T + 1)}`
    );
    expect(status.querySelector('title')).toHaveTextContent(`ckbadger has synced #${fmt(T)}`);
    expect(svg.querySelectorAll('[data-pool-grid]')).toHaveLength(0);
    for (const id of ['pending', 'proposed']) {
      expect(svg.querySelector(`[data-pool-card="${id}"] [data-count]`)?.textContent).toBe('—');
    }
    expect(svg.querySelector('[data-next]')).toBeNull();
    expect(svg.querySelector('[data-next-column]')?.textContent).toBe('unavailable');
    expect(svg.querySelectorAll('[data-card]').length).toBeGreaterThanOrEqual(G.minBlocks);
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
    usePipelineHoverStore.getState().clear();
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
    usePipelineHoverStore.getState().clear();
  });

  const at = (ms: number) =>
    act(() => {
      vi.advanceTimersByTime(ms);
    });
  const landingBox = (svg: Element) =>
    svg.querySelector(`[data-card="${newHash}"] [data-box]`) as SVGRectElement;

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

  it('turns the dashed outline into a solid tile, then lets a fresh outline decay over 2.4 s', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    // Settled tiles, as the motion layer paints them: no outline, a full fill.
    expect(tile(svg, T)).toHaveAttribute('stroke-opacity', '0');
    expect(tile(svg, T)).toHaveAttribute('fill-opacity', '1');
    rerender(<PipelineCanvas snapshot={s2} />);
    const box = landingBox(svg);
    // Not solid yet: stroke 0.6 · (1 − solid), fill 0.35 + 0.65 · solid, dashed.
    expect(box).toHaveAttribute('stroke-opacity', '0.6');
    expect(box).toHaveAttribute('fill-opacity', '0.35');
    expect(box).toHaveAttribute('stroke-dasharray', '3 2');
    // Contact: the fresh outline comes on while the block solidifies; the sum is capped at 1.
    at(t.contact + 20);
    expect(box).toHaveAttribute('stroke-opacity', '1');
    // Solid (contact + 340): the outline is the fresh one alone, fading.
    at(400);
    expect(box).not.toHaveAttribute('stroke-dasharray');
    expect(box).toHaveAttribute('fill-opacity', '1');
    const fading = Number(box.getAttribute('stroke-opacity'));
    expect(fading).toBeGreaterThan(0);
    expect(fading).toBeLessThan(0.6);
    expect(FRESH_MS).toBe(2400);
    at(FRESH_MS);
    expect(box).toHaveAttribute('stroke-opacity', '0');
    expect(box).toHaveAttribute('fill-opacity', '1');
  });

  it('outlines a highlighted tile while the motion layer is painting it', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    at(t.contact + 100);
    // Hovered (here from the Latest Blocks row) while it is still landing and fresh.
    act(() => usePipelineHoverStore.getState().setHover({ block: T + 1, tx: null, window: null }));
    at(40);
    expect(landingBox(svg)).toHaveAttribute('stroke-opacity', '0.7');
    expect(landingBox(svg)).toHaveAttribute('fill-opacity', '1');
    act(() => usePipelineHoverStore.getState().clearHover());
    at(FRESH_MS + 800);
    expect(landingBox(svg)).toHaveAttribute('stroke-opacity', '0');
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

  it('keeps the landing cellbase invisible until it is born at contact', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    const cb = svg.querySelector(`[data-sq="${cellbaseHash(T + 1)}"]`) as SVGPathElement;
    expect(cb).toHaveAttribute('opacity', '0.000');
    at(t.contact - 20);
    expect(cb).toHaveAttribute('opacity', '0.000');
    at(20 + 540);
    expect(cb).toHaveAttribute('opacity', '1.000');
  });

  it('completes the landing beat at the NEXT position before the block joins the chain', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    at(t.settleAt - 10);
    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    expect(landing).toHaveAttribute('data-slot', 'landing');
    expect(landing.getAttribute('transform')).toBe(`translate(${G.nextX.toFixed(2)},0.00)`);
    // Solid, its cellbase at full size, the ripples of the landing gone.
    expect(landingBox(svg)).not.toHaveAttribute('stroke-dasharray');
    expect(landingBox(svg)).toHaveAttribute('fill-opacity', '1');
    const q = buildScene(s2, VIEW_W, 'landing').cards[0].squares[0];
    expect(q.kind).toBe('cellbase');
    expect(landing.querySelector(`[data-sq="${q.id}"]`)).toHaveAttribute(
      'd',
      diamondPath(q.x, q.y, q.s)
    );
    expect(svg.querySelector('[data-layer="fx"] circle')).toBeNull();
    at(20);
    expect(svg.querySelector(`[data-card="${newHash}"]`)).toHaveAttribute('data-slot', 'chain');
  });

  it.each([
    { width: CONTAINER_VIEW_W, colour: 'gold', opacity: '0.7' },
    { width: VIEW_W, colour: 'jade', opacity: '0.25' },
  ] as const)(
    'repaints the dotted tail $colour from the moving cards at a $width px view',
    ({ width, colour, opacity }) => {
      // At 1246 px the window of NEXT runs past the last block on screen; at 1600 it ends on it.
      viewW = width;
      const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
      const svg = getByTestId('pipeline-svg');
      const tail = path(svg, 'tail');
      expect(tail.style.getPropertyValue('stroke')).toBe(tok(colour));
      expect(tail).toHaveAttribute('stroke-opacity', opacity);
      // Spoil what React drew: only the motion layer can repaint it now (React sees no change).
      tail.style.setProperty('stroke', 'red');
      tail.setAttribute('stroke-opacity', '0');
      rerender(<PipelineCanvas snapshot={s2} />);
      at(100);
      expect(tail.style.getPropertyValue('stroke')).toBe(tok(colour));
      expect(tail).toHaveAttribute('stroke-opacity', opacity);
    }
  );

  it('keeps a hovered window label inside the view and off the ticks while the cards move', () => {
    viewW = CONTAINER_VIEW_W;
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    // Hovered from the Latest Blocks row: the window of T−4 starts near the right edge.
    act(() => usePipelineHoverStore.getState().setHover({ block: T - 4, tx: null, window: T - 4 }));
    expectLabelClear(svg, CONTAINER_VIEW_W);
    rerender(<PipelineCanvas snapshot={s2} />);
    // Through the landing and the 700 ms move one slot to the right, frame by frame.
    for (let ms = 0; ms < t.settleAt + 900; ms += 16) {
      at(16);
      expectLabelClear(svg, CONTAINER_VIEW_W);
    }
    expect(svg.querySelector(`[data-card="${newHash}"]`)).toHaveAttribute('data-slot', 'chain');
  });

  /** The landing reward link in the effects layer: its line and its open tail. */
  const rewardFx = (svg: Element) => ({
    line: svg.querySelector('[data-layer="fx"] [data-reward="line"]') as SVGPathElement | null,
    tail: svg.querySelector('[data-layer="fx"] [data-reward="tail"]') as SVGPathElement | null,
  });
  /** Where the chain's dotted tail starts right now, as the motion layer painted it. */
  const chainTailX0 = (svg: Element) =>
    Number(/^M([\d.]+) /.exec(path(svg, 'tail').getAttribute('d') ?? '')?.[1]);

  it('runs the landing reward link off the right edge when the paid block is not on screen', () => {
    // At 1246 px the landing block's cellbase pays tip − 10; the chain on screen ends at tip − 7.
    viewW = CONTAINER_VIEW_W;
    const pays = s2.blocks[0].cellbase.pays;
    expect(pays?.number).toBe(T - 10);
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    at(t.contact + 500 + 16);
    const { line, tail } = rewardFx(svg);
    expect(line).not.toBeNull();
    expect(tail).not.toBeNull();
    let shown = false;
    // Frame by frame through the landing beat, the move into the chain and the fade.
    for (let ms = 0; ms < 2200 - 48; ms += 16) {
      at(16);
      const d = line?.getAttribute('d') ?? '';
      // Up into the lane, then right to where the chain's own dotted tail starts: never down.
      const m = new RegExp(
        `^M[\\d.]+ [\\d.]+V${G.winY + 5}Q[\\d.]+ ${G.winY} [\\d.]+ ${G.winY}H([\\d.]+)$`
      ).exec(d);
      expect(m).not.toBeNull();
      expect(Number(m?.[1])).toBeCloseTo(chainTailX0(svg), 1);
      expect(tail).toHaveAttribute(
        'd',
        `M${Number(m?.[1]).toFixed(1)} ${G.winY}H${(CONTAINER_VIEW_W - 4).toFixed(1)}`
      );
      if (Number(tail?.getAttribute('opacity')) > 0.5) shown = true;
    }
    expect(shown).toBe(true);
    at(100);
    expect(rewardFx(svg).line).toBeNull();
    expect(rewardFx(svg).tail).toBeNull();
  });

  it('runs the landing reward link off the edge too when the paid block is not in the snapshot', () => {
    const [newest, ...rest] = s2.blocks;
    const pays = { number: T - 40, hash: blockHash(T - 40) };
    const far: PipelineSnapshot = {
      ...s2,
      blocks: [{ ...newest, cellbase: { ...newest.cellbase, pays } }, ...rest],
    };
    const mined = diffSnapshots(s1, far).events.find((e) => e.kind === 'mined');
    expect(mined).toMatchObject({ kind: 'mined', payee: null });
    viewW = CONTAINER_VIEW_W;
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={far} />);
    at(t.contact + 500 + 0.5 * 2200);
    const { line, tail } = rewardFx(svg);
    expect(line?.getAttribute('d')).toMatch(new RegExp(`${G.winY}H[\\d.]+$`));
    expect(tail?.getAttribute('d')).toMatch(new RegExp(`H${(CONTAINER_VIEW_W - 4).toFixed(1)}$`));
    expect(Number(tail?.getAttribute('opacity'))).toBeGreaterThan(0.5);
  });

  it('drops the landing reward link into the paid cellbase when it is on screen', () => {
    // At 2000 px tip − 10 stays on screen through the landing and the move into the chain.
    viewW = 2000;
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    const payee = card(svg, T - 10);
    const q = buildScene(s2, 2000, 'landing').cards.find((c) => c.block.number === T - 10)
      ?.squares[0];
    if (!q || q.kind !== 'cellbase') throw new Error('no cellbase for tip − 10');
    const centreX = () =>
      Number(/translate\(([\d.]+),/.exec(payee.getAttribute('transform') ?? '')?.[1]) +
      q.x +
      q.s / 2;
    at(t.contact + 500 + 16);
    for (let ms = 0; ms < 1200; ms += 16) {
      at(16);
      const d = rewardFx(svg).line?.getAttribute('d') ?? '';
      // Down into the paid cellbase, wherever its card is drawn in this frame.
      const m = new RegExp(
        `Q([\\d.]+) ${G.winY} ([\\d.]+) ${G.winY + 5}V${(q.y + q.s / 2).toFixed(1)}$`
      ).exec(d);
      expect(m).not.toBeNull();
      expect(Number(m?.[1])).toBeCloseTo(centreX(), 0);
      expect(Number(m?.[2])).toBe(Number(m?.[1]));
      expect(rewardFx(svg).tail).toHaveAttribute('opacity', '0.000');
    }
    // It landed: a ripple opened on the paid cellbase.
    const rings = [...svg.querySelectorAll('[data-layer="fx"] circle')];
    expect(rings.some((r) => Math.abs(Number(r.getAttribute('cx')) - centreX()) < 0.5)).toBe(true);
  });

  it('resolves a proposal dot when its commit lands, not before', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    const chip = svg.querySelector(
      `[data-card="${blockHash(T - 1)}"] [data-chip="${shortIdOf(w0)}|"]`
    ) as SVGCircleElement;
    expect(chip.tagName).toBe('circle');
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
    card(svg, n).querySelector(':scope > [data-age] > [data-dim]')?.getAttribute('data-dim');
  /** Hover NEXT's header row (no tx or proposal under the pointer). */
  const hoverNext = (svg: Element) =>
    fireEvent.pointerMove(svg.querySelector('[data-next] [data-box]') as Element, {
      clientX: G.nextX + 60,
      clientY: G.cardTop + 4,
    });

  it('rings the proposal of a hovered committed tx, links them in gold and steps the rest back', async () => {
    const svg = await renderPanel(snapshot);
    const id = committedTxHash(T, 2);
    fireEvent.pointerMove(svg, squareCentre(T, id));

    expect(tooltip()).toHaveTextContent('committed in');
    expect(tooltip()).toHaveTextContent(`#${fmt(T)}`);
    expect(tooltip()).toHaveTextContent(`#${fmt(T - 2)} (2 blocks earlier)`);
    expect(svg).toHaveClass('is-focus');
    expect(svg.querySelector(`[data-sq="${id}"]`)).toHaveClass('is-on');
    // The proposal is a dot, so its ring is a circle around it.
    const ring = svg.querySelector('[data-hover="chip-ring"]') as SVGCircleElement;
    expect(ring.tagName).toBe('circle');
    const chip = chipCentre(T - 2, `${shortIdOf(id)}|`);
    expect(Number(ring.getAttribute('cx'))).toBeCloseTo(chip.clientX, 1);
    expect(Number(ring.getAttribute('cy'))).toBeCloseTo(chip.clientY, 1);
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

  it('names the uncle that carried a hovered proposal dot', async () => {
    const svg = await renderPanel(snapshot);
    fireEvent.pointerMove(svg, chipCentre(T - 1, `${shortIdOf(poolTxHash('u0'))}|0`));
    expect(tooltip()).toHaveTextContent('Proposal');
    expect(tooltip()).toHaveTextContent(new RegExp(`carried by\\s*uncle #${fmt(T - 2)}`));
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
    expect(tooltip()).toHaveTextContent(`height#${fmt(T - 2)}`);
    expect(tooltip()).toHaveTextContent(`included by#${fmt(T - 1)}`);
    expect(usePipelineHoverStore.getState()).toMatchObject({
      hoverBlock: T - 1,
      windowBlock: null,
    });
    expect(dimOfCard(svg, T - 3)).toBe('false');
    expect(svg.querySelector('[data-window-label]')).toBeNull();
  });

  it('explains the window lane with a help cursor and steps back pool txs outside it', async () => {
    const svg = await renderPanel(snapshot);
    const ruler = windowRuler(
      rulerSlots(scene),
      T + 1,
      snapshot.consensus,
      VIEW_W,
      windowLabel(T + 1, T, snapshot.consensus)
    );
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
    // The hovered lane draws the window in full and thickens its gold links.
    expect(svg.querySelector('[data-ruler-ticks]')).not.toBeNull();
    expect(path(svg, 'win')).toHaveAttribute('stroke-width', '2');
  });

  it('draws the window of NEXT in full while NEXT is hovered: ticks, line, label, gold numbers', async () => {
    const svg = await renderPanel(snapshot);
    hoverNext(svg);
    expect(tooltip()).toHaveTextContent(`Next #${fmt(T + 1)}`);
    const text = windowLabel(T + 1, T, snapshot.consensus);
    expect(text).toBe('next block can commit txs proposed in these 9 blocks');
    expect(svg.querySelector('[data-window-label]')?.textContent).toBe(text);
    const ruler = windowRuler(
      rulerSlots(scene),
      T + 1,
      snapshot.consensus,
      VIEW_W,
      windowLabel(T + 1, T, snapshot.consensus)
    );
    const ticks = svg.querySelector('[data-ruler-ticks]') as SVGPathElement;
    // One path for the static render and the motion layer.
    expect(ticks).toHaveAttribute('d', rulerPath(ruler));
    // Two ticks and the dimension line between them, broken around the label.
    expect(ticks.getAttribute('d')?.match(/V/g)).toHaveLength(2);
    expect(ticks.getAttribute('d')?.match(/H/g)).toHaveLength(2);
    for (const c of scene.cards) {
      const n = c.block.number;
      const number = card(svg, n).querySelector('[data-block-number]');
      const inside = inWindow(n, T + 1, snapshot.consensus);
      if (inside) expect(number).toHaveClass('fill-gold');
      else expect(number).not.toHaveClass('fill-gold');
      expect(dimOfCard(svg, n)).toBe(inside ? 'false' : 'true');
    }
    expect(svg.querySelector('[data-next] > [data-dim]')).toHaveAttribute('data-dim', 'false');
  });

  it("draws NEXT's window while the proposed well is hovered: it holds exactly those proposals", async () => {
    const svg = await renderPanel(snapshot);
    const tray = svg.querySelector('[data-pool-card="proposed"] [data-tray]') as Element;
    fireEvent.pointerMove(tray, { clientX: G.proposedX + 45, clientY: G.cardTop + CARD_H - 8 });
    expect(tooltip()).toHaveTextContent('Proposal window');
    expect(tooltip()).toHaveTextContent(`next block#${fmt(T + 1)}`);
    expect(svg.querySelector('[data-window-label]')?.textContent).toBe(
      windowLabel(T + 1, T, snapshot.consensus)
    );
    expect(card(svg, T - 1).querySelector('[data-block-number]')).toHaveClass('fill-gold');
    expect(card(svg, T).querySelector('[data-block-number]')).not.toHaveClass('fill-gold');
  });

  it('anchors the window on a hovered block and steps back the blocks outside it', async () => {
    const svg = await renderPanel(snapshot);
    const c = cardOf(T - 3);
    const box = card(svg, T - 3).querySelector('[data-box]') as SVGRectElement;
    fireEvent.pointerMove(box, { clientX: c.x + c.w / 2, clientY: G.cardTop + 5 });
    expect(tooltip()).toHaveTextContent(`Block #${fmt(T - 3)}`);
    // The hovered tile gets its outline.
    expect(box).toHaveAttribute('stroke-opacity', '0.7');
    expect(dimOfCard(svg, T - 3)).toBe('false');
    expect(dimOfCard(svg, T - 5)).toBe('false');
    expect(dimOfCard(svg, T)).toBe('true');
    expect(dimOfCard(svg, T - 2)).toBe('true');
    expect(svg.querySelector('[data-window-label]')?.textContent).toBe(
      `${tail4(T - 3)} could commit txs proposed in these 9 blocks`
    );
    expect(svg.querySelector(`[data-sq="${poolTxHash('q0')}"]`)?.parentElement).toHaveAttribute(
      'data-dim',
      'soft'
    );
  });

  it.each([T - 3, T - 4, T - 5])(
    'keeps the window label of #%i inside the 1280 px container and off the ticks',
    async (n) => {
      viewW = CONTAINER_VIEW_W;
      vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
        x: 0,
        y: 0,
        left: 0,
        top: 0,
        width: CONTAINER_VIEW_W,
        height: G.H,
        right: CONTAINER_VIEW_W,
        bottom: G.H,
        toJSON: () => ({}),
      } as DOMRect);
      const svg = await renderPanel(snapshot);
      const c = buildScene(snapshot, CONTAINER_VIEW_W, 'settled').cards.find(
        (k) => k.block.number === n
      );
      if (!c) throw new Error(`#${n} is not on screen`);
      fireEvent.pointerMove(card(svg, n).querySelector('[data-box]') as Element, {
        clientX: c.x + c.w / 2,
        clientY: G.cardTop + 5,
      });
      expect(svg.querySelector('[data-window-label]')?.textContent).toBe(
        `${tail4(n)} could commit txs proposed in these 9 blocks`
      );
      expectLabelClear(svg, CONTAINER_VIEW_W);
    }
  );

  it('turns the dotted tail gold while the drawn window runs off the screen', async () => {
    const svg = await renderPanel(snapshot);
    expect(path(svg, 'tail').style.getPropertyValue('stroke')).toBe(tok('jade'));
    // The window of T−3 is T−13 … T−5; the screen ends at T−10, inside it.
    const c = cardOf(T - 3);
    fireEvent.pointerMove(card(svg, T - 3).querySelector('[data-box]') as Element, {
      clientX: c.x + c.w / 2,
      clientY: G.cardTop + 5,
    });
    expect(path(svg, 'tail').style.getPropertyValue('stroke')).toBe(tok('gold'));
    expect(path(svg, 'tail')).toHaveAttribute('stroke-opacity', '0.7');
    fireEvent.pointerLeave(svg);
    expect(path(svg, 'tail').style.getPropertyValue('stroke')).toBe(tok('jade'));
    expect(path(svg, 'tail')).toHaveAttribute('stroke-opacity', '0.25');
  });

  it('draws the reward link of a hovered cellbase and says what it pays in the tooltip', async () => {
    // A cellbase pays the block 11 behind it: both ends are on screen only in a wide view.
    const wide = 2000;
    viewW = wide;
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      width: wide,
      height: G.H,
      right: wide,
      bottom: G.H,
      toJSON: () => ({}),
    } as DOMRect);
    const svg = await renderPanel(snapshot);
    const wideScene = buildScene(snapshot, wide, 'settled');
    const c = wideScene.cards.find((k) => k.block.number === T - 2);
    const payee = wideScene.cards.find((k) => k.block.number === T - 13);
    const cb = c?.squares[0];
    const pb = payee?.squares[0];
    if (!c || !cb || cb.kind !== 'cellbase') throw new Error('no cellbase for T−2');
    if (!payee || !pb || pb.kind !== 'cellbase') throw new Error('no cellbase for T−13');
    fireEvent.pointerMove(svg, { clientX: c.x + cb.x + cb.s / 2, clientY: cb.y + cb.s / 2 });
    const reward = svg.querySelector('[data-hover="reward"]') as SVGPathElement;
    expect(reward).not.toBeNull();
    // It runs in the lane above the cards, where the hovered window would be drawn, and drops
    // into the cellbase it pays: unchanged by the off-screen rule.
    const a = { X: c.x + cb.x + cb.s / 2, Y: cb.y + cb.s / 2 };
    const b = { X: payee.x + pb.x + pb.s / 2, Y: pb.y + pb.s / 2 };
    const f = (v: number) => v.toFixed(1);
    expect(reward).toHaveAttribute(
      'd',
      `M${f(a.X)} ${f(a.Y)}V${G.winY + 5}Q${f(a.X)} ${G.winY} ${f(a.X + 5)} ${G.winY}H${f(b.X - 5)}Q${f(b.X)} ${G.winY} ${f(b.X)} ${G.winY + 5}V${f(b.Y)}`
    );
    expect(svg.querySelector('[data-hover="reward-tail"]')).toBeNull();
    // No caption on the canvas, and no window in the lane meanwhile.
    expect(svg.querySelector('[data-chain-sub]')).toBeNull();
    expect(svg.querySelector('[data-ruler-ticks]')).toBeNull();
    expect(svg.querySelector('[data-window-label]')).toBeNull();
    expect(tooltip()).toHaveTextContent('Cellbase');
    expect(tooltip()).toHaveTextContent(`pays reward of#${fmt(T - 13)}`);
  });

  it('runs the reward link off the right edge in an open dotted tail when the paid block is not on screen', async () => {
    // The 1280 px container shows tip … tip − 7: the block the tip's cellbase pays (tip − 11) is
    // past the right edge.
    viewW = CONTAINER_VIEW_W;
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      width: CONTAINER_VIEW_W,
      height: G.H,
      right: CONTAINER_VIEW_W,
      bottom: G.H,
      toJSON: () => ({}),
    } as DOMRect);
    const svg = await renderPanel(snapshot);
    const fit = buildScene(snapshot, CONTAINER_VIEW_W, 'settled');
    expect(fit.cards).toHaveLength(8);
    const c = fit.cards[0];
    const last = fit.cards[fit.cards.length - 1];
    const cb = c.squares[0];
    expect(cb.kind).toBe('cellbase');
    fireEvent.pointerMove(svg, { clientX: c.x + cb.x + cb.s / 2, clientY: cb.y + cb.s / 2 });
    expect(tooltip()).toHaveTextContent(`pays reward of#${fmt(T - 11)}`);
    const reward = svg.querySelector('[data-hover="reward"]') as SVGPathElement;
    expect(reward).not.toBeNull();
    // Up from the cellbase into the lane, then right to where the chain's own tail starts; it
    // never drops anywhere.
    const tailX0 = last.x + last.w + 2;
    const f = (v: number) => v.toFixed(1);
    const ax = c.x + cb.x + cb.s / 2;
    expect(reward).toHaveAttribute(
      'd',
      `M${f(ax)} ${f(cb.y + cb.s / 2)}V${G.winY + 5}Q${f(ax)} ${G.winY} ${f(ax + 5)} ${G.winY}H${f(tailX0)}`
    );
    // The open end: dotted like the chain's tail and over the same span, one lane up.
    const tail = svg.querySelector('[data-hover="reward-tail"]') as SVGPathElement;
    expect(tail).toHaveAttribute('d', `M${f(tailX0)} ${G.winY}H${f(CONTAINER_VIEW_W - 4)}`);
    expect(tail).toHaveAttribute(
      'stroke-dasharray',
      path(svg, 'tail').getAttribute('stroke-dasharray')
    );
    expect(path(svg, 'tail').getAttribute('d')).toBe(
      `M${f(tailX0)} ${G.chainY + 0.5}L${CONTAINER_VIEW_W - 4} ${G.chainY + 0.5}`
    );
    // Both leave with the hover.
    fireEvent.pointerLeave(svg);
    expect(svg.querySelector('[data-hover="reward"]')).toBeNull();
    expect(svg.querySelector('[data-hover="reward-tail"]')).toBeNull();
  });

  it('steps the hovered window back while a reward link holds its lane', async () => {
    // Hovered from the Latest Blocks row while the pointer is on a cellbase: both want the lane.
    viewW = CONTAINER_VIEW_W;
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      width: CONTAINER_VIEW_W,
      height: G.H,
      right: CONTAINER_VIEW_W,
      bottom: G.H,
      toJSON: () => ({}),
    } as DOMRect);
    const svg = await renderPanel(snapshot);
    const c = buildScene(snapshot, CONTAINER_VIEW_W, 'settled').cards[0];
    const cb = c.squares[0];
    fireEvent.pointerMove(svg, { clientX: c.x + cb.x + cb.s / 2, clientY: cb.y + cb.s / 2 });
    act(() => usePipelineHoverStore.getState().setHover({ block: T, tx: null, window: T }));
    expect(svg.querySelector('[data-hover="reward"]')).not.toBeNull();
    expect(svg.querySelector('[data-ruler-ticks]')).toHaveAttribute('opacity', '0.15');
    expect(svg.querySelector('[data-window-label]')).toHaveAttribute('opacity', '0.15');
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

  it('keeps the footer one fixed-height row whether it shows the legend or the selection', async () => {
    // jsdom does not lay out, so the contract is the shared row class: one fixed height, no wrap,
    // no extra margin — pinning a tx must not change the panel height.
    const row = (el: HTMLElement) => el.className.split(/\s+/);
    const svg = await renderPanel(snapshot);
    const legend = row(screen.getByTestId('pipeline-legend'));
    const q = scene.pool?.pending.squares.find((k) => k.id === poolTxHash('q1'));
    fireEvent.click(svg, { clientX: (q?.x as number) + 3, clientY: (q?.y as number) + 3 });
    const selection = row(screen.getByTestId('pipeline-selection'));
    for (const classes of [legend, selection]) {
      expect(classes).toContain('h-6');
      expect(classes).not.toContain('flex-wrap');
      expect(classes.some((c) => /^(m[by]?|min-h)-/.test(c))).toBe(false);
    }
  });

  it('follows hovers coming from the Latest Blocks and Latest Transactions rows', async () => {
    const svg = await renderPanel(snapshot);
    act(() => usePipelineHoverStore.getState().setHover({ block: T - 3, tx: null, window: T - 3 }));
    expect(card(svg, T - 3).querySelector('[data-box]')).toHaveAttribute('stroke-opacity', '0.7');
    expect(dimOfCard(svg, T)).toBe('true');
    const id = committedTxHash(T, 1);
    act(() => usePipelineHoverStore.getState().setHover({ block: T, tx: id, window: T }));
    expect(svg.querySelector(`[data-sq="${id}"]`)).toHaveClass('is-on');
    expect(svg.querySelector('[data-hover="ring"]')).not.toBeNull();
    act(() => usePipelineHoverStore.getState().clearHover());
    expect(svg).not.toHaveClass('is-focus');
    expect(card(svg, T - 3).querySelector('[data-box]')).toHaveAttribute('stroke-opacity', '0');
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
    expect(tooltip()).toHaveTextContent(`#${fmt(T + 1)}`);
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
