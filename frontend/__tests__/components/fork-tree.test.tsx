import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { ForkTree } from '@/components/forks/fork-tree';
import { forkTreeLayout, forkTreeModel, type ForkTreeModel } from '@/lib/fork-tree';
import {
  allTextLines,
  expectApart,
  expectInside as expectLineInside,
  meetings,
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

  // Re-pinned three times: lane F3-d moved this to 370 px; the ruling on F3-d set it back to 358,
  // the last column's labels ending at its tile's right edge (6 + 4 × 72 + 3 × 20 + 4 = 358, time
  // and miner under the boxes); lane P4 turns the phone form to a column (it was "fits 358 px:
  // blocks shrink to 72 px, time and miner move under them"): one block a row, the time and miner
  // beside the tile, on its row, from x 132 (4 + 56 + 8 + 56 + 8).
  it('turns to a column at 358 px: one block a row, its time and miner beside it, nothing scrolls (V4, P4)', () => {
    stubChartWidth(358);
    const model = forkTreeModel(multi as unknown as ReorgDetail);
    const { container } = render(<ForkTree model={model} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    expect(svg).toHaveAttribute('width', '358');
    expect(svg.getAttribute('viewBox')).toMatch(/^0 0 358 /);
    expectInside(svg, 358);
    // the branch line, the dashed lost blocks, the uncle arcs and ① ② are all still drawn
    expect(svg.querySelectorAll('[data-tile][data-dropped] rect')[0]).toHaveAttribute(
      'stroke-dasharray',
      '3 3'
    );
    expect(svg.querySelectorAll('[data-uncle-link]')).toHaveLength(2);
    expect(svg.querySelectorAll('[data-tip-mark]')).toHaveLength(2);
    // a tile's time stands beside its box, on its row
    const tile = svg.querySelector('[data-tile="11"]:not([data-dropped])')!;
    const box = tile.querySelector('rect')!;
    const time = tile.querySelector('[data-time]')!;
    const top = Number(box.getAttribute('y'));
    expect(Number(time.getAttribute('y'))).toBeGreaterThan(top);
    expect(Number(time.getAttribute('y'))).toBeLessThan(top + Number(box.getAttribute('height')));
    expect(Number(time.getAttribute('x'))).toBe(132);
    expect(Number(box.getAttribute('width'))).toBe(56);
    // the lanes are named over them: today's chain, then the branch
    expect(
      [...svg.querySelectorAll('[data-lane-name]')].map((t) => [t.textContent, t.getAttribute('x')])
    ).toEqual([
      ['main chain', '4'],
      ['branch', '68'],
    ]);
  });

  // re-pinned twice: lane F3-d had [1248, 6], [370, 6], [358, 4] (358 folded); the ruling on F3-d
  // draws every tile at 358 again
  it.each([
    [1248, 6],
    [358, 6],
  ])('draws every block tile square at %i px, the dropped ones too (U-02, R1)', (width, boxes) => {
    stubChartWidth(width);
    const { container } = render(
      <ForkTree model={forkTreeModel(multi as unknown as ReorgDetail)} />
    );
    const svg = container.querySelector('svg[data-fork-tree]')!;
    expect(rects(svg).length).toBeGreaterThanOrEqual(boxes);
    expect(roundedRects(svg)).toEqual([]);
  });

  // Re-pinned (lane P4): it was "collapses a seven-deep fork's oldest blocks into "+N earlier"
  // instead of scrolling", the fold taking the fork point and the branch's first blocks; a fold
  // never takes the branch, so the column draws every block, the column growing a row a block
  it('draws a seven-deep fork whole in the column, every block a row, instead of folding (V4, P4)', () => {
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
    expect(svg.querySelector('[data-collapsed]')).toBeNull();
    expect(screen.queryByText(/earlier$/)).toBeNull();
    // the fork point, today's seven and the dropped seven: 15 tiles, 15 rows, oldest first
    const tiles = [...svg.querySelectorAll('[data-tile]')];
    expect(tiles).toHaveLength(15);
    const ys = tiles.map((g) => Number(g.querySelector('rect')!.getAttribute('y')));
    expect(new Set(ys).size).toBe(15);
    expect(svg.querySelector('[data-tile="100"]')).not.toBeNull();
    expect(svg.querySelectorAll('[data-tile][data-dropped]')).toHaveLength(7);
    expect(svg.querySelectorAll('[data-tip-mark]')).toHaveLength(2);
    expect(svg.querySelectorAll('[data-uncle-link]')).toHaveLength(1);
    // U-02, R1: every tile square-cornered
    expect(roundedRects(svg)).toEqual([]);
    // 15 rows of 36 px, 18 px apart, under two lane names (15 px each) and 18 px: 30 + 18 + 15 × 36
    // + 14 × 18 + 4 = 844 px
    expect(svg.getAttribute('height')).toBe('844');
  });
});

// Readability plan 2026-11-18, lane F3 (the figures' text budgets): at the 12 px floor every word
// of the tree stands whole inside its tile (the wide form: number, hash, time, miner) or inside the
// band under it (the compact form: time and miner, an unnamed miner's address in the id form its
// column holds; in the last column the labels end at the tile's right edge), touches no other word, and
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

/**
 * The faults of the tree's phone form that are known and not this lane's to fix, each pinned by
 * what it measures (the ruling on review F-B's test finding 1: no case is filtered out of the
 * table; a case whose fault a later lane owns runs, and its fault is held at its measured size).
 * Every other case holds the property whole. The list can only shrink: a fix fails its entry's
 * pin, and the entry goes with the fix.
 */
// Lane P4 (rd/p-topics) took its one entry: at 358 px the times of today's +2 (from its tile's
// left edge) and of +3 (ending at its tile's right edge, the last column), 86.4 px each under
// columns that span 72 + 20 + 72 = 164 px, met by 2 × 86.4 − 164 = 8.8 px (#21,022,666 and
// #21,006,834); the phone form is a column now, each tile's labels beside it on its own row.
const KNOWN_FAULTS: readonly {
  width: number;
  shape: string;
  meets: { a: string; b: string; by: number };
  owner: string;
  why: string;
}[] = [];

describe('ForkTree: every word inside its tile (readability F3)', () => {
  const shapes = [
    ['four heights, the last an uncle inclusion', usual],
    ['three heights', three],
    ['four heights, the last two today’s', full],
    ['five heights, two dropped', five],
    ['a 7-deep fork', deep],
  ] as const;
  // lane P4 adds the 360 phone's 326 px (a 326 px canvas folded every usual fork before)
  const cases = [1248, 608, 358, 326].flatMap((width) =>
    shapes.map(([name, model]) => [width, name, model] as const)
  );

  it('pins the known faults it holds, which only shrink', () => {
    // Lane P4: the one fault went with the column form (above); the list is empty
    expect(KNOWN_FAULTS).toHaveLength(0);
    for (const fault of KNOWN_FAULTS)
      expect(cases.some(([w, name]) => w === fault.width && name === fault.shape)).toBe(true);
  });

  /*
   * Re-pinned (lane P4): the compact form's band under a tile (its tile and 16 px of the 20 px gap
   * after it, 38 px under its foot) went with the compact form. In the column a tile's own words
   * (its number and hash) stand inside its box, and its time and miner in its row's band of the
   * words' column: from 8 px right of the last lane's right edge to 4 px short of the svg's right
   * edge, across, and the tile's own top and foot, down; measured here by literal arithmetic and
   * the drawn rects, not by the module's constants.
   */
  it.each(cases)('at %i px, %s', (width, name, model) => {
    stubChartWidth(width);
    const layout = forkTreeLayout(model, width);
    const { container } = render(<ForkTree model={model} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    const all = allTextLines(svg);
    for (const line of all) expectLineInside(line, viewBox(svg), 'the tree', 'flush');
    // no two words meet but a known fault, held at its measured overlap
    expect(meetings(all), 'the tree').toEqual(
      KNOWN_FAULTS.filter((f) => f.width === width && f.shape === name).map((f) => f.meets)
    );
    expect(layout.form).toBe(
      width === 1248 || (width === 608 && model !== five && model !== deep) ? 'wide' : 'column'
    );
    const tiles = [...svg.querySelectorAll('[data-tile]')];
    const lanesRight = Math.max(...tiles.map((g) => rectBox(g.querySelector('rect')!).right));
    const svgRight = viewBox(svg).right;
    for (const g of tiles) {
      const rect = g.querySelector('rect')!;
      const box = rectBox(rect);
      const what = `${g.getAttribute('data-tile')} (${layout.form})`;
      for (const text of [...g.querySelectorAll('text')]) {
        const beside = layout.form === 'column' && Number(text.getAttribute('x')) > box.right;
        const band = beside
          ? {
              left: lanesRight + 8 - 0.5,
              right: svgRight - 4 + 0.5,
              top: box.top,
              bottom: box.bottom,
            }
          : box;
        for (const line of textLines(text))
          expectLineInside(line, band, what, beside ? 'flush' : 'clear');
      }
    }
    // An unnamed miner is its address in one of the id forms (lib/shorten.ts) chosen by its room,
    // never cut again: the row form, 13 characters (93.6 px at 12 px), where it stands whole — in
    // a wide tile, its width less 8 px each side (110 − 16 = 94), or in the column's words, from
    // the last lane's right edge and 8 px to 4 px short of the svg's edge (192 px at 326 with two
    // lanes, 128 with three) — else the tight form, 11 characters (79.2 px). Each has the one
    // ellipsis.
    const miners = all.filter((l) => l.text.startsWith('ckb1'));
    expect(miners.length).toBeGreaterThan(0);
    let checked = 0;
    for (const g of tiles) {
      const n = Number(g.getAttribute('data-tile'));
      const room = layout.form === 'wide' ? layout.tileW - 16 : svgRight - 4 - (lanesRight + 8);
      for (const text of [...g.querySelectorAll('text')].filter((t) =>
        t.textContent!.startsWith('ckb1')
      )) {
        expect(text.textContent, `${n} in ${room} px`).toBe(
          13 * 7.2 <= room ? 'ckb1…qs93hhsj' : 'ckb1…93hhsj'
        );
        checked++;
      }
    }
    expect(checked).toBe(miners.length);
  });
});

// The ruling on lane F3-d (2026-11-18): the dropped block is the subject of a fork's page, so at a
// 390 phone (358 px) a fork of four heights draws all four heights with its branch tile. Re-pinned
// (lane P4): it was "ForkTree at 358 px: four heights, every column and the branch", four 72 px
// columns with each tile's time and miner under it (6 + 4 × 72 + 3 × 20 + 4 = 358), the miner in
// the tight form (an 88 px column); the phone form is a column now, one block a row, and the words
// beside the tiles have 358 − 132 − 4 = 222 px, so the miner is the row form, the tables' own.
describe('ForkTree at 358 px: four heights, every block and the branch, in a column (F3-d, P4)', () => {
  it('draws the four heights, the branch tile and their words, inside the svg', () => {
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
    // a time and a miner beside each block that has them: today's +1 and +2 and the dropped +1
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
    expectApart(lines, 'the tree at 358');
    expect(forkTreeLayout(usual, 358).tileW).toBe(56);
  });

  // Re-pinned (lane P4): it was "ends the last column's labels at its tile's right edge"; in the
  // column every row's words start at one x, 8 px right of the last lane
  it('starts every row’s words at one x, right of the lanes, in row order of height', () => {
    stubChartWidth(358);
    const { container } = render(<ForkTree model={three} />);
    const svg = container.querySelector('svg[data-fork-tree]')!;
    const words = [...svg.querySelectorAll('[data-tile] text')].filter(
      (t) => t.hasAttribute('data-time') || t.textContent!.startsWith('ckb1')
    );
    // today's +1 and +2 and the dropped +1, each a time and a miner
    expect(words).toHaveLength(6);
    for (const text of words) {
      expect(text).not.toHaveAttribute('text-anchor');
      expect(Number(text.getAttribute('x'))).toBe(132);
    }
    // the rows: the fork point, today's +1, the dropped +1, today's +2
    expect(
      [...svg.querySelectorAll('[data-tile]')]
        .map((g) => ({
          n: Number(g.getAttribute('data-tile')) - FP,
          dropped: g.hasAttribute('data-dropped'),
          y: Number(g.querySelector('rect')!.getAttribute('y')),
        }))
        .sort((a, b) => a.y - b.y)
        .map((r) => `${r.n}${r.dropped ? '′' : ''}`)
    ).toEqual(['0', '1', '1′', '2']);
    expectApart(allTextLines(svg), 'the tree at 358');
  });
});

// Review F-B (logic finding 5): a pool's name is the operator's text (docs/metadata/pools.toml)
// and may be in any script. It was cut by UTF-16 unit and measured at 0.6 em a unit; no face the
// site serves holds a CJK character or an emoji, which draw from a fallback face. It is measured
// at 1.25 em a grapheme outside the served faces' Latin subset (15 px at 12 px; a Latin glyph
// 7.2; the figures' measure, not a bound: the lead's ruling), and cut at whole graphemes with the
// one ellipsis. In a wide tile the room is 94 px between its insets. Re-pinned (lane P4): under a
// 72 px compact tile the room was 88 px, and at 358 px "蚂蚁矿池 AntPool 🐜" was cut to "蚂蚁矿池 A…"
// and eight ants to five and "…"; in the column the words have 358 − 132 − 4 = 222 px (192 at
// 326), so that name stands whole (4 × 15 + 9 × 7.2 + 15 = 139.8 px) and sixteen ants are cut to
// fourteen (222) or twelve (192) and the ellipsis.
describe('ForkTree: a pool’s name in any script (review F-B, logic finding 5)', () => {
  it.each([
    [358, '蚂蚁矿池 AntPool 🐜', '蚂蚁矿池 AntPool 🐜', 4 * 15 + 9 * 7.2 + 15, 222],
    [1248, '蚂蚁矿池 AntPool 🐜', '蚂蚁矿池 An…', 4 * 15 + 4 * 7.2, 94],
    [358, '🐜'.repeat(16), `${'🐜'.repeat(14)}…`, 14 * 15 + 7.2, 222],
    [326, '🐜'.repeat(16), `${'🐜'.repeat(12)}…`, 12 * 15 + 7.2, 192],
  ])(
    'writes it at %i px in whole graphemes within its room: %s',
    (width, pool, drawn, px, room) => {
      stubChartWidth(width);
      const named: ForkTreeModel = {
        ...usual,
        main: usual.main.map((t) => (t.minerAddress ? { ...t, minerPool: pool } : t)),
        rows: usual.rows.map((r) => ({
          ...r,
          tiles: r.tiles.map((t) => ({ ...t, minerPool: pool })),
        })),
      };
      const { container } = render(<ForkTree model={named} />);
      const svg = container.querySelector('svg[data-fork-tree]')!;
      const pools = [...svg.querySelectorAll('text')].filter((t) =>
        t.textContent!.startsWith(pool.slice(0, 2))
      );
      // today's +1 and +2 and the dropped +1 carry a miner
      expect(pools).toHaveLength(3);
      for (const text of pools) expect(text.textContent).toBe(drawn);
      expect(px).toBeLessThanOrEqual(room);
    }
  );
});

// Review fb-logic 1 (lane rd/fb-fixes-2, L1): in Chrome on /forks/21022666/07c5ad4e and
// /forks/21006834/7b4ee8ae the last two miners overlapped by 11.2 / 5.2 / 3.2 px at 414 / 428 /
// 430 px windows (containers 382 / 396 / 398). The last column's labels end at its tile's right
// edge and run back into the gap before it, while the column before it runs its labels into the
// same gap: each claimed 16 of its 20 px, and the id form was chosen against that claim, so from a
// 78 px tile both wrote the row form (93.6 px) where 2 × 93.6 > 2 × TW + 20 (below 83.6). Every
// width a phone or a narrow window gives the tree, 1 px apart, for #21,022,666's shape (today's +1
// to +3 with unnamed miners, the dropped +1): no two miners meet.
describe('ForkTree: the miners of the last two columns never meet (review fb-logic 1)', () => {
  it('keeps every miner clear of every other at each container width from 326 to 460 px', () => {
    const met: string[] = [];
    for (let width = 326; width <= 460; width++) {
      stubChartWidth(width);
      const { container, unmount } = render(<ForkTree model={full} />);
      const svg = container.querySelector('svg[data-fork-tree]')!;
      // the last two main tiles name their miners at every width
      for (const n of [FP + 2, FP + 3]) {
        const tile = svg.querySelector(`[data-tile="${n}"]:not([data-dropped])`)!;
        expect(
          [...tile.querySelectorAll('text')].filter((t) => t.textContent!.startsWith('ckb1')),
          `${n} at ${width}`
        ).toHaveLength(1);
      }
      const miners = allTextLines(svg).filter((line) => line.text.startsWith('ckb1'));
      for (const m of meetings(miners)) met.push(`${width} px: ${m.a} and ${m.b} by ${m.by}`);
      unmount();
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
    expect(met).toEqual([]);
  });
});

// Lane P4 (rd/p-topics), THE FORK TREE ON A PHONE (PAGE-LANE-NOTES, P4, F3-d): at 358 px two of
// mainnet's 58 forks drew labels that met by 8.8 px (#21,022,666, #21,006,834), the five-height fork
// (#21,043,989) folded its dropped blocks into "+3 earlier", and at 326 px (a 360 phone) every
// usual fork folded its fork point and its branch. A fold must never take the branch: on a phone
// the tree turns to a column, one block a row, each tile with its time and miner beside it.
describe('ForkTree on a phone: a column, every block a row of its own, nothing folded (P4)', () => {
  const shapes = [
    ['four heights, the last an uncle inclusion', usual],
    ['three heights', three],
    ['four heights, the last two today’s', full],
    ['five heights, two dropped', five],
    ['a 7-deep fork', deep],
  ] as const;
  const cases = [358, 326].flatMap((width) =>
    shapes.map(([name, model]) => [width, name, model] as const)
  );

  it.each(cases)(
    'at %i px, %s: every block is its own tile, the branch whole',
    (width, _, model) => {
      stubChartWidth(width);
      const { container } = render(<ForkTree model={model} />);
      const svg = container.querySelector('svg[data-fork-tree]')!;
      expect(svg.querySelector('[data-collapsed]'), 'a fold').toBeNull();
      const drawn = (n: number, dropped: boolean) =>
        svg.querySelector(
          `[data-tile="${n}"]${dropped ? '[data-dropped]' : ':not([data-dropped])'}`
        );
      for (const tile of model.main)
        expect(drawn(tile.number, false), `${tile.number}`).not.toBeNull();
      for (const row of model.rows)
        for (const tile of row.tiles)
          expect(drawn(tile.number, true), `${tile.number}′ (${row.label})`).not.toBeNull();
      expect(svg.querySelectorAll('[data-tile]')).toHaveLength(
        model.main.length + model.rows.reduce((n, row) => n + row.tiles.length, 0)
      );
    }
  );

  it.each(cases)(
    'at %i px, %s: a tile’s time and miner stand beside it, on its row',
    (width, _, model) => {
      stubChartWidth(width);
      const { container } = render(<ForkTree model={model} />);
      const svg = container.querySelector('svg[data-fork-tree]')!;
      const tiles = [...svg.querySelectorAll('[data-tile]')];
      const boxes = tiles.map((g) => rectBox(g.querySelector('rect')!));
      let labelled = 0;
      tiles.forEach((g, i) => {
        const box = boxes[i];
        const time = g.querySelector('[data-time]');
        if (!time) return;
        labelled++;
        const words = [...g.querySelectorAll('text')].filter(
          (t) => t === time || t.textContent!.startsWith('ckb1')
        );
        expect(words, `${g.getAttribute('data-tile')}`).toHaveLength(2);
        for (const line of words.flatMap((t) => textLines(t))) {
          // beside the tile: right of every tile's lane, inside the tile's own band of height
          expect(line.left, line.text).toBeGreaterThan(Math.max(...boxes.map((b) => b.right)));
          expect(line.top, line.text).toBeGreaterThanOrEqual(box.top);
          expect(line.bottom, line.text).toBeLessThanOrEqual(box.bottom);
        }
      });
      expect(labelled).toBeGreaterThan(0);
      // one block a row: no two tiles share a band of height
      const sorted = [...boxes].sort((a, b) => a.top - b.top);
      for (let k = 1; k < sorted.length; k++)
        expect(sorted[k].top).toBeGreaterThanOrEqual(sorted[k - 1].bottom);
      const all = allTextLines(svg);
      for (const line of all) expectLineInside(line, viewBox(svg), 'the tree', 'flush');
      expect(meetings(all), 'the tree').toEqual([]);
    }
  );
});
