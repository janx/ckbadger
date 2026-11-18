import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { ForkTree } from '@/components/forks/fork-tree';
import { FORK_TREE, forkTreeLayout, forkTreeModel, type ForkTreeModel } from '@/lib/fork-tree';
import {
  allTextLines,
  expectApart,
  expectInside as expectLineInside,
  rectBox,
  textLines,
  viewBox,
} from '@/__tests__/utils/svg-text-boxes';
import type { ReorgDetail } from '@/lib/api';
import multi from '@/__tests__/fixtures/fork-api-producer/fork-multi-uncle-detail.json';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { rects, roundedRects } from '@/__tests__/utils/square-corners';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Every rect and circle of the tree lies inside `width` (nothing to scroll sideways). */
function expectInside(svg: Element, width: number) {
  for (const rect of Array.from(svg.querySelectorAll('rect'))) {
    expect(Number(rect.getAttribute('x'))).toBeGreaterThanOrEqual(0);
    expect(Number(rect.getAttribute('x')) + Number(rect.getAttribute('width'))).toBeLessThanOrEqual(
      width
    );
  }
  for (const circle of Array.from(svg.querySelectorAll('circle'))) {
    const [cx, r] = [Number(circle.getAttribute('cx')), Number(circle.getAttribute('r'))];
    expect(cx - r).toBeGreaterThanOrEqual(0);
    expect(cx + r).toBeLessThanOrEqual(width);
  }
}

describe('ForkTree (Fork detail V67, FF-D4)', () => {
  it('draws main-row tiles, the dashed branch above, one jade arrow per uncle and both tip marks', () => {
    stubChartWidth(1248);
    const model = forkTreeModel(multi as unknown as ReorgDetail);
    const { container } = render(<ForkTree model={model} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    const tiles = Array.from(svg.querySelectorAll('[data-tile]'));
    expect(
      tiles.filter((t) => !t.hasAttribute('data-dropped')).map((t) => t.getAttribute('data-tile'))
    ).toEqual(['10', '11', '12', '13']);
    const dropped = svg.querySelectorAll('[data-tile][data-dropped] rect');
    expect(dropped).toHaveLength(2);
    expect(dropped[0]).toHaveAttribute('stroke-dasharray', '3 3');
    const links = svg.querySelectorAll('[data-uncle-link]');
    expect(links).toHaveLength(2);
    expect(links[0]).toHaveAttribute('stroke', 'var(--c-jade)');
    expect(Array.from(svg.querySelectorAll('[data-tip-mark]')).map((m) => m.textContent)).toEqual([
      '1',
      '2',
    ]);
    // dropped tiles sit above the main row
    const y = (sel: string) => Number(svg.querySelector(sel)!.getAttribute('y'));
    expect(y('[data-tile][data-dropped] rect')).toBeLessThan(
      y('[data-tile="11"]:not([data-dropped]) rect')
    );
    // drawn at its natural size: 1:1 text at 1440, a sideways scroll at 390 (visual pass)
    const viewBoxWidth = svg.getAttribute('viewBox')!.split(' ')[2];
    expect(svg.getAttribute('width')).toBe(viewBoxWidth);
    expect(svg).not.toHaveAttribute('width', '100%');
  });

  // Re-pinned (readability F3): four 72 px blocks with their 12 px time and miner under them need
  // 370 px (6 + 4 × 72 + 3 × 20 + the last column's 16 px label run); they fitted 358 at 10 px.
  // At 358 the oldest heights fold (the seven-deep case below and the layout's own test).
  it('fits 370 px: blocks shrink to 72 px, time and miner move under them, nothing scrolls (V4)', () => {
    stubChartWidth(370);
    const model = forkTreeModel(multi as unknown as ReorgDetail);
    const { container } = render(<ForkTree model={model} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    expect(svg).toHaveAttribute('width', '370');
    expect(svg.getAttribute('viewBox')).toMatch(/^0 0 370 /);
    expectInside(svg, 370);
    // the branch line, the dashed lost blocks, the uncle arcs and ① ② are all still drawn
    expect(svg.querySelectorAll('[data-tile][data-dropped] rect')[0]).toHaveAttribute(
      'stroke-dasharray',
      '3 3'
    );
    expect(svg.querySelectorAll('[data-uncle-link]')).toHaveLength(2);
    expect(svg.querySelectorAll('[data-tip-mark]')).toHaveLength(2);
    // a tile's time sits under its box, not inside it
    const tile = svg.querySelector('[data-tile="11"]:not([data-dropped])')!;
    const box = tile.querySelector('rect')!;
    const time = tile.querySelector('[data-time]')!;
    expect(Number(time.getAttribute('y'))).toBeGreaterThan(
      Number(box.getAttribute('y')) + Number(box.getAttribute('height'))
    );
    expect(Number(box.getAttribute('width'))).toBe(72);
  });

  // re-pinned (readability F3): at 358 the four heights fold to "+2 earlier" and two columns, so
  // four boxes are drawn there (the fold's, two main tiles, one dropped); 370 draws all six
  it.each([
    [1248, 6],
    [370, 6],
    [358, 4],
  ])('draws every block tile square at %i px, the dropped ones too (U-02, R1)', (width, boxes) => {
    stubChartWidth(width);
    const { container } = render(
      <ForkTree model={forkTreeModel(multi as unknown as ReorgDetail)} />
    );
    const svg = container.querySelector('svg[data-fork-tree]')!;
    expect(rects(svg).length).toBeGreaterThanOrEqual(boxes);
    expect(roundedRects(svg)).toEqual([]);
  });

  it('collapses a seven-deep fork’s oldest blocks into "+N earlier" instead of scrolling (V4)', () => {
    stubChartWidth(358);
    const hash = (tag: string) => `0x${tag.padEnd(64, '0')}`;
    const tile = (number: number, tag: string, stub = false) => ({
      number,
      hash: hash(tag),
      timestamp: stub ? null : '2026-11-01T07:47:24.564Z',
      minerLockHash: stub ? null : hash('11'),
      minerAddress: stub ? null : 'ckb1qyqexample93hhsj',
      minerPool: null,
      transactionsCount: stub ? null : 1,
    });
    const deep: ForkTreeModel = {
      main: [
        tile(100, 'f0', true),
        ...[101, 102, 103, 104, 105, 106, 107].map((n) => tile(n, `a${n}`)),
      ],
      rows: [
        {
          label: 'dropped at this switch',
          tiles: [101, 102, 103, 104, 105, 106, 107].map((n) => tile(n, `b${n}`)),
        },
      ],
      uncleLinks: [{ from: hash('b105'), to: 107 }],
      marks: [
        { n: 1, hash: hash('b107') },
        { n: 2, hash: hash('a107') },
      ],
      first: 100,
      last: 107,
    };
    const { container } = render(<ForkTree model={deep} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    expectInside(svg, 358);
    const collapsed = svg.querySelector('[data-collapsed]')!;
    expect(collapsed).not.toBeNull();
    expect(screen.getByText(/^\+\d+ earlier$/)).toBeInTheDocument();
    const n = Number(screen.getByText(/^\+\d+ earlier$/).textContent!.match(/\d+/)![0]);
    // the hidden heights are named, and the newest blocks are drawn whole
    expect(collapsed.textContent).toContain(`#100–#${100 + n - 1}`);
    expect(svg.querySelector('[data-tile="107"]:not([data-dropped])')).not.toBeNull();
    expect(svg.querySelector('[data-tile="100"]')).toBeNull();
    expect(svg.querySelectorAll('[data-tip-mark]')).toHaveLength(2);
    expect(svg.querySelectorAll('[data-uncle-link]')).toHaveLength(1);
    // U-02, R1: the "+N earlier" box is a square-cornered tile like the blocks it stands for
    expect(roundedRects(collapsed)).toEqual([]);
  });
});

// Readability plan 2026-11-18, lane F3 (the figures' text budgets): at the 12 px floor every word
// of the tree stands whole inside its tile (the wide form: number, hash, time, miner) or inside the
// band under it (the compact form: time and miner, a miner cut to its column by the tree's own
// rule), touches no other word, and stays inside the svg, measured by the type modules (0.6 em a
// glyph; a tight line over its baseline, its descent under it). The real case: a miner named by
// its address in the row form, 13 characters ("ckb1…" and its last 8: 93.6 px at 12 px), a clock
// to the millisecond, the live fork of 4 heights and a 7-deep fork, at the 1280 column (1248), at
// 640 (608) and on a phone (358).
describe('ForkTree: every word inside its tile (readability F3)', () => {
  const address =
    'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqw0vhz9pmnc0k2lv5g9yjxu4fz3k3swqs93hhsj';
  const hash = (tag: string) => `0x${tag.padEnd(64, '0')}`;
  const tile = (number: number, tag: string, stub = false) => ({
    number,
    hash: hash(tag),
    timestamp: stub ? null : '2026-11-01T07:47:24.564Z',
    minerLockHash: stub ? null : hash('11'),
    minerAddress: stub ? null : address,
    minerPool: null,
    transactionsCount: stub ? null : 1,
  });
  const live: ForkTreeModel = {
    main: [
      tile(20_894_011, 'f0', true),
      tile(20_894_012, 'a12'),
      tile(20_894_013, 'a13'),
      tile(20_894_014, 'a14'),
    ],
    rows: [{ label: 'dropped at this switch', tiles: [tile(20_894_012, 'b12')] }],
    uncleLinks: [{ from: hash('b12'), to: 20_894_014 }],
    marks: [
      { n: 1, hash: hash('b12') },
      { n: 2, hash: hash('a13') },
    ],
    first: 20_894_011,
    last: 20_894_014,
  };
  const deepHeights = [101, 102, 103, 104, 105, 106, 107].map((n) => 20_894_000 + n);
  const deep: ForkTreeModel = {
    main: [tile(20_894_100, 'f0', true), ...deepHeights.map((n) => tile(n, `a${n}`))],
    rows: [
      { label: 'dropped at this switch', tiles: deepHeights.map((n) => tile(n, `b${n}`)) },
      {
        label: 'adopted then, dropped since',
        tiles: deepHeights.slice(0, 3).map((n) => tile(n, `c${n}`)),
      },
    ],
    uncleLinks: [{ from: hash('b20894105'), to: 20_894_107 }],
    marks: [
      { n: 1, hash: hash('b20894107') },
      { n: 2, hash: hash('a20894107') },
      { n: 3, hash: hash('c20894103') },
    ],
    first: 20_894_100,
    last: 20_894_107,
  };
  const cases = [1248, 608, 358].flatMap((width) =>
    (
      [
        ['the live fork', live],
        ['a 7-deep fork', deep],
      ] as const
    ).map(([name, model]) => [width, name, model] as const)
  );

  it.each(cases)('at %i px, %s', (width, _name, model) => {
    stubChartWidth(width);
    const layout = forkTreeLayout(model, width);
    const { container } = render(<ForkTree model={model} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    const all = allTextLines(svg);
    for (const line of all) expectLineInside(line, viewBox(svg), 'the tree', 'flush');
    expectApart(all, 'the tree');
    for (const g of [...svg.querySelectorAll('[data-tile], [data-collapsed]')]) {
      const rect = g.querySelector('rect')!;
      const box = rectBox(rect);
      const what = `${g.getAttribute('data-tile') ?? '+N earlier'} (${layout.form})`;
      for (const text of [...g.querySelectorAll('text')]) {
        const under = layout.labelsUnder && Number(text.getAttribute('y')) > box.bottom;
        // under a compact tile: its column, the tile and the run into the gap its miner is cut
        // to, 4 px short of the next column; no edge is drawn there, so a label may start flush
        // with the tile's side
        const band = under
          ? {
              left: box.left - 0.5,
              right: box.left - 0.5 + layout.tileW + FORK_TREE.compact.labelRun,
              top: box.bottom,
              bottom: box.bottom + FORK_TREE.compact.under,
            }
          : box;
        for (const line of textLines(text))
          expectLineInside(line, band, what, under ? 'flush' : 'clear');
      }
    }
    // the miner's 13 characters stand whole in a wide tile
    if (layout.form === 'wide')
      expect(all.filter((l) => l.text === 'ckb1…qs93hhsj').length).toBeGreaterThan(0);
  });
});
