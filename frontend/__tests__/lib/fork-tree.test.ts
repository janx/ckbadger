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
    // 8 + 93.6 + 8 → 110 px (96 at 10 px), on a 126 px pitch (112)
    expect(layout.tileW).toBe(110);
    expect(layout.width).toBe(30 + 3 * 126 + 110 + 30);
    expect(layout.collapsed).toBeNull();
    expect(layout.labelsUnder).toBe(false);
  });

  // Re-pinned twice: lane F3-d moved this to 370 px (the last column's 12 px labels kept 16 px
  // after it, so at 358 the oldest two heights folded); the ruling on F3-d sets it back to 358:
  // the last column's labels end at its tile's right edge, so the svg keeps 4 px after it again,
  // 6 + 4 × 72 + 3 × 20 + 4 = 358, and the fork's dropped block keeps its column.
  it('fits four 72 px blocks into 358 px with the time and miner under the boxes', () => {
    const layout = forkTreeLayout(live, 358);
    expect(layout.form).toBe('compact');
    expect(layout.tileW).toBe(FORK_TREE.compact.minTile);
    expect(layout.labelsUnder).toBe(true);
    expect(layout.collapsed).toBeNull();
    expect(layout.width).toBe(358);
    for (const n of [11, 12, 13, 14]) {
      expect(layout.x(n)).toBeGreaterThanOrEqual(0);
      expect(layout.x(n) + layout.tileW).toBeLessThanOrEqual(358);
    }
    // blocks keep their order with a gap for the branch line between them
    expect(layout.x(12) - layout.x(11)).toBe(layout.tileW + FORK_TREE.compact.gap);
  });

  it('collapses the oldest blocks of a seven-deep fork into one "+N earlier" column', () => {
    const layout = forkTreeLayout(deep, 358);
    expect(layout.form).toBe('compact');
    expect(layout.collapsed).not.toBeNull();
    const { count, first, last } = layout.collapsed!;
    expect(first).toBe(100);
    expect(last).toBe(100 + count - 1);
    // the collapsed column plus the newest blocks, every one of them at least 72 px
    expect(layout.columns[0]).toBe(last + 1);
    expect(layout.columns.at(-1)).toBe(107);
    // re-pinned (the ruling on F3-d): the svg keeps 4 px after the last tile again (lane F3-d had
    // 16), and the fold's box holds its "+6 earlier" (6 + 72 + 6 = 84 px at 12 px), so where the
    // tree folds every tile is at least 84 px: floor((358 − 6 − 4 + 20) / (84 + 20)) = 3 columns
    // (4 with 72 px tiles, before the type change and in lane F3-d)
    expect(layout.columns.length + 1).toBe(
      Math.floor((358 - 6 - FORK_TREE.compact.right + 20) / (84 + 20))
    );
    expect(layout.tileW).toBeGreaterThanOrEqual(84);
    // a hidden block is drawn in the collapsed column
    expect(layout.x(101)).toBe(layout.x(100));
    expect(layout.x(107) + layout.tileW).toBeLessThanOrEqual(358);
  });

  it('collapses less where there is more room (the 640 column)', () => {
    const layout = forkTreeLayout(deep, 608);
    expect(layout.collapsed!.count).toBeLessThan(forkTreeLayout(deep, 358).collapsed!.count);
    expect(layout.x(107) + layout.tileW).toBeLessThanOrEqual(608);
  });

  it('refuses a width that cannot hold the collapsed column and one block', () => {
    expect(() => forkTreeLayout(deep, 150)).toThrow(/150 px/);
  });

  // Review F-B (logic finding 8): from "+100 earlier" (12 glyphs at 12 px, 86.4 px, and 6 px each
  // side: 99 px) the fold's box needs more than the 96 px a tile stops growing at, and the layout
  // capped it there, its words running 2.4 px past it. The box is as wide as its words need.
  it('draws the "+N earlier" box as wide as its words, past a hundred folded heights too', () => {
    // 120 heights: the fork point 100 and today's 101–219, one dropped block at 219
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
      expect(layout.form).toBe('compact');
      const count = layout.collapsed!.count;
      expect(count).toBeGreaterThanOrEqual(100);
      // its words at 0.6 em of 12 px, 6 px in from each side, up to a whole pixel
      const words = `+${count} earlier`.length * 7.2 + 2 * 6;
      expect(layout.tileW, `${width} px: +${count} earlier`).toBeGreaterThanOrEqual(words);
      expect(layout.x(219) + layout.tileW, `${width} px`).toBeLessThanOrEqual(width - 4);
    }
  });

  // Review F-B (test finding 9; logic finding 4): the compact form's room, pinned to literals so
  // a change of the module's constants shows here and in the component's band test, which
  // derives the same numbers on its own. A tile's labels run 16 px into the 20 px gap after it,
  // 4 px short of the next tile; under it they take 4 + 15 + 15 + 4 = 38 px; the svg keeps 4 px
  // after the last tile. The narrowest column, 72 + 16 = 88 px, holds a tight address (11
  // characters at 12 px, 79.2 px) and not the row form (13, 93.6); a wide tile's 110 − 2 × 8 = 94
  // px between its insets holds the row form.
  it('keeps the room an unnamed miner is written in', () => {
    const c = FORK_TREE.compact;
    expect([c.minTile, c.gap, c.labelRun, c.under, c.right]).toEqual([72, 20, 16, 38, 4]);
    expect(c.minTile + c.labelRun).toBe(88);
    expect(FORK_TREE.wide.tile - 2 * FORK_TREE.wide.inset).toBe(94);
  });
});
