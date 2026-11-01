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
    expect(layout.tileW).toBe(96);
    expect(layout.width).toBe(30 + 3 * 112 + 96 + 30);
    expect(layout.collapsed).toBeNull();
    expect(layout.labelsUnder).toBe(false);
  });

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
    expect(layout.columns.length + 1).toBe(Math.floor((358 - 6 - 4 + 20) / (72 + 20)));
    expect(layout.tileW).toBeGreaterThanOrEqual(72);
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
});
