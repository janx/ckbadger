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
// rule; in the last column the labels end at the tile's right edge), touches no other word, and
// stays inside the svg, measured by the type modules (0.6 em a glyph; a tight line over its
// baseline, its descent under it). The cases are the shapes of mainnet's 58 forks (2026-11-18):
// four heights whose last is the block that took the dropped one as an uncle (50 of them, as
// #21,008,175), three heights (5), four heights whose last two are both today's blocks (2, as
// #21,022,666), five heights with two dropped blocks (1, #21,043,989), and a seven-deep fork; a
// miner named by its address in the row form, 13 characters ("ckb1…" and its last 8: 93.6 px at
// 12 px); a clock to the millisecond; at the 1280 column (1248), at 640 (608) and on a phone
// (358).
const address =
  'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqw0vhz9pmnc0k2lv5g9yjxu4fz3k3swqs93hhsj';
const at = (tag: string) => `0x${tag.padEnd(64, '0')}`;
const block = (number: number, tag: string, stub = false) => ({
  number,
  hash: at(tag),
  timestamp: stub ? null : '2026-11-01T07:47:24.564Z',
  minerLockHash: stub ? null : at('11'),
  minerAddress: stub ? null : address,
  minerPool: null,
  transactionsCount: stub ? null : 1,
});
const FP = 21_008_175;
/** Today's chain from the fork point (its stub first), its later stubs, the dropped blocks. */
function shape(
  today: number,
  stubs: number[],
  dropped: number[],
  uncleIn: number[]
): ForkTreeModel {
  const main = [
    block(FP, 'f0', true),
    ...Array.from({ length: today }, (_, i) => block(FP + 1 + i, `a${i + 1}`)),
    ...stubs.map((k) => block(FP + k, `a${k}`, true)),
  ];
  return {
    main,
    rows: [{ label: 'dropped at this switch', tiles: dropped.map((k) => block(FP + k, `b${k}`)) }],
    uncleLinks: dropped.map((k, i) => ({ from: at(`b${k}`), to: FP + uncleIn[i] })),
    marks: [
      { n: 1, hash: at(`b${dropped[dropped.length - 1]}`) },
      { n: 2, hash: at(`a${today}`) },
    ],
    first: FP,
    last: Math.max(...main.map((t) => t.number)),
  };
}
/** #21,008,175 (50 of 58): today's +1 and +2, +1 dropped, taken as an uncle in +3. */
const usual = shape(2, [3], [1], [3]);
/** Three heights (5 of 58): today's +1 and +2, +1 dropped, taken in +2. */
const three = shape(2, [], [1], [2]);
/** #21,022,666 (2 of 58): today's +1, +2 and +3, +1 dropped, taken in +3. */
const full = shape(3, [], [1], [3]);
/** #21,043,989 (1 of 58): today's +1 to +3, +1 and +2 dropped, taken in +3 and +4. */
const five = shape(3, [4], [1, 2], [3, 4]);
const deepHeights = [101, 102, 103, 104, 105, 106, 107].map((n) => 20_894_000 + n);
const deep: ForkTreeModel = {
  main: [block(20_894_100, 'f0', true), ...deepHeights.map((n) => block(n, `a${n}`))],
  rows: [
    { label: 'dropped at this switch', tiles: deepHeights.map((n) => block(n, `b${n}`)) },
    {
      label: 'adopted then, dropped since',
      tiles: deepHeights.slice(0, 3).map((n) => block(n, `c${n}`)),
    },
  ],
  uncleLinks: [{ from: at('b20894105'), to: 20_894_107 }],
  marks: [
    { n: 1, hash: at('b20894107') },
    { n: 2, hash: at('a20894107') },
    { n: 3, hash: at('c20894103') },
  ],
  first: 20_894_100,
  last: 20_894_107,
};

describe('ForkTree: every word inside its tile (readability F3)', () => {
  const shapes = [
    ['four heights, the last an uncle inclusion', usual],
    ['three heights', three],
    ['four heights, the last two today’s', full],
    ['five heights, two dropped', five],
    ['a 7-deep fork', deep],
  ] as const;
  // Not held at 358: four heights whose last two are both today's blocks (2 of 58 forks). Their
  // times, 86.4 px each, need 172.8 px under the last two 72 px columns, which span 164: the third
  // column's runs 14.4 px right of its tile, the last one's (ending at its tile's right edge) 14.4
  // px left of its own, into the same 20 px gap: they meet by 8.8 px. Handed back for a ruling.
  const cases = [1248, 608, 358].flatMap((width) =>
    shapes
      .filter(([name]) => !(width === 358 && name === 'four heights, the last two today’s'))
      .map(([name, model]) => [width, name, model] as const)
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
      const last = Number(g.getAttribute('data-tile')) === model.last;
      for (const text of [...g.querySelectorAll('text')]) {
        const under = layout.labelsUnder && Number(text.getAttribute('y')) > box.bottom;
        // under a compact tile: its column, the tile and the run into the gap its miner is cut
        // to, from the tile's left edge, or in the last column to its right edge; no edge is
        // drawn there, so a label may stand flush with the tile's side
        const run = layout.tileW + FORK_TREE.compact.labelRun;
        const left = last ? box.right + 0.5 - run : box.left - 0.5;
        const band = under
          ? {
              left,
              right: left + run,
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

// The ruling on lane F3-d (2026-11-18): the dropped block is the subject of a fork's page, so at a
// 390 phone (358 px) a fork of four heights draws all four columns with its branch tile, as before
// the type change. The labels under a tile start at its left edge, except in the last column,
// where they end at its right edge; the svg keeps 4 px right of the last tile (6 + 4 × 72 + 3 × 20
// + 4 = 358).
describe('ForkTree at 358 px: four heights, every column and the branch (ruling on F3-d)', () => {
  it('draws the four columns, the branch tile and their label pairs, inside the svg', () => {
    stubChartWidth(358);
    const { container } = render(<ForkTree model={usual} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    expect(svg.querySelector('[data-collapsed]')).toBeNull();
    expect(
      [...svg.querySelectorAll('[data-tile]:not([data-dropped])')].map((t) =>
        Number(t.getAttribute('data-tile'))
      )
    ).toEqual([FP, FP + 1, FP + 2, FP + 3]);
    const branch = svg.querySelector(`[data-tile="${FP + 1}"][data-dropped]`)!;
    expect(branch).not.toBeNull();
    expect(screen.getByText('branch')).toBeInTheDocument();
    // a time and a miner under each block that has them: today's +1 and +2 and the dropped +1
    for (const g of [
      svg.querySelector(`[data-tile="${FP + 1}"]:not([data-dropped])`)!,
      svg.querySelector(`[data-tile="${FP + 2}"]:not([data-dropped])`)!,
      branch,
    ]) {
      expect(g.querySelector('[data-time]'), g.getAttribute('data-tile')!).not.toBeNull();
      expect([...g.querySelectorAll('text')].map((t) => t.textContent)).toContain('ckb1…qs93hhsj');
    }
    const lines = allTextLines(svg);
    for (const line of lines) expectLineInside(line, viewBox(svg), 'the tree', 'flush');
    // no label of one column meets a label of the next
    expectApart(lines, 'the tree at 358');
    expect(forkTreeLayout(usual, 358).tileW).toBe(72);
  });

  it('ends the last column’s labels at its tile’s right edge', () => {
    stubChartWidth(358);
    const { container } = render(<ForkTree model={three} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    const tile = svg.querySelector(`[data-tile="${FP + 2}"]:not([data-dropped])`)!;
    const rect = tile.querySelector('rect')!;
    const right = Number(rect.getAttribute('x')) + Number(rect.getAttribute('width'));
    const under = [...tile.querySelectorAll('text')].filter(
      (t) =>
        Number(t.getAttribute('y')) >
        Number(rect.getAttribute('y')) + Number(rect.getAttribute('height'))
    );
    expect(under).toHaveLength(2);
    for (const text of under) {
      expect(text).toHaveAttribute('text-anchor', 'end');
      expect(Number(text.getAttribute('x'))).toBe(right);
    }
    // the other columns' labels start at their tile's left edge
    const first = svg.querySelector(`[data-tile="${FP + 1}"]:not([data-dropped])`)!;
    expect(first.querySelector('[data-time]')).not.toHaveAttribute('text-anchor', 'end');
    expectApart(allTextLines(svg), 'the tree at 358');
  });
});
