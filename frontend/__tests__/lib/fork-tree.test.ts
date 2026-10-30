import { describe, expect, it } from 'vitest';
import { forkTreeModel } from '@/lib/fork-tree';
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

  it('links both blocks of a two-block branch into the block that took them as uncles', () => {
    const model = forkTreeModel(detail(multi));
    expect(model.rows[0].tiles.map((t) => t.number)).toEqual([11, 12]);
    expect(model.uncleLinks.map((l) => l.to)).toEqual([13, 13]);
  });
});
