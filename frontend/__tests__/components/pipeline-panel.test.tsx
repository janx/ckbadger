import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { act, cleanup, fireEvent, render as renderPlain } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { QueryClientProvider } from '@tanstack/react-query';
import {
  createTestQueryClient,
  render,
  screen,
  waitFor,
  within,
} from '@/__tests__/utils/test-utils';
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
  uncleHash,
  withSmallTxs,
} from '@/__tests__/msw/fixtures/pipeline-snapshot';
import type { PipelineSnapshot } from '@/lib/api';
import { PipelineCanvas, PipelinePanel } from '@/components/pipeline/pipeline-panel';
import Link from '@/components/ui/link';
import {
  blockTip,
  pct,
  predictedFacts,
  predictedTip,
} from '@/components/pipeline/pipeline-tooltip';
import { shortenHash } from '@/lib/shorten';
import { formatCount } from '@/lib/format-count';
import {
  CUT_FILL,
  GILT_FILL,
  PLATE_FILL,
  STAGE_BASE,
  WELL_FILL,
  ageOpacity,
  agedChip,
  agedPaint,
  chipPaint,
  diamondPath,
  mixColor,
  paintSquare,
  tok,
} from '@/lib/pipeline/encoding';
import {
  BLOCK_FIELD_CAPACITY,
  CARD_H,
  COL,
  G,
  GUT,
  UNC,
  buildScene,
  cardBox,
  chainPaths,
  chainTail,
  columnChainPaths,
  columnChainTail,
  columnFlowPaths,
  columnHeadAt,
  columnRewardLink,
  columnWellHeadAt,
  columnWellRect,
  columnWellShade,
  flowPaths,
  inWindow,
  labelWidth,
  pick,
  pickTargets,
  side,
  strapBox,
  tail4,
  wellRect,
  wellShade,
  windowLabel,
  windowLane,
  type BlockSpan,
  type ChainSlotState,
  type Scene,
} from '@/lib/pipeline/layout';
import { FRESH_MS } from '@/lib/pipeline/motion';
import { drawSeal } from '@/lib/seal/stones';
import { usePipelineHoverStore, type PipelinePin } from '@/hooks/usePipelineHoverStore';
import { diffSnapshots } from '@/lib/pipeline/transitions';
import {
  color,
  contrastRatio,
  hexToRgb,
  PALETTE,
  THEMES,
  type PaletteKey,
  type ThemeName,
} from '@/lib/palette';
import { rects, roundedRects } from '@/__tests__/utils/square-corners';

const VIEW_W = 1600;
/** The page container is 1280 px wide; the canvas inside its padding is 1246 px. */
const CONTAINER_VIEW_W = 1246;
const T = FIXTURE_TIP;
const fmt = (n: number) => n.toLocaleString('en-US');
/** The width the stubbed ResizeObserver reports; a test may change it before rendering. */
let viewW = VIEW_W;

/** The observers the canvas is using, so a test can resize the view. */
const observers = new Set<ResizeObserverStub>();

class ResizeObserverStub {
  constructor(private readonly cb: ResizeObserverCallback) {}
  observe() {
    observers.add(this);
    this.report();
  }
  report() {
    this.cb(
      [{ contentRect: { width: viewW } } as ResizeObserverEntry],
      this as unknown as ResizeObserver
    );
  }
  unobserve() {}
  disconnect() {
    observers.delete(this);
  }
}

/** The page is resized: every live observer reports the new width. */
function resizeTo(width: number) {
  viewW = width;
  act(() => {
    for (const o of observers) o.report();
  });
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
  observers.clear();
  vi.stubGlobal('ResizeObserver', ResizeObserverStub);
  stubMatchMedia(false);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/** A hover arriving from outside the canvas (the Latest Blocks / Latest Transactions rows). */
const hoverStoreSet = (h: { block: number | null; tx: string | null; window: number | null }) =>
  act(() => usePipelineHoverStore.getState().setHover(h));

async function renderPanel(snapshot: PipelineSnapshot = buildPipelineSnapshot()) {
  server.use(http.get('/api/:network/v1/pipeline', () => HttpResponse.json(snapshot)));
  render(<PipelinePanel />);
  return screen.findByTestId('pipeline-svg');
}

/** The router's location, where a test can read where a click went. */
function RouterLocation() {
  return <output data-testid="router-location">{useLocation().pathname}</output>;
}
const routerLocation = () => screen.getByTestId('router-location').textContent;

/**
 * The panel on a network's home page, under a router (lane K): a click on the canvas navigates
 * through it, so where the click went is the router's location.
 */
async function renderRoutedPanel(
  snapshot: PipelineSnapshot = buildPipelineSnapshot(),
  at = '/mainnet'
) {
  server.use(http.get('/api/:network/v1/pipeline', () => HttpResponse.json(snapshot)));
  render(
    <MemoryRouter initialEntries={[at]}>
      <RouterLocation />
      <PipelinePanel />
    </MemoryRouter>
  );
  return screen.findByTestId('pipeline-svg');
}

type ClientPoint = { clientX: number; clientY: number };

/** `(x, y)` as a pointer position, after checking that it picks no tx and no proposal dot. */
function unpicked(sc: Scene, x: number, y: number): ClientPoint {
  const hit = pick(pickTargets(sc), x, y);
  if (hit !== null) throw new Error(`(${x}, ${y}) picks ${JSON.stringify(hit)}`);
  return { clientX: x, clientY: y };
}

/** A point on block `n`'s plate that picks no tx and no proposal dot: the bare plate. */
function bareSpot(sc: Scene, n: number): ClientPoint {
  const c = sc.cards.find((k) => k.block.number === n);
  if (!c) throw new Error(`#${n} is not on screen`);
  const targets = pickTargets(sc);
  const top = c.y + (sc.orientation === 'row' ? G.cardTop : 0);
  for (let y = top + c.h - 3; y > top + 2; y--) {
    for (let x = c.x + c.w - 3; x > c.x + 2; x--) {
      if (pick(targets, x, y) === null) return { clientX: x, clientY: y };
    }
  }
  throw new Error(`#${n} has no bare spot on its plate`);
}

/** A point in a pool well (its tray, as drawn) that picks no tx, and the tray it is on. */
function bareWellSpot(
  svg: Element,
  sc: Scene,
  which: 'pending' | 'proposed'
): [Element, ClientPoint] {
  const tray = svg.querySelector(`[data-pool-card="${which}"] [data-tray]`) as SVGRectElement;
  const [x0, y0, w, h] = ['x', 'y', 'width', 'height'].map((a) => Number(tray.getAttribute(a)));
  const targets = pickTargets(sc);
  for (let y = y0 + h - 3; y > y0 + 2; y--) {
    for (let x = x0 + w - 3; x > x0 + 2; x--) {
      if (pick(targets, x, y) === null) return [tray, { clientX: x, clientY: y }];
    }
  }
  throw new Error(`the ${which} well has no spot without a tx`);
}

/** jsdom has no PointerEvent: a MouseEvent that carries its pointer's type, as a browser's does. */
class PointerEventStub extends MouseEvent {
  readonly pointerType: string;
  constructor(type: string, init: PointerEventInit = {}) {
    super(type, init);
    this.pointerType = init.pointerType ?? '';
  }
}

/** A gesture that ends in a click, begun by a pointer of `type` (`touch`: a tap). */
function press(el: Element, at: ClientPoint, type: 'touch' | 'mouse' | 'pen') {
  fireEvent.pointerDown(el, { ...at, pointerType: type });
  fireEvent.click(el, at);
}
/** A touch tap (lane K): the first on a mark pins what it opens, the second opens it. */
const tap = (el: Element, at: ClientPoint) => press(el, at, 'touch');

describe('the tip flares (material pass v2, move 4)', () => {
  it('lights the tip for a new block, not on mount', async () => {
    const s1 = buildPipelineSnapshot();
    server.use(http.get('/api/:network/v1/pipeline', () => HttpResponse.json(s1)));
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <PipelinePanel />
      </QueryClientProvider>
    );
    await screen.findByTestId('pipeline-svg');
    const tip = screen.getByTestId('pipeline-tip');
    expect(tip).toHaveTextContent(`--tip #${fmt(FIXTURE_TIP)}`);
    expect(tip.querySelector('.phosphor-flare')).toBeNull();
    const s2 = mineNext(s1, { commit: [poolTxHash('w0')], propose: [] });
    act(() => {
      client.setQueryData(['pipeline'], s2);
    });
    // react-query notifies its observers on the next tick
    await waitFor(() => expect(tip).toHaveTextContent(`--tip #${fmt(FIXTURE_TIP + 1)}`));
    // the flare is on the tip's link itself (rubbing × Song print, phase 5, H1)
    const flared = tip.querySelector('.phosphor-flare');
    expect(flared).toHaveTextContent(`#${fmt(FIXTURE_TIP + 1)}`);
    expect(flared?.tagName).toBe('A');
  });
});

const card = (svg: Element, n: number) =>
  svg.querySelector(`[data-card="${blockHash(n)}"]`) as SVGGElement;
const tile = (svg: Element, n: number) => card(svg, n).querySelector('[data-box]') as SVGElement;
/**
 * How strongly `el` is drawn inside its card: the product of the opacities (style and attribute)
 * of `el` and its ancestors below the card, whose own `opacity` the motion layer owns.
 */
const strengthInCard = (el: Element, cardEl: Element) => {
  let strength = 1;
  for (let a: Element | null = el; a && a !== cardEl; a = a.parentElement) {
    const style = (a as SVGElement).style.opacity;
    if (style !== '') strength *= Number(style);
    const attr = a.getAttribute('opacity');
    if (attr !== null) strength *= Number(attr);
  }
  return strength;
};
/**
 * Direction F (lanes R and P): a plate is never faded, and nothing on it is set back by an
 * opacity: not the plate, its stone, its cuts, its marks or its words (R4), in a row and in a
 * column. An older block's marks step back by their fill alone (`agedPaint` / `agedChip` toward the
 * cut or the plate), its words never. The words let the pointer through to the plate and the marks
 * beneath them.
 */
const expectPlatesAtFullStrength = (svg: Element) => {
  const cards = [...svg.querySelectorAll('[data-card]')];
  // the tip and at least three blocks behind it, each a step further back
  expect(cards.length).toBeGreaterThan(3);
  const words = cards.flatMap((c, i) => [...c.querySelectorAll('text')].map((t) => ({ c, i, t })));
  expect(words.length).toBeGreaterThanOrEqual(cards.length);
  const faded = cards.flatMap((c, i) =>
    [...c.querySelectorAll('*')]
      .filter((el) => strengthInCard(el, c) !== 1)
      .map((el) => `card ${i} <${el.tagName}> "${el.textContent}" at ${strengthInCard(el, c)}`)
  );
  expect(faded).toEqual([]);
  for (const c of cards) {
    expect(c.querySelector('[data-words]')).toHaveAttribute('pointer-events', 'none');
  }
};

// ---- Lane E2 follow-up, ruling 2: every word the pipeline writes reads at 4.5:1 on its ground ----
const GLOBALS_CSS = readFileSync(join(__dirname, '..', '..', 'app', 'globals.css'), 'utf8');
/** The palette key a CSS variable resolves to in app/globals.css (`pipeline-elevated` → `base-elevated`). */
function paletteKeyOfVar(name: string): PaletteKey {
  let current = name;
  for (let hop = 0; hop < 6; hop++) {
    if (current.startsWith('c-')) return current.slice(2) as PaletteKey;
    const m = new RegExp(`--${current}:\\s*var\\(--([a-z0-9-]+)\\)\\s*;`).exec(GLOBALS_CSS);
    if (!m) throw new Error(`globals.css: --${current} is not a variable reference`);
    current = m[1];
  }
  throw new Error(`globals.css: --${name} does not reach a palette key`);
}
/** `over` at `a` laid over `under` (two hexes), as overGround lays a key over a ground. */
function mixHex(under: string, over: string, a: number): string {
  const [u, o] = [hexToRgb(under), hexToRgb(over)];
  const ch = (x: number, y: number) =>
    Math.round(a * y + (1 - a) * x)
      .toString(16)
      .padStart(2, '0');
  return `#${ch(u.r, o.r)}${ch(u.g, o.g)}${ch(u.b, o.b)}`;
}
/** A fill as the canvas writes it, a `var(--…)` or an sRGB `color-mix` of two, as a hex. */
function fillHex(theme: ThemeName, fill: string): string {
  const v = /^var\(--([a-z0-9-]+)\)$/.exec(fill);
  if (v) return PALETTE[theme][paletteKeyOfVar(v[1])];
  const m = /^color-mix\(in srgb, (var\(--[a-z0-9-]+\)) ([\d.]+)%, (var\(--[a-z0-9-]+\))\)$/.exec(
    fill
  );
  if (m) return mixHex(fillHex(theme, m[3]), fillHex(theme, m[1]), Number(m[2]) / 100);
  throw new Error(`pipeline fill "${fill}" is not a palette reference`);
}
/** The word inks of the canvas, by their fill class. */
const WORD_INK: Readonly<Record<string, PaletteKey>> = {
  'fill-text-dim': 'text-dim',
  'fill-text': 'text',
  'fill-text-bright': 'text-bright',
  'fill-gold': 'gold',
  'fill-aqua': 'aqua',
};
const PAGE_GROUNDS: PaletteKey[] = ['base-void', 'base-bg', 'base-surface', 'base-elevated'];
/** A word's ink: its fill class, a named ink or a pipeline token (`fill-[var(--pipeline-…)]`). */
function wordInk(word: Element, svg: Element): PaletteKey {
  for (let a: Element | null = word; a && a !== svg; a = a.parentElement) {
    const cls = [...a.classList].find((c) => c.startsWith('fill-'));
    if (!cls) continue;
    const token = /^fill-\[var\(--([a-z0-9-]+)\)\]$/.exec(cls);
    if (token) return paletteKeyOfVar(token[1]);
    const key = WORD_INK[cls];
    if (!key) throw new Error(`pipeline word "${word.textContent}" in unknown ink ${cls}`);
    return key;
  }
  throw new Error(`pipeline word "${word.textContent}" has no fill class`);
}

/**
 * Every word the canvas writes, its ink (its fill class), and the composite it stands on, over
 * each page ground the canvas can show: a block's plate (opaque, never faded: the words on it, an
 * uncle's too, stand on the plate alone, and on its gilt strap inside the window), NEXT's outline
 * (no fill: the page), a pool's well (a row's "unavailable" in it, every word of a column's well),
 * or the page itself (a row's pool name and total, above its well). The plate and NEXT's outline are one rule in both orientations
 * (lane P). One line per ground, ink and theme under 4.5:1, with its worst ratio.
 */
function wordInkFailures(svg: Element, form: 'row' | 'column'): string[] {
  const styleFill = (el: Element) => (el as SVGElement).style.getPropertyValue('fill');
  const alphaOf = (el: Element) => Number(el.getAttribute('fill-opacity') ?? '1');
  const groundOf = (
    word: Element
  ): { kind: string; hexes: (theme: ThemeName, page: PaletteKey) => string[] } => {
    const card = word.closest('[data-card]');
    if (card) {
      const plate = card.querySelector('[data-box]') as Element;
      expect(alphaOf(plate)).toBe(1);
      expect(strengthInCard(word, card)).toBe(1);
      // Direction G: a window plate's head line is its gilt strap; its words must read on both
      const strap = card.querySelector('[data-strap]');
      if (strap) {
        expect(alphaOf(strap)).toBe(1);
        expect(strengthInCard(strap, card)).toBe(1);
        return {
          kind: 'a plate and its gilt',
          hexes: (t) => [fillHex(t, styleFill(plate)), fillHex(t, styleFill(strap))],
        };
      }
      return { kind: 'a plate', hexes: (t) => [fillHex(t, styleFill(plate))] };
    }
    const next = word.closest('[data-next]');
    if (next) {
      const box = next.querySelector('[data-box]') as Element;
      expect(styleFill(box)).toBe('none');
      return { kind: 'the page (NEXT is an outline)', hexes: (t, p) => [PALETTE[t][p]] };
    }
    const pool = word.closest('[data-pool-card]');
    if (pool && (form === 'column' || word.closest('[data-pool-status]'))) {
      const tray = pool.querySelector('[data-tray]') as Element;
      return { kind: 'a well', hexes: (t) => [fillHex(t, styleFill(tray))] };
    }
    return { kind: 'the page', hexes: (t, p) => [PALETTE[t][p]] };
  };
  const words = [...svg.querySelectorAll('text, tspan')].filter((el) =>
    [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent!.trim() !== '')
  );
  expect(words.length).toBeGreaterThan(4);
  const worst = new Map<string, { ratio: number; example: string }>();
  for (const word of words) {
    const ink = wordInk(word, svg);
    const ground = groundOf(word);
    for (const theme of THEMES)
      for (const page of PAGE_GROUNDS) {
        const ratio = Math.min(
          ...ground.hexes(theme, page).map((hex) => contrastRatio(PALETTE[theme][ink], hex))
        );
        if (ratio >= 4.5) continue;
        const key = `${form}: ${ink} on ${ground.kind} (${theme})`;
        const seen = worst.get(key);
        if (!seen || ratio < seen.ratio)
          worst.set(key, { ratio, example: word.textContent!.trim() });
      }
  }
  return [...worst].map(([key, w]) => `${key}, "${w.example}": ${w.ratio.toFixed(2)}`);
}
const path = (svg: Element, name: string) =>
  svg.querySelector(`[data-path="${name}"]`) as SVGPathElement;
/** A box's rect, as its four attributes. */
const rectOf = (el: Element) =>
  ['x', 'y', 'width', 'height'].map((a) => Number(el.getAttribute(a)));
const asRect = (r: { x: number; y: number; width: number; height: number }) => [
  r.x,
  r.y,
  r.width,
  r.height,
];

/** The block spans a row scene hands the window's words: NEXT (when shown), then the chain. */
const windowSpans = (scene: Scene): BlockSpan[] => [
  ...(scene.next
    ? [{ number: scene.next.number, x: scene.next.x, w: scene.next.w, mined: false }]
    : []),
  ...scene.cards.map((c) => ({ number: c.block.number, x: c.x, w: c.w, mined: true })),
];

/** The chain slots of a settled scene, as the panel hands them to `chainPaths`. */
const chainSlots = (scene: Scene): ChainSlotState[] =>
  scene.cards.map((c) => ({
    number: c.block.number,
    x: c.x,
    w: c.w,
    mined: true,
    hash: c.block.hash,
    parentHash: c.block.parentHash,
    plate: true,
  }));
/** The links of a settled row scene with the window of `H` drawn, from the library. */
const rowChain = (scene: Scene, H: number) =>
  chainPaths(
    chainSlots(scene),
    scene.next ? { x: scene.next.x, w: scene.next.w } : null,
    H,
    scene.snapshot.consensus,
    scene.viewW
  );
/** The chain's paths as drawn (`data-path`) and as `chainPaths` names them. */
const STRAP_PATHS = [
  ['solid', 'solid'],
  ['pend', 'pend'],
  ['win', 'win'],
  ['tail', 'tail'],
  ['tail-win', 'tailWin'],
] as const;
/**
 * The filled rectangles of a strap path (a link, a tail), as its corner and size; anything else in
 * the path, a stroked segment above all, fails the test. The empty placeholder has none.
 */
function strapRects(d: string | null): { x: number; y: number; w: number; h: number }[] {
  const all = d === 'M0 0' ? '' : (d ?? '');
  const found = [...all.matchAll(/M([\d.]+) ([\d.]+)h([\d.]+)v([\d.]+)h-([\d.]+)Z/g)];
  expect(found.map((m) => m[0]).join(''), `${d}: filled rectangles only`).toBe(all);
  for (const m of found) expect(m[5], `${m[0]} closes on its own width`).toBe(m[3]);
  return found.map((m) => ({ x: Number(m[1]), y: Number(m[2]), w: Number(m[3]), h: Number(m[4]) }));
}
/** The pool's flow arrowheads of a path: filled triangles, nothing else. */
const ARROWS = /^(M[\d.]+ [\d.]+L[\d.]+ [\d.]+L[\d.]+ [\d.]+Z)+$/;
/**
 * What sets `el` back on the canvas: an opacity under 1 on it or an ancestor (attribute or style),
 * its own fill-opacity under 1, or a step-back group (`data-dim` true or soft), each as a reason.
 */
function setBack(el: Element, svg: Element): string[] {
  const out: string[] = [];
  const fillOpacity = el.getAttribute('fill-opacity');
  if (fillOpacity !== null && Number(fillOpacity) < 1) out.push(`fill-opacity ${fillOpacity}`);
  for (let a: Element | null = el; a && a !== svg; a = a.parentElement) {
    const attr = a.getAttribute('opacity');
    if (attr !== null && Number(attr) < 1) out.push(`<${a.tagName}> opacity ${attr}`);
    const style = (a as SVGElement).style.opacity;
    if (style !== '' && Number(style) < 1) out.push(`<${a.tagName}> style opacity ${style}`);
    const dim = a.getAttribute('data-dim');
    if (dim === 'true' || dim === 'soft') out.push(`<${a.tagName}> data-dim ${dim}`);
  }
  return out;
}
/**
 * The rounded rects of `svg` outside its stones: a stone is a seal (lib/seal), whose corners are
 * its own drawing; every plate, cut, well, square and ring is square (U-02, R1).
 */
const roundedOutsideStones = (svg: Element) => {
  const bare = svg.cloneNode(true) as Element;
  bare.querySelectorAll('[data-stone]').forEach((s) => s.remove());
  return roundedRects(bare);
};
/** A block's seal as the plate presses it: its row stone, 14 px, on the plate. */
const stoneOf = (hash: string, txs: number) =>
  drawSeal(
    { kind: 'block', hash, txs },
    {
      form: 'row',
      height: G.stone,
      state: 'pressed',
      ink: color('mark'),
      ground: color('pipeline-plate'),
      dead: color('text-dim'),
    }
  ).svg;
/** `markup` as the DOM serialises it inside an SVG group (the stone's own group). */
const asInSvg = (markup: string) => {
  const g = document.createElementNS('http://www.w3.org/2000/svg', 'g');
  g.innerHTML = markup;
  return g.innerHTML;
};

/** The fixture's consensus (mainnet's): the window of H is H − 10 … H − 2. */
const CONSENSUS = buildPipelineSnapshot().consensus;
/** A card's x as drawn: the translation the static render or the motion layer gave it. */
const drawnX = (el: Element) =>
  Number(/translate\(([-\d.]+),/.exec(el.getAttribute('transform') ?? '')?.[1]);

/**
 * The drawn window label of the window of `H` (direction G): start-anchored, wholly inside
 * [0, width], starting at the left edge of the window's first plate in view (the one nearest NEXT),
 * or ending at the view's edge when it would pass it; no tick and no dimension line is drawn. Reads
 * only the DOM, so it checks the static render and the motion layer alike.
 */
function expectLabelClear(svg: Element, width: number, H: number) {
  const label = svg.querySelector('[data-window-label]') as SVGTextElement;
  expect(label).not.toBeNull();
  expect(label).toHaveAttribute('text-anchor', 'start');
  // drawn in the small text its width is measured at (labelWidth, SMALL_TEXT_PX)
  expect(label).toHaveClass('text-meta');
  const w = labelWidth(label.textContent ?? '');
  const x = Number(label.getAttribute('x'));
  expect(x).toBeGreaterThanOrEqual(0);
  expect(x + w).toBeLessThanOrEqual(width + 0.05);
  const plates = [...svg.querySelectorAll('[data-layer="cards"] [data-card]')]
    .filter((c) => inWindow(Number(c.getAttribute('data-number')), H, CONSENSUS))
    .map(drawnX)
    .filter((px) => px < width);
  expect(plates.length).toBeGreaterThan(0);
  expect(Math.abs(x - Math.min(Math.min(...plates), width - w))).toBeLessThan(0.1);
  expect(svg.querySelector('[data-ruler-ticks]')).toBeNull();
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
    // T−7 proposed all 299 txs of T−5. The plate holds two rows of 9 dots (7 px on a 9 px pitch);
    // "+285" at the end of the last row gives up one slot per glyph.
    expect(states(T - 7)).toHaveLength(18 - 4);
    expect(card(svg, T - 7).querySelector('[data-chip-overflow]')).toHaveTextContent(
      `+${299 - (18 - 4)}`
    );
  });

  // Direction F (lane L's UNC, lane R): an uncle is its word and its dots, no box and no stone (a
  // block's stone says its tx count, which the pipeline does not know of an uncle).
  it('draws an uncle as its word and its dots on the plate: no box, no stone', async () => {
    const svg = await renderPanel();
    const host = card(svg, T - 1);
    const u = buildScene(buildPipelineSnapshot(), VIEW_W, 'settled').cards.find(
      (c) => c.block.number === T - 1
    )?.uncles[0];
    if (!u) throw new Error('T−1 carries no uncle');
    const uncle = host.querySelector('[data-uncle="0"]') as SVGGElement;
    expect(uncle).not.toBeNull();
    // Just the word: the uncle's number is in its tooltip. It sits in the card's words group, at
    // full strength, in the text ink (the dim ink is 4.1:1 on the plate).
    expect(uncle.closest('[data-words]')).not.toBeNull();
    expect([...uncle.querySelectorAll('text')].map((t) => t.textContent)).toEqual(['uncle']);
    const word = uncle.querySelector('text') as SVGTextElement;
    expect(word).toHaveClass('fill-text');
    expect(Number(word.getAttribute('x'))).toBe(u.x);
    expect(uncle.querySelectorAll('rect, path, circle, [data-stone]')).toHaveLength(0);
    // The word is what a pointer reads the uncle by, though the card's words let it through.
    expect(uncle).toHaveAttribute('pointer-events', 'visiblePainted');
    // Its two dots under its word, at its own row.
    const dots = [...host.querySelectorAll('[data-chip][data-uncle-index="0"]')];
    expect(dots).toHaveLength(2);
    for (const d of dots) expect(Number(d.getAttribute('cy')) - G.chip / 2).toBe(u.y + UNC.head);
    // An uncle carries a header and proposals, never transactions; the plate has one stone, its own.
    expect(uncle.querySelectorAll('[data-sq]')).toHaveLength(0);
    expect(host.querySelectorAll('[data-stone]')).toHaveLength(1);
  });

  it('draws its plates, cuts, wells, NEXT, ghost txs and tx squares square-cornered (U-02, R1)', async () => {
    const svg = await renderPanel();
    expect(svg.querySelectorAll('[data-box]').length).toBeGreaterThan(1);
    expect(svg.querySelectorAll('rect[data-cut]').length).toBeGreaterThan(0);
    expect(svg.querySelectorAll('rect[data-tray]')).toHaveLength(2);
    expect(svg.querySelectorAll('[data-ghost="tx"]').length).toBeGreaterThan(0);
    expect(svg.querySelectorAll('rect[data-sq]').length).toBeGreaterThan(0);
    // a stone is a seal: its corners are its own drawing (lib/seal), outside the rule
    expect(svg.querySelectorAll('[data-stone]').length).toBeGreaterThan(0);
    expect(roundedOutsideStones(svg)).toEqual([]);
  });

  it('draws committed txs as squares after the cellbase and counts the rest as +N', async () => {
    const svg = await renderPanel();
    const full = card(svg, T - 5);
    const squares = full.querySelectorAll('[data-sq]');
    // 5 × 4 cells hold the cellbase and 19 of the 39 listed txs (all 1 × 1); the other 20 listed
    // and the 260 the backend omitted are counted.
    expect(squares).toHaveLength(BLOCK_FIELD_CAPACITY);
    expect(squares[0]).toHaveAttribute('data-stage', 'cellbase');
    const count = full.querySelector('[data-count]') as SVGTextElement;
    expect(count.textContent).toBe('+280');
    // Right-aligned on the head line, in the text ink.
    expect(count).toHaveAttribute('text-anchor', 'end');
    expect(count).toHaveClass('fill-text');
    expect(Number(count.getAttribute('x'))).toBe(G.cardW - G.padX);
    expect(Number(count.getAttribute('y'))).toBe(G.headY + 3.5);
  });

  // Direction F, §2.1 (lane L's A6, lane R): a plate is cut only where a transaction is.
  it('cuts a plate only where a mark is: as many cuts as marks, each its square grown by the gutter', async () => {
    const svg = await renderPanel();
    const scene = buildScene(buildPipelineSnapshot(), VIEW_W, 'settled');
    for (const c of scene.cards) {
      const el = card(svg, c.block.number);
      const cuts = el.querySelector('[data-cuts]') as SVGGElement;
      const field = el.querySelector('[data-field]') as SVGGElement;
      // the field holds marks only, its cuts group cuts only: no empty cell, no bed, no grid
      expect([...cuts.children].every((x) => x.matches('rect[data-cut]'))).toBe(true);
      expect([...field.children].every((x) => x.hasAttribute('data-sq'))).toBe(true);
      expect(cuts.children).toHaveLength(field.children.length);
      expect(cuts.children).toHaveLength(c.squares.length);
      // drawn first: the cuts lie under the marks
      expect(cuts.compareDocumentPosition(field)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
      for (const q of c.squares) {
        const cut = cuts.querySelector(`[data-cut="${q.id}"]`) as SVGRectElement;
        expect(cut, `the cut of ${q.id}`).not.toBeNull();
        expect(['x', 'y', 'width', 'height'].map((a) => Number(cut.getAttribute(a)))).toEqual([
          q.x - GUT,
          q.y - GUT,
          q.s + 2 * GUT,
          q.s + 2 * GUT,
        ]);
        expect(cut.style.getPropertyValue('fill')).toBe(CUT_FILL);
      }
    }
  });

  // Direction G: the gilt strap across a plate's head is a quantity too: the plate lies inside the
  // window the next block may commit from.
  it('draws nothing on a plate without a quantity: the plate, its strap in the window, its stone, cuts, marks, dots and words', async () => {
    const svg = await renderPanel();
    const scene = buildScene(buildPipelineSnapshot(), VIEW_W, 'settled');
    const stoneOn = new Map(scene.cards.map((c) => [c.id, c.stone]));
    const cards = [...svg.querySelectorAll('[data-card]')];
    expect(cards.length).toBeGreaterThan(8);
    const what = (el: Element) =>
      el.closest('[data-stone]')
        ? 'stone'
        : el.matches('[data-box]')
          ? 'plate'
          : el.matches('[data-strap]')
            ? 'strap'
            : el.matches('[data-cut]')
              ? 'cut'
              : el.matches('[data-sq]')
                ? 'mark'
                : el.matches('[data-chip]')
                  ? 'dot'
                  : el.matches('text, tspan')
                    ? 'word'
                    : el.matches('g')
                      ? 'group'
                      : null;
    for (const c of cards) {
      const stray = [...c.querySelectorAll('*')].filter((el) => what(el) === null);
      expect(stray.map((el) => el.outerHTML.slice(0, 80))).toEqual([]);
      // a group draws nothing of its own
      for (const g of c.querySelectorAll('g')) {
        if (g.closest('[data-stone]')) continue;
        expect(g.getAttribute('fill')).toBeNull();
        expect(g.getAttribute('stroke')).toBeNull();
      }
      expect(c.querySelectorAll('[data-box]')).toHaveLength(1);
      // a stone exactly where the scene presses one (lane F: it gives way to the head's data)
      const id = c.getAttribute('data-card') as string;
      expect(stoneOn.has(id)).toBe(true);
      expect(c.querySelectorAll(':scope [data-stone]')).toHaveLength(stoneOn.get(id) ? 1 : 0);
      // a strap exactly where the next block's window runs
      const n = Number(c.getAttribute('data-number'));
      expect(c.querySelectorAll('[data-strap]')).toHaveLength(
        inWindow(n, T + 1, CONSENSUS) ? 1 : 0
      );
    }
    // the fixture's T−5 hides 280 txs: its head has no room for a stone
    expect([...stoneOn].filter(([, s]) => !s).length).toBeGreaterThan(0);
  });

  it('presses the block’s own seal into the head of its plate: its row stone, 14 px', async () => {
    const svg = await renderPanel();
    const scene = buildScene(buildPipelineSnapshot(), VIEW_W, 'settled');
    for (const c of scene.cards) {
      if (!c.stone) {
        expect(card(svg, c.block.number).querySelector('[data-stone]')).toBeNull();
        continue;
      }
      const stone = card(svg, c.block.number).querySelector('[data-stone]') as SVGGElement;
      expect(stone.getAttribute('transform')).toBe(`translate(${G.stoneX},${G.stoneY})`);
      // the same markup as the block's seal: its hash and its transaction count
      expect(stone.innerHTML).toBe(asInSvg(stoneOf(c.block.hash, c.block.txCount)));
      const seal = stone.querySelector('svg') as SVGSVGElement;
      expect([seal.getAttribute('width'), seal.getAttribute('height')]).toEqual(['14', '14']);
      // it lets the pointer through to the plate
      expect(stone).toHaveAttribute('pointer-events', 'none');
    }
    // two blocks, two stones
    const [a, b] = scene.cards;
    expect(stoneOf(a.block.hash, a.block.txCount)).not.toBe(stoneOf(b.block.hash, b.block.txCount));
    // the number stands after the stone
    const number = card(svg, T).querySelector('[data-block-number]') as SVGTextElement;
    expect(Number(number.getAttribute('x'))).toBe(G.numberX);
    expect(number).toHaveClass('fill-text-bright');
  });

  // Ruling on lane F: the stone is an accent and gives way to the head's data; a plate hiding 100
  // txs or more has none, its number from the plate's padding, at least 4 px clear of its "+N".
  it('presses no stone into a plate hiding 100 txs or more: its number from x 8, clear of its "+N"', async () => {
    const svg = await renderPanel();
    const scene = buildScene(buildPipelineSnapshot(), VIEW_W, 'settled');
    const big = scene.cards.find((c) => c.block.number === T - 5) as Scene['cards'][number];
    expect(big).toMatchObject({ hidden: 280, stone: false, numberX: G.padX });
    const el = card(svg, T - 5);
    expect(el.querySelector('[data-stone]')).toBeNull();
    const number = el.querySelector('[data-block-number]') as SVGTextElement;
    const count = el.querySelector('[data-count]') as SVGTextElement;
    expect(count.textContent).toBe('+280');
    expect(count).toHaveAttribute('text-anchor', 'end');
    // measured at the sizes they are drawn in: the number and the "+N" text-meta (12 px; the number
    // was text-small when that was 12 px, re-pinned with readability R4)
    expect(number).toHaveClass('text-meta');
    expect(count).toHaveClass('text-meta');
    const x = (t: Element) => Number(t.getAttribute('x'));
    const w = (t: Element, size: string) => (t.textContent ?? '').length * GLYPH[size];
    expect(x(number)).toBe(8);
    expect(
      x(count) - w(count, 'text-meta') - (x(number) + w(number, 'text-meta'))
    ).toBeGreaterThanOrEqual(4);
    // a plate hiding fewer keeps its stone and its number at 27
    const tip = card(svg, T);
    expect(scene.cards[0]).toMatchObject({ stone: true, numberX: G.numberX });
    expect(tip.querySelector('[data-stone]')).not.toBeNull();
    expect(x(tip.querySelector('[data-block-number]') as Element)).toBe(27);
  });

  it('shows only the number in a block header, and "+N" only when txs are hidden', async () => {
    const svg = await renderPanel();
    const scene = buildScene(buildPipelineSnapshot(), VIEW_W, 'settled');
    expect(scene.cards.some((c) => c.hidden > 0)).toBe(true);
    expect(scene.cards.some((c) => c.hidden === 0)).toBe(true);
    for (const c of scene.cards) {
      const el = card(svg, c.block.number);
      // re-pinned with lane E2 (R4): the header's words sit in the card's words group
      const header = [...el.querySelectorAll(':scope > [data-dim] > [data-words] > text')].map(
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

  // Re-pinned with lane P from 11 px / 400: the boards' `.no` (Plate.dc.html, PlatePhone.dc.html)
  // is 12 px / 500, a block's number and NEXT's, in both orientations. Re-pinned with readability
  // R4: 12 px is the meta step now (text-small is 13), and the canvas keeps its 12 px numbers.
  it('writes the block and NEXT numbers in the card headers at 12 px, 500', async () => {
    const svg = await renderPanel();
    expect(card(svg, T).querySelector('[data-block-number]')).toHaveClass(
      'text-meta',
      'font-medium'
    );
    const nextNumber = within(svg.querySelector('[data-next]') as unknown as HTMLElement).getByText(
      `…${String(T + 1).slice(-4)}`
    );
    expect(nextNumber).toHaveClass('text-meta', 'font-medium');
    // in the window too: the same size, weight and ink, on its gilt (direction G: the word gold
    // would be 4.2 : 1 on the gilt by night and 3.8 by day, so a number never turns gold)
    hoverStoreSet({ block: null, tx: null, window: T + 1 });
    expect(card(svg, T - 1).querySelector('[data-strap]')).not.toBeNull();
    const inWin = card(svg, T - 1).querySelector('[data-block-number]');
    expect(inWin).toHaveClass('fill-text-bright', 'text-meta', 'font-medium');
    hoverStoreSet({ block: null, tx: null, window: null });
    // every number at 12 px: the meta step since readability R4 (re-pinned: it was "never the
    // meta step", when that was 11 px), never the small step, 13 px
    for (const el of svg.querySelectorAll('[data-block-number]')) {
      expect(el).toHaveClass('text-meta', 'font-medium');
      expect(el).not.toHaveClass('text-small');
    }
  });

  it('draws a tx whose cycles are still pending as a dashed square', async () => {
    // The tip's 2 × 2 whose cycles are pending: at full size its 4 × 4 leaves the field no room
    // for it, so the txs are kept at most 2 × 2 here.
    const svg = await renderPanel(withSmallTxs(buildPipelineSnapshot()));
    const sq = card(svg, T).querySelector(`[data-sq="${committedTxHash(T, 0)}"]`);
    expect(sq).toHaveAttribute('data-style', 'dashed');
    expect(sq).toHaveAttribute('stroke-dasharray', '2 1.5');
  });

  it('draws a mined block as a solid plate: its own tone, its exact 99 × 122 px, no stroke at rest', async () => {
    const svg = await renderPanel();
    // the tip, a full block, and the last plate in a 1600 px view (T−9)
    for (const n of [T, T - 5, T - 9]) {
      const box = tile(svg, n);
      expect(box.tagName).toBe('rect');
      expect(box.style.getPropertyValue('fill')).toBe(PLATE_FILL);
      expect(box).toHaveAttribute('fill-opacity', '1');
      expect(box.style.getPropertyValue('stroke')).toBe('none');
      expect(box).not.toHaveAttribute('stroke-dasharray');
      expect(['x', 'y', 'width', 'height'].map((a) => Number(box.getAttribute(a)))).toEqual([
        0,
        G.cardTop,
        G.cardW,
        CARD_H,
      ]);
    }
  });

  // Direction F, §2.5: a faded mark is mixed, not made transparent; the plate and the words never
  // fade (re-pinned from the age group's opacity, lane E2's "1 at the tip, 0.4 from tip − 7 on").
  it('steps an older block’s marks back by their fill, toward the cut or the plate: 1 at the tip, 0.4 from tip − 7 on', async () => {
    const svg = await renderPanel();
    const scene = buildScene(buildPipelineSnapshot(), VIEW_W, 'settled');
    expect(scene.cards.length).toBeGreaterThan(8);
    scene.cards.forEach((c, i) => {
      const el = card(svg, c.block.number);
      expect(el).toHaveAttribute('data-age', String(i));
      // The outer group's opacity attribute belongs to the motion layer (fade in / retire).
      expect(el.getAttribute('opacity')).toBe('1.000');
      for (const q of c.squares) {
        const sq = el.querySelector(`[data-sq="${q.id}"]`) as SVGElement;
        const own = paintSquare(q.stage, q.style, q.step);
        expect(sq.style.getPropertyValue('fill')).toBe(
          agedPaint(own, q.kind === 'cellbase' ? 'cellbase' : 'square', i).fill
        );
      }
      for (const k of c.chips) {
        const dot = el.querySelector(`[data-chip="${k.key}"]`) as SVGElement;
        expect(dot.style.getPropertyValue('fill')).toBe(agedChip(k.proposal.state, i).fill);
      }
    });
    // the tip's marks are their own colours; from tip − 7 on a mark keeps 40 % of itself
    const tipCellbase = card(svg, T).querySelector('[data-stage="cellbase"]') as SVGElement;
    expect(tipCellbase.style.getPropertyValue('fill')).toBe(STAGE_BASE.cellbase);
    expect(ageOpacity(7)).toBeCloseTo(0.4, 6);
    const old = card(svg, T - 7).querySelector('[data-stage="cellbase"]') as SVGElement;
    expect(old.style.getPropertyValue('fill')).toBe(
      `color-mix(in srgb, ${STAGE_BASE.cellbase} 40%, ${CUT_FILL})`
    );
  });

  it('never fades a plate or anything on it, its words least of all (R4, lane E2)', async () => {
    expectPlatesAtFullStrength(await renderPanel());
  });

  // Lane E2 follow-up, ruling 2: a tile is a ground of its own, so its words are chosen for it.
  // With the window drawn (its gilt under the numbers, its label), and once with no pool (the
  // well's word).
  it('writes every word at 4.5:1 on what it stands on, in both themes (R4, lane E2)', async () => {
    const svg = await renderPanel();
    act(() => usePipelineHoverStore.getState().setHover({ block: null, tx: null, window: T }));
    expect(svg.querySelector('[data-window-label]')).not.toBeNull();
    const failures = wordInkFailures(svg, 'row');
    act(() => usePipelineHoverStore.getState().clearHover());
    expect(failures).toEqual([]);
  });

  // Direction G (re-pinned from lane E2 round 3's word gold): the window is the gilt strap under
  // the head's words, where the word gold falls under 4.5 : 1 (4.2 by night, 3.8 by day), so an
  // in-window number keeps the bright ink: 7.6 : 1 on the gilt by night, 8.4 by day.
  it('keeps an in-window number in the bright ink on its gilt, at 4.5:1 in both themes', async () => {
    const svg = await renderPanel();
    act(() => usePipelineHoverStore.getState().setHover({ block: null, tx: null, window: T }));
    const inside = [...svg.querySelectorAll('[data-card]')].filter((c) =>
      inWindow(Number(c.getAttribute('data-number')), T, CONSENSUS)
    );
    expect(inside.length).toBeGreaterThan(0);
    const short: string[] = [];
    for (const c of inside) {
      const number = c.querySelector('[data-block-number]') as Element;
      expect(wordInk(number, svg), `ink of ${number.textContent}`).toBe('text-bright');
      const strap = c.querySelector('[data-strap]') as SVGElement;
      expect(strengthInCard(strap, c)).toBe(1);
      for (const theme of THEMES) {
        const ratio = contrastRatio(
          PALETTE[theme]['text-bright'],
          fillHex(theme, strap.style.fill)
        );
        if (ratio < 4.5) short.push(`${theme} ${number.textContent} on the gilt: ${ratio}`);
      }
    }
    act(() => usePipelineHoverStore.getState().clearHover());
    expect(short).toEqual([]);
  });

  it('writes the well’s and NEXT’s "unavailable" at 4.5:1 when the pool tip differs', async () => {
    const svg = await renderPanel({
      ...buildPipelineSnapshot(),
      status: 'poolTipMismatch',
      poolTip: { number: T + 1, hash: blockHash(T + 1) },
      pool: null,
      next: null,
    });
    expect(svg.querySelector('[data-pool-status]')).not.toBeNull();
    expect(svg.querySelector('[data-next-column]')).not.toBeNull();
    expect(wordInkFailures(svg, 'row')).toEqual([]);
  });

  // Direction F (Plate.dc.html `.wl`, `.tl`, `.lk.f`): a well is square, its 1 px edge in the
  // line ink and the theme's inset under it (its six round corners were a path the radius gate
  // never saw); the pool's name and total stand on the head line above it.
  it('draws the pool as two square wells: a 1 px edge, the inset, the stage name and its total above', async () => {
    const svg = await renderPanel();
    const wells = [
      { id: 'pending', title: 'mempool', tone: 'fill-aqua', total: '5', x: G.mempoolX },
      { id: 'proposed', title: 'proposed', tone: 'fill-gold', total: '9', x: G.proposedX },
    ];
    for (const w of wells) {
      const well = svg.querySelector(`[data-pool-card="${w.id}"]`) as SVGGElement;
      const tray = well.querySelector('[data-tray]') as SVGRectElement;
      expect(tray.tagName).toBe('rect');
      expect(tray).not.toHaveAttribute('rx');
      expect(tray).not.toHaveAttribute('ry');
      expect(tray.style.getPropertyValue('fill')).toBe(WELL_FILL);
      expect(tray.style.getPropertyValue('stroke')).toBe(color('base-border'));
      expect(tray).toHaveAttribute('stroke-width', '1');
      const r = wellRect(w.x);
      expect(['x', 'y', 'width', 'height'].map((a) => Number(tray.getAttribute(a)))).toEqual([
        r.x,
        r.y,
        r.width,
        r.height,
      ]);
      // the inset: a 3 px shade under the top edge, the theme's well inset fading to nothing
      const inset = well.querySelector('[data-inset]') as SVGRectElement;
      const s = wellShade(w.x);
      expect(['x', 'y', 'width', 'height'].map((a) => Number(inset.getAttribute(a)))).toEqual([
        s.x,
        s.y,
        s.width,
        s.height,
      ]);
      const ref = /^url\(#(.+)\)$/.exec(inset.getAttribute('fill') ?? '')?.[1];
      const shade = svg.querySelector(`linearGradient[id="${ref}"]`) as SVGLinearGradientElement;
      expect(shade).not.toBeNull();
      expect(
        [shade.getAttribute('x1'), shade.getAttribute('y1')].concat([
          shade.getAttribute('x2'),
          shade.getAttribute('y2'),
        ])
      ).toEqual(['0', '0', '0', '1']);
      const stops = [...shade.querySelectorAll('stop')] as SVGStopElement[];
      expect(stops.map((x) => x.style.getPropertyValue('stop-color'))).toEqual([
        'var(--m-well-inset)',
        'var(--m-well-inset)',
      ]);
      expect(stops.map((x) => x.getAttribute('stop-opacity'))).toEqual(['1', '0']);
      // the name in the stage's colour and the total, on the head line above the well
      const name = within(well as unknown as HTMLElement).getByText(w.title);
      expect(name).toHaveClass(w.tone);
      const nameText = name.closest('text') as SVGTextElement;
      expect(Number(nameText.getAttribute('x'))).toBe(w.x + G.poolHeadPad);
      expect(Number(nameText.getAttribute('y'))).toBe(G.headY + 3.5);
      const count = well.querySelector('[data-count]') as SVGTextElement;
      expect(count.textContent).toBe(w.total);
      expect(Number(count.getAttribute('x'))).toBe(w.x + G.cardW - G.poolHeadPad);
      expect(count).toHaveAttribute('text-anchor', 'end');
    }
    // mempool ⇢ proposed ⇢ NEXT (direction G, Strap.dc.html `.fw`): arrowheads, two in the 18 px
    // gap and four in the 30 px gap, filled in the muted ink.
    const flows = [...svg.querySelectorAll('[data-flow]')] as SVGPathElement[];
    expect(flows.map((f) => f.getAttribute('d'))).toEqual([
      flowPaths().toProposed,
      flowPaths().toNext,
    ]);
    expect(flows.map((f) => (f.getAttribute('d') ?? '').match(/Z/g)?.length)).toEqual([2, 4]);
    for (const f of flows) {
      expect(f.getAttribute('d')).toMatch(ARROWS);
      expect(f.style.getPropertyValue('fill')).toBe(tok('ink-dim'));
      expect(f).toHaveAttribute('fill-opacity', '0.8');
      expect(f).not.toHaveAttribute('stroke-dasharray');
      expect(f.style.getPropertyValue('stroke')).toBe('');
    }
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

  it('draws NEXT as the only dashed outline, at the plate’s size, with dashed squares and hollow dots', async () => {
    const svg = await renderPanel();
    const next = svg.querySelector('[data-next]') as SVGGElement;
    const nextBox = next.querySelector('[data-box]') as SVGRectElement;
    // No plate yet: an outline only (Plate.dc.html `.nx rect`: 2 2), its line on the plate's own
    // outer pixels.
    expect(nextBox).toHaveAttribute('stroke-dasharray', '2 2');
    expect(nextBox.style.getPropertyValue('stroke')).toBe(tok('jade'));
    expect(nextBox.style.getPropertyValue('fill')).toBe('none');
    const r = cardBox('row', G.cardW, CARD_H, true);
    expect(['x', 'y', 'width', 'height'].map((a) => Number(nextBox.getAttribute(a)))).toEqual([
      r.x,
      r.y,
      r.width,
      r.height,
    ]);
    // One look whatever is hovered: its dashed outline at 0.6.
    expect(nextBox).toHaveAttribute('stroke-opacity', '0.6');
    hoverStoreSet({ block: T + 1, tx: null, window: T + 1 });
    expect(nextBox).toHaveAttribute('stroke-opacity', '0.6');
    hoverStoreSet({ block: null, tx: null, window: null });
    // its number in the dim ink, at the plate's padding (no stone: nothing is pressed yet)
    const number = within(next as unknown as HTMLElement).getByText(tail4(T + 1));
    expect(number).toHaveClass('fill-text-dim');
    expect(Number(number.getAttribute('x'))).toBe(G.padX);
    expect(next.querySelector('[data-stone]')).toBeNull();
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
    // Plates and wells have no dashed outline.
    const outlines = [...svg.querySelectorAll('[data-box], [data-tray]')];
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
    expect(Number(count.getAttribute('y'))).toBe(G.headY + 3.5);
    expect(screen.getByTestId('pipeline-tip')).not.toHaveTextContent('Next');
  });

  // Direction G (Strap.dc.html, at rest): the window of the next block is the gilt strap across
  // the head of every plate inside it and across the joints between them, at rest too; nothing
  // else marks it, and no number turns gold.
  it('draws the window at rest as the gilt strap: across every window plate and its joints, nothing else', async () => {
    const s = buildPipelineSnapshot();
    const svg = await renderPanel(s);
    const scene = buildScene(s, VIEW_W, 'settled');
    expect(svg.querySelector('[data-window-label]')).toBeNull();
    expect(svg.querySelector('[data-ruler-ticks]')).toBeNull();
    expect(svg.querySelector('[data-window-marks]')).toBeNull();
    // NEXT's window is T−9 … T−1: a gilt strap across each of those plates' heads, none elsewhere
    const inside = scene.cards.filter((c) => inWindow(c.block.number, T + 1, s.consensus));
    expect(inside.map((c) => c.block.number)).toEqual(
      Array.from({ length: 9 }, (_, i) => T - 1 - i)
    );
    for (const c of scene.cards) {
      const el = card(svg, c.block.number);
      const strap = el.querySelector('[data-strap]') as SVGRectElement | null;
      if (!inside.includes(c)) {
        expect(strap, `#${c.block.number}`).toBeNull();
        continue;
      }
      if (!strap) throw new Error(`#${c.block.number} has no strap`);
      expect(rectOf(strap)).toEqual(asRect(strapBox('row', c.w, c.h)));
      expect(rectOf(strap)).toEqual([0, G.strapY, G.cardW, G.strapH]);
      expect(strap.style.getPropertyValue('fill')).toBe(GILT_FILL);
      expect(strap).toHaveAttribute('pointer-events', 'none');
      // on the plate: right above its fill, under its stone, cuts, dots and words
      const layer = [...(strap.parentElement as Element).children];
      expect(layer.indexOf(strap)).toBe(layer.indexOf(tile(svg, c.block.number)) + 1);
    }
    // the eight joints between them are gilt, at full strength
    const win = path(svg, 'win');
    expect(strapRects(win.getAttribute('d'))).toHaveLength(8);
    expect(win.getAttribute('d')).toBe(rowChain(scene, T + 1).win);
    expect(win.style.getPropertyValue('fill')).toBe(GILT_FILL);
    expect(win).not.toHaveAttribute('fill-opacity');
    // no number turns gold
    for (const el of svg.querySelectorAll('[data-block-number]')) {
      expect(el).toHaveClass('fill-text-bright');
    }
  });

  // Direction G (Strap.dc.html `.sp`, `.sq2`): one strap through the heads, in the plate's own tone
  // across a gap between two blocks, so the chain reads as one piece; from NEXT two dashed lines
  // in NEXT's outline paint, the only stroke in the chain; past the last plate it runs on, fading.
  it('draws the strap: the plate’s tone between blocks, two dashed lines from NEXT, the tail fading', async () => {
    const s = buildPipelineSnapshot();
    const svg = await renderPanel(s);
    const chain = svg.querySelector('[data-chain]') as SVGGElement;
    expect([...chain.querySelectorAll('path')].map((p) => p.getAttribute('data-path'))).toEqual([
      'solid',
      'tail',
      'pend',
      'win',
      'tail-win',
    ]);
    const scene = buildScene(s, VIEW_W, 'settled');
    const paths = rowChain(scene, T + 1);
    for (const [name, key] of STRAP_PATHS) {
      expect(path(svg, name)).toHaveAttribute('d', paths[key] || 'M0 0');
    }
    // filled rectangles, never stroked, but for NEXT's dashed pair; the pool's arrowheads too
    for (const p of [
      ...['solid', 'win', 'tail', 'tail-win'].map((n) => path(svg, n)),
      ...svg.querySelectorAll<SVGPathElement>('[data-flow]'),
    ]) {
      if (!p.matches('[data-flow]')) strapRects(p.getAttribute('d'));
      expect(p.style.getPropertyValue('stroke')).toBe('');
      expect(p).not.toHaveAttribute('stroke-width');
      expect(p).not.toHaveAttribute('stroke-dasharray');
    }
    // between settled blocks: the plate's own tone, at full strength
    expect(strapRects(path(svg, 'solid').getAttribute('d')).length).toBeGreaterThan(0);
    expect(path(svg, 'solid').style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(path(svg, 'solid')).not.toHaveAttribute('fill-opacity');
    // NEXT (246 … 345) ⇢ the tip (363 …): two dashed lines on the strap's top and bottom pixel
    // rows, in NEXT's own outline paint, no fill
    const pend = path(svg, 'pend');
    expect(pend.getAttribute('d')).toBe('M345.0 26.5H363.0M345.0 45.5H363.0');
    expect(pend).toHaveAttribute('fill', 'none');
    expect(pend.style.getPropertyValue('stroke')).toBe(tok('jade'));
    expect(pend).toHaveAttribute('stroke-opacity', '0.6');
    expect(pend).toHaveAttribute('stroke-dasharray', '2 2');
    expect(pend).toHaveAttribute('stroke-width', '1');
    // the only stroke in the chain
    const stroked = [...chain.querySelectorAll('*')].filter(
      (el) =>
        el.hasAttribute('stroke-width') ||
        el.hasAttribute('stroke') ||
        (el as SVGElement).style.getPropertyValue('stroke') !== ''
    );
    expect(stroked).toEqual([pend]);
    // The window of NEXT ends on screen (T−9 ends at 1515): the tail is the plate's tone, 66 px.
    const tail = path(svg, 'tail');
    expect(strapRects(tail.getAttribute('d'))).toEqual([{ x: 1515, y: 26, w: 66, h: 20 }]);
    expect(path(svg, 'tail-win')).toHaveAttribute('d', 'M0 0');
    expect(tail.style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(path(svg, 'tail-win').style.getPropertyValue('fill')).toBe(GILT_FILL);
    // both fade to nothing along their length, through one alpha mask
    for (const t of [tail, path(svg, 'tail-win')]) {
      expect(t).not.toHaveAttribute('fill-opacity');
      expect(t.getAttribute('mask')).toBe(tail.getAttribute('mask'));
    }
    const maskId = /^url\(#(.+)\)$/.exec(tail.getAttribute('mask') ?? '')?.[1];
    const mask = svg.querySelector(`mask[id="${maskId}"]`) as SVGMaskElement;
    expect(mask).not.toBeNull();
    expect(mask).toHaveAttribute('maskContentUnits', 'objectBoundingBox');
    // an alpha mask: no colour in it, only how much of the tail is kept
    expect(mask.style.getPropertyValue('mask-type')).toBe('alpha');
    const fadeId = /^url\(#(.+)\)$/.exec(
      mask.querySelector('rect')?.getAttribute('fill') ?? ''
    )?.[1];
    const fade = svg.querySelector(`linearGradient[id="${fadeId}"]`) as SVGLinearGradientElement;
    // along the row: from the last plate on, to nothing
    expect(['x1', 'y1', 'x2', 'y2'].map((a) => fade.getAttribute(a))).toEqual(['0', '0', '1', '0']);
    expect([...fade.querySelectorAll('stop')].map((x) => x.getAttribute('stop-opacity'))).toEqual([
      '1',
      '0',
    ]);
  });

  // Direction G: a link fills the whole gap between a block and its parent, at the strap's y and
  // height, with no inset, and only where the newer block names the older one as its parent.
  it('fills the whole gap of every parent link at the strap’s y, and none across a break', async () => {
    const s = buildPipelineSnapshot();
    // T−4 names another parent: the chain breaks between T−4 and T−5 (a reorg, an orphan)
    const broken: PipelineSnapshot = {
      ...s,
      blocks: s.blocks.map((b) =>
        b.number === T - 4 ? { ...b, parentHash: blockHash(T - 100) } : b
      ),
    };
    const svg = await renderPanel(broken);
    const scene = buildScene(broken, VIEW_W, 'settled');
    const links = (['solid', 'win'] as const).flatMap((n) =>
      strapRects(path(svg, n).getAttribute('d'))
    );
    const gaps = scene.cards.slice(1).map((c, i) => ({
      a: scene.cards[i].x + scene.cards[i].w,
      b: c.x,
      linked: scene.cards[i].block.parentHash === c.block.hash,
      name: `#${scene.cards[i].block.number} → #${c.block.number}`,
    }));
    expect(gaps.filter((g) => !g.linked).map((g) => g.name)).toEqual([`#${T - 4} → #${T - 5}`]);
    for (const g of gaps) {
      const inGap = links.filter((r) => r.x >= g.a && r.x < g.b);
      expect(inGap, g.name).toEqual(
        g.linked ? [{ x: g.a, y: G.strapY, w: g.b - g.a, h: G.strapH }] : []
      );
    }
    expect(links).toHaveLength(gaps.filter((g) => g.linked).length);
    // NEXT ⇢ the tip: the dashed pair across its whole gap
    const next = scene.next as NonNullable<Scene['next']>;
    const [a, b] = [next.x + next.w, scene.cards[0].x].map((v) => v.toFixed(1));
    expect(path(svg, 'pend')).toHaveAttribute('d', `M${a} 26.5H${b}M${a} 45.5H${b}`);
  });

  // Direction G: the strap is a plate-like ground: it never fades with age and is never set back
  // with its block's marks; under the pointer the gilt stays as it is, wherever the window moves.
  it('never sets the gilt back: no opacity over a gilt strap or joint, at rest or under the pointer', async () => {
    const svg = await renderPanel();
    const gilt = () => [
      ...svg.querySelectorAll('[data-strap]'),
      path(svg, 'win'),
      path(svg, 'tail-win'),
    ];
    const check = (when: string) => {
      const straps = svg.querySelectorAll('[data-strap]');
      expect(straps.length, when).toBeGreaterThan(0);
      for (const el of gilt())
        expect(setBack(el, svg), `${when}: ${el.outerHTML.slice(0, 60)}`).toEqual([]);
      // the oldest gilt plate's strap is the gilt itself, never mixed toward a ground
      for (const el of straps) {
        expect((el as SVGElement).style.getPropertyValue('fill'), when).toBe(GILT_FILL);
      }
    };
    check('at rest');
    expect(card(svg, T - 9).querySelector('[data-strap]')).not.toBeNull();
    // pointing at T−3 (here from its Latest Blocks row): the window moves to T−3's, T−5 … T−9 on
    // screen carry the gilt and T−1 no longer; the blocks outside it step back, the gilt does not
    hoverStoreSet({ block: T - 3, tx: null, window: T - 3 });
    expect(card(svg, T - 1).querySelector('[data-strap]')).toBeNull();
    expect(card(svg, T - 5).querySelector('[data-strap]')).not.toBeNull();
    expect(card(svg, T - 1).querySelector(':scope > [data-dim]')).toHaveAttribute(
      'data-dim',
      'true'
    );
    check('pointing at T−3');
    // a hovered committed tx anchors the window on its block: the other squares step back
    hoverStoreSet({ block: T, tx: committedTxHash(T, 2), window: T });
    expect(svg).toHaveClass('is-focus');
    check('pointing at a tx of the tip');
    hoverStoreSet({ block: null, tx: null, window: null });
    check('after the pointer left');
  });

  // The dimming of what lies outside the window stays (direction G): the plate-tone strap and
  // NEXT's dashed pair step back with the plates they join, the gilt never.
  it('steps the plate-tone strap and NEXT’s dashed pair back with the plates they join, under the pointer', async () => {
    const svg = await renderPanel();
    const groupOf = (name: string) => path(svg, name).parentElement as Element;
    // at rest nothing steps back
    for (const name of ['solid', 'pend', 'tail']) {
      expect(groupOf(name)).toHaveAttribute('data-dim', 'false');
    }
    // pointing at T−3 (from its Latest Blocks row): the plates outside its window step back, and
    // the plate-tone strap that joins them with them
    hoverStoreSet({ block: T - 3, tx: null, window: T - 3 });
    expect(card(svg, T).querySelector(':scope > [data-dim]')).toHaveAttribute('data-dim', 'true');
    for (const name of ['solid', 'pend', 'tail']) {
      expect(groupOf(name), name).toHaveAttribute('data-dim', 'true');
    }
    for (const name of ['win', 'tail-win']) expect(setBack(path(svg, name), svg)).toEqual([]);
    hoverStoreSet({ block: null, tx: null, window: null });
    expect(groupOf('solid')).toHaveAttribute('data-dim', 'false');
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

  it('keys the canvas in one legend line: the scale, the shapes, the window, the stages, then cellbase, cycles, unknown', async () => {
    await renderPanel();
    const legend = screen.getByTestId('pipeline-legend');
    const keys = [...legend.children];
    // The empty entries are the dividers between the shapes, the stage colours and the rest.
    expect(keys.map((el) => el.textContent)).toEqual([
      'tx, area ∝ bytes',
      'proposal',
      'proposal window',
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
    // ■ tx, area ∝ bytes: the unit is said here only; ● proposal.
    expect(keys[0].querySelector('rect')).not.toBeNull();
    expect(keys[0].querySelector('circle')).toBeNull();
    expect(keys[1].querySelector('circle')).not.toBeNull();
    // ▬ proposal window (direction G): a 16 × 9 px piece of the gilt strap.
    const strapKey = keys[2].querySelector('[data-swatch="window"]') as SVGRectElement;
    expect([strapKey.getAttribute('width'), strapKey.getAttribute('height')]).toEqual(['16', '9']);
    expect(strapKey.style.getPropertyValue('fill')).toBe(GILT_FILL);
    // The stage keys are 9 px squares in the canvas's own paints (Plate.dc.html `.lg i`).
    const paints = [
      paintSquare('pending', 'solid', 0),
      paintSquare('proposed', 'solid', 0),
      paintSquare('proposed', 'hollow', 0),
      paintSquare('committed', 'solid', 0),
      { ...chipPaint('expired'), strokeDasharray: null },
    ];
    keys.slice(4, 9).forEach((k, i) => {
      const sq = k.querySelector('rect') as SVGRectElement;
      expect([sq.getAttribute('width'), sq.getAttribute('height')]).toEqual(['9', '9']);
      expect(sq.style.getPropertyValue('fill')).toBe(paints[i].fill);
      expect(sq.style.getPropertyValue('stroke')).toBe(paints[i].stroke ?? 'none');
    });
    // "too early" is the hollow one, as the proposed well draws it.
    expect(keys[6].querySelector('rect')).toHaveAttribute('fill-opacity', '0.14');
    // No key breaks from its swatch when the line wraps.
    for (const k of keys.filter((x) => x.textContent)) expect(k).toHaveClass('whitespace-nowrap');
    // No usage hint under the canvas while nothing is pinned.
    expect(screen.queryByTestId('pipeline-selection')).toBeNull();
    expect(screen.getByTestId('pipeline-panel')).not.toHaveTextContent(/hover a/i);
  });

  it('puts what the panel shows and how to use it in the "?" title', async () => {
    await renderPanel();
    const title = screen.getByText('?').getAttribute('title') ?? '';
    expect(title).toContain('proposed in one block and committed in a later block');
    expect(title).toContain('Square area = bytes; shade = cycles.');
    // Lane K: a click, or a second tap (the first pins the readout), opens the page.
    expect(title.split('\n')).toEqual([
      'Two-step confirmation: a tx is proposed in one block and committed in a later block inside its proposal window.',
      'Square area = bytes; shade = cycles.',
      'Tap or hover a tx or a proposal to see how they connect; a click, or a second tap, opens the page of a block or a tx.',
    ]);
  });

  // Rubbing × Song print, phase 5 (H1, the Main board): the head is the command line
  // `■ pipeline --tip #20,969,560`, the "?" beside the command, "all blocks →" at the right.
  // Re-pinned with lane R (PlateHome.dc.html `.ph`): the section's own 17 px heading, an h2 whose
  // square is its own `::before`, as on every other home section; no frame's head.
  it('heads the section as a command: pipeline, the "?" beside it, then --tip and the tip', async () => {
    await renderPanel();
    const command = screen.getByText('pipeline');
    expect(command.closest('.lowercase')).not.toBeNull();
    // the help sits right after the command's name, before the args
    const help = screen.getByText('?');
    expect(command).toContainElement(help);
    const head = command.closest('.prompt') as HTMLElement;
    expect(head.tagName).toBe('H2');
    expect(head).toHaveClass('text-section');
    expect(screen.queryByTestId('terminal-panel-mark')).toBeNull();
    expect(head).toHaveTextContent(new RegExp(`^pipeline\\?--tip #${fmt(T)}$`));
    expect(head).not.toHaveTextContent(/Transaction/i);
    // the args: dim and regular, in the head after the command; the tip only — the pool counts
    // are on the canvas
    const tip = screen.getByTestId('pipeline-tip');
    expect(head).toContainElement(tip);
    expect(tip.parentElement).toHaveClass('text-text-dim', 'font-normal', 'normal-case');
    expect(help.compareDocumentPosition(tip)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(tip).toHaveTextContent(new RegExp(`^--tip #${fmt(T)}$`));
    // Lane L: the tip is a block, so it links to it.
    expect(within(tip).getByRole('link', { name: `#${fmt(T)}` })).toHaveAttribute(
      'href',
      `/mainnet/blocks/${T}`
    );
    for (const gone of ['Tip', 'Mempool', 'Proposed', 'Next']) {
      expect(tip).not.toHaveTextContent(gone);
    }
  });

  it('writes --tip with a dash, never a number, until the first snapshot', async () => {
    server.use(http.get('/api/:network/v1/pipeline', () => new Promise<Response>(() => {})));
    render(<PipelinePanel />);
    expect(screen.getByTestId('pipeline-loading')).toBeInTheDocument();
    const tip = screen.getByTestId('pipeline-tip');
    expect(tip).toHaveTextContent(/^--tip —$/);
    expect(within(tip).queryByRole('link')).toBeNull();
  });

  it('links from the head’s right end, outside the command, to the list of all blocks', async () => {
    await renderPanel();
    const link = screen.getByRole('link', { name: 'all blocks →' });
    expect(link).toHaveAttribute('href', '/mainnet/blocks');
    expect(screen.queryByRole('link', { name: /All blocks/ })).toBeNull();
    // the only action: not inside the command line, and nothing else beside it
    const head = screen.getByText('pipeline').closest('.prompt') as HTMLElement;
    expect(head).not.toContainElement(link);
    const actions = link.parentElement as HTMLElement;
    expect(actions.children).toHaveLength(1);
    expect(actions.parentElement).toBe(head.parentElement);
  });

  // Direction F (PlateHome.dc.html): the pipeline is an unframed section like the others on the
  // home page; the legend a plain dim line under the canvas, with no rule over it.
  it('stands unframed like the other home sections: no frame, no ruled footer', async () => {
    await renderPanel();
    const section = screen.getByTestId('pipeline-panel');
    expect(section.tagName).toBe('SECTION');
    expect(section).toHaveAttribute('aria-label', 'Transaction pipeline');
    expect(section).not.toHaveAttribute('data-instrument-frame');
    expect(section.querySelector('[data-instrument-frame]')).toBeNull();
    const ruled = [section, ...section.querySelectorAll('*')].filter((el) =>
      [...el.classList].some((c) => /^border(-[tblrxy])?$/.test(c))
    );
    // the "?" mark is the one bordered thing: a 14 px box round its glyph
    expect(ruled.map((el) => el.textContent)).toEqual(['?']);
    const legend = screen.getByTestId('pipeline-legend');
    expect(legend).toHaveClass('text-text-dim');
    expect(screen.getByTestId('pipeline-canvas').compareDocumentPosition(legend)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING
    );
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

/**
 * What breaks the strap in the frame on screen, read from the DOM alone (the static render and the
 * motion layer alike), as one line per fault. The rule (direction G, the coordinator's ruling on
 * 26b7f8e3): the strap never shows a gap that is not a real parentHash break. Only the plates that
 * stay (the cards layer; a card leaving through the exit layer fades out over the strap and takes
 * no part in it) count: between two of them that the snapshot links, the whole gap is filled — a
 * link (solid or gilt) from a set plate, the dashed pair from one still landing — and no link lies
 * anywhere else (none across a parentHash break, none over a plate); the tail starts at the far
 * edge of the last of them; NEXT's dashed pair fills the gap to the first while NEXT shows; and a
 * plate carries its gilt exactly while it lies inside the window of `H`.
 */
function strapFaults(svg: Element, s: PipelineSnapshot, o: 'row' | 'column', H: number): string[] {
  const row = o === 'row';
  // a card's place is written to 2 decimals, a path's to 1, a path's far end as a start and a length
  const TOL = 0.11;
  const near = (p: number, q: number) => Math.abs(p - q) <= TOL;
  const along = (el: Element) => {
    const m = /translate\(([-\d.]+),([-\d.]+)\)/.exec(el.getAttribute('transform') ?? '');
    if (!m) throw new Error(`${el.outerHTML.slice(0, 60)} has no translation`);
    return Number(m[row ? 1 : 2]);
  };
  const length = (el: Element) => {
    if (row) return G.cardW;
    const box = el.querySelector('[data-box]') as Element;
    return Number(box.getAttribute('height')) + 2 * Number(box.getAttribute('y'));
  };
  const blocks = new Map(s.blocks.map((b) => [b.number, b]));
  const plates = [...svg.querySelectorAll('[data-layer="cards"] [data-card]')]
    .map((el) => ({
      el,
      n: Number(el.getAttribute('data-number')),
      a: along(el),
      b: along(el) + length(el),
      set:
        (el.querySelector('[data-box]') as SVGElement).style.getPropertyValue('fill') ===
        PLATE_FILL,
    }))
    .sort((p, q) => p.a - q.a);
  const span = (r: { x: number; y: number; w: number; h: number }): [number, number] =>
    row ? [r.x, r.x + r.w] : [r.y, r.y + r.h];
  const rectsOf = (names: string[]) =>
    names.flatMap((n) => strapRects(path(svg, n).getAttribute('d'))).map(span);
  const dashedOf = () => {
    const d = path(svg, 'pend').getAttribute('d') ?? '';
    const re = row ? /M([\d.]+) [\d.]+H([\d.]+)/g : /M[\d.]+ ([\d.]+)V([\d.]+)/g;
    const runs = [...d.matchAll(re)].map((m) => [Number(m[1]), Number(m[2])] as [number, number]);
    // the pair: two lines over one run
    return runs.filter((r, i) => runs.findIndex((q) => q[0] === r[0] && q[1] === r[1]) === i);
  };
  const faults: string[] = [];
  const links = rectsOf(['solid', 'win']);
  const dashed = dashedOf();
  const used = { links: new Set<number>(), dashed: new Set<number>() };
  const claim = (pool: [number, number][], seen: Set<number>, p0: number, p1: number) => {
    const i = pool.findIndex(([l0, l1], k) => !seen.has(k) && near(l0, p0) && near(l1, p1));
    if (i >= 0) seen.add(i);
    return i >= 0;
  };
  // NEXT ⇢ the first plate, while NEXT shows
  const next = svg.querySelector('[data-layer="cards"] [data-next]');
  if (next && plates.length) {
    const op = Number(next.getAttribute('opacity') ?? '1');
    const [p0, p1] = [along(next) + length(next), plates[0].a];
    const filled = claim(dashed, used.dashed, p0, p1);
    if (op >= 0.03 && p1 - p0 > TOL && !filled) {
      faults.push(`NEXT ${p0.toFixed(1)} ⇢ #${plates[0].n} ${p1.toFixed(1)}: no dashed pair`);
    }
  }
  // between two staying plates
  for (let i = 0; i + 1 < plates.length; i++) {
    const [p, q] = [plates[i], plates[i + 1]];
    const linked = blocks.get(p.n)?.parentHash === blocks.get(q.n)?.hash;
    if (!linked || q.a - p.b <= 0) continue;
    const filled = p.set
      ? claim(links, used.links, p.b, q.a)
      : claim(dashed, used.dashed, p.b, q.a);
    if (!filled && q.a - p.b > TOL) {
      faults.push(
        `#${p.n} ${p.b.toFixed(1)} → #${q.n} ${q.a.toFixed(1)}: no ${p.set ? 'link' : 'dashed pair'}`
      );
    }
  }
  links.forEach(([l0, l1], k) => {
    if (!used.links.has(k)) faults.push(`a link ${l0} … ${l1} across no gap between linked plates`);
  });
  dashed.forEach(([l0, l1], k) => {
    if (!used.dashed.has(k)) faults.push(`a dashed pair ${l0} … ${l1} across no gap`);
  });
  // the tail, from the far edge of the last staying plate
  const last = plates[plates.length - 1];
  const tails = rectsOf(['tail', 'tail-win']);
  if (tails.length !== 1) faults.push(`${tails.length} tails after #${last.n}`);
  else if (!near(tails[0][0], last.b)) {
    faults.push(`the tail starts at ${tails[0][0]}, #${last.n} ends at ${last.b.toFixed(2)}`);
  }
  // a staying plate's own gilt, exactly while it lies inside the window and its plate is set
  for (const p of plates) {
    const strap = p.el.querySelector('[data-strap]');
    const shown = strap !== null && strap.getAttribute('opacity') !== '0';
    if (shown !== (p.set && inWindow(p.n, H, CONSENSUS))) {
      faults.push(`#${p.n}: gilt ${shown ? 'outside' : 'missing inside'} the window of #${H}`);
    }
  }
  return faults;
}

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
    // Dashed like NEXT until its commits land.
    expect(landing.querySelector('[data-box]')).toHaveAttribute('stroke-dasharray', '2 2');
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

  // Lane K: a block still landing is a mined block, so a click on it opens its page.
  it('opens the page of a block that is still landing', () => {
    const rect = vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
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
    try {
      const routed = (s: PipelineSnapshot) => (
        <MemoryRouter initialEntries={['/mainnet']}>
          <RouterLocation />
          <PipelineCanvas snapshot={s} />
        </MemoryRouter>
      );
      const { getByTestId, rerender } = renderPlain(routed(s1));
      rerender(routed(s2));
      const svg = getByTestId('pipeline-svg');
      expect(svg.querySelector(`[data-card="${newHash}"]`)).toHaveAttribute('data-slot', 'landing');
      fireEvent.click(landingBox(svg), bareSpot(buildScene(s2, VIEW_W, 'landing'), T + 1));
      expect(routerLocation()).toBe(`/mainnet/blocks/${T + 1}`);
    } finally {
      rect.mockRestore();
    }
  });

  // Direction F (Plate.dc.html, "a block lands"): until its commits land the block is NEXT's
  // dashed outline; at contact its plate is set, with its cuts and its stone (re-pinned from the
  // dashed tile that solidified and the 2.4 s fresh outline, which are gone).
  it('keeps a landing block NEXT’s dashed outline until contact, then sets its plate, cuts and stone', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    // Settled plates, as the motion layer paints them: their own tone, no stroke, exact.
    expect(tile(svg, T).style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(tile(svg, T).style.getPropertyValue('stroke')).toBe('none');
    expect(rectOf(tile(svg, T))).toEqual(asRect(cardBox('row', G.cardW, CARD_H, false)));
    rerender(<PipelineCanvas snapshot={s2} />);
    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    const box = landingBox(svg);
    const hidden = () =>
      ['[data-stone]', '[data-cuts]'].map((sel) =>
        landing.querySelector(sel)?.getAttribute('opacity')
      );
    const dashed = () => {
      expect(box.style.getPropertyValue('fill')).toBe('none');
      expect(box.style.getPropertyValue('stroke')).toBe(tok('jade'));
      expect(box).toHaveAttribute('stroke-opacity', '0.6');
      expect(box).toHaveAttribute('stroke-dasharray', '2 2');
      expect(rectOf(box)).toEqual(asRect(cardBox('row', G.cardW, CARD_H, true)));
      expect(hidden()).toEqual(['0', '0']);
    };
    dashed();
    at(t.contact - 20);
    dashed();
    // Contact: the plate is set, its cuts and its stone with it.
    at(40);
    expect(box.style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(box.style.getPropertyValue('stroke')).toBe('none');
    expect(box).not.toHaveAttribute('stroke-dasharray');
    expect(box).toHaveAttribute('fill-opacity', '1');
    expect(rectOf(box)).toEqual(asRect(cardBox('row', G.cardW, CARD_H, false)));
    expect(hidden()).toEqual([null, null]);
    // and stays set while it settles and slides into the chain
    at(t.settleAt - t.contact + 900);
    expect(box.style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(box).not.toHaveAttribute('stroke-dasharray');
  });

  // Ruling on lane F: a landing block that hides 100 txs or more gets its plate without a stone.
  it('sets a landing block hiding 100 txs or more as a plate with no stone, its number from x 8', () => {
    const big: PipelineSnapshot = {
      ...s2,
      blocks: s2.blocks.map((b) =>
        b.number === T + 1 ? { ...b, txCount: b.txCount + 980, omittedTxs: b.omittedTxs + 980 } : b
      ),
    };
    expect(buildScene(big, VIEW_W, 'landing').cards[0]).toMatchObject({
      id: newHash,
      stone: false,
    });
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={big} />);
    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    const head = () => [
      landing.querySelector('[data-stone]'),
      Number(landing.querySelector('[data-block-number]')?.getAttribute('x')),
    ];
    expect(head()).toEqual([null, 8]);
    // contact: the plate is set, its cuts with it, and still no stone
    at(t.contact + 20);
    expect(landingBox(svg).style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(landing.querySelector('[data-cuts]')).not.toHaveAttribute('opacity');
    expect(head()).toEqual([null, 8]);
    // and none once it has joined the chain
    at(t.settleAt - t.contact);
    at(800);
    const joined = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    expect(joined).toHaveAttribute('data-slot', 'chain');
    expect(joined.querySelector('[data-stone]')).toBeNull();
    expect(Number(joined.querySelector('[data-block-number]')?.getAttribute('x'))).toBe(8);
  });

  // Direction F (Plate.dc.html, "found" → "400 ms"): the landed block's marks light in the
  // phosphor white at contact and settle to their own fill over 400 ms (re-pinned from the fresh
  // outline's 2.4 s decay).
  it('flares the landing block’s marks neon-white at contact and settles them to their own fill in 400 ms', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    const landed = buildScene(s2, VIEW_W, 'landing').cards[0];
    const marks = landed.squares.map((q) => ({
      el: svg.querySelector(`[data-card="${newHash}"] [data-sq="${q.id}"]`) as SVGElement,
      own: agedPaint(
        paintSquare(q.stage, q.style, q.step),
        q.kind === 'cellbase' ? 'cellbase' : 'square',
        0
      ).fill,
    }));
    expect(marks.map((m) => m.el.getAttribute('data-sq'))).toContain(w0);
    const fills = () => marks.map((m) => m.el.style.getPropertyValue('fill'));
    const NEON = color('neon-white');
    // The commit is in flight and the cellbase unborn: nothing is lit before contact.
    at(t.contact - 20);
    expect(fills().filter((f) => f === NEON)).toEqual([]);
    // Frame by frame to the first frame at contact: every mark, the cellbase too, is the neon white.
    let ms = t.contact - 20;
    while (!fills().includes(NEON)) {
      at(16);
      ms += 16;
      expect(ms).toBeLessThan(t.contact + 40);
    }
    expect(fills()).toEqual(marks.map(() => NEON));
    // On the way back: lit, toward its own fill.
    at(FRESH_MS / 2);
    for (const [i, f] of fills().entries()) {
      expect(f).not.toBe(NEON);
      expect(f).toMatch(new RegExp(`^color-mix\\(in srgb, ${NEON.replace(/[()]/g, '\\$&')} `));
      expect(f).toContain(marks[i].own);
    }
    // 400 ms after contact: every mark its own fill.
    expect(FRESH_MS).toBe(400);
    at(FRESH_MS / 2 + 16);
    expect(fills()).toEqual(marks.map((m) => m.own));
  });

  // Direction F (lane L's cuts, lane R): a moving square carries its cut, so no cut ever stands
  // empty and no square stands on bare plate.
  it('carries every square’s cut with it while it moves into the landing block', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    const sq = landing.querySelector(`[data-sq="${w0}"]`) as SVGRectElement;
    const cut = landing.querySelector(`[data-cut="${w0}"]`) as SVGRectElement;
    const around = () => {
      const [x, y, w, h] = rectOf(sq);
      const want = [x - GUT, y - GUT, w + 2 * GUT, h + 2 * GUT];
      rectOf(cut).forEach((v, i) => expect(v).toBeCloseTo(want[i], 1));
      expect(cut.getAttribute('opacity')).toBe(sq.getAttribute('opacity'));
    };
    for (let ms = 0; ms < t.contact + 100; ms += 16) {
      at(16);
      around();
    }
    // landed: the cut is where the scene puts it
    const q = buildScene(s2, VIEW_W, 'landing').cards[0].cuts.find((c) => c.id === w0);
    expect(rectOf(cut)).toEqual([q?.x, q?.y, q?.s, q?.s]);
  });

  // Lane E2 follow-up, item 3: the chips' "+N" left `data-chips` for the card's words; an entrance
  // is motion, not a resting state, so it enters with its dots (the same opacity, step for step)
  // and stands at full strength once the block is solid.
  it('lets a landing block’s chip count enter with its dots, then stand at full strength', () => {
    const pool = s1.pool as NonNullable<PipelineSnapshot['pool']>;
    const extra = Array.from({ length: 40 }, (_, i) => ({
      ...pool.pending[0],
      hash: poolTxHash(`x${i}`),
    }));
    const many: PipelineSnapshot = {
      ...s1,
      pool: {
        ...pool,
        pending: [...pool.pending, ...extra],
        pendingTotal: pool.pendingTotal + extra.length,
      },
    };
    const landed = mineNext(many, { commit: [w0], propose: extra.map((x) => x.hash) });
    const tr = diffSnapshots(many, landed);
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={many} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={landed} />);
    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    const chips = landing.querySelector('[data-chips]') as SVGGElement;
    const label = landing.querySelector('[data-chips-label]') as SVGGElement;
    expect(label).not.toBeNull();
    // Dashed: the dots at 0.2, and their count with them.
    expect(chips).toHaveAttribute('opacity', '0.200');
    expect(label).toHaveAttribute('opacity', '0.200');
    // Solidifying (from contact − 80 ms over 420 ms): the count follows the dots.
    at(tr.contact + 100);
    const mid = Number(chips.getAttribute('opacity'));
    expect(mid).toBeGreaterThan(0.2);
    expect(mid).toBeLessThan(1);
    expect(label.getAttribute('opacity')).toBe(chips.getAttribute('opacity'));
    // Solid: both at rest, at full strength.
    at(600);
    expect(chips).not.toHaveAttribute('opacity');
    expect(label).not.toHaveAttribute('opacity');
  });

  // Re-pinned with lane R: the hover's jade outline on the tile became a 1 px text-dim edge on the
  // plate, on the plate's own outer pixels.
  it('edges a highlighted plate in the dim ink while the motion layer is painting it', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    at(t.contact + 100);
    // Hovered (here from the Latest Blocks row) while it is still landing and its marks still lit.
    act(() => usePipelineHoverStore.getState().setHover({ block: T + 1, tx: null, window: null }));
    at(40);
    const box = landingBox(svg);
    expect(box.style.getPropertyValue('stroke')).toBe(color('text-dim'));
    expect(box).toHaveAttribute('stroke-width', '1');
    expect(box).not.toHaveAttribute('stroke-dasharray');
    expect(box.style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(rectOf(box)).toEqual(asRect(cardBox('row', G.cardW, CARD_H, true)));
    act(() => usePipelineHoverStore.getState().clearHover());
    at(FRESH_MS + 800);
    expect(box.style.getPropertyValue('stroke')).toBe('none');
    expect(rectOf(box)).toEqual(asRect(cardBox('row', G.cardW, CARD_H, false)));
  });

  // Measured in Chrome on 26b7f8e3: while the chain slid after a landing, the gilt strap broke at
  // its far end for about 400 ms (frames 7–11 of land-right.png). The block leaving the screen
  // still counted: the tail started past its right edge while it faded, and its link to the block
  // sliding into its slot had no extent. A leaving block takes no part in the strap; the tail
  // starts at the last plate that stays and moves with it. Sampled frame by frame through the
  // landing and the slide, at rest and with a block pointed at (its window moves the gilt).
  it.each([null, T - 3])(
    'keeps the strap whole through a landing and the slide, a block leaving at the far end (pointing at %s)',
    (pointing) => {
      viewW = CONTAINER_VIEW_W;
      const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
      const svg = getByTestId('pipeline-svg');
      if (pointing !== null) {
        act(() =>
          usePipelineHoverStore.getState().setHover({ block: pointing, tx: null, window: pointing })
        );
      }
      // seven plates on screen before and after: T−6 leaves while T+1 … T−5 slide one slot right
      expect(buildScene(s1, CONTAINER_VIEW_W, 'settled').cards.map((c) => c.block.number)).toEqual(
        Array.from({ length: 7 }, (_, i) => T - i)
      );
      rerender(<PipelineCanvas snapshot={s2} />);
      const exitLayer = svg.querySelector('[data-layer="exit"]') as SVGGElement;
      const faults: string[] = [];
      let leaving = 0;
      let nearEnd = 0;
      for (let ms = 16; ms <= t.settleAt + 1000; ms += 16) {
        at(16);
        const landed = svg.querySelector(`[data-card="${newHash}"]`);
        const H = pointing ?? (landed?.getAttribute('data-slot') === 'landing' ? T + 1 : T + 2);
        if (exitLayer.querySelector('g')) leaving++;
        // the near end: the new NEXT showing while the landed block slides off its slot
        if (path(svg, 'pend').getAttribute('d') !== 'M0 0') {
          if (landed?.getAttribute('data-slot') === 'chain') nearEnd++;
        }
        faults.push(...strapFaults(svg, s2, 'row', H).map((f) => `${ms} ms: ${f}`));
      }
      // the slide was sampled while T−6 left (600 ms of it), and while NEXT's dashed pair ran to
      // the landed block sliding into the tip's slot
      expect(leaving).toBeGreaterThan(30);
      expect(nearEnd).toBeGreaterThan(10);
      expect(faults).toEqual([]);
    }
  );

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

  it('retires and brings back whole blocks as the view is resized between wide and narrow', () => {
    viewW = 1000;
    const { getByTestId } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    const cardsLayer = svg.querySelector('[data-layer="cards"]') as SVGGElement;
    const exitLayer = svg.querySelector('[data-layer="exit"]') as SVGGElement;
    const shown = () =>
      [...cardsLayer.querySelectorAll('[data-card]')].map((el) =>
        Number(el.getAttribute('data-number'))
      );
    const cardX = (el: Element) =>
      Number(/translate\(([\d.]+),/.exec(el.getAttribute('transform') ?? '')?.[1]);
    // NEXT's window is drawn (hovered from outside the canvas) the whole time.
    act(() => usePipelineHoverStore.getState().setHover({ block: null, tx: null, window: T + 1 }));
    /** At rest at `width`: only whole blocks, and the chain and label drawn for them. */
    const expectRow = (width: number) => {
      const scene = buildScene(s1, width, 'settled');
      expect(svg.getAttribute('viewBox')).toBe(`0 0 ${width} ${G.H}`);
      expect(shown()).toEqual(scene.cards.map((c) => c.block.number));
      for (const el of cardsLayer.querySelectorAll('[data-card]')) {
        expect(cardX(el) + G.cardW).toBeLessThanOrEqual(width - G.marginR);
      }
      expect(exitLayer.childElementCount).toBe(0);
      // the tail: the strap runs on from the last plate's edge for at most 66 px
      const last = scene.cards[scene.cards.length - 1];
      const tail = chainTail(scene.cards, width);
      expect(tail?.x0).toBe(last.x + last.w);
      expect(svg.querySelector('[data-window-marks]')).toBeNull();
      // The window of NEXT runs past the last block at every one of these widths: a gilt tail.
      expect(path(svg, 'tail-win')).toHaveAttribute('d', rowChain(scene, T + 1).tailWin);
      expect(strapRects(path(svg, 'tail-win').getAttribute('d'))).toHaveLength(1);
      expect(path(svg, 'tail')).toHaveAttribute('d', 'M0 0');
      expectLabelClear(svg, width, T + 1);
    };
    expectRow(1000);
    expect(shown()).toHaveLength(5);

    // Narrower: the two blocks that no longer fit whole retire through the exit layer.
    resizeTo(760);
    expect(shown()).toHaveLength(3);
    expect(exitLayer.querySelectorAll('g').length).toBeGreaterThanOrEqual(2);
    for (let ms = 0; ms < 700; ms += 16) {
      at(16);
      expectLabelClear(svg, 760, T + 1);
    }
    expectRow(760);

    // Wider again: the blocks that fit now fade in from nothing (seven whole plates at 1246 px).
    resizeTo(CONTAINER_VIEW_W);
    expect(shown()).toHaveLength(7);
    expect(card(svg, T - 5)).toHaveAttribute('opacity', '0.000');
    for (let ms = 0; ms < 400; ms += 16) {
      at(16);
      expectLabelClear(svg, CONTAINER_VIEW_W, T + 1);
    }
    expect(card(svg, T - 5)).toHaveAttribute('opacity', '1.000');
    expectRow(CONTAINER_VIEW_W);
  });

  it('keeps the landing cellbase and its cut invisible until it is born at contact', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    const cb = svg.querySelector(`[data-sq="${cellbaseHash(T + 1)}"]`) as SVGPathElement;
    const cut = svg.querySelector(`[data-cut="${cellbaseHash(T + 1)}"]`) as SVGRectElement;
    expect(cb).toHaveAttribute('opacity', '0.000');
    expect(cut).toHaveAttribute('opacity', '0.000');
    at(t.contact - 20);
    expect(cb).toHaveAttribute('opacity', '0.000');
    expect(cut).toHaveAttribute('opacity', '0.000');
    at(20 + 540);
    expect(cb).toHaveAttribute('opacity', '1.000');
    expect(cut).toHaveAttribute('opacity', '1.000');
    // born to its cell: the cut round it is the cell grown by the gutter
    expect(['x', 'width'].map((a) => Number(cut.getAttribute(a)))).toEqual([
      G.padX - GUT,
      side(1) + 2 * GUT,
    ]);
  });

  it('completes the landing beat at the NEXT position before the block joins the chain', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    at(t.settleAt - 10);
    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    expect(landing).toHaveAttribute('data-slot', 'landing');
    expect(landing.getAttribute('transform')).toBe(`translate(${G.nextX.toFixed(2)},0.00)`);
    // Its plate set, its cellbase at full size, the ripples of the landing gone.
    expect(landingBox(svg)).not.toHaveAttribute('stroke-dasharray');
    expect(landingBox(svg)).toHaveAttribute('fill-opacity', '1');
    expect(landingBox(svg).style.getPropertyValue('fill')).toBe(PLATE_FILL);
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

  // Direction G: the tail is the strap run on, gilt while the window runs past the last plate
  // (`tail-win`), the plate's tone otherwise (`tail`); the motion layer redraws both every frame.
  it.each([
    { width: CONTAINER_VIEW_W, on: 'tail-win', off: 'tail', fill: GILT_FILL },
    { width: VIEW_W, on: 'tail', off: 'tail-win', fill: PLATE_FILL },
  ] as const)(
    'redraws the $on from the moving cards at a $width px view',
    ({ width, on, off, fill }) => {
      // At 1246 px the window of NEXT runs past the last block on screen; at 1600 it ends on it.
      viewW = width;
      const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
      const svg = getByTestId('pipeline-svg');
      expect(strapRects(path(svg, on).getAttribute('d'))).toHaveLength(1);
      expect(path(svg, off)).toHaveAttribute('d', 'M0 0');
      rerender(<PipelineCanvas snapshot={s2} />);
      // Spoil what React drew: only the motion layer can redraw it now (React renders nothing new).
      path(svg, on).setAttribute('d', 'M0 0');
      path(svg, off).setAttribute('d', 'M1.0 1.0h1.0v1.0h-1.0Z');
      at(100);
      expect(strapRects(path(svg, on).getAttribute('d'))).toHaveLength(1);
      expect(path(svg, off)).toHaveAttribute('d', 'M0 0');
      expect(path(svg, on).style.getPropertyValue('fill')).toBe(fill);
      expect(path(svg, on).style.getPropertyValue('stroke')).toBe('');
    }
  );

  // Direction G: a block still landing has no plate yet, so its link to the tip is the dashed
  // pair, as NEXT's was, until contact; then the strap joins them in the plate's tone.
  it('dashes a landing block’s strap to the tip until contact, then joins them in the plate’s tone', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    // the landing block stands at NEXT (246 … 345), the tip at 363
    const dashed = 'M345.0 26.5H363.0M345.0 45.5H363.0';
    expect(path(svg, 'pend')).toHaveAttribute('d', dashed);
    at(t.contact - 40);
    expect(path(svg, 'pend')).toHaveAttribute('d', dashed);
    expect(strapRects(path(svg, 'solid').getAttribute('d')).map((r) => r.x)).not.toContain(345);
    // contact: the plate is set, and its strap with it
    at(60);
    expect(path(svg, 'pend')).toHaveAttribute('d', 'M0 0');
    expect(strapRects(path(svg, 'solid').getAttribute('d'))[0]).toEqual({
      x: 345,
      y: 26,
      w: 18,
      h: 20,
    });
  });

  // Direction G: a block still landing has no plate yet, so no gilt is laid on it until its plate
  // is set, even inside a drawn window (here that of T+5, T−5 … T+3, hovered from outside).
  it('lays no gilt on a landing block until its plate is set at contact', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    act(() => usePipelineHoverStore.getState().setHover({ block: null, tx: null, window: T + 5 }));
    const strap = svg.querySelector(`[data-card="${newHash}"] [data-strap]`) as SVGRectElement;
    expect(strap).not.toBeNull();
    expect(strap).toHaveAttribute('opacity', '0');
    at(t.contact - 20);
    expect(strap).toHaveAttribute('opacity', '0');
    at(40);
    expect(strap).not.toHaveAttribute('opacity');
    expect(strap.style.getPropertyValue('fill')).toBe(GILT_FILL);
  });

  it('keeps a hovered window label inside the view and on its first window plate while the cards move', () => {
    viewW = CONTAINER_VIEW_W;
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    // Hovered from the Latest Blocks row: the window of T−3 starts near the right edge (seven
    // whole plates at 1246 px: its first plate, T−5, is the second from it).
    act(() => usePipelineHoverStore.getState().setHover({ block: T - 3, tx: null, window: T - 3 }));
    expectLabelClear(svg, CONTAINER_VIEW_W, T - 3);
    rerender(<PipelineCanvas snapshot={s2} />);
    // Through the landing and the 700 ms move one slot to the right, frame by frame.
    for (let ms = 0; ms < t.settleAt + 900; ms += 16) {
      at(16);
      expectLabelClear(svg, CONTAINER_VIEW_W, T - 3);
    }
    expect(svg.querySelector(`[data-card="${newHash}"]`)).toHaveAttribute('data-slot', 'chain');
  });

  it.each([
    { width: CONTAINER_VIEW_W, where: 'past the right edge' },
    { width: 2000, where: 'on screen' },
  ])('draws no reward link when a block lands, the paid block $where', ({ width }) => {
    viewW = width;
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    const fx = svg.querySelector('[data-layer="fx"]') as SVGGElement;
    // The landing block's cellbase pays tip − 10: on screen at 2000 px, past the edge at 1246.
    const pays = s2.blocks[0].cellbase.pays;
    expect(pays?.number).toBe(T - 10);
    const shown = buildScene(s2, width, 'landing').cards.some((c) => c.block.number === T - 10);
    expect(shown).toBe(width === 2000);
    rerender(<PipelineCanvas snapshot={s2} />);
    const payee = card(svg, T - 10);
    let ripples = 0;
    // Through the landing beat, the move into the chain and well past where a link would fade.
    for (let ms = 0; ms < t.settleAt + 3000; ms += 16) {
      at(16);
      // Only ripples play in the effects layer: no line, no tail, nothing in the reward lane.
      expect([...fx.children].map((el) => el.tagName).filter((tag) => tag !== 'circle')).toEqual(
        []
      );
      expect(svg.querySelector('[data-hover="reward"]')).toBeNull();
      ripples += fx.querySelectorAll('circle').length;
      // And none of them on the paid cellbase (its cell's centre on the plate).
      if (payee?.isConnected) {
        const x = Number(/translate\(([\d.]+),/.exec(payee.getAttribute('transform') ?? '')?.[1]);
        for (const r of fx.querySelectorAll('circle')) {
          expect(
            Math.abs(Number(r.getAttribute('cx')) - (x + G.padX + side(1) / 2))
          ).toBeGreaterThan(1);
        }
      }
    }
    // The landing itself still plays.
    expect(ripples).toBeGreaterThan(0);
    expect(svg.querySelector(`[data-card="${newHash}"]`)).toHaveAttribute('data-slot', 'chain');
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
    // T−1 stands two places behind the landing block: its dots step back by their fill
    const age = buildScene(s2, VIEW_W, 'landing').cards.findIndex((c) => c.block.number === T - 1);
    expect(age).toBe(2);
    expect(card(svg, T - 1)).toHaveAttribute('data-age', '2');
    expect(chip.style.getPropertyValue('fill')).toBe(agedChip('waiting', age).fill);
    at(t.contact + 20);
    expect(chip.style.getPropertyValue('fill')).toBe(agedChip('committed', age).fill);
  });

  it('shows the final state at once with reduced motion: no landing, no flare', () => {
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
    // its plate set and its stone pressed at once, and no mark ever lit
    const box = card.querySelector('[data-box]') as SVGRectElement;
    expect(box.style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(box).not.toHaveAttribute('stroke-dasharray');
    expect(card.querySelector('[data-stone]')).not.toHaveAttribute('opacity');
    expect(card.querySelector('[data-cuts]')).not.toHaveAttribute('opacity');
    for (const q of buildScene(s2, VIEW_W, 'settled').cards[0].squares) {
      const el = card.querySelector(`[data-sq="${q.id}"]`) as SVGElement;
      expect(el.style.getPropertyValue('fill')).toBe(
        agedPaint(
          paintSquare(q.stage, q.style, q.step),
          q.kind === 'cellbase' ? 'cellbase' : 'square',
          0
        ).fill
      );
    }
    expect(svg.innerHTML).not.toContain(color('neon-white'));
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
      value: PointerEventStub,
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
  // re-pinned with lane E2 (R4): the hover's step-back group now holds the age group and the words
  const dimOfCard = (svg: Element, n: number) =>
    card(svg, n).querySelector(':scope > [data-dim]')?.getAttribute('data-dim');
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
    // Its halo: its stage colour toward the theme's step-to ink (lighter by night, inkier by
    // day), never toward a literal white.
    const halo = svg.querySelector('[data-hover="halo"]') as SVGRectElement;
    expect(halo.style.getPropertyValue('stroke')).toBe(
      mixColor(STAGE_BASE.committed, tok('step-to'), 0.4)
    );
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

  // Re-pinned with lane R: an uncle has no box; its word is what the pointer reads it by.
  it('explains an uncle by its word without moving the window', async () => {
    const svg = await renderPanel(snapshot);
    const c = cardOf(T - 1);
    const u = c.uncles[0];
    const word = card(svg, T - 1).querySelector('[data-uncle="0"] text') as SVGTextElement;
    expect(word.textContent).toBe('uncle');
    fireEvent.pointerMove(word, { clientX: c.x + u.x + 10, clientY: u.y + 6 });
    expect(tooltip()).toHaveTextContent('not on the chain');
    // The plate only says "uncle"; its number is here.
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
    const lane = windowLane(
      windowSpans(scene),
      T + 1,
      snapshot.consensus,
      VIEW_W,
      windowLabel(T + 1, T, snapshot.consensus)
    );
    // the lane runs over the window's plates, from the first one's left edge (T−1 at 480)
    expect(lane.hit?.xn).toBe(480);
    fireEvent.pointerMove(svg, {
      clientX: (lane.hit?.xn as number) + 10,
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
    // The hovered lane draws the window's one line of words, and no tick or line (direction G);
    // the gilt is as it is at rest.
    expect(svg.querySelector('[data-window-label]')).not.toBeNull();
    expect(svg.querySelector('[data-ruler-ticks]')).toBeNull();
    expect(path(svg, 'win').style.getPropertyValue('fill')).toBe(GILT_FILL);
    expect(path(svg, 'win')).not.toHaveAttribute('fill-opacity');
    expect(path(svg, 'win')).not.toHaveAttribute('stroke-width');
    // The head line between two of the window's plates is the lane too: T−1 ends at 579.
    fireEvent.pointerLeave(svg);
    fireEvent.pointerMove(svg, { clientX: 579 + 9, clientY: G.headY });
    expect(tooltip()).toHaveTextContent('Proposal window');
    expect(svg.style.cursor).toBe('help');
  });

  // Direction G: under the pointer no tick and no dimension line; the window's one line of words
  // starts at the left edge of its first plate in view (T−1 at 480), start-anchored, and a number
  // inside the window keeps its bright ink (the gilt says it).
  it('draws the window of NEXT while NEXT is hovered: its words from its first plate, no tick, no line', async () => {
    const svg = await renderPanel(snapshot);
    hoverNext(svg);
    expect(tooltip()).toHaveTextContent(`Next #${fmt(T + 1)}`);
    const text = windowLabel(T + 1, T, snapshot.consensus);
    expect(text).toBe('next block can commit txs proposed in these 9 blocks');
    const label = svg.querySelector('[data-window-label]') as SVGTextElement;
    expect(label.textContent).toBe(text);
    // One placement for the static render and the motion layer.
    const lane = windowLane(windowSpans(scene), T + 1, snapshot.consensus, VIEW_W, text);
    expect(lane.label).toEqual({ x0: 480, x1: 480 + labelWidth(text) });
    expect(label).toHaveAttribute('x', '480.0');
    expect(label).toHaveAttribute('y', String(G.winY + 3.5));
    expect(label).toHaveAttribute('text-anchor', 'start');
    // the lane's emphasis: the bright ink while the lane, NEXT or the proposed well is hovered
    expect(label).toHaveClass('fill-text-bright', 'text-meta');
    expectLabelClear(svg, VIEW_W, T + 1);
    for (const c of scene.cards) {
      const n = c.block.number;
      const number = card(svg, n).querySelector('[data-block-number]');
      const inside = inWindow(n, T + 1, snapshot.consensus);
      expect(number).toHaveClass('fill-text-bright');
      expect(card(svg, n).querySelector('[data-strap]') !== null, `#${n}`).toBe(inside);
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
    // re-pinned with direction G: the gilt across T−1's head, none across the tip's, and both
    // numbers in the bright ink
    expect(card(svg, T - 1).querySelector('[data-strap]')).not.toBeNull();
    expect(card(svg, T).querySelector('[data-strap]')).toBeNull();
    for (const n of [T, T - 1]) {
      expect(card(svg, n).querySelector('[data-block-number]')).toHaveClass('fill-text-bright');
    }
  });

  it('anchors the window on a hovered block and steps back the blocks outside it', async () => {
    const svg = await renderPanel(snapshot);
    const c = cardOf(T - 3);
    const box = card(svg, T - 3).querySelector('[data-box]') as SVGRectElement;
    fireEvent.pointerMove(box, { clientX: c.x + c.w / 2, clientY: G.cardTop + 5 });
    expect(tooltip()).toHaveTextContent(`Block #${fmt(T - 3)}`);
    // The hovered plate gets its edge: 1 px of the dim ink on its own outer pixels.
    expect(box.style.getPropertyValue('stroke')).toBe(color('text-dim'));
    expect(['x', 'y', 'width', 'height'].map((a) => Number(box.getAttribute(a)))).toEqual([
      0.5,
      G.cardTop + 0.5,
      G.cardW - 1,
      CARD_H - 1,
    ]);
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
    // The gilt moves to its window (T−13 … T−5; T−5 … T−9 on screen), and its words start at its
    // first plate, T−5 at 948, in the gold of the window.
    for (const c of scene.cards) {
      const inside = inWindow(c.block.number, T - 3, snapshot.consensus);
      expect(card(svg, c.block.number).querySelector('[data-strap]') !== null).toBe(inside);
    }
    const label = svg.querySelector('[data-window-label]') as SVGTextElement;
    expect(label).toHaveAttribute('x', '948.0');
    expect(label).toHaveClass('fill-gold');
  });

  // The blocks whose windows start nearest the right edge of the 1280 px container: seven whole
  // plates (T … T−6) put the first plate of T−4's window last on screen; T−3's and T−4's words end
  // at the view's edge (922.6 … 1246), T−2's start at T−4 (831).
  it.each([T - 2, T - 3, T - 4])(
    'keeps the window label of #%i inside the 1280 px container, from its first window plate',
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
      expectLabelClear(svg, CONTAINER_VIEW_W, n);
    }
  );

  it.each([
    { width: 606, blocks: 2 },
    { width: 760, blocks: 3 },
    { width: 1000, blocks: 5 },
    { width: CONTAINER_VIEW_W, blocks: 7 },
  ])(
    'shows $blocks whole blocks at $width px, its tail after the last one, every window label in view',
    async ({ width, blocks }) => {
      viewW = width;
      vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
        x: 0,
        y: 0,
        left: 0,
        top: 0,
        width,
        height: G.H,
        right: width,
        bottom: G.H,
        toJSON: () => ({}),
      } as DOMRect);
      const svg = await renderPanel(snapshot);
      const fit = buildScene(snapshot, width, 'settled');
      expect(svg.getAttribute('viewBox')).toBe(`0 0 ${width} ${G.H}`);
      expect(svg.querySelectorAll('[data-card]')).toHaveLength(blocks);
      expect(fit.cards.map((c) => c.block.number)).toEqual(
        Array.from({ length: blocks }, (_, i) => T - i)
      );
      for (const c of fit.cards) {
        expect(card(svg, c.block.number)).toHaveAttribute(
          'transform',
          `translate(${c.x.toFixed(2)},0.00)`
        );
        expect(c.x + c.w).toBeLessThanOrEqual(width - G.marginR);
      }
      const last = fit.cards[blocks - 1];
      // the tail, from the last plate's edge, inside the view: gilt, NEXT's window runs on past it
      const tail = chainTail(fit.cards, width);
      expect(tail?.x0).toBe(last.x + last.w);
      expect(tail?.x1).toBeLessThanOrEqual(width);
      expect(path(svg, 'tail-win')).toHaveAttribute('d', rowChain(fit, T + 1).tailWin);
      expect(path(svg, 'tail')).toHaveAttribute('d', 'M0 0');
      // NEXT, then every block: its window's label stays in view from its first plate, or, when its
      // near end is past the last block, no words are drawn at all.
      hoverNext(svg);
      expectLabelClear(svg, width, T + 1);
      for (const c of fit.cards) {
        const n = c.block.number;
        fireEvent.pointerMove(card(svg, n).querySelector('[data-box]') as Element, {
          clientX: c.x + c.w / 2,
          clientY: G.cardTop + 5,
        });
        expect(tooltip()).toHaveTextContent(`Block #${fmt(n)}`);
        if (n - snapshot.consensus.proposalWindowClosest < last.block.number) {
          expect(svg.querySelector('[data-window-label]')).toBeNull();
        } else {
          expectLabelClear(svg, width, n);
        }
        expect(svg.querySelector('[data-ruler-ticks]')).toBeNull();
      }
    }
  );

  it('gilds the tail while the drawn window runs off the screen', async () => {
    const svg = await renderPanel(snapshot);
    // at rest NEXT's window ends on T−9, the last plate: the tail is the plate's tone
    const plain = path(svg, 'tail').getAttribute('d');
    expect(strapRects(plain)).toHaveLength(1);
    expect(path(svg, 'tail-win')).toHaveAttribute('d', 'M0 0');
    // The window of T−3 is T−13 … T−5; the screen ends at T−9, inside it.
    expect(scene.cards[scene.cards.length - 1].block.number).toBe(T - 9);
    const c = cardOf(T - 3);
    fireEvent.pointerMove(card(svg, T - 3).querySelector('[data-box]') as Element, {
      clientX: c.x + c.w / 2,
      clientY: G.cardTop + 5,
    });
    expect(path(svg, 'tail-win')).toHaveAttribute('d', plain);
    expect(path(svg, 'tail')).toHaveAttribute('d', 'M0 0');
    expect(path(svg, 'tail-win').style.getPropertyValue('fill')).toBe(GILT_FILL);
    expect(path(svg, 'tail-win')).not.toHaveAttribute('fill-opacity');
    fireEvent.pointerLeave(svg);
    expect(path(svg, 'tail')).toHaveAttribute('d', plain);
    expect(path(svg, 'tail-win')).toHaveAttribute('d', 'M0 0');
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
    // The 1280 px container shows tip … tip − 6: the block the tip's cellbase pays (tip − 11) is
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
    expect(fit.cards).toHaveLength(7);
    const c = fit.cards[0];
    const last = fit.cards[fit.cards.length - 1];
    const cb = c.squares[0];
    expect(cb.kind).toBe('cellbase');
    fireEvent.pointerMove(svg, { clientX: c.x + cb.x + cb.s / 2, clientY: cb.y + cb.s / 2 });
    expect(tooltip()).toHaveTextContent(`pays reward of#${fmt(T - 11)}`);
    const reward = svg.querySelector('[data-hover="reward"]') as SVGPathElement;
    expect(reward).not.toBeNull();
    // Up from the cellbase into the lane, then right to where the chain's own tail starts (the
    // last plate's edge); it never drops anywhere.
    const tailX0 = last.x + last.w;
    const f = (v: number) => v.toFixed(1);
    const ax = c.x + cb.x + cb.s / 2;
    expect(reward).toHaveAttribute(
      'd',
      `M${f(ax)} ${f(cb.y + cb.s / 2)}V${G.winY + 5}Q${f(ax)} ${G.winY} ${f(ax + 5)} ${G.winY}H${f(tailX0)}`
    );
    // The open end: dotted, over the span of the chain's own tail (the strap run on, 66 px at
    // most), one lane up.
    const span = chainTail(fit.cards, CONTAINER_VIEW_W);
    expect(span).toEqual({ x0: tailX0, x1: tailX0 + G.tail });
    const tail = svg.querySelector('[data-hover="reward-tail"]') as SVGPathElement;
    expect(tail).toHaveAttribute('d', `M${f(tailX0)} ${G.winY}H${f(tailX0 + G.tail)}`);
    expect(tail).toHaveAttribute('stroke-dasharray', '1 3');
    // NEXT's window runs on past the last plate here: the gilt tail, over the same span
    expect(path(svg, 'tail-win').getAttribute('d')).toBe(rowChain(fit, T + 1).tailWin);
    expect(strapRects(path(svg, 'tail-win').getAttribute('d'))).toEqual([
      { x: tailX0, y: G.strapY, w: G.tail, h: G.strapH },
    ]);
    // Both leave with the hover.
    fireEvent.pointerLeave(svg);
    expect(svg.querySelector('[data-hover="reward"]')).toBeNull();
    expect(svg.querySelector('[data-hover="reward-tail"]')).toBeNull();
  });

  it('ends the off-screen reward link at the chain tail of the narrowest view too', async () => {
    // The narrowest row holds the tip and the blocks after it that fit whole (G.minBlocks): the
    // tail starts just past the last of them and the link stops there.
    viewW = G.minViewW;
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      width: G.minViewW,
      height: G.H,
      right: G.minViewW,
      bottom: G.H,
      toJSON: () => ({}),
    } as DOMRect);
    const svg = await renderPanel(snapshot);
    const fit = buildScene(snapshot, G.minViewW, 'settled');
    expect(fit.cards).toHaveLength(G.minBlocks);
    const last = fit.cards[G.minBlocks - 1];
    // the chain's tail has the 10 px to the view's edge left (589 − 579): the link's open end too
    const span = chainTail(fit.cards, G.minViewW) as NonNullable<ReturnType<typeof chainTail>>;
    expect(span).toEqual({ x0: last.x + last.w, x1: G.minViewW });
    for (const c of fit.cards) {
      const cb = c.squares[0];
      fireEvent.pointerMove(svg, { clientX: c.x + cb.x + cb.s / 2, clientY: cb.y + cb.s / 2 });
      expect(tooltip()).toHaveTextContent(`pays reward of#${fmt(c.block.number - 11)}`);
      const ax = (c.x + cb.x + cb.s / 2).toFixed(1);
      const x0 = (last.x + last.w).toFixed(1);
      expect(svg.querySelector('[data-hover="reward"]')).toHaveAttribute(
        'd',
        `M${ax} ${(cb.y + cb.s / 2).toFixed(1)}V${G.winY + 5}Q${ax} ${G.winY} ${(c.x + cb.x + cb.s / 2 + 5).toFixed(1)} ${G.winY}H${x0}`
      );
      expect(svg.querySelector('[data-hover="reward-tail"]')).toHaveAttribute(
        'd',
        `M${x0} ${G.winY}H${span.x1.toFixed(1)}`
      );
    }
  });

  it('draws the hover reward link of a block that has just landed, with no landing link', () => {
    // Reduced motion: the landed block joins the chain at once, so it can be hovered right away.
    stubMatchMedia(true);
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
    const s2 = mineNext(snapshot, { commit: [poolTxHash('w0')], propose: [] });
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={snapshot} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    expect(svg.querySelector('[data-layer="fx"]')?.childElementCount).toBe(0);
    expect(svg.querySelector('[data-hover="reward"]')).toBeNull();
    const fit = buildScene(s2, CONTAINER_VIEW_W, 'settled');
    const c = fit.cards[0];
    expect(c.block.number).toBe(T + 1);
    const cb = c.squares[0];
    fireEvent.pointerMove(svg, { clientX: c.x + cb.x + cb.s / 2, clientY: cb.y + cb.s / 2 });
    // It pays tip − 10, past the edge: the link runs off it, as for any other cellbase.
    const last = fit.cards[fit.cards.length - 1];
    expect(svg.querySelector('[data-hover="reward"]')?.getAttribute('d')).toMatch(
      new RegExp(`H${(last.x + last.w).toFixed(1)}$`)
    );
    expect(svg.querySelector('[data-hover="reward-tail"]')).not.toBeNull();
    expect(svg.querySelector('[data-layer="fx"]')?.childElementCount).toBe(0);
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
    expect(svg.querySelector('[data-ruler-ticks]')).toBeNull();
    expect(svg.querySelector('[data-window-label]')).toHaveAttribute('opacity', '0.15');
  });

  // Lane K: the pin is a touch tap's now (a click opens the page); its ring is unchanged.
  it('rings a hovered and a pinned tx square with square corners (U-02, R1)', async () => {
    const svg = await renderPanel(snapshot);
    const id = poolTxHash('q1');
    const q = scene.pool?.pending.squares.find((k) => k.id === id);
    tap(svg, { clientX: (q?.x as number) + 3, clientY: (q?.y as number) + 3 });
    fireEvent.pointerMove(svg, squareCentre(T, committedTxHash(T, 2)));
    const rings = ['halo', 'ring', 'pin-ring'].map((h) => svg.querySelector(`[data-hover="${h}"]`));
    expect(rings.map((r) => r?.tagName)).toEqual(['rect', 'rect', 'rect']);
    expect(roundedOutsideStones(svg)).toEqual([]);
  });

  it('pins a tapped tx and lets Escape unpin it', async () => {
    const svg = await renderPanel(snapshot);
    const id = poolTxHash('q1');
    const q = scene.pool?.pending.squares.find((k) => k.id === id);
    const at = { clientX: (q?.x as number) + 3, clientY: (q?.y as number) + 3 };
    tap(svg, at);
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'tx', hash: id });
    expect(screen.getByTestId('pipeline-selection')).toHaveTextContent('Selected');
    expect(screen.getByTestId('pipeline-selection')).toHaveTextContent('waiting to be proposed');
    const ring = svg.querySelector('[data-hover="pin-ring"]') as SVGRectElement;
    expect(ring).toHaveAttribute('stroke-dasharray', '3 2');
    expect([ring.getAttribute('x'), ring.getAttribute('y')].map(Number)).toEqual([
      (q?.x as number) - 2.5,
      (q?.y as number) - 2.5,
    ]);
    expect(Number(ring.getAttribute('width'))).toBe((q?.s as number) + 5);
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    expect(svg.querySelector('[data-hover="pin-ring"]')).toBeNull();
  });

  it('replaces the legend with the selection line while a tx is pinned', async () => {
    const svg = await renderPanel(snapshot);
    expect(screen.getByTestId('pipeline-legend')).toBeInTheDocument();
    expect(screen.queryByTestId('pipeline-selection')).toBeNull();

    const q = scene.pool?.pending.squares.find((k) => k.id === poolTxHash('q1'));
    tap(svg, { clientX: (q?.x as number) + 3, clientY: (q?.y as number) + 3 });
    expect(screen.getByTestId('pipeline-selection')).toHaveTextContent('Selected');
    expect(screen.queryByTestId('pipeline-legend')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Clear selection' }));
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    expect(screen.getByTestId('pipeline-legend')).toBeInTheDocument();
    expect(screen.queryByTestId('pipeline-selection')).toBeNull();
  });

  it('lets either footer wrap to the available panel width', async () => {
    // jsdom does not lay out, so check that no state forces clipping at a breakpoint.
    const row = (el: HTMLElement) => el.className.split(/\s+/);
    const svg = await renderPanel(snapshot);
    const legend = row(screen.getByTestId('pipeline-legend'));
    const q = scene.pool?.pending.squares.find((k) => k.id === poolTxHash('q1'));
    tap(svg, { clientX: (q?.x as number) + 3, clientY: (q?.y as number) + 3 });
    const txLine = row(screen.getByTestId('pipeline-selection'));
    tap(tile(svg, T - 3), bareSpot(scene, T - 3));
    const blockLine = row(screen.getByTestId('pipeline-selection'));
    for (const classes of [legend, txLine, blockLine]) {
      expect(classes).toContain('min-h-6');
      expect(classes).toContain('flex-wrap');
      expect(classes).not.toContain('overflow-hidden');
      expect(
        classes.every((c) => !/^(?:md:|lg:)(?:h-6|flex-nowrap|overflow-hidden)$/.test(c))
      ).toBe(true);
    }
  });

  it('follows hovers coming from the Latest Blocks and Latest Transactions rows', async () => {
    const svg = await renderPanel(snapshot);
    act(() => usePipelineHoverStore.getState().setHover({ block: T - 3, tx: null, window: T - 3 }));
    const box = card(svg, T - 3).querySelector('[data-box]') as SVGElement;
    expect(box.style.getPropertyValue('stroke')).toBe(color('text-dim'));
    expect(dimOfCard(svg, T)).toBe('true');
    const id = committedTxHash(T, 1);
    act(() => usePipelineHoverStore.getState().setHover({ block: T, tx: id, window: T }));
    expect(svg.querySelector(`[data-sq="${id}"]`)).toHaveClass('is-on');
    expect(svg.querySelector('[data-hover="ring"]')).not.toBeNull();
    act(() => usePipelineHoverStore.getState().clearHover());
    expect(svg).not.toHaveClass('is-focus');
    expect(box.style.getPropertyValue('stroke')).toBe('none');
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
    // Tapping the ghost pins the pool tx it predicts.
    tap(svg, ghostCentre(id));
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'tx', hash: id });
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

/** A column scene's chain slots: each row's top and height along the chain. */
const columnSlots = (sc: Scene): ChainSlotState[] =>
  sc.cards.map((c) => ({
    number: c.block.number,
    x: c.y,
    w: c.h,
    mined: true,
    hash: c.block.hash,
    parentHash: c.block.parentHash,
    plate: true,
  }));
/** NEXT's row as a column block span: its top and height along the chain. */
const columnNextSlot = (sc: Scene): BlockSpan => {
  const n = sc.next as NonNullable<Scene['next']>;
  return { number: n.number, x: n.y, w: n.h, mined: false };
};
/** The links of a settled column scene with the window of `H` drawn, from the library. */
const columnChain = (sc: Scene, H: number) =>
  columnChainPaths(columnSlots(sc), columnNextSlot(sc), H, sc.snapshot.consensus, sc.H);

/** `s` with T−1 carrying a second uncle (T−3's sibling) with three proposals of its own. */
const withTwoUncles = (s: PipelineSnapshot): PipelineSnapshot => ({
  ...s,
  blocks: s.blocks.map((b) =>
    b.number === T - 1
      ? {
          ...b,
          uncles: [...b.uncles, { number: T - 3, hash: uncleHash(T - 3) }],
          proposals: [
            ...b.proposals,
            ...[0, 1, 2].map((i) => ({
              shortId: shortIdOf(committedTxHash(T - 30, 5000 + i)),
              uncleIndex: 1,
              state: 'waiting' as const,
              txHash: null,
              committedBlock: null,
            })),
          ],
        }
      : b
  ),
});

/** The px a word advances per glyph at its size class: 0.6 em of JetBrains Mono. */
// re-pinned with readability R4: the canvas's words are text-meta, 12 px (was 11)
const GLYPH: Readonly<Record<string, number>> = { 'text-meta': 0.6 * 12, 'text-small': 0.6 * 13 };

/**
 * The extent of everything the canvas draws, in canvas px, through the translations of the groups
 * it sits in: every rect, circle and filled path (the strap, the arrowheads, a diamond), a stone
 * as its seal's box, and every word at its size's advance from its anchor (its x only). A path
 * command it cannot read fails the test.
 */
function drawnExtents(
  svg: Element
): { what: string; x0: number; x1: number; y0: number; y1: number }[] {
  const shift = (from: Element | null) => {
    let dx = 0;
    let dy = 0;
    for (let a = from; a && a !== svg; a = a.parentElement) {
      const m = /translate\(([-\d.]+),\s*([-\d.]+)\)/.exec(a.getAttribute('transform') ?? '');
      if (m) {
        dx += Number(m[1]);
        dy += Number(m[2]);
      }
    }
    return { dx, dy };
  };
  const out: { what: string; x0: number; x1: number; y0: number; y1: number }[] = [];
  for (const el of svg.querySelectorAll('rect, circle, path, text, [data-stone]')) {
    if (el.closest('defs, mask') || el.parentElement?.closest('[data-stone]')) continue;
    const at = (a: string) => Number(el.getAttribute(a));
    const what = el.outerHTML.slice(0, 90);
    if (el.matches('[data-stone]')) {
      const { dx, dy } = shift(el);
      const seal = el.querySelector('svg') as SVGSVGElement;
      const [w, h] = [Number(seal.getAttribute('width')), Number(seal.getAttribute('height'))];
      out.push({ what, x0: dx, x1: dx + w, y0: dy, y1: dy + h });
    } else if (el.tagName === 'rect') {
      const { dx, dy } = shift(el.parentElement);
      const [x, y] = [dx + at('x'), dy + at('y')];
      out.push({ what, x0: x, x1: x + at('width'), y0: y, y1: y + at('height') });
    } else if (el.tagName === 'circle') {
      const { dx, dy } = shift(el.parentElement);
      const [cx, cy, r] = [dx + at('cx'), dy + at('cy'), at('r')];
      out.push({ what, x0: cx - r, x1: cx + r, y0: cy - r, y1: cy + r });
    } else if (el.tagName === 'path') {
      const { dx, dy } = shift(el.parentElement);
      const xs: number[] = [];
      const ys: number[] = [];
      let [x, y] = [0, 0];
      for (const [, cmd, args] of (el.getAttribute('d') ?? '').matchAll(
        /([A-Za-z])([^A-Za-z]*)/g
      )) {
        const n = args.trim()
          ? args
              .trim()
              .split(/[\s,]+/)
              .map(Number)
          : [];
        if (cmd === 'M' || cmd === 'L') [x, y] = [n[0], n[1]];
        else if (cmd === 'H') x = n[0];
        else if (cmd === 'V') y = n[0];
        else if (cmd === 'h') x += n[0];
        else if (cmd === 'v') y += n[0];
        else if (cmd !== 'Z') throw new Error(`a path command the check cannot read: ${cmd}`);
        xs.push(dx + x);
        ys.push(dy + y);
      }
      out.push({
        what,
        x0: Math.min(...xs),
        x1: Math.max(...xs),
        y0: Math.min(...ys),
        y1: Math.max(...ys),
      });
    } else {
      const { dx } = shift(el.parentElement);
      const size = Object.keys(GLYPH).find((c) => el.classList.contains(c));
      if (!size) throw new Error(`word "${el.textContent}" has no size class`);
      const w = (el.textContent ?? '').length * GLYPH[size];
      const x = dx + at('x');
      const anchor = el.getAttribute('text-anchor') ?? 'start';
      const x0 = anchor === 'end' ? x - w : anchor === 'middle' ? x - w / 2 : x;
      out.push({ what, x0, x1: x0 + w, y0: NaN, y1: NaN });
    }
  }
  return out;
}

describe('PipelinePanel below 589 px: the pipeline stood on end', () => {
  /** The phone reference: a 390 px viewport less its 16 px gutters (no panel frame). */
  const PHONE_W = 358;
  const quiet = withSmallTxs(buildPipelineSnapshot());
  const scene: Scene = buildScene(quiet, PHONE_W, 'settled');
  const bands = scene.bands as NonNullable<Scene['bands']>;
  const rowOf = (n: number) => {
    const c = scene.cards.find((k) => k.block.number === n);
    if (!c) throw new Error(`row ${n} not on screen`);
    return c;
  };
  const box = (width: number, height: number) =>
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      width,
      height,
      right: width,
      bottom: height,
      toJSON: () => ({}),
    } as DOMRect);

  beforeEach(() => {
    viewW = PHONE_W;
    usePipelineHoverStore.getState().clear();
    Object.defineProperty(window, 'PointerEvent', {
      configurable: true,
      writable: true,
      value: PointerEventStub,
    });
    box(PHONE_W, scene.H);
  });

  afterEach(() => {
    Reflect.deleteProperty(window, 'PointerEvent');
    vi.restoreAllMocks();
    usePipelineHoverStore.getState().clear();
  });

  const tooltip = () => screen.getByTestId('pipeline-tooltip');

  // Re-pinned with lane P (PlatePhone.dc.html, plan §8): seven 44 px rows, 462 px at 358 px.
  it('draws the canvas 1:1 at the measured width: no scroller, no minimum width, 462 px tall', async () => {
    const svg = await renderPanel(quiet);
    const wrap = screen.getByTestId('pipeline-canvas');
    expect(wrap.className).not.toMatch(/overflow/);
    expect(svg.closest('[class*="overflow-x"]')).toBeNull();
    expect(scene.H).toBe(462);
    expect(svg.getAttribute('viewBox')).toBe(`0 0 ${PHONE_W} 462`);
    expect(svg.style.height).toBe('462px');
    expect(svg.style.minWidth).toBe('');
    expect(svg).toHaveAttribute('aria-label', expect.stringContaining('top to bottom'));
    expect(svg.getAttribute('aria-label')).toContain('plates');
    // The tip and three older blocks, newest first.
    expect(
      [...svg.querySelectorAll('[data-card]')].map((c) => Number(c.getAttribute('data-number')))
    ).toEqual([T, T - 1, T - 2, T - 3]);
  });

  it('draws its plates, cuts, uncles, NEXT, ghost txs and pool wells square-cornered (U-02, R1)', async () => {
    const svg = await renderPanel(quiet);
    expect(svg.querySelectorAll('rect[data-tray]')).toHaveLength(2);
    expect(svg.querySelectorAll('[data-box]').length).toBeGreaterThan(1);
    expect(svg.querySelectorAll('rect[data-cut]').length).toBeGreaterThan(0);
    expect(rects(svg).length).toBeGreaterThan(10);
    // a stone is a seal: its corners are its own drawing (lib/seal), outside the rule
    expect(svg.querySelectorAll('[data-stone]')).toHaveLength(COL.blocks);
    expect(roundedOutsideStones(svg)).toEqual([]);
  });

  it('keeps the one-row canvas at the 640–767 band: 606 px, two whole blocks, no minimum width', async () => {
    viewW = 606;
    box(606, G.H);
    const svg = await renderPanel(quiet);
    expect(svg.getAttribute('viewBox')).toBe(`0 0 606 ${G.H}`);
    expect(svg.style.minWidth).toBe('');
    expect(svg.querySelectorAll('[data-card]')).toHaveLength(2);
  });

  // Lane P: at the narrowest column, the phone and the widest column, with a 4 × 4 line and two
  // uncles on a line of their own, nothing the canvas draws reaches past its width or its height.
  it.each([COL.minViewW, 326, PHONE_W, G.minViewW - 1])(
    'never throws at %i px: whole rows, a canvas exactly as tall as they are, nothing past its edges',
    async (width) => {
      for (const snap of [buildPipelineSnapshot(), withTwoUncles(buildPipelineSnapshot())]) {
        viewW = width;
        const sc = buildScene(snap, width, 'settled');
        box(width, sc.H);
        const svg = await renderPanel(snap);
        expect(screen.queryByTestId('pipeline-error')).toBeNull();
        expect(svg.getAttribute('viewBox')).toBe(`0 0 ${width} ${sc.H}`);
        expect(svg.querySelectorAll('[data-card]')).toHaveLength(COL.blocks);
        for (const c of sc.cards) {
          expect(card(svg, c.block.number)).toHaveAttribute(
            'transform',
            `translate(${c.x.toFixed(2)},${c.y.toFixed(2)})`
          );
        }
        // with the strap in place: the gilt down every row of NEXT's window (T−1 … T−3), x 5 … 25
        const gilded = sc.cards.filter((c) => inWindow(c.block.number, T + 1, snap.consensus));
        expect(gilded.map((c) => c.block.number)).toEqual([T - 1, T - 2, T - 3]);
        const straps = [...svg.querySelectorAll('[data-strap]')];
        expect(straps.map((el) => el.closest('[data-card]')?.getAttribute('data-number'))).toEqual(
          gilded.map((c) => String(c.block.number))
        );
        // each in its row (placed at the row's x, y above), the row's full height
        for (const c of gilded) {
          const strap = card(svg, c.block.number).querySelector('[data-strap]') as Element;
          expect(rectOf(strap), `#${c.block.number}`).toEqual([5, 0, 20, c.h]);
        }
        const drawn = drawnExtents(svg);
        expect(drawn.length).toBeGreaterThan(50);
        // it measured every word, and the four plates that span the canvas edge to edge
        const words = [...svg.querySelectorAll('text')].filter((t) => !t.closest('[data-stone]'));
        expect(drawn.filter((d) => Number.isNaN(d.y0))).toHaveLength(words.length);
        // every "+N" and "uncle" in the small text the layout measures it at (SMALL_TEXT_PX)
        const measured = words.filter((t) => /^(\+\d|uncle$)/.test(t.textContent ?? ''));
        expect(measured.length).toBeGreaterThan(0);
        for (const t of measured) expect(t).toHaveClass('text-meta');
        expect(drawn.filter((d) => d.x0 === 0 && d.x1 === width)).toHaveLength(COL.blocks);
        expect(drawn.filter((d) => d.x0 < 0 || d.x1 > width).map((d) => d.what)).toEqual([]);
        expect(
          drawn.filter((d) => !Number.isNaN(d.y0) && (d.y0 < 0 || d.y1 > sc.H)).map((d) => d.what)
        ).toEqual([]);
        cleanup();
      }
    }
  );

  // Re-pinned with lane P from the old look (a well as a recessed tray, NEXT dashed "3 2" over a
  // half-strength box, blocks as tiles fading with age): direction F at the phone board's numbers.
  it('draws the rows in the three materials: square wells, NEXT the one dashed outline, blocks as plates', async () => {
    const svg = await renderPanel(quiet);
    const wells = [
      { id: 'pending', title: 'mempool', tone: 'fill-aqua', total: '5', band: bands.pending },
      { id: 'proposed', title: 'proposed', tone: 'fill-gold', total: '9', band: bands.proposed },
    ];
    expect(wells.map((w) => w.band)).toEqual([
      { y: 0, h: 44 },
      { y: 62, h: 44 },
    ]);
    for (const w of wells) {
      const well = svg.querySelector(`[data-pool-card="${w.id}"]`) as SVGGElement;
      // a well across the canvas, square, its 1 px edge in the line ink on its outer pixels
      const tray = well.querySelector('[data-tray]') as SVGRectElement;
      expect(tray.tagName).toBe('rect');
      expect(tray).not.toHaveAttribute('rx');
      expect(tray.style.getPropertyValue('fill')).toBe(WELL_FILL);
      expect(tray.style.getPropertyValue('stroke')).toBe(color('base-border'));
      expect(tray).toHaveAttribute('stroke-width', '1');
      expect(rectOf(tray)).toEqual(asRect(columnWellRect(PHONE_W, w.band)));
      expect(rectOf(tray)).toEqual([0.5, w.band.y + 0.5, 357, 43]);
      // the inset: 3 px under its top edge, the theme's well inset fading to nothing
      const inset = well.querySelector('[data-inset]') as SVGRectElement;
      expect(rectOf(inset)).toEqual(asRect(columnWellShade(PHONE_W, w.band)));
      const ref = /^url\(#(.+)\)$/.exec(inset.getAttribute('fill') ?? '')?.[1];
      const shade = svg.querySelector(`linearGradient[id="${ref}"]`) as SVGLinearGradientElement;
      expect(['x1', 'y1', 'x2', 'y2'].map((a) => shade.getAttribute(a))).toEqual([
        '0',
        '0',
        '0',
        '1',
      ]);
      expect(
        [...shade.querySelectorAll('stop')].map((x) => [
          (x as SVGStopElement).style.getPropertyValue('stop-color'),
          x.getAttribute('stop-opacity'),
        ])
      ).toEqual([
        ['var(--m-well-inset)', '1'],
        ['var(--m-well-inset)', '0'],
      ]);
      // its name in the stage's colour from x 8, its total ending at 108, on the band's middle
      const head = columnWellHeadAt(w.band);
      const name = within(well as unknown as HTMLElement).getByText(w.title);
      expect(name).toHaveClass(w.tone);
      const nameText = name.closest('text') as SVGTextElement;
      expect([nameText.getAttribute('x'), nameText.getAttribute('y')].map(Number)).toEqual([
        8,
        head.y,
      ]);
      const count = well.querySelector('[data-count]') as SVGTextElement;
      expect(count.textContent).toBe(w.total);
      expect(count).toHaveAttribute('text-anchor', 'end');
      expect([count.getAttribute('x'), count.getAttribute('y')].map(Number)).toEqual([108, head.y]);
    }
    // mempool ⇢ proposed ⇢ NEXT (direction G): arrowheads pointing down each gap, on the strap's
    // axis, two in the 18 px gap and four in the 30 px gap.
    const flows = [...svg.querySelectorAll('[data-flow]')] as SVGPathElement[];
    expect(flows.map((f) => f.getAttribute('d'))).toEqual([
      columnFlowPaths(bands).toProposed,
      columnFlowPaths(bands).toNext,
    ]);
    expect(flows.map((f) => (f.getAttribute('d') ?? '').match(/Z/g)?.length)).toEqual([2, 4]);
    for (const f of flows) {
      expect(f.getAttribute('d')).toMatch(ARROWS);
      expect(f.style.getPropertyValue('fill')).toBe(tok('ink-dim'));
      expect(f).toHaveAttribute('fill-opacity', '0.8');
      expect(f.style.getPropertyValue('stroke')).toBe('');
      expect(f).not.toHaveAttribute('stroke-dasharray');
    }
    // NEXT: the dashed outline at the row's size (2 2), no fill, its number in the dim ink at x 8.
    const next = svg.querySelector('[data-next]') as SVGGElement;
    expect(next).toHaveAttribute('transform', 'translate(0.00,136.00)');
    const nextBox = next.querySelector('[data-box]') as SVGRectElement;
    expect(nextBox).toHaveAttribute('stroke-dasharray', '2 2');
    expect(nextBox).toHaveAttribute('stroke-opacity', '0.6');
    expect(nextBox.style.getPropertyValue('stroke')).toBe(tok('jade'));
    expect(nextBox.style.getPropertyValue('fill')).toBe('none');
    expect(rectOf(nextBox)).toEqual(asRect(cardBox('column', PHONE_W, 44, true)));
    const nextNumber = within(next as unknown as HTMLElement).getByText(tail4(T + 1));
    expect(nextNumber).toHaveClass('fill-text-dim', 'text-meta', 'font-medium');
    expect([nextNumber.getAttribute('x'), nextNumber.getAttribute('y')].map(Number)).toEqual([
      COL.nameX,
      columnHeadAt({ h: 44, uncles: [] }).y,
    ]);
    expect(next.querySelector('[data-stone]')).toBeNull();
    expect(next.querySelectorAll('[data-ghost-chip]')).toHaveLength(5);
    const outlines = [...svg.querySelectorAll('[data-box], [data-tray]')];
    expect(outlines).toHaveLength(2 + 1 + COL.blocks);
    expect(outlines.filter((el) => el.hasAttribute('stroke-dasharray'))).toEqual([nextBox]);
    // Blocks: solid plates spanning the canvas, their exact 358 × 44 px, no stroke at rest, never
    // faded (no group opacity: an older block's marks step back by their fill).
    scene.cards.forEach((c, i) => {
      const el = card(svg, c.block.number);
      expect(el).toHaveAttribute('data-age', String(i));
      expect(el.querySelector('[data-age]')).toBeNull();
      const plate = tile(svg, c.block.number);
      expect(plate.style.getPropertyValue('fill')).toBe(PLATE_FILL);
      expect(plate).toHaveAttribute('fill-opacity', '1');
      expect(plate.style.getPropertyValue('stroke')).toBe('none');
      expect(plate).not.toHaveAttribute('stroke-dasharray');
      expect(plate).not.toHaveAttribute('rx');
      expect(rectOf(plate)).toEqual([0, 0, PHONE_W, 44]);
      expect(rectOf(plate)).toEqual(asRect(cardBox('column', c.w, c.h, false)));
      // the number after the stone, 12 px / 500 in the bright ink, on the middle of the line
      const num = el.querySelector('[data-block-number]') as SVGTextElement;
      expect(num.textContent).toBe(tail4(c.block.number));
      expect(num).toHaveClass('fill-text-bright', 'text-meta', 'font-medium');
      expect([num.getAttribute('x'), num.getAttribute('y')].map(Number)).toEqual([27, 25.5]);
    });
    // The cellbase diamond starts each line at x 126; the tip's three dots are at 74, 83, 92.
    const tip = rowOf(T);
    const cb = card(svg, T).querySelector('[data-stage="cellbase"]') as SVGPathElement;
    expect(cb).toHaveAttribute('d', diamondPath(126, 14, side(1)));
    const dots = [...card(svg, T).querySelectorAll('[data-chip]')].map((d) =>
      Number(d.getAttribute('cx'))
    );
    expect(dots.map((x) => tip.x + x - G.chip / 2)).toEqual([74, 83, 92]);
  });

  // Direction F, §2.1 (lane P): a row is cut only where a transaction is.
  it('cuts a row only where a mark is: as many cuts as marks on its line, each its square grown by the gutter', async () => {
    const svg = await renderPanel(quiet);
    for (const c of scene.cards) {
      const el = card(svg, c.block.number);
      const cuts = el.querySelector('[data-cuts]') as SVGGElement;
      const field = el.querySelector('[data-field]') as SVGGElement;
      expect([...cuts.children].every((x) => x.matches('rect[data-cut]'))).toBe(true);
      expect([...field.children].every((x) => x.hasAttribute('data-sq'))).toBe(true);
      expect(cuts.children).toHaveLength(field.children.length);
      expect(cuts.children).toHaveLength(c.squares.length);
      expect(cuts.compareDocumentPosition(field)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
      for (const q of c.squares) {
        const cut = cuts.querySelector(`[data-cut="${q.id}"]`) as SVGRectElement;
        expect(cut, `the cut of ${q.id}`).not.toBeNull();
        expect(rectOf(cut)).toEqual([q.x - GUT, q.y - GUT, q.s + 2 * GUT, q.s + 2 * GUT]);
        expect(cut.style.getPropertyValue('fill')).toBe(CUT_FILL);
      }
    }
    // A well cuts nothing: its marks lie loose in it.
    expect(svg.querySelectorAll('[data-pool] [data-cut]')).toHaveLength(0);
  });

  // Direction G, turned (lane S2): the gilt strap down a row is a quantity too, as in the row: the
  // block lies inside the window the next block may commit from.
  it('draws nothing on a plate row without a quantity: the plate, its strap in the window, its stone, cuts, marks, dots and words', async () => {
    const svg = await renderPanel(quiet);
    const cards = [...svg.querySelectorAll('[data-card]')];
    expect(cards).toHaveLength(COL.blocks);
    const what = (el: Element) =>
      el.closest('[data-stone]')
        ? 'stone'
        : el.matches('[data-box]')
          ? 'plate'
          : el.matches('[data-strap]')
            ? 'strap'
            : el.matches('[data-cut]')
              ? 'cut'
              : el.matches('[data-sq]')
                ? 'mark'
                : el.matches('[data-chip]')
                  ? 'dot'
                  : el.matches('text, tspan')
                    ? 'word'
                    : el.matches('g')
                      ? 'group'
                      : null;
    for (const c of cards) {
      const stray = [...c.querySelectorAll('*')].filter((el) => what(el) === null);
      expect(stray.map((el) => el.outerHTML.slice(0, 80))).toEqual([]);
      for (const g of c.querySelectorAll('g')) {
        if (g.closest('[data-stone]')) continue;
        expect(g.getAttribute('fill')).toBeNull();
        expect(g.getAttribute('stroke')).toBeNull();
      }
      expect(c.querySelectorAll('[data-box]')).toHaveLength(1);
      expect(c.querySelectorAll(':scope [data-stone]')).toHaveLength(1);
      // a strap exactly where the next block's window runs
      const n = Number(c.getAttribute('data-number'));
      expect(c.querySelectorAll('[data-strap]'), `#${n}`).toHaveLength(
        inWindow(n, T + 1, CONSENSUS) ? 1 : 0
      );
    }
    expect(svg.querySelectorAll('[data-strap]')).toHaveLength(3);
  });

  it('presses the block’s own seal into the head of its row: its row stone, 14 px, on the line’s middle', async () => {
    const svg = await renderPanel(quiet);
    for (const c of scene.cards) {
      const stone = card(svg, c.block.number).querySelector('[data-stone]') as SVGGElement;
      const head = columnHeadAt(c);
      expect(stone.getAttribute('transform')).toBe(`translate(${head.stoneX},${head.stoneY})`);
      expect(stone.getAttribute('transform')).toBe('translate(8,15)');
      expect(stone.innerHTML).toBe(asInSvg(stoneOf(c.block.hash, c.block.txCount)));
      const seal = stone.querySelector('svg') as SVGSVGElement;
      expect([seal.getAttribute('width'), seal.getAttribute('height')]).toEqual(['14', '14']);
      expect(stone).toHaveAttribute('pointer-events', 'none');
    }
    const stones = scene.cards.map((c) => stoneOf(c.block.hash, c.block.txCount));
    expect(new Set(stones).size).toBe(scene.cards.length);
  });

  it('steps an older row’s marks back by their fill, toward the cut or the plate, never by an opacity', async () => {
    const svg = await renderPanel(quiet);
    scene.cards.forEach((c, i) => {
      const el = card(svg, c.block.number);
      expect(el.getAttribute('opacity')).toBe('1.000');
      for (const q of c.squares) {
        const sq = el.querySelector(`[data-sq="${q.id}"]`) as SVGElement;
        const own = paintSquare(q.stage, q.style, q.step);
        expect(sq.style.getPropertyValue('fill')).toBe(
          agedPaint(own, q.kind === 'cellbase' ? 'cellbase' : 'square', i).fill
        );
      }
      for (const k of c.chips) {
        const dot = el.querySelector(`[data-chip="${k.key}"]`) as SVGElement;
        expect(dot.style.getPropertyValue('fill')).toBe(agedChip(k.proposal.state, i).fill);
      }
    });
    const old = card(svg, T - 3).querySelector('[data-stage="cellbase"]') as SVGElement;
    expect(old.style.getPropertyValue('fill')).toBe(
      `color-mix(in srgb, ${STAGE_BASE.cellbase} ${Math.round(ageOpacity(3) * 1000) / 10}%, ${CUT_FILL})`
    );
  });

  // Re-pinned with lane P from "words at full strength while the shapes step back": the row's rule.
  it('never fades a plate row or anything on it, its words least of all (R4, lane E2)', async () => {
    expectPlatesAtFullStrength(await renderPanel(quiet));
  });

  it('writes every word of a row at 4.5:1 on what it stands on, in both themes (R4, lane E2)', async () => {
    const svg = await renderPanel(quiet);
    act(() => usePipelineHoverStore.getState().setHover({ block: null, tx: null, window: T }));
    const failures = wordInkFailures(svg, 'column');
    act(() => usePipelineHoverStore.getState().clearHover());
    expect(failures).toEqual([]);
  });

  it('writes the well’s and NEXT’s "unavailable" of a row at 4.5:1 when the pool tip differs', async () => {
    const svg = await renderPanel({
      ...quiet,
      status: 'poolTipMismatch',
      poolTip: { number: T + 1, hash: blockHash(T + 1) },
      pool: null,
      next: null,
    });
    expect(svg.querySelector('[data-pool-status]')).not.toBeNull();
    expect(wordInkFailures(svg, 'column')).toEqual([]);
  });

  // Direction F (lane L's SceneUncle, lane P): an uncle is its word and its dots, no box, no stone.
  it('ends the tx line with an uncle: its word and its dots, no box, no stone, on the line’s middle', async () => {
    const svg = await renderPanel(quiet);
    const host = rowOf(T - 1);
    const u = host.uncles[0];
    // re-pinned (readability R4): "uncle" is 36 px at 12 px (33 at 11), so the uncle is 74 px and
    // ends the line from 276 (was 71 from 279)
    expect(u).toMatchObject({ line: 'tx', x: 276, y: 16, w: 74, h: 12 });
    const uncle = card(svg, T - 1).querySelector('[data-uncle="0"]') as SVGGElement;
    expect(uncle.closest('[data-words]')).not.toBeNull();
    expect(uncle).toHaveAttribute('pointer-events', 'visiblePainted');
    expect([...uncle.querySelectorAll('text')].map((t) => t.textContent)).toEqual(['uncle']);
    const word = uncle.querySelector('text') as SVGTextElement;
    expect(word).toHaveClass('fill-text');
    // drawn in the small text its width is measured at (labelWidth, SMALL_TEXT_PX)
    expect(word).toHaveClass('text-meta');
    // its word on the line, beside the number
    expect([word.getAttribute('x'), word.getAttribute('y')].map(Number)).toEqual([
      276,
      columnHeadAt(host).y,
    ]);
    expect(uncle.querySelectorAll('rect, path, circle, [data-stone], [data-sq]')).toHaveLength(0);
    // its two dots after the word, centred on the line
    const dots = [...card(svg, T - 1).querySelectorAll('[data-chip][data-uncle-index="0"]')];
    expect(dots.map((d) => Number(d.getAttribute('cx')) - G.chip / 2)).toEqual([
      u.x + 40,
      u.x + 49,
    ]);
    for (const d of dots) expect(Number(d.getAttribute('cy'))).toBe(host.h / 2);
    expect(card(svg, T - 1).querySelectorAll('[data-stone]')).toHaveLength(1);
  });

  it('puts two uncles on a line of their own under the tx line: the row 16 px taller, its head and squares still on the line', async () => {
    const two = withSmallTxs(withTwoUncles(buildPipelineSnapshot()));
    const sc = buildScene(two, PHONE_W, 'settled');
    box(PHONE_W, sc.H);
    const svg = await renderPanel(two);
    expect(sc.H).toBe(478);
    expect(svg.getAttribute('viewBox')).toBe(`0 0 ${PHONE_W} 478`);
    const host = sc.cards.find((c) => c.block.number === T - 1) as Scene['cards'][number];
    expect(host.h).toBe(60);
    const el = card(svg, T - 1);
    // the plate is the whole grown row
    expect(rectOf(el.querySelector('[data-box]') as Element)).toEqual([0, 0, PHONE_W, 60]);
    // both uncles under the line, from x 126, their words and dots on their own line's middle
    expect(host.uncles.map((u) => [u.line, u.x, u.y])).toEqual([
      ['own', 126, 42],
      ['own', 204, 42],
    ]);
    for (const u of host.uncles) {
      const word = el.querySelector(`[data-uncle="${u.index}"] text`) as SVGTextElement;
      expect(word.textContent).toBe('uncle');
      expect([word.getAttribute('x'), word.getAttribute('y')].map(Number)).toEqual([
        u.x,
        u.y + u.h / 2 + 3.5,
      ]);
      const dots = [...el.querySelectorAll(`[data-chip][data-uncle-index="${u.index}"]`)];
      expect(dots).toHaveLength(u.index === 0 ? 2 : 3);
      for (const d of dots) expect(Number(d.getAttribute('cy'))).toBe(u.y + u.h / 2);
    }
    // the head, the block's own dots and its squares stay on the 44 px tx line, not the 60 px row
    const stone = el.querySelector('[data-stone]') as SVGGElement;
    expect(stone).toHaveAttribute('transform', 'translate(8,15)');
    const num = el.querySelector('[data-block-number]') as SVGTextElement;
    expect(Number(num.getAttribute('y'))).toBe(22 + 3.5);
    expect(Number(num.getAttribute('y'))).not.toBe(host.h / 2 + 3.5);
    for (const d of el.querySelectorAll('[data-chip]:not([data-uncle-index])')) {
      expect(Number(d.getAttribute('cy'))).toBe(22);
    }
    expect(el.querySelector('[data-stage="cellbase"]')).toHaveAttribute(
      'd',
      diamondPath(126, 14, side(1))
    );
    for (const q of host.squares) {
      expect(Math.abs(q.y + q.s / 2 - 22)).toBeLessThanOrEqual(0.5);
      const sq = el.querySelector(`[data-sq="${q.id}"]`) as Element;
      if (q.kind === 'tx') expect(Number(sq.getAttribute('y'))).toBe(q.y);
    }
    // the rows below step down by the 16 px
    expect(card(svg, T - 2)).toHaveAttribute('transform', 'translate(0.00,338.00)');
    expect(card(svg, T - 3)).toHaveAttribute('transform', 'translate(0.00,400.00)');
  });

  // Direction G, turned (StrapPhone.dc.html `.vs`, `.vd`): the strap runs down through the stones,
  // x 5 … 25; dashed from NEXT's row, the plate's tone between two rows, gilt inside the window,
  // and the gilt tail fading down below the last row.
  it('runs the strap down through the stones at x 5 … 25: dashed from NEXT, plate, gilt, the gilt tail fading down', async () => {
    const svg = await renderPanel(quiet);
    const paths = columnChain(scene, T + 1);
    for (const [name, key] of STRAP_PATHS) {
      expect(path(svg, name)).toHaveAttribute('d', paths[key] || 'M0 0');
    }
    // NEXT (136 … 180) ⇢ the tip (198 …): two dashed lines down the strap's outer pixel columns
    const pend = path(svg, 'pend');
    expect(pend).toHaveAttribute('d', 'M5.5 180.0V198.0M24.5 180.0V198.0');
    expect(pend).toHaveAttribute('fill', 'none');
    expect(pend.style.getPropertyValue('stroke')).toBe(tok('jade'));
    expect(pend).toHaveAttribute('stroke-dasharray', '2 2');
    // the tip → T−1 in the plate's tone (the tip is outside NEXT's window); T−1 … T−3 gilt
    expect(strapRects(path(svg, 'solid').getAttribute('d'))).toEqual([
      { x: 5, y: 242, w: 20, h: 18 },
    ]);
    expect(path(svg, 'solid').style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(strapRects(path(svg, 'win').getAttribute('d'))).toEqual([
      { x: 5, y: 304, w: 20, h: 18 },
      { x: 5, y: 366, w: 20, h: 18 },
    ]);
    expect(path(svg, 'win').style.getPropertyValue('fill')).toBe(GILT_FILL);
    expect(path(svg, 'win')).not.toHaveAttribute('fill-opacity');
    // The window of NEXT runs past the last row: the tail is gilt, 30 px from its bottom edge,
    // fading to nothing down the column.
    const tail = path(svg, 'tail-win');
    expect(strapRects(tail.getAttribute('d'))).toEqual([{ x: 5, y: 428, w: 20, h: 30 }]);
    expect(path(svg, 'tail')).toHaveAttribute('d', 'M0 0');
    expect(tail.style.getPropertyValue('fill')).toBe(GILT_FILL);
    const maskId = /^url\(#(.+)\)$/.exec(tail.getAttribute('mask') ?? '')?.[1];
    const mask = svg.querySelector(`mask[id="${maskId}"]`) as SVGMaskElement;
    const fadeId = /^url\(#(.+)\)$/.exec(
      mask.querySelector('rect')?.getAttribute('fill') ?? ''
    )?.[1];
    const fade = svg.querySelector(`linearGradient[id="${fadeId}"]`) as SVGLinearGradientElement;
    expect(['x1', 'y1', 'x2', 'y2'].map((a) => fade.getAttribute(a))).toEqual(['0', '0', '0', '1']);
    // At rest: no mark, no label, no ticks across the gaps.
    expect(svg.querySelector('[data-window-marks]')).toBeNull();
    expect(svg.querySelector('[data-window-label]')).toBeNull();
    expect(svg.querySelector('[data-ruler-ticks]')).toBeNull();
  });

  it('fills the whole gap down every parent link at the strap’s x, and none across a break', async () => {
    // T−1 names another parent: the chain breaks between T−1 and T−2 (a reorg, an orphan)
    const broken: PipelineSnapshot = {
      ...quiet,
      blocks: quiet.blocks.map((b) =>
        b.number === T - 1 ? { ...b, parentHash: blockHash(T - 100) } : b
      ),
    };
    const svg = await renderPanel(broken);
    const sc = buildScene(broken, PHONE_W, 'settled');
    const links = (['solid', 'win'] as const).flatMap((n) =>
      strapRects(path(svg, n).getAttribute('d'))
    );
    const gaps = sc.cards.slice(1).map((c, i) => ({
      a: sc.cards[i].y + sc.cards[i].h,
      b: c.y,
      linked: sc.cards[i].block.parentHash === c.block.hash,
      name: `#${sc.cards[i].block.number} → #${c.block.number}`,
    }));
    expect(gaps.filter((g) => !g.linked).map((g) => g.name)).toEqual([`#${T - 1} → #${T - 2}`]);
    for (const g of gaps) {
      const inGap = links.filter((r) => r.y >= g.a && r.y < g.b);
      expect(inGap, g.name).toEqual(
        g.linked ? [{ x: COL.strapX, y: g.a, w: COL.strapW, h: g.b - g.a }] : []
      );
    }
    expect(links).toHaveLength(gaps.filter((g) => g.linked).length);
  });

  /**
   * The gilt down the column, absolute, top to bottom: each window row's own strap (from its row's
   * top, as drawn) and the gilt joints and tail of the chain between and below them.
   */
  const giltRun = (svg: Element) => {
    const own = [...svg.querySelectorAll('[data-strap]')].map((el) => {
      const [x, y, w, h] = rectOf(el);
      return { x, y: drawnY(el.closest('[data-card]') as Element) + y, w, h };
    });
    const chain = (['win', 'tail-win'] as const).flatMap((n) =>
      strapRects(path(svg, n).getAttribute('d'))
    );
    return [...own, ...chain].sort((a, b) => a.y - b.y);
  };
  /** A card's y as drawn: the translation the static render or the motion layer gave it. */
  const drawnY = (el: Element) =>
    Number(/translate\([-\d.]+,([-\d.]+)\)/.exec(el.getAttribute('transform') ?? '')?.[1]);
  /** The gilt runs unbroken from `from` to `to` down x 5 … 25. */
  const expectGiltUnbroken = (svg: Element, from: number, to: number) => {
    const run = giltRun(svg);
    for (const r of run) expect([r.x, r.w]).toEqual([COL.strapX, COL.strapW]);
    expect(run[0].y).toBe(from);
    for (let i = 1; i < run.length; i++) expect(run[i].y).toBe(run[i - 1].y + run[i - 1].h);
    const last = run[run.length - 1];
    expect(last.y + last.h).toBe(to);
  };

  // Direction G, turned (StrapPhone.dc.html `.rw .vs.g`; lane S2): inside the window of the anchor
  // (the next block at rest) the strap is gilt down the whole height of every row, x 5 … 25, on the
  // plate under its stone, the 14 px stone at x 8 … 22 with 3 px of gilt on each side; none on a
  // row outside the window, on NEXT's row or on a well. It is a ground, as the plate is: never
  // under an opacity. With the gilt joints and the gilt tail it reads as one strap.
  it('lays the gilt down the window’s rows at rest: x 5 … 25, the row’s full height, under the stone', async () => {
    const svg = await renderPanel(quiet);
    const inside = scene.cards.filter((c) => inWindow(c.block.number, T + 1, CONSENSUS));
    expect(inside.map((c) => [c.block.number, c.y, c.h])).toEqual([
      [T - 1, 260, 44],
      [T - 2, 322, 44],
      [T - 3, 384, 44],
    ]);
    for (const c of scene.cards) {
      const el = card(svg, c.block.number);
      const strap = el.querySelector('[data-strap]') as SVGRectElement | null;
      if (!inside.includes(c)) {
        expect(strap, `#${c.block.number}`).toBeNull();
        continue;
      }
      if (!strap) throw new Error(`#${c.block.number} has no strap`);
      expect(rectOf(strap)).toEqual(asRect(strapBox('column', c.w, c.h)));
      expect(rectOf(strap)).toEqual([5, 0, 20, 44]);
      expect(strap.style.getPropertyValue('fill')).toBe(GILT_FILL);
      expect(strap).toHaveAttribute('pointer-events', 'none');
      expect(setBack(strap, svg)).toEqual([]);
      // on the plate: right above its fill, under its stone
      const layer = [...(strap.parentElement as Element).children];
      expect(layer.indexOf(strap)).toBe(layer.indexOf(tile(svg, c.block.number)) + 1);
      const stone = el.querySelector('[data-stone]') as SVGGElement;
      expect(strap.compareDocumentPosition(stone)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
      const [sx, sy] = (
        /translate\(([\d.]+),([\d.]+)\)/.exec(stone.getAttribute('transform') ?? '') ?? []
      )
        .slice(1)
        .map(Number);
      expect([sx - 5, 25 - (sx + G.stone)]).toEqual([3, 3]);
      expect(sy).toBeGreaterThanOrEqual(0);
      expect(sy + G.stone).toBeLessThanOrEqual(c.h);
    }
    // none on NEXT's row, none in a well: the three rows' straps are all
    expect(svg.querySelectorAll('[data-strap]')).toHaveLength(3);
    expect(svg.querySelector('[data-next] [data-strap]')).toBeNull();
    expect(svg.querySelector('[data-pool] [data-strap]')).toBeNull();
    // with the gilt joints and the gilt tail: one gilt strap from T−1's top to the tail's end
    expectGiltUnbroken(svg, 260, 458);
  });

  it('moves the gilt to the window of a pointed row and never sets it back', async () => {
    const svg = await renderPanel(quiet);
    const gilt = () =>
      scene.cards
        .filter((c) => card(svg, c.block.number).querySelector('[data-strap]') !== null)
        .map((c) => c.block.number);
    const check = (when: string) => {
      for (const el of svg.querySelectorAll('[data-strap]')) {
        expect(setBack(el, svg), when).toEqual([]);
        expect((el as SVGElement).style.getPropertyValue('fill'), when).toBe(GILT_FILL);
      }
    };
    expect(gilt()).toEqual([T - 1, T - 2, T - 3]);
    check('at rest');
    // pointing at the tip's plate: its window is T−10 … T−2, so T−1 has no gilt and steps back
    fireEvent.pointerMove(tile(svg, T), bareSpot(scene, T));
    expect(tooltip()).toHaveTextContent(`Block #${fmt(T)}`);
    expect(gilt()).toEqual([T - 2, T - 3]);
    expect(card(svg, T - 1).querySelector(':scope > [data-dim]')).toHaveAttribute(
      'data-dim',
      'true'
    );
    check('pointing at the tip');
    expectGiltUnbroken(svg, 322, 458);
    // pointing at T−2 (from its Latest Blocks row): its window, T−12 … T−4, has no row on screen
    hoverStoreSet({ block: T - 2, tx: null, window: T - 2 });
    expect(gilt()).toEqual([]);
    // NEXT's row is never gilt, whatever the anchor: the window of T+3 (T−7 … T+1) holds NEXT's
    // number and every row on screen, and only the four rows carry the gilt
    hoverStoreSet({ block: null, tx: null, window: T + 3 });
    expect(inWindow(T + 1, T + 3, CONSENSUS)).toBe(true);
    expect(gilt()).toEqual([T, T - 1, T - 2, T - 3]);
    expect(svg.querySelector('[data-next] [data-strap]')).toBeNull();
    expect(svg.querySelector('[data-pool] [data-strap]')).toBeNull();
    check('the window of T+3');
    fireEvent.pointerLeave(svg);
    hoverStoreSet({ block: null, tx: null, window: null });
    expect(gilt()).toEqual([T - 1, T - 2, T - 3]);
    check('after the pointer left');
  });

  // StrapPhone.dc.html, "a taller row": the strap is as tall as the row it crosses.
  it.each([
    {
      what: 'a 3 × 3 square (61 px)',
      n: T - 2,
      h: 61,
      snap: (s: PipelineSnapshot): PipelineSnapshot => ({
        ...s,
        blocks: s.blocks.map((b) =>
          b.number === T - 2
            ? { ...b, txs: b.txs.map((t, i) => (i === 0 ? { ...t, size: 5400 } : t)) }
            : b
        ),
      }),
    },
    { what: 'a line of uncles (60 px)', n: T - 1, h: 60, snap: withTwoUncles },
  ])('keeps the gilt the full height of a taller row: $what', async ({ n, h, snap }) => {
    const s = snap(withSmallTxs(buildPipelineSnapshot()));
    const sc = buildScene(s, PHONE_W, 'settled');
    const row = sc.cards.find((c) => c.block.number === n) as Scene['cards'][number];
    expect(row.h).toBe(h);
    box(PHONE_W, sc.H);
    const svg = await renderPanel(s);
    const strap = card(svg, n).querySelector('[data-strap]') as SVGRectElement;
    expect(rectOf(strap)).toEqual(asRect(strapBox('column', row.w, h)));
    expect(rectOf(strap)).toEqual([5, 0, 20, h]);
    expect(rectOf(tile(svg, n))).toEqual([0, 0, PHONE_W, h]);
    const gilded = sc.cards.filter((c) => inWindow(c.block.number, T + 1, CONSENSUS));
    expect(gilded).toContain(row);
    const last = gilded[gilded.length - 1];
    expectGiltUnbroken(svg, gilded[0].y, last.y + last.h + COL.tail);
  });

  // The Touch rule's pin (lane K) on a gilt row: its 1 px edge in the dim ink is drawn over the
  // strap, so the strap does not break the edge where it crosses the row's top and bottom.
  it('draws a pinned row’s 1 px edge over its gilt strap, and takes it away with the pin', async () => {
    const svg = await renderPanel(quiet);
    const c = rowOf(T - 1);
    tap(tile(svg, T - 1), bareSpot(scene, T - 1));
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'block', number: T - 1 });
    const el = card(svg, T - 1);
    expect(tile(svg, T - 1).style.getPropertyValue('stroke')).toBe(color('text-dim'));
    const strap = el.querySelector('[data-strap]') as SVGRectElement;
    expect(rectOf(strap)).toEqual([5, 0, 20, 44]);
    const edge = el.querySelector('[data-edge]') as SVGRectElement;
    expect(edge).not.toBeNull();
    expect(strap.compareDocumentPosition(edge)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(rectOf(edge)).toEqual(asRect(cardBox('column', c.w, c.h, true)));
    expect(rectOf(edge)).toEqual([0.5, 0.5, PHONE_W - 1, 43]);
    expect(edge).toHaveAttribute('fill', 'none');
    expect(edge.style.getPropertyValue('stroke')).toBe(color('text-dim'));
    expect(edge).toHaveAttribute('stroke-width', '1');
    expect(edge).toHaveAttribute('pointer-events', 'none');
    expect(setBack(edge, svg)).toEqual([]);
    // one edge on the canvas: the pinned row's
    expect(svg.querySelectorAll('[data-edge]')).toHaveLength(1);
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(el.querySelector('[data-edge]')).toBeNull();
    expect(tile(svg, T - 1).style.getPropertyValue('stroke')).toBe('none');
    expect(el.querySelector('[data-strap]')).not.toBeNull();
  });

  it('explains a hovered tx, rings it where it is drawn, and links it to its proposal dot', async () => {
    const svg = await renderPanel(quiet);
    const tip = rowOf(T);
    const id = committedTxHash(T, 2);
    const q = tip.squares.find((k) => k.id === id) as NonNullable<(typeof tip.squares)[number]>;
    fireEvent.pointerMove(svg, {
      clientX: tip.x + q.x + q.s / 2,
      clientY: tip.y + q.y + q.s / 2,
    });
    expect(tooltip()).toHaveTextContent('committed in');
    expect(tooltip()).toHaveTextContent(`#${fmt(T)}`);
    const ring = svg.querySelector('[data-hover="ring"]') as SVGRectElement;
    expect(Number(ring.getAttribute('x'))).toBe(tip.x + q.x - 2);
    expect(Number(ring.getAttribute('y'))).toBe(tip.y + q.y - 2);
    // Its proposal is a dot in T−2's row: ringed there, absolute.
    const host = rowOf(T - 2);
    const k = host.chips.find((c) => c.key === `${shortIdOf(id)}|`);
    const chipRing = svg.querySelector('[data-hover="chip-ring"]') as SVGCircleElement;
    expect(Number(chipRing.getAttribute('cx'))).toBe(host.x + (k?.x as number) + G.chip / 2);
    expect(Number(chipRing.getAttribute('cy'))).toBe(host.y + (k?.y as number) + G.chip / 2);
  });

  // Lane P: the row's hover edge (lane R), on a row's plate.
  it('edges a hovered plate row in the dim ink, 1 px on its own outer pixels, and lets it go', async () => {
    const svg = await renderPanel(quiet);
    const c = rowOf(T - 2);
    const plate = tile(svg, T - 2);
    // on the plate between its number and its dots: no tx or dot under the pointer
    fireEvent.pointerMove(plate, { clientX: 66, clientY: c.y + 4 });
    expect(tooltip()).toHaveTextContent(`Block #${fmt(T - 2)}`);
    expect(plate.style.getPropertyValue('stroke')).toBe(color('text-dim'));
    expect(plate).toHaveAttribute('stroke-width', '1');
    expect(rectOf(plate)).toEqual(asRect(cardBox('column', c.w, c.h, true)));
    expect(plate.style.getPropertyValue('fill')).toBe(PLATE_FILL);
    fireEvent.pointerLeave(svg);
    expect(plate.style.getPropertyValue('stroke')).toBe('none');
    expect(rectOf(plate)).toEqual(asRect(cardBox('column', c.w, c.h, false)));
  });

  // Re-pinned with direction G: under the pointer no tick and no line, and a column has no lane
  // for the window's words (the tips name its range); no number turns gold inside the window.
  it("draws NEXT's window while NEXT's row is hovered: no tick, no words, numbers in the bright ink", async () => {
    const svg = await renderPanel(quiet);
    fireEvent.pointerMove(svg.querySelector('[data-next] [data-box]') as Element, {
      clientX: 40,
      clientY: bands.next.y + 8,
    });
    expect(tooltip()).toHaveTextContent(`Next #${fmt(T + 1)}`);
    expect(svg.querySelector('[data-ruler-ticks]')).toBeNull();
    expect(svg.querySelector('[data-window-marks]')).toBeNull();
    expect(svg.querySelector('[data-window-label]')).toBeNull();
    for (const n of [T, T - 1, T - 2, T - 3]) {
      expect(card(svg, n).querySelector('[data-block-number]'), `#${n}`).toHaveClass(
        'fill-text-bright'
      );
    }
    // the tip lies outside NEXT's window and steps back; T−1 … T−3 inside it do not
    const dim = (n: number) =>
      card(svg, n).querySelector(':scope > [data-dim]')?.getAttribute('data-dim');
    expect([T, T - 1, T - 2, T - 3].map(dim)).toEqual(['true', 'false', 'false', 'false']);
  });

  it('runs the reward link of a hovered cellbase down its lane into the dotted tail', async () => {
    const svg = await renderPanel(quiet);
    const tip = rowOf(T);
    const cb = tip.squares[0];
    const a = { X: tip.x + cb.x + cb.s / 2, Y: tip.y + cb.y + cb.s / 2 };
    fireEvent.pointerMove(svg, { clientX: a.X, clientY: a.Y });
    expect(tooltip()).toHaveTextContent(`pays reward of#${fmt(T - 11)}`);
    const link = columnRewardLink(a, null, columnChainTail(columnSlots(scene), scene.H) as never);
    expect(svg.querySelector('[data-hover="reward"]')).toHaveAttribute('d', link.d);
    expect(svg.querySelector('[data-hover="reward-tail"]')).toHaveAttribute('d', link.tail);
  });

  it('keeps every row in place when the pool tip differs from the synced tip', async () => {
    const mismatch: PipelineSnapshot = {
      ...quiet,
      status: 'poolTipMismatch',
      poolTip: { number: T + 1, hash: blockHash(T + 1) },
      pool: null,
      next: null,
    };
    const sc = buildScene(mismatch, PHONE_W, 'settled');
    // The wells and NEXT's row hold their 44 px places: the blocks sit where they always do.
    expect(sc.bands).toEqual(scene.bands);
    expect(sc.cards.map((c) => c.y)).toEqual(scene.cards.map((c) => c.y));
    expect(sc.H).toBe(scene.H);
    const svg = await renderPanel(mismatch);
    const status = svg.querySelector('[data-pool-status]') as SVGTextElement;
    expect(status).toHaveTextContent(/unavailable$/);
    expect([status.getAttribute('x'), status.getAttribute('y')].map(Number)).toEqual([
      COL.lineX,
      columnWellHeadAt(bands.pending).y,
    ]);
    expect(status.querySelector('title')).toHaveTextContent(
      `The node's tx pool is at #${fmt(T + 1)}`
    );
    expect(svg.querySelectorAll('[data-pool-grid]')).toHaveLength(0);
    expect(svg.querySelectorAll('[data-tray]')).toHaveLength(2);
    for (const id of ['pending', 'proposed']) {
      expect(svg.querySelector(`[data-pool-card="${id}"] [data-count]`)?.textContent).toBe('—');
    }
    expect(svg.querySelector('[data-next]')).toBeNull();
    const nextText = svg.querySelector('[data-next-column]') as SVGTextElement;
    expect(nextText).toHaveTextContent('unavailable');
    expect([nextText.getAttribute('x'), nextText.getAttribute('y')].map(Number)).toEqual([
      COL.nameX,
      136 + 22 + 3.5,
    ]);
    expect(svg.querySelectorAll('[data-card]')).toHaveLength(COL.blocks);
  });

  it('pins a tapped tx and shows where it is now in the footer', async () => {
    const svg = await renderPanel(quiet);
    const q = scene.pool?.pending.squares.find((k) => k.id === poolTxHash('q1'));
    tap(svg, { clientX: (q?.x as number) + 3, clientY: (q?.y as number) + 3 });
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'tx', hash: poolTxHash('q1') });
    expect(screen.getByTestId('pipeline-selection')).toHaveTextContent('waiting to be proposed');
    const pin = svg.querySelector('[data-hover="pin-ring"]') as SVGRectElement;
    expect(Number(pin.getAttribute('y'))).toBe((q?.y as number) - 2.5);
  });

  // Lane S2: a tapped predicted tx whose pool tx is not listed, on NEXT's row: proposed, its size
  // and the block it is predicted for, and the dashed pin ring on its ghost in NEXT's row.
  it('reads a pinned predicted tx out of the listed pool as proposed and rings its ghost on NEXT’s row', async () => {
    const outside = `0x${'ab'.repeat(32)}`;
    const next0 = quiet.next as NonNullable<PipelineSnapshot['next']>;
    const s: PipelineSnapshot = {
      ...quiet,
      next: { ...next0, txs: [{ hash: outside, size: 900 }, ...next0.txs] },
    };
    const sc = buildScene(s, PHONE_W, 'settled');
    const next = sc.next as NonNullable<Scene['next']>;
    const g = next.ghosts.find((k) => k.id === outside) as (typeof next.ghosts)[number];
    expect(next.y).toBe(136);
    box(PHONE_W, sc.H);
    const svg = await renderPanel(s);
    tap(svg, { clientX: next.x + g.x + g.s / 2, clientY: next.y + g.y + g.s / 2 });
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'tx', hash: outside });
    expect(
      [...screen.getByTestId('pipeline-selection').children].map((el) => el.textContent)
    ).toEqual([
      'Selected',
      shortenHash(outside, 'row'),
      'proposed',
      predictedFacts({ size: 900 }, s)
        .map(([k, v]) => `${k} ${v}`)
        .join(' · '),
      'open tx ↗',
      '×',
    ]);
    expect(screen.getByTestId('pipeline-selection')).toHaveTextContent(
      `predicted for #${fmt(T + 1)}`
    );
    const rings = svg.querySelectorAll('[data-hover="pin-ring"]');
    expect(rings).toHaveLength(1);
    expect(rectOf(rings[0])).toEqual([next.x + g.x - 2.5, next.y + g.y - 2.5, g.s + 5, g.s + 5]);
    expect(rings[0]).toHaveAttribute('stroke-dasharray', '3 2');
    // the ring lies on NEXT's row
    expect(Number(rings[0].getAttribute('y'))).toBeGreaterThan(next.y);
    expect(Number(rings[0].getAttribute('y')) + g.s + 5).toBeLessThan(next.y + next.h);
  });

  /** Where a pointer lands on the phone canvas, and the page it opens (null: none). */
  const COLUMN_SPOTS: [
    string,
    (svg: Element) => [Element, ClientPoint],
    { href: string; pin: PipelinePin } | null,
  ][] = [
    [
      'a pending tx in its well',
      (svg) => {
        const q = scene.pool?.pending.squares.find((k) => k.id === poolTxHash('q1'));
        return [svg, { clientX: (q?.x as number) + 3, clientY: (q?.y as number) + 3 }];
      },
      {
        href: `/mainnet/tx/${poolTxHash('q1')}`,
        pin: { kind: 'tx', hash: poolTxHash('q1') },
      },
    ],
    [
      'a committed tx on a row',
      (svg) => {
        const c = rowOf(T);
        const q = c.squares.find((k) => k.id === committedTxHash(T, 2));
        if (!q) throw new Error('the row of T does not draw its tx 2');
        return [svg, { clientX: c.x + q.x + q.s / 2, clientY: c.y + q.y + q.s / 2 }];
      },
      {
        href: `/mainnet/tx/${committedTxHash(T, 2)}`,
        pin: { kind: 'tx', hash: committedTxHash(T, 2) },
      },
    ],
    [
      'the bare plate of a row',
      (svg) => [tile(svg, T - 2), bareSpot(scene, T - 2)],
      { href: `/mainnet/blocks/${T - 2}`, pin: { kind: 'block', number: T - 2 } },
    ],
    [
      // the words let the pointer through to the plate
      'the number of a row',
      (svg) => {
        const c = rowOf(T - 3);
        return [tile(svg, T - 3), unpicked(scene, c.x + c.numberX + 10, c.y + columnHeadAt(c).y)];
      },
      { href: `/mainnet/blocks/${T - 3}`, pin: { kind: 'block', number: T - 3 } },
    ],
    [
      'the word of an uncle on a row',
      (svg) => {
        const c = rowOf(T - 1);
        const u = c.uncles[0];
        const word = card(svg, T - 1).querySelector('[data-uncle="0"] text') as Element;
        return [word, unpicked(scene, c.x + u.x + 10, c.y + u.y + 6)];
      },
      { href: `/mainnet/blocks/${T - 1}`, pin: { kind: 'block', number: T - 1 } },
    ],
    [
      'the next block’s row',
      (svg) => {
        const n = scene.next as NonNullable<Scene['next']>;
        return [
          svg.querySelector('[data-next] [data-box]') as Element,
          { clientX: n.x + n.w - 4, clientY: n.y + 4 },
        ];
      },
      null,
    ],
    [
      'the proposed well with no tx under the pointer',
      (svg) => bareWellSpot(svg, scene, 'proposed'),
      null,
    ],
    [
      'the mempool well with no tx under the pointer',
      (svg) => bareWellSpot(svg, scene, 'pending'),
      null,
    ],
  ];

  // Lane K: a pointer that hovers opens the page of what it clicks at once, below 589 px too.
  it.each(COLUMN_SPOTS)('opens the page of %s on a click below 589 px', async (_, at, opens) => {
    const svg = await renderRoutedPanel(quiet);
    const [el, point] = at(svg);
    press(el, point, 'mouse');
    expect(routerLocation()).toBe(opens?.href ?? '/mainnet');
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    expect(screen.getByTestId('pipeline-legend')).toBeInTheDocument();
  });

  // Lane K, the Touch rule: on a phone the first tap pins the readout of what it would open, the
  // second opens it; a tap where nothing opens clears the pin.
  it.each(COLUMN_SPOTS)(
    'pins %s on a first tap below 589 px and opens it on the second',
    async (_, at, opens) => {
      const svg = await renderRoutedPanel(quiet);
      const [el, point] = at(svg);
      // something else pinned first: a tap where nothing opens clears it
      tap(tile(svg, T), bareSpot(scene, T));
      expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'block', number: T });
      tap(el, point);
      expect(routerLocation()).toBe('/mainnet');
      if (opens === null) {
        expect(usePipelineHoverStore.getState().pinned).toBeNull();
        expect(screen.getByTestId('pipeline-legend')).toBeInTheDocument();
        return;
      }
      expect(usePipelineHoverStore.getState().pinned).toEqual(opens.pin);
      const line = screen.getByTestId('pipeline-selection');
      expect(within(line).getByRole('link')).toHaveTextContent(
        opens.pin.kind === 'tx' ? 'open tx ↗' : 'open block ↗'
      );
      if (opens.pin.kind === 'block') {
        // its row has the hovered row's 1 px edge
        expect(tile(svg, opens.pin.number).style.getPropertyValue('stroke')).toBe(
          color('text-dim')
        );
      }
      tap(el, point);
      expect(routerLocation()).toBe(opens.href);
      expect(usePipelineHoverStore.getState().pinned).toBeNull();
    }
  );
});

describe('PipelineCanvas motion below 589 px', () => {
  const PHONE_W = 358;
  const s1 = withSmallTxs(buildPipelineSnapshot());
  const w0 = poolTxHash('w0');
  const s2 = mineNext(s1, { commit: [w0], propose: [poolTxHash('q0')] });
  const t = diffSnapshots(s1, s2);
  const newHash = blockHash(T + 1);
  /** NEXT's row, where the landing block lands: 136 down, 44 px tall. */
  const LANDING = { y: 136, h: 44 };
  const xy = (el: Element) =>
    (/translate\(([\d.]+),([\d.]+)\)/.exec(el.getAttribute('transform') ?? '') ?? [])
      .slice(1)
      .map(Number);

  beforeEach(() => {
    viewW = PHONE_W;
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

  it('lobs the commit down from the proposed row into the landing row at NEXT, bowing out to the right', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    const pool = svg.querySelector(`[data-pool] [data-sq="${w0}"]`) as SVGRectElement;
    const from = { X: Number(pool.getAttribute('x')), Y: Number(pool.getAttribute('y')) };
    rerender(<PipelineCanvas snapshot={s2} />);
    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    expect(landing).toHaveAttribute('data-slot', 'landing');
    // The landing block takes NEXT's row.
    expect(buildScene(s2, PHONE_W, 'landing').bands?.next).toEqual(LANDING);
    expect(xy(landing)).toEqual([0, LANDING.y]);
    const sq = landing.querySelector(`[data-sq="${w0}"]`) as SVGRectElement;
    const abs = () => ({
      X: Number(sq.getAttribute('x')),
      Y: LANDING.y + Number(sq.getAttribute('y')),
    });
    // It starts where it sat in the proposed row.
    expect(abs().X).toBeCloseTo(from.X, 1);
    expect(abs().Y).toBeCloseTo(from.Y, 1);
    const target = buildScene(s2, PHONE_W, 'landing').cards[0].squares.find((q) => q.id === w0);
    const to = { X: target?.x as number, Y: LANDING.y + (target?.y as number) };
    const ys: number[] = [];
    let widest = 0;
    for (let ms = 0; ms < t.contact + 20; ms += 16) {
      at(16);
      ys.push(abs().Y);
      widest = Math.max(widest, abs().X);
    }
    // Down all the way, never back up; out to the right of both ends on the way (the bow peaks
    // near half of its control point's offset: 24 px + 10 % of the distance past the wider end).
    for (let i = 1; i < ys.length; i++) expect(ys[i]).toBeGreaterThanOrEqual(ys[i - 1] - 1e-6);
    expect(ys[ys.length - 1] - ys[0]).toBeGreaterThan(40);
    expect(widest).toBeGreaterThan(Math.max(from.X, to.X) + 8);
    expect(abs().X).toBeCloseTo(to.X, 1);
    expect(abs().Y).toBeCloseTo(to.Y, 1);
  });

  // Direction F in the column (lane P): the landing keeps lane R's rules on the row's geometry.
  it('keeps a landing row NEXT’s dashed outline until contact, then sets its plate, cuts and stone', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    // Settled plates, as the motion layer paints them: their own tone, no stroke, the whole row.
    expect(tile(svg, T).style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(tile(svg, T).style.getPropertyValue('stroke')).toBe('none');
    expect(rectOf(tile(svg, T))).toEqual(asRect(cardBox('column', PHONE_W, 44, false)));
    rerender(<PipelineCanvas snapshot={s2} />);
    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    const box = landing.querySelector('[data-box]') as SVGRectElement;
    const hidden = () =>
      ['[data-stone]', '[data-cuts]'].map((sel) =>
        landing.querySelector(sel)?.getAttribute('opacity')
      );
    const dashed = () => {
      expect(box.style.getPropertyValue('fill')).toBe('none');
      expect(box.style.getPropertyValue('stroke')).toBe(tok('jade'));
      expect(box).toHaveAttribute('stroke-opacity', '0.6');
      expect(box).toHaveAttribute('stroke-dasharray', '2 2');
      expect(rectOf(box)).toEqual(asRect(cardBox('column', PHONE_W, LANDING.h, true)));
      expect(hidden()).toEqual(['0', '0']);
    };
    dashed();
    at(t.contact - 20);
    dashed();
    // Contact: the plate is set, its cuts and its stone with it.
    at(40);
    expect(box.style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(box.style.getPropertyValue('stroke')).toBe('none');
    expect(box).not.toHaveAttribute('stroke-dasharray');
    expect(rectOf(box)).toEqual(asRect(cardBox('column', PHONE_W, LANDING.h, false)));
    expect(hidden()).toEqual([null, null]);
    // and stays set while it settles and slides down into the chain
    at(t.settleAt - t.contact + 900);
    expect(box.style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(box).not.toHaveAttribute('stroke-dasharray');
  });

  // Direction G, turned (lane S2): a row still landing has no plate yet, so neither the gilt down it
  // nor the edge drawn over the gilt until its plate is set at contact, even inside a drawn window
  // (here that of T+5, T−5 … T+3, and the landing row pointed at from outside).
  it('lays no gilt and no edge on a landing row until its plate is set at contact', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    act(() => usePipelineHoverStore.getState().setHover({ block: T + 1, tx: null, window: T + 5 }));
    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    expect(landing).toHaveAttribute('data-slot', 'landing');
    const strap = landing.querySelector('[data-strap]') as SVGRectElement;
    const edge = landing.querySelector('[data-edge]') as SVGRectElement;
    expect(rectOf(strap)).toEqual([5, 0, 20, LANDING.h]);
    expect([strap, edge].map((el) => el.getAttribute('opacity'))).toEqual(['0', '0']);
    at(t.contact - 20);
    expect([strap, edge].map((el) => el.getAttribute('opacity'))).toEqual(['0', '0']);
    at(40);
    expect([strap, edge].map((el) => el.getAttribute('opacity'))).toEqual([null, null]);
    expect(strap.style.getPropertyValue('fill')).toBe(GILT_FILL);
    expect(edge.style.getPropertyValue('stroke')).toBe(color('text-dim'));
    expect(strap.compareDocumentPosition(edge)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it('flares the landing row’s marks neon-white at contact and settles them to their own fill in 400 ms', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    const landed = buildScene(s2, PHONE_W, 'landing').cards[0];
    const marks = landed.squares.map((q) => ({
      el: svg.querySelector(`[data-card="${newHash}"] [data-sq="${q.id}"]`) as SVGElement,
      own: agedPaint(
        paintSquare(q.stage, q.style, q.step),
        q.kind === 'cellbase' ? 'cellbase' : 'square',
        0
      ).fill,
    }));
    expect(marks.map((m) => m.el.getAttribute('data-sq'))).toContain(w0);
    const fills = () => marks.map((m) => m.el.style.getPropertyValue('fill'));
    const NEON = color('neon-white');
    at(t.contact - 20);
    expect(fills().filter((f) => f === NEON)).toEqual([]);
    let ms = t.contact - 20;
    while (!fills().includes(NEON)) {
      at(16);
      ms += 16;
      expect(ms).toBeLessThan(t.contact + 40);
    }
    expect(fills()).toEqual(marks.map(() => NEON));
    at(FRESH_MS + 16);
    expect(fills()).toEqual(marks.map((m) => m.own));
  });

  // The column (lane S1's ruling on 26b7f8e3, the same code path as the row): while the rows step
  // down after a landing, T−3 leaves below the last row; the strap stays whole down the rows that
  // stay and its tail starts at the bottom of the last of them, at rest and with T pointed at.
  it.each([null, T])(
    'keeps the strap whole down the rows through a landing and the slide, a row leaving at the bottom (pointing at %s)',
    (pointing) => {
      const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
      const svg = getByTestId('pipeline-svg');
      if (pointing !== null) {
        act(() =>
          usePipelineHoverStore.getState().setHover({ block: pointing, tx: null, window: pointing })
        );
      }
      rerender(<PipelineCanvas snapshot={s2} />);
      const exitLayer = svg.querySelector('[data-layer="exit"]') as SVGGElement;
      const faults: string[] = [];
      let leaving = 0;
      let nearEnd = 0;
      for (let ms = 16; ms <= t.settleAt + 1000; ms += 16) {
        at(16);
        const landed = svg.querySelector(`[data-card="${newHash}"]`);
        const H = pointing ?? (landed?.getAttribute('data-slot') === 'landing' ? T + 1 : T + 2);
        if (exitLayer.querySelector('g')) leaving++;
        if (path(svg, 'pend').getAttribute('d') !== 'M0 0') {
          if (landed?.getAttribute('data-slot') === 'chain') nearEnd++;
        }
        faults.push(...strapFaults(svg, s2, 'column', H).map((f) => `${ms} ms: ${f}`));
      }
      expect(leaving).toBeGreaterThan(30);
      expect(nearEnd).toBeGreaterThan(10);
      expect(faults).toEqual([]);
    }
  );

  it('slides the landed block down one row into the chain while the older rows step down', () => {
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    at(t.settleAt - 10);
    const landing = svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement;
    expect(xy(landing)).toEqual([0, LANDING.y]);
    at(20);
    // The 700 ms move: partway down, never sideways.
    at(300);
    const mid = xy(svg.querySelector(`[data-card="${newHash}"]`) as SVGGElement);
    expect(mid[0]).toBe(0);
    expect(mid[1]).toBeGreaterThan(LANDING.y);
    expect(mid[1]).toBeLessThan(198);
    // The oldest row leaves through the exit layer, moving on down as it fades.
    const exitLayer = svg.querySelector('[data-layer="exit"]') as SVGGElement;
    const gone = exitLayer.querySelector('g') as SVGGElement;
    expect(gone).not.toBeNull();
    expect(xy(gone)[0]).toBe(0);
    expect(xy(gone)[1]).toBeGreaterThan(384);
    at(800);
    const settled = buildScene(s2, PHONE_W, 'settled');
    expect(settled.cards.map((c) => c.y)).toEqual([198, 260, 322, 384]);
    for (const c of settled.cards) {
      expect(xy(card(svg, c.block.number))).toEqual([0, c.y]);
    }
    expect(card(svg, T + 1)).toHaveAttribute('data-slot', 'chain');
    expect(svg.querySelector('[data-next]')).not.toBeNull();
    expect(exitLayer.childElementCount).toBe(0);
    // The chain followed the rows down: its gilt tail below the last row (T+2's window runs on).
    expect(path(svg, 'tail-win')).toHaveAttribute('d', columnChain(settled, T + 2).tailWin);
    expect(strapRects(path(svg, 'tail-win').getAttribute('d'))).toEqual([
      { x: 5, y: 428, w: 20, h: 30 },
    ]);
  });

  it('shows the final column at once with reduced motion: no landing, no flare', () => {
    stubMatchMedia(true);
    const { getByTestId, rerender } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    rerender(<PipelineCanvas snapshot={s2} />);
    const row = card(svg, T + 1);
    expect(xy(row)).toEqual([0, 198]);
    expect(row).toHaveAttribute('data-slot', 'chain');
    expect(svg.querySelector('[data-layer="exit"]')?.childElementCount).toBe(0);
    expect(svg.querySelector('[data-layer="fx"]')?.childElementCount).toBe(0);
    // its plate set and its stone pressed at once, and no mark ever lit
    const plate = row.querySelector('[data-box]') as SVGRectElement;
    expect(plate.style.getPropertyValue('fill')).toBe(PLATE_FILL);
    expect(plate).not.toHaveAttribute('stroke-dasharray');
    expect(row.querySelector('[data-stone]')).not.toHaveAttribute('opacity');
    expect(row.querySelector('[data-cuts]')).not.toHaveAttribute('opacity');
    expect(svg.innerHTML).not.toContain(color('neon-white'));
    expect(vi.getTimerCount()).toBe(0);
  });

  it('redraws in place when the width crosses 589 px: nothing flies between a row and a column', () => {
    const { getByTestId } = renderPlain(<PipelineCanvas snapshot={s1} />);
    const svg = getByTestId('pipeline-svg');
    expect(xy(card(svg, T))).toEqual([0, 198]);
    resizeTo(606);
    expect(svg.getAttribute('viewBox')).toBe(`0 0 606 ${G.H}`);
    expect(xy(card(svg, T))).toEqual([G.chainX0, 0]);
    expect(svg.querySelector('[data-layer="exit"]')?.childElementCount).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
    resizeTo(PHONE_W);
    expect(svg.getAttribute('viewBox')).toBe(`0 0 ${PHONE_W} 462`);
    expect(xy(card(svg, T))).toEqual([0, 198]);
    // the plate is the row's again, at the column's geometry
    expect(rectOf(tile(svg, T))).toEqual(asRect(cardBox('column', PHONE_W, 44, false)));
    expect(svg.querySelector('[data-layer="exit"]')?.childElementCount).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });
});

// Lane K ("点击 pipeline 中的 block/tx 应该跳转对应页面"): a click opens the page of what it hits,
// read by the same decision the hover reads it by: a tx of any stage, a predicted tx and a dot whose
// tx is known open the transaction; a dot whose tx is not known and anything else on a mined block
// open the block; NEXT, the wells, the window lane and the empty canvas open nothing. The tip says
// which page in one closing sentence, and the cursor is a pointer exactly there. On a touch screen
// (the Touch rule) the first tap pins the readout of what it would open and the second opens it.
describe('PipelinePanel: a click opens the page of what it hits', () => {
  const snapshot = buildPipelineSnapshot();
  const scene: Scene = buildScene(snapshot, VIEW_W, 'settled');
  const cardOf = (sc: Scene, n: number) => {
    const c = sc.cards.find((k) => k.block.number === n);
    if (!c) throw new Error(`card ${n} not on screen`);
    return c;
  };
  const squareAt = (n: number, id: string): ClientPoint => {
    const c = cardOf(scene, n);
    const q = c.squares.find((k) => k.id === id);
    if (!q) throw new Error(`square ${id} not in card ${n}`);
    return { clientX: c.x + q.x + q.s / 2, clientY: q.y + q.s / 2 };
  };
  const poolAt = (id: string): ClientPoint => {
    const pool = scene.pool as NonNullable<Scene['pool']>;
    const q = [...pool.pending.squares, ...pool.proposed.squares].find((k) => k.id === id);
    if (!q) throw new Error(`square ${id} not in the pool`);
    return { clientX: q.x + q.s / 2, clientY: q.y + q.s / 2 };
  };
  const ghostAt = (id: string): ClientPoint => {
    const next = scene.next as NonNullable<Scene['next']>;
    const g = next.ghosts.find((k) => k.id === id);
    if (!g) throw new Error(`no ghost for ${id}`);
    return { clientX: next.x + g.x + g.s / 2, clientY: g.y + g.s / 2 };
  };
  const chipAt = (sc: Scene, n: number, key: string): ClientPoint => {
    const c = cardOf(sc, n);
    const k = c.chips.find((x) => x.key === key);
    if (!k) throw new Error(`chip ${key} not in card ${n}`);
    return { clientX: c.x + k.x + G.chip / 2, clientY: c.y + k.y + G.chip / 2 };
  };
  const laneAt = (): ClientPoint => {
    const lane = windowLane(
      windowSpans(scene),
      T + 1,
      snapshot.consensus,
      VIEW_W,
      windowLabel(T + 1, T, snapshot.consensus)
    );
    return { clientX: (lane.hit?.xn as number) + 10, clientY: G.cardTop - 8 };
  };
  const txPage = (hash: string, net = 'mainnet') => `/${net}/tx/${hash}`;
  const blockPage = (n: number, net = 'mainnet') => `/${net}/blocks/${n}`;
  const tooltip = () => screen.getByTestId('pipeline-tooltip');
  const tipFoot = () =>
    screen.queryByTestId('pipeline-tooltip')?.lastElementChild?.textContent ?? null;
  const auxClick = (el: Element, at: ClientPoint, button: number) =>
    fireEvent(el, new MouseEvent('auxclick', { bubbles: true, cancelable: true, button, ...at }));

  beforeEach(() => {
    usePipelineHoverStore.getState().clear();
    Object.defineProperty(window, 'PointerEvent', {
      configurable: true,
      writable: true,
      value: PointerEventStub,
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
    usePipelineHoverStore.getState().clear();
  });

  /** Where the pointer is: the element under it and its position. */
  type Spot = (svg: Element) => [Element, ClientPoint];
  const on =
    (at: () => ClientPoint): Spot =>
    (svg) => [svg, at()];
  const T2 = committedTxHash(T, 2);

  const OPENS: [string, Spot, string, 'transaction' | 'block'][] = [
    [
      'a pending tx in the mempool well',
      on(() => poolAt(poolTxHash('q1'))),
      txPage(poolTxHash('q1')),
      'transaction',
    ],
    [
      'a proposed tx in the proposed well',
      on(() => poolAt(poolTxHash('w2'))),
      txPage(poolTxHash('w2')),
      'transaction',
    ],
    ['a committed tx on a plate', on(() => squareAt(T, T2)), txPage(T2), 'transaction'],
    ['a cellbase', on(() => squareAt(T, cellbaseHash(T))), txPage(cellbaseHash(T)), 'transaction'],
    [
      'a predicted tx on NEXT',
      on(() => ghostAt(poolTxHash('w2'))),
      txPage(poolTxHash('w2')),
      'transaction',
    ],
    [
      'a proposal dot whose tx is known',
      on(() => chipAt(scene, T - 2, `${shortIdOf(T2)}|`)),
      txPage(T2),
      'transaction',
    ],
    [
      'the bare plate of a block',
      (svg) => [tile(svg, T - 3), bareSpot(scene, T - 3)],
      blockPage(T - 3),
      'block',
    ],
    [
      // the stone and the words let the pointer through to the plate
      'the stone of a block',
      (svg) => [tile(svg, T), unpicked(scene, cardOf(scene, T).x + G.stoneX + 7, G.stoneY + 7)],
      blockPage(T),
      'block',
    ],
    [
      'the number of a block',
      (svg) => [
        tile(svg, T - 1),
        unpicked(scene, cardOf(scene, T - 1).x + G.numberX + 12, G.stoneY + 7),
      ],
      blockPage(T - 1),
      'block',
    ],
    [
      'the word of an uncle (the block that carries it)',
      (svg) => {
        const c = cardOf(scene, T - 1);
        const u = c.uncles[0];
        const word = card(svg, T - 1).querySelector('[data-uncle="0"] text') as Element;
        return [word, unpicked(scene, c.x + u.x + 10, u.y + 6)];
      },
      blockPage(T - 1),
      'block',
    ],
  ];

  it.each(OPENS)(
    'opens the page of %s, as its tip says, under a pointer cursor',
    async (_, spot, page, noun) => {
      const svg = await renderRoutedPanel(snapshot);
      const [el, at] = spot(svg);
      fireEvent.pointerMove(el, at);
      expect(svg.style.cursor).toBe('pointer');
      const foot = tipFoot() as string;
      expect(foot.match(/Click to open/g)).toHaveLength(1);
      expect(foot.endsWith(`Click to open the ${noun}.`)).toBe(true);
      expect(routerLocation()).toBe('/mainnet');
      press(el, at, 'mouse');
      expect(routerLocation()).toBe(page);
      // A mouse pins nothing: no dashed ring, and the line under the canvas is still the legend.
      expect(usePipelineHoverStore.getState().pinned).toBeNull();
      expect(svg.querySelector('[data-hover="pin-ring"]')).toBeNull();
      expect(screen.getByTestId('pipeline-legend')).toBeInTheDocument();
      expect(screen.queryByTestId('pipeline-selection')).toBeNull();
    }
  );

  it('opens the block of a proposal dot whose tx is not known, as its tip says', async () => {
    const two = withTwoUncles(snapshot);
    const sc = buildScene(two, VIEW_W, 'settled');
    const dot = cardOf(sc, T - 1).chips.find(
      (k) => k.proposal.uncleIndex === 1 && k.proposal.txHash === null
    );
    if (!dot) throw new Error('T−1 draws no dot of its second uncle');
    const svg = await renderRoutedPanel(two);
    const at = chipAt(sc, T - 1, dot.key);
    fireEvent.pointerMove(svg, at);
    expect(tooltip()).toHaveTextContent('Proposal');
    expect(svg.style.cursor).toBe('pointer');
    expect(tipFoot()?.endsWith('Click to open the block.')).toBe(true);
    expect(tipFoot()?.match(/Click to open/g)).toHaveLength(1);
    fireEvent.click(svg, at);
    expect(routerLocation()).toBe(blockPage(T - 1));
  });

  /** A tx's square in the scene, in canvas coordinates: in a pool well or on a block. */
  const squareOf = (id: string) => {
    const pool = scene.pool as NonNullable<Scene['pool']>;
    const inPool = [...pool.pending.squares, ...pool.proposed.squares].find((k) => k.id === id);
    if (inPool) return { X: inPool.x, Y: inPool.y, s: inPool.s };
    for (const c of scene.cards) {
      const q = c.squares.find((k) => k.id === id);
      if (q) return { X: c.x + q.x, Y: c.y + q.y, s: q.s };
    }
    throw new Error(`${id} has no square on screen`);
  };

  /** What a tap pins: what it would open, read back from the page it opens. */
  const pinOf = (page: string): PipelinePin => {
    const [, , kind, id] = page.split('/');
    return kind === 'tx' ? { kind: 'tx', hash: id } : { kind: 'block', number: Number(id) };
  };

  it.each(OPENS)(
    'pins %s on a first tap, marks it and shows its readout, and opens it on the second',
    async (_, spot, page) => {
      const svg = await renderRoutedPanel(snapshot);
      const [el, at] = spot(svg);
      const pin = pinOf(page);
      tap(el, at);
      expect(routerLocation()).toBe('/mainnet');
      expect(usePipelineHoverStore.getState().pinned).toEqual(pin);
      expect(screen.queryByTestId('pipeline-legend')).toBeNull();
      const line = screen.getByTestId('pipeline-selection');
      expect(line).toHaveTextContent(/^Selected/);
      const link = within(line).getByRole('link');
      expect(link).toHaveAttribute('href', page);
      if (pin.kind === 'tx') {
        expect(link).toHaveTextContent('open tx ↗');
        // the dashed pin ring, 2.5 px around the tx's square wherever the scene draws it
        const ring = svg.querySelector('[data-hover="pin-ring"]') as SVGRectElement;
        const sq = squareOf(pin.hash);
        expect(['x', 'y', 'width'].map((a) => Number(ring.getAttribute(a)))).toEqual([
          sq.X - 2.5,
          sq.Y - 2.5,
          sq.s + 5,
        ]);
        expect(ring).toHaveAttribute('stroke-dasharray', '3 2');
        for (const c of scene.cards) {
          expect(tile(svg, c.block.number).style.getPropertyValue('stroke')).toBe('none');
        }
      } else {
        expect(link).toHaveTextContent('open block ↗');
        expect(svg.querySelector('[data-hover="pin-ring"]')).toBeNull();
        // the pinned block has the hovered block's 1 px edge, and no other block has
        for (const c of scene.cards) {
          expect(tile(svg, c.block.number).style.getPropertyValue('stroke')).toBe(
            c.block.number === pin.number ? color('text-dim') : 'none'
          );
        }
      }
      tap(el, at);
      expect(routerLocation()).toBe(page);
      expect(usePipelineHoverStore.getState().pinned).toBeNull();
      expect(screen.getByTestId('pipeline-legend')).toBeInTheDocument();
    }
  );

  it('pins the block of a tapped proposal dot whose tx is not known', async () => {
    const two = withTwoUncles(snapshot);
    const sc = buildScene(two, VIEW_W, 'settled');
    const dot = cardOf(sc, T - 1).chips.find(
      (k) => k.proposal.uncleIndex === 1 && k.proposal.txHash === null
    );
    if (!dot) throw new Error('T−1 draws no dot of its second uncle');
    const svg = await renderRoutedPanel(two);
    const at = chipAt(sc, T - 1, dot.key);
    tap(svg, at);
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'block', number: T - 1 });
    expect(within(screen.getByTestId('pipeline-selection')).getByRole('link')).toHaveTextContent(
      'open block ↗'
    );
    expect(routerLocation()).toBe('/mainnet');
    tap(svg, at);
    expect(routerLocation()).toBe(blockPage(T - 1));
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
  });

  it('moves the pin to another tapped mark, the same mark of another kind included', async () => {
    const svg = await renderRoutedPanel(snapshot);
    tap(svg, poolAt(poolTxHash('q1')));
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'tx', hash: poolTxHash('q1') });
    tap(tile(svg, T - 3), bareSpot(scene, T - 3));
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'block', number: T - 3 });
    expect(svg.querySelector('[data-hover="pin-ring"]')).toBeNull();
    tap(svg, squareAt(T, T2));
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'tx', hash: T2 });
    expect(tile(svg, T - 3).style.getPropertyValue('stroke')).toBe('none');
    // The dot of the pinned tx opens what is pinned: its second tap.
    tap(svg, chipAt(scene, T - 2, `${shortIdOf(T2)}|`));
    expect(routerLocation()).toBe(txPage(T2));
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    expect(screen.queryByTestId('pipeline-error')).toBeNull();
  });

  it('opens a pinned or an unpinned mark at once on a mouse or pen click, and clears the pin', async () => {
    const svg = await renderRoutedPanel(snapshot);
    tap(svg, squareAt(T, T2));
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'tx', hash: T2 });
    press(svg, squareAt(T, T2), 'mouse');
    expect(routerLocation()).toBe(txPage(T2));
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    tap(svg, poolAt(poolTxHash('q1')));
    press(tile(svg, T - 3), bareSpot(scene, T - 3), 'pen');
    expect(routerLocation()).toBe(blockPage(T - 3));
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
  });

  // The pointer's type is the gesture's own: what its pointerdown said, read by the click it ends.
  it('takes a click no pointerdown began for a mouse, even right after a tap', async () => {
    const svg = await renderRoutedPanel(snapshot);
    tap(svg, poolAt(poolTxHash('q1')));
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'tx', hash: poolTxHash('q1') });
    fireEvent.click(tile(svg, T - 3), bareSpot(scene, T - 3));
    expect(routerLocation()).toBe(blockPage(T - 3));
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
  });

  it('opens the pinned page from the readout’s link, and clears the pin', async () => {
    const svg = await renderRoutedPanel(snapshot);
    tap(tile(svg, T - 3), bareSpot(scene, T - 3));
    fireEvent.click(screen.getByRole('link', { name: 'open block ↗' }));
    expect(routerLocation()).toBe(blockPage(T - 3));
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    tap(svg, poolAt(poolTxHash('q1')));
    fireEvent.click(screen.getByRole('link', { name: 'open tx ↗' }));
    expect(routerLocation()).toBe(txPage(poolTxHash('q1')));
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
  });

  it('clears a pinned block with Escape and with the readout’s ×', async () => {
    const svg = await renderRoutedPanel(snapshot);
    tap(tile(svg, T - 3), bareSpot(scene, T - 3));
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    expect(tile(svg, T - 3).style.getPropertyValue('stroke')).toBe('none');
    tap(tile(svg, T - 3), bareSpot(scene, T - 3));
    fireEvent.click(screen.getByRole('button', { name: 'Clear selection' }));
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    expect(screen.getByTestId('pipeline-legend')).toBeInTheDocument();
    expect(routerLocation()).toBe('/mainnet');
  });

  // The block's readout: the same pieces as a tx's, its facts the tip's own (blockFacts).
  it('reads a pinned block out in one line: its number, its tag, the facts its tip gives', async () => {
    const svg = await renderRoutedPanel(snapshot);
    tap(tile(svg, T - 1), bareSpot(scene, T - 1));
    const line = screen.getByTestId('pipeline-selection');
    expect(line).toHaveAttribute('aria-live', 'polite');
    const parts = [...line.children].map((el) => el.textContent);
    expect(parts).toEqual([
      'Selected',
      `#${fmt(T - 1)}`,
      'block',
      `transactions cellbase + 2 · proposals 2 + 2 via uncle · uncles #${fmt(T - 2)}`,
      'open block ↗',
      '×',
    ]);
    const tag = line.children[2];
    expect(tag.className).toBe(
      'text-meta border px-1.5 py-[3px] leading-none text-jade border-jade/50'
    );
    // the tip says the same three facts, from the same function
    const block = snapshot.blocks.find(
      (b) => b.number === T - 1
    ) as PipelineSnapshot['blocks'][number];
    expect(blockTip(block, snapshot).rows.slice(0, 3)).toEqual([
      ['transactions', 'cellbase + 2'],
      ['proposals', '2 + 2 via uncle'],
      ['uncles', `#${fmt(T - 2)}`],
    ]);
    // a block that has left the snapshot is no longer in view
    act(() => usePipelineHoverStore.getState().pin({ kind: 'block', number: T - 40 }));
    expect(
      [...screen.getByTestId('pipeline-selection').children].map((el) => el.textContent)
    ).toEqual(['Selected', `#${fmt(T - 40)}`, 'no longer in view', 'open block ↗', '×']);
  });

  it('reads a pinned tx out as it did: its short hash, its stage and where it is now', async () => {
    const svg = await renderRoutedPanel(snapshot);
    tap(svg, squareAt(T, T2));
    const parts = [...screen.getByTestId('pipeline-selection').children].map(
      (el) => el.textContent
    );
    expect(parts).toEqual([
      'Selected',
      shortenHash(T2, 'row'),
      'committed',
      `committed in #${fmt(T)} · its proposal is in #${fmt(T - 2)}, ${formatCount(2, 'block')} earlier`,
      'open tx ↗',
      '×',
    ]);
  });

  // Lane S2, a defect lane K reported: a predicted tx on NEXT whose pool tx is not in the listed
  // pool (the list is truncated) can be pinned by a tap. Its readout says what it is, a proposed tx
  // the next block is expected to commit, in the facts and words of its own tip (predictedFacts,
  // which predictedTip opens with), and its ghost carries the dashed pin ring a pinned square gets.
  // "no longer in view" is only for a hash that is nowhere in the snapshot.
  const OUTSIDE = `0x${'ab'.repeat(32)}`;
  const withOutsideGhost = (s: PipelineSnapshot): PipelineSnapshot => {
    const next = s.next as NonNullable<PipelineSnapshot['next']>;
    return { ...s, next: { ...next, txs: [{ hash: OUTSIDE, size: 900 }, ...next.txs] } };
  };
  it('reads a pinned predicted tx out of the listed pool as proposed, from its tip’s facts, and rings its ghost', async () => {
    const s = withOutsideGhost(snapshot);
    expect(s.pool?.pending.some((t) => t.hash === OUTSIDE)).toBe(false);
    expect(s.pool?.proposed.some((t) => t.hash === OUTSIDE)).toBe(false);
    const sc = buildScene(s, VIEW_W, 'settled');
    const next = sc.next as NonNullable<Scene['next']>;
    const g = next.ghosts.find((k) => k.id === OUTSIDE) as (typeof next.ghosts)[number];
    const svg = await renderRoutedPanel(s);
    const at = { clientX: next.x + g.x + g.s / 2, clientY: g.y + g.s / 2 };
    tap(svg, at);
    expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'tx', hash: OUTSIDE });
    const line = screen.getByTestId('pipeline-selection');
    const parts = [...line.children].map((el) => el.textContent);
    const facts = `size · area 900 B · ${pct(900, s.consensus.maxBlockBytes)} · predicted for #${fmt(T + 1)}`;
    expect(parts).toEqual([
      'Selected',
      shortenHash(OUTSIDE, 'row'),
      'proposed',
      facts,
      'open tx ↗',
      '×',
    ]);
    // the tip's own facts, from the same function, in the order it gives them
    const tip = predictedTip({ hash: OUTSIDE, size: 900 }, null, s);
    expect(tip.rows.slice(0, 2)).toEqual(predictedFacts({ size: 900 }, s));
    expect(
      predictedFacts({ size: 900 }, s)
        .map(([k, v]) => `${k} ${v}`)
        .join(' · ')
    ).toBe(facts);
    // the tag in the proposed tone
    expect(line.children[2].className).toBe(
      'text-meta border px-1.5 py-[3px] leading-none text-gold border-gold/50'
    );
    // the dashed pin ring on its ghost: 2.5 px around the ghost's box on NEXT, the only one
    const rings = svg.querySelectorAll('[data-hover="pin-ring"]');
    expect(rings).toHaveLength(1);
    expect(rectOf(rings[0])).toEqual([next.x + g.x - 2.5, next.y + g.y - 2.5, g.s + 5, g.s + 5]);
    expect(rings[0]).toHaveAttribute('stroke-dasharray', '3 2');
    // the second tap opens it
    tap(svg, at);
    expect(routerLocation()).toBe(txPage(OUTSIDE));
    // a hash that is nowhere in the snapshot: no longer in view, and no ring
    const gone = `0x${'cd'.repeat(32)}`;
    act(() => usePipelineHoverStore.getState().pin({ kind: 'tx', hash: gone }));
    expect(
      [...screen.getByTestId('pipeline-selection').children].map((el) => el.textContent)
    ).toEqual(['Selected', shortenHash(gone, 'row'), 'no longer in view', 'open tx ↗', '×']);
    expect(svg.querySelector('[data-hover="pin-ring"]')).toBeNull();
  });

  // Lane S2 (and a fix to the row of lane S1): a highlighted plate inside the window keeps its 1 px
  // edge in the dim ink over the gilt strap that crosses its head, a tapped one and one whose dot is
  // pointed at alike; a highlighted plate outside the window has its edge on its box alone.
  it('draws a highlighted plate’s 1 px edge over its gilt strap', async () => {
    const svg = await renderRoutedPanel(snapshot);
    const expectEdgeOverStrap = (n: number) => {
      const el = card(svg, n);
      const strap = el.querySelector('[data-strap]') as SVGRectElement;
      const edge = el.querySelector('[data-edge]') as SVGRectElement;
      expect(edge, `#${n}`).not.toBeNull();
      expect(strap.compareDocumentPosition(edge)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
      expect(rectOf(edge)).toEqual(asRect(cardBox('row', G.cardW, CARD_H, true)));
      expect(rectOf(edge)).toEqual(rectOf(tile(svg, n)));
      expect(edge).toHaveAttribute('fill', 'none');
      expect(edge.style.getPropertyValue('stroke')).toBe(color('text-dim'));
      expect(tile(svg, n).style.getPropertyValue('stroke')).toBe(color('text-dim'));
      expect(svg.querySelectorAll('[data-edge]')).toHaveLength(1);
    };
    tap(tile(svg, T - 3), bareSpot(scene, T - 3));
    expectEdgeOverStrap(T - 3);
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(svg.querySelector('[data-edge]')).toBeNull();
    // a proposal dot of T−2 under the pointer: T−2 is highlighted, inside NEXT's window
    fireEvent.pointerMove(svg, chipAt(scene, T - 2, `${shortIdOf(T2)}|`));
    expect(usePipelineHoverStore.getState().hoverBlock).toBe(T - 2);
    expectEdgeOverStrap(T - 2);
    // the tip lies outside NEXT's window: no strap, so its edge is its box's alone
    fireEvent.pointerMove(svg, chipAt(scene, T, scene.cards[0].chips[0].key));
    expect(usePipelineHoverStore.getState().hoverBlock).toBe(T);
    expect(card(svg, T).querySelector('[data-strap]')).toBeNull();
    expect(tile(svg, T).style.getPropertyValue('stroke')).toBe(color('text-dim'));
    expect(svg.querySelector('[data-edge]')).toBeNull();
  });

  const OPENS_NOTHING: [string, Spot, string][] = [
    [
      'NEXT',
      (svg) => [
        svg.querySelector('[data-next] [data-box]') as Element,
        unpicked(scene, G.nextX + 60, G.cardTop + 4),
      ],
      '',
    ],
    [
      'the proposed well with no tx under the pointer',
      (svg) => bareWellSpot(svg, scene, 'proposed'),
      '',
    ],
    [
      'the mempool well with no tx under the pointer',
      (svg) => bareWellSpot(svg, scene, 'pending'),
      '',
    ],
    ['the window lane', on(() => unpicked(scene, laneAt().clientX, laneAt().clientY)), 'help'],
    ['the empty canvas', on(() => unpicked(scene, 5, 5)), ''],
  ];

  it.each(OPENS_NOTHING)(
    'opens nothing on a click or a tap on %s, and its tip says nothing of clicking',
    async (_, spot, cursor) => {
      const open = vi.spyOn(window, 'open').mockReturnValue(null);
      const svg = await renderRoutedPanel(snapshot);
      const [el, at] = spot(svg);
      fireEvent.pointerMove(el, at);
      expect(svg.style.cursor).toBe(cursor);
      expect(tipFoot() ?? '').not.toMatch(/click|tap|pin/i);
      fireEvent.click(el, at);
      fireEvent.click(el, { ...at, ctrlKey: true });
      auxClick(el, at, 1);
      expect(routerLocation()).toBe('/mainnet');
      expect(open).not.toHaveBeenCalled();
      // A tap there clears the pin, and a click leaves it.
      tap(svg, squareAt(T, T2));
      press(el, at, 'mouse');
      expect(usePipelineHoverStore.getState().pinned).toEqual({ kind: 'tx', hash: T2 });
      tap(el, at);
      expect(usePipelineHoverStore.getState().pinned).toBeNull();
      expect(screen.getByTestId('pipeline-legend')).toBeInTheDocument();
      expect(routerLocation()).toBe('/mainnet');
    }
  );

  it('opens a new tab, and leaves this one where it is, on a Ctrl-click, a Meta-click or a middle click', async () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    const svg = await renderRoutedPanel(snapshot);
    tap(svg, poolAt(poolTxHash('q1')));
    fireEvent.click(svg, { ...squareAt(T, T2), ctrlKey: true });
    expect(open).toHaveBeenLastCalledWith(txPage(T2), '_blank', 'noopener,noreferrer');
    // Opening a page from the pipeline, in a new tab too, clears the pin.
    expect(usePipelineHoverStore.getState().pinned).toBeNull();
    fireEvent.click(tile(svg, T - 3), { ...bareSpot(scene, T - 3), metaKey: true });
    expect(open).toHaveBeenLastCalledWith(blockPage(T - 3), '_blank', 'noopener,noreferrer');
    auxClick(svg, poolAt(poolTxHash('q1')), 1);
    expect(open).toHaveBeenLastCalledWith(
      txPage(poolTxHash('q1')),
      '_blank',
      'noopener,noreferrer'
    );
    expect(open).toHaveBeenCalledTimes(3);
    // A right button's auxclick is not a middle click: nothing opens.
    auxClick(svg, squareAt(T, T2), 2);
    expect(open).toHaveBeenCalledTimes(3);
    expect(routerLocation()).toBe('/mainnet');
    // A plain click navigates here and opens no tab.
    fireEvent.click(svg, squareAt(T, T2));
    expect(open).toHaveBeenCalledTimes(3);
    expect(routerLocation()).toBe(txPage(T2));
  });

  it('goes to the URL the site’s Link writes for the same href, on the network being browsed', async () => {
    window.__CKBADGER_RUNTIME_CONFIG__ = {
      networks: [{ name: 'mainnet' }, { name: 'testnet' }],
      defaultNetwork: 'mainnet',
    };
    try {
      const open = vi.spyOn(window, 'open').mockReturnValue(null);
      server.use(http.get('/api/:network/v1/pipeline', () => HttpResponse.json(snapshot)));
      render(
        <MemoryRouter initialEntries={['/testnet']}>
          <RouterLocation />
          <Link href={`/tx/${T2}`}>tx link</Link>
          <Link href={`/blocks/${T - 3}`}>block link</Link>
          <PipelinePanel />
        </MemoryRouter>
      );
      const svg = await screen.findByTestId('pipeline-svg');
      const hrefOf = (name: string) => screen.getByRole('link', { name }).getAttribute('href');
      expect(hrefOf('tx link')).toBe(txPage(T2, 'testnet'));
      fireEvent.click(svg, { ...squareAt(T, T2), ctrlKey: true });
      expect(open).toHaveBeenLastCalledWith(hrefOf('tx link'), '_blank', 'noopener,noreferrer');
      fireEvent.click(tile(svg, T - 3), bareSpot(scene, T - 3));
      expect(routerLocation()).toBe(hrefOf('block link'));
      expect(routerLocation()).toBe(blockPage(T - 3, 'testnet'));
    } finally {
      delete window.__CKBADGER_RUNTIME_CONFIG__;
    }
  });

  // A pointer exactly where a click opens a page. Every point of a mined block opens a page (its
  // tx, its dot or the block), so a block's group carries the pointer itself; NEXT opens nothing,
  // so it carries none (it did); elsewhere the hover sets it: a pointer over a pool tx or a
  // predicted one, the help cursor over the window lane, the default over NEXT, a well, the canvas.
  it('shows a pointer exactly where a click opens a page', async () => {
    const svg = await renderRoutedPanel(snapshot);
    const marked = [...svg.querySelectorAll('[class*="cursor-"]')].map((el) => [
      el.getAttribute('data-number'),
      el.getAttribute('class'),
    ]);
    expect(marked).toEqual(scene.cards.map((c) => [String(c.block.number), 'cursor-pointer']));
    expect(svg.querySelector('[data-next]')).not.toHaveAttribute('class');
    expect(svg.style.cursor).toBe('');
    fireEvent.pointerMove(svg, poolAt(poolTxHash('q1')));
    expect(svg.style.cursor).toBe('pointer');
    fireEvent.pointerMove(svg, laneAt());
    expect(svg.style.cursor).toBe('help');
    fireEvent.pointerMove(svg, ghostAt(poolTxHash('w2')));
    expect(svg.style.cursor).toBe('pointer');
    const next = svg.querySelector('[data-next] [data-box]') as Element;
    fireEvent.pointerMove(next, unpicked(scene, G.nextX + 60, G.cardTop + 4));
    expect(svg.style.cursor).toBe('');
    fireEvent.pointerMove(tile(svg, T - 3), bareSpot(scene, T - 3));
    expect(svg.style.cursor).toBe('pointer');
    fireEvent.pointerLeave(svg);
    expect(svg.style.cursor).toBe('');
  });

  it('lets Escape clear the pin and nothing else', async () => {
    const svg = await renderRoutedPanel(snapshot);
    tap(svg, squareAt(T, T2));
    fireEvent.pointerMove(svg, squareAt(T, T2));
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(usePipelineHoverStore.getState()).toMatchObject({ pinned: null, hoverTx: T2 });
    expect(routerLocation()).toBe('/mainnet');
  });
});
