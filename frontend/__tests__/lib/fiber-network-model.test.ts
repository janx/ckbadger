import { describe, expect, it } from 'vitest';
import { fiberGraphModel } from '@/lib/fiber-network-model';
import type { FiberNetwork } from '@/lib/api';
import mainnet from '@/__tests__/fixtures/fiber-network-mainnet.json';
import small from '@/__tests__/fixtures/fiber-network.json';

const network = mainnet as FiberNetwork;
const byTail = (model: ReturnType<typeof fiberGraphModel>, tail: string) => {
  const node = model.nodes.find((n) => n.label === tail);
  if (!node) throw new Error(`no node ${tail}`);
  return node;
};

describe('fiberGraphModel (Fiber board V65 encodings, FF-F1 / FF-F2)', () => {
  it('lays the main component out on the left and the separate pairs in a strip on the right', () => {
    const model = fiberGraphModel(network);
    expect(model.components.map((c) => c.length)).toEqual([21, 2, 2]);
    expect(model.sideX).not.toBeNull();
    const main = model.nodes.filter((n) => !n.side);
    const side = model.nodes.filter((n) => n.side);
    expect(main).toHaveLength(21);
    expect(side.map((n) => n.label).sort()).toEqual(['5eyy6y', 'as20ze', 'nhk9ej', 'ppedrr']);
    expect(Math.max(...main.map((n) => n.x))).toBeLessThan(model.sideX!);
    for (const n of side) expect(n.x).toBeGreaterThan(model.sideX!);
    // Kamada–Kawai, not a ring: the two hubs are near the middle, a leaf is at the edge.
    const cx = (model.sideX! + 0) / 2;
    expect(Math.abs(byTail(model, 'hspqyz').x - cx)).toBeLessThan(200);
  });

  it('sizes a node by the channels it took part in and rings it when one is open now', () => {
    const model = fiberGraphModel(network);
    const kdeakp = byTail(model, 'kdeakp');
    const leaf = byTail(model, '298juj');
    expect(kdeakp.r).toBeCloseTo(5 + 2.2 * Math.sqrt(kdeakp.channels), 5);
    expect(kdeakp.r).toBeGreaterThan(30);
    expect(leaf.r).toBeLessThan(9);
    expect(kdeakp.ring).toBe(true);
    expect(leaf.ring).toBe(leaf.openChannels > 0);
  });

  it('draws grey by log(channels ever), jade by √(CKB locked now), dashed for a force close', () => {
    const model = fiberGraphModel(network);
    const busiest = [...model.edges].sort((a, b) => b.pair.channels - a.pair.channels)[0];
    expect(busiest.greyWidth).toBeCloseTo(0.8 + 1.3 * Math.log(busiest.pair.channels), 5);
    const closedOnly = model.edges.find((e) => e.pair.openChannels === 0)!;
    expect(closedOnly.jadeWidth).toBeNull();
    const open = model.edges.find((e) => e.pair.openChannels > 0)!;
    const ckb = Number(BigInt(open.pair.openCapacity) / BigInt(100_000_000));
    expect(open.jadeWidth).toBeCloseTo(1.2 + Math.sqrt(ckb) / 18, 5);
    const forced = model.edges.filter(
      (e) => e.pair.forceClosedSettledChannels + e.pair.forceClosedWaitingChannels > 0
    );
    expect(forced.length).toBeGreaterThan(0);
    for (const e of model.edges)
      expect(e.forceClosed).toBe(
        e.pair.forceClosedSettledChannels + e.pair.forceClosedWaitingChannels > 0
      );
  });

  it('knows each node’s neighbourhood for hover dimming', () => {
    const model = fiberGraphModel(network);
    const ppedrr = byTail(model, 'ppedrr');
    const yy = byTail(model, '5eyy6y');
    expect([...model.neighbours.get(ppedrr.id)!]).toEqual([yy.id]);
  });

  it('labels a party without an indexed lock script by its lock hash tail', () => {
    const model = fiberGraphModel(small as FiberNetwork);
    expect(model.nodes.map((n) => n.label).sort()).toEqual(['aaaaaa', 'bbbbbb']);
    expect(model.unpairedChannels).toBe(1);
  });

  it('refuses a pair that names a party the response does not list', () => {
    const broken = {
      ...(small as FiberNetwork),
      pairs: [
        { ...(small as FiberNetwork).pairs[0], secondLockScriptHash: `0x${'cc'.repeat(32)}` },
      ],
    };
    expect(() => fiberGraphModel(broken)).toThrow(/unknown party 0xcccc/);
  });
});
