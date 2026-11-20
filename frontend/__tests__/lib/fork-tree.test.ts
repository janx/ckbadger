import { describe, expect, it } from 'vitest';
import { FORK_TREE, forkTreeLayout, forkTreeModel, type ForkTreeModel } from '@/lib/fork-tree';
import type { ReorgDetail } from '@/lib/api';
import uncle from '@/__tests__/fixtures/fork-api-producer/fork-uncle-detail.json';
import reentry from '@/__tests__/fixtures/fork-api-producer/fork-reentry-detail.json';
import multi from '@/__tests__/fixtures/fork-api-producer/fork-multi-uncle-detail.json';

const detail = (value: unknown) => value as ReorgDetail;

describe('forkTreeModel (Fork detail V67 "The two branches", FF-D4)', () => {
  it('puts today’s chain on the main row from the fork point, the dropped block above it, linked to its uncle inclusion', () => {
    const model = forkTreeModel(detail(uncle));
    expect(model.main.map((t) => t.number)).toEqual([10, 11, 12]);
    expect(model.main[0].hash).toBe(uncle.event.forkPointHash);
    expect(model.rows).toHaveLength(1);
    expect(model.rows[0].label).toBe('dropped at this switch');
    expect(model.rows[0].tiles.map((t) => [t.number, t.hash])).toEqual([
      [11, uncle.orphanedBlocks[0].hash],
    ]);
    expect(model.uncleLinks).toEqual([{ from: uncle.orphanedBlocks[0].hash, to: 12 }]);
    // ① the tip it had, ② the tip it switched to
    expect(model.marks).toEqual([
      { n: 1, hash: uncle.event.oldTipHash },
      { n: 2, hash: uncle.event.newTipHash },
    ]);
  });

  it('shows a block that came back on the main row and the branch adopted then as the dropped one', () => {
    const model = forkTreeModel(detail(reentry));
    expect(model.main.map((t) => t.hash)).toEqual([
      reentry.event.forkPointHash,
      reentry.canonicalBranch[0].hash,
    ]);
    expect(model.rows.map((r) => r.label)).toEqual(['adopted then, dropped since']);
    expect(model.rows[0].tiles[0].hash).toBe(reentry.adoptedBranch[0].hash);
    expect(model.uncleLinks).toEqual([]);
  });

  it('marks ③ on the tip the reversing switch adopted (R-6b `revertedByEvent.newTip`)', () => {
    const model = forkTreeModel(detail(reentry));
    expect(reentry.event.revertedByEvent?.newTip).toEqual({
      number: 11,
      hash: reentry.canonicalBranch[0].hash,
    });
    expect(model.marks).toEqual([
      { n: 1, hash: reentry.event.oldTipHash },
      { n: 2, hash: reentry.event.newTipHash },
      { n: 3, hash: reentry.canonicalBranch[0].hash },
    ]);
  });

  it('draws no ③ for a switch nothing reversed', () => {
    expect(forkTreeModel(detail(uncle)).marks.map((m) => m.n)).toEqual([1, 2]);
  });

  it('links both blocks of a two-block branch into the block that took them as uncles', () => {
    const model = forkTreeModel(detail(multi));
    expect(model.rows[0].tiles.map((t) => t.number)).toEqual([11, 12]);
    expect(model.uncleLinks.map((l) => l.to)).toEqual([13, 13]);
  });
});

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
/** The live fork at #20,894,011: main 011–013 and the uncle inclusion 014, dropped 012′ above. */
const live: ForkTreeModel = {
  main: [tile(11, 'f0', true), tile(12, 'a12'), tile(13, 'a13'), tile(14, 'a14', true)],
  rows: [{ label: 'dropped at this switch', tiles: [tile(12, 'b12')] }],
  uncleLinks: [{ from: hash('b12'), to: 14 }],
  marks: [
    { n: 1, hash: hash('b12') },
    { n: 2, hash: hash('a13') },
  ],
  first: 11,
  last: 14,
};
/** A seven-block-deep fork: main 101–107 over the fork point 100, seven dropped blocks above. */
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
  uncleLinks: [],
  marks: [
    { n: 1, hash: hash('b107') },
    { n: 2, hash: hash('a107') },
  ],
  first: 100,
  last: 107,
};

describe('forkTreeLayout (responsive V4: the two branches drawn to the measured width)', () => {
  it('keeps the desktop tree, at its natural width, wherever it fits', () => {
    const layout = forkTreeLayout(live, 1248);
    expect(layout.form).toBe('wide');
    // re-pinned (readability F3): a wide tile holds a miner's 13-character address at 12 px,
    // 8 + 93.6 + 8 → 110 px (96 at 10 px), on a 126 px pitch (112). Re-pinned (lane P4): the
    // layout no longer says `collapsed: null` and `labelsUnder: false`; no form folds, and only the
    // wide form is laid out by height across
    expect(layout.tileW).toBe(110);
    expect(layout.width).toBe(30 + 3 * 126 + 110 + 30);
  });

  // Re-pinned three times: lane F3-d moved this to 370 px, the ruling on F3-d set it back to 358
  // with four 72 px tiles and the time and miner under the boxes (6 + 4 × 72 + 3 × 20 + 4 = 358);
  // lane P4 turns the phone form to a column (it was "fits four 72 px blocks into 358 px with the
  // time and miner under the boxes"): one block a row, oldest first, at one height today's block
  // before the dropped one; today's chain in the first lane, the branch in the second; the words
  // from 4 + 56 + 8 + 56 + 8 = 132 px, 222 px of room at 358 (358 − 132 − 4)
  it('turns the tree to a column at 358 px: one block a row, a lane a branch, the words beside', () => {
    const layout = forkTreeLayout(live, 358);
    if (layout.form !== 'column') throw new Error(`the ${layout.form} form at 358 px`);
    expect(layout.width).toBe(358);
    expect(layout.tileW).toBe(56);
    expect(layout.laneX).toEqual([4, 68]);
    expect(layout.labelsX).toBe(132);
    expect(layout.labelRoom(live.main[1])).toBe(222);
    expect(layout.rows.map((r) => [r.tile.number, r.lane, r.dropped])).toEqual([
      [11, 0, false],
      [12, 0, false],
      [12, 1, true],
      [13, 0, false],
      [14, 0, false],
    ]);
    // rows 36 px tall and 18 px apart under the two lanes' names (15 px each) and 18 px
    expect(layout.rows.map((r) => r.y)).toEqual([48, 102, 156, 210, 264]);
    expect(layout.height).toBe(264 + 36 + 4);
    expect(layout.at(hash('b12')).lane).toBe(1);
    expect(() => layout.at(hash('zz'))).toThrow('is not a tile of the tree');
    expect(layout.laneNames.map((n) => [n.text, n.x, n.anchor])).toEqual([
      ['main chain', 4, 'start'],
      ['branch', 68, 'start'],
    ]);
  });

  // Re-pinned (lane P4): it was "collapses the oldest blocks of a seven-deep fork into one "+N
  // earlier" column" (and "collapses less where there is more room (the 640 column)"): the fold
  // took the fork point and the branch's first blocks. A fold never takes the branch: the column
  // holds every block, at 358 and at 608, where the eight heights do not fit the wide tree.
  it.each([358, 608])(
    'draws every block of a seven-deep fork at %i px, nothing folded',
    (width) => {
      const layout = forkTreeLayout(deep, width);
      if (layout.form !== 'column') throw new Error(`the ${layout.form} form at ${width} px`);
      expect(layout.rows).toHaveLength(15);
      expect(new Set(layout.rows.map((r) => r.tile.hash)).size).toBe(15);
      expect(layout.rows.filter((r) => r.dropped)).toHaveLength(7);
    }
  );

  // Re-pinned (lane P4): it was "refuses a width that cannot hold the collapsed column and one
  // block"; the column refuses a width whose words' room cannot hold a block's time (86.4 px at
  // 12 px): 132 + 86.4 + 4 → below 222.4 px with two lanes
  it('refuses a width that leaves less than a block’s time beside the lanes', () => {
    expect(() => forkTreeLayout(deep, 222)).toThrow(/222 px canvas leaves 86 px beside 2 lanes/);
    expect(forkTreeLayout(deep, 223).form).toBe('column');
  });

  // Lane P4: a third lane (a branch adopted then, dropped since) starts at 132, the words at 196:
  // at a 360 phone (328 px) 128 px of room; at a 320 window (288 px) 88, which still holds a time
  // (86.4); its name, 27 characters (194.4 px), ends at the canvas's right edge where it would run
  // past it.
  it('lays out a third lane and ends a name that would run past the canvas at its right edge', () => {
    const three: ForkTreeModel = {
      ...deep,
      rows: [
        ...deep.rows,
        { label: 'adopted then, dropped since', tiles: [tile(101, 'c101'), tile(102, 'c102')] },
      ],
    };
    const at328 = forkTreeLayout(three, 328);
    if (at328.form !== 'column') throw new Error(at328.form);
    expect(at328.laneX).toEqual([4, 68, 132]);
    expect(at328.labelsX).toBe(196);
    expect(at328.labelRoom(three.main[1])).toBe(128);
    // 132 + 194.4 = 326.4, past 328 − 4: the name ends at the right edge; at 358 it starts at 132
    expect(at328.laneNames[2]).toMatchObject({ x: 324, anchor: 'end' });
    expect(forkTreeLayout(three, 358)).toMatchObject({
      laneNames: [{}, {}, { x: 132, anchor: 'start' }],
    });
    const at288 = forkTreeLayout(three, 288);
    if (at288.form !== 'column') throw new Error(at288.form);
    expect(at288.labelRoom(three.main[1])).toBe(88);
    expect(at288.laneNames[2]).toMatchObject({ x: 284, anchor: 'end' });
    // at one height: today's block, the dropped one, the one adopted then dropped since
    expect(at328.rows.slice(0, 4).map((r) => [r.tile.number, r.lane])).toEqual([
      [100, 0],
      [101, 0],
      [101, 1],
      [101, 2],
    ]);
  });

  // Review F-B (test finding 9; logic finding 4): the forms' room, pinned to literals so a change
  // of the module's constants shows here and in the component's band test, which derives the same
  // numbers on its own. A wide tile's 110 − 2 × 8 = 94 px between its insets holds the row form.
  // Re-pinned (lane P4): it pinned the compact form's 72 px tile, 20 px gap, 16 px run, 8 px shared
  // run, 38 px under the tile and 4 px after the last; the column's tile is 56 px (6 + 43.2 + 6),
  // its lanes 8 px apart, its words 8 px after the last lane and 4 px short of the right edge,
  // its rows 36 px tall and 18 px apart (a tip mark, r 8, hangs whole between them).
  it('keeps the room an unnamed miner is written in', () => {
    const c = FORK_TREE.column;
    expect([c.tile, c.tileH, c.laneGap, c.labelGap, c.right, c.rowGap, c.markR, c.x0]).toEqual([
      56, 36, 8, 8, 4, 18, 8, 4,
    ]);
    expect(FORK_TREE.wide.tile - 2 * FORK_TREE.wide.inset).toBe(94);
  });

  // Review fb-logic 1: each tile's words have the room they really have. Re-pinned (lane P4): at
  // 382 px the compact form drew 78 px tiles and gave the last two columns 86 px each (the gap
  // between them shared) and the others 94; the column gives every tile the one room of its words,
  // 382 − 132 − 4 = 246 px. The wide form's room is the tile less its insets.
  it('gives each tile the room its words have', () => {
    const main = [tile(10, 'f0', true), tile(11, 'a1'), tile(12, 'a2'), tile(13, 'a3')];
    const dropped = tile(12, 'b2');
    const model: ForkTreeModel = {
      main,
      rows: [{ label: 'dropped at this switch', tiles: [dropped] }],
      uncleLinks: [],
      marks: [],
      first: 10,
      last: 13,
    };
    const layout = forkTreeLayout(model, 382);
    expect(layout.form).toBe('column');
    expect(main.map(layout.labelRoom)).toEqual([246, 246, 246, 246]);
    expect(layout.labelRoom(dropped)).toBe(246);
    expect(() => layout.labelRoom(tile(14, 'zz'))).toThrow('is not a tile of the tree');
    expect(forkTreeLayout(model, 1248).labelRoom(main[3])).toBe(94);
  });

  // Review F-B (logic finding 8) held the "+N earlier" box as wide as its words past a hundred
  // folded heights. Re-pinned (lane P4): it was "draws the "+N earlier" box as wide as its words,
  // past a hundred folded heights too"; nothing folds now, so a fork of 120 heights draws its 121
  // blocks, one a row, at every width the wide tree does not hold.
  it('draws a fork of 120 heights whole, a row a block, at every width under the wide tree', () => {
    const tall: ForkTreeModel = {
      main: Array.from({ length: 120 }, (_, i) => tile(100 + i, `m${i}`, i === 0)),
      rows: [{ label: 'dropped at this switch', tiles: [tile(219, 'b219')] }],
      uncleLinks: [],
      marks: [{ n: 1, hash: hash('b219') }],
      first: 100,
      last: 219,
    };
    for (const width of [1248, 608, 358]) {
      const layout = forkTreeLayout(tall, width);
      if (layout.form !== 'column') throw new Error(`the ${layout.form} form at ${width} px`);
      expect(layout.rows, `${width} px`).toHaveLength(121);
      expect(layout.rows.at(-1)).toMatchObject({ lane: 1, dropped: true });
    }
  });
});
