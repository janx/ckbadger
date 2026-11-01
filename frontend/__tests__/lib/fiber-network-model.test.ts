import { describe, expect, it } from 'vitest';
import { FIBER_GRAPH, fiberGraphModel } from '@/lib/fiber-network-model';
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
    const model = fiberGraphModel(network, 860);
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
    const model = fiberGraphModel(network, 860);
    const kdeakp = byTail(model, 'kdeakp');
    const leaf = byTail(model, '298juj');
    expect(kdeakp.r).toBeCloseTo(5 + 2.2 * Math.sqrt(kdeakp.channels), 5);
    expect(kdeakp.r).toBeGreaterThan(30);
    expect(leaf.r).toBeLessThan(9);
    expect(kdeakp.ring).toBe(true);
    expect(leaf.ring).toBe(leaf.openChannels > 0);
  });

  it('draws grey by log(channels ever), jade by √(CKB locked now), dashed for a force close', () => {
    const model = fiberGraphModel(network, 860);
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
    const model = fiberGraphModel(network, 860);
    const ppedrr = byTail(model, 'ppedrr');
    const yy = byTail(model, '5eyy6y');
    expect([...model.neighbours.get(ppedrr.id)!]).toEqual([yy.id]);
  });

  it('labels a party without an indexed lock script by its lock hash tail', () => {
    const model = fiberGraphModel(small as FiberNetwork, 860);
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
    expect(() => fiberGraphModel(broken, 860)).toThrow(/unknown party 0xcccc/);
  });
});

describe('fiberGraphModel laid out at the measured width (R4, responsive V1)', () => {
  /** How far a node reaches sideways: its ring, or half its 11 px six-character label. */
  const reach = (n: { r: number }) => Math.max(n.r + 4, FIBER_GRAPH.labelHalf);

  it('keeps every party, ring and label inside a 358 px canvas and sinks the islands below', () => {
    const model = fiberGraphModel(network, 358);
    expect(model.width).toBe(358);
    expect(model.sideX).toBeNull();
    expect(model.nodes).toHaveLength(25);
    for (const n of model.nodes) {
      expect(n.x - reach(n)).toBeGreaterThanOrEqual(0);
      expect(n.x + reach(n)).toBeLessThanOrEqual(358);
      expect(n.y - n.r - 4).toBeGreaterThanOrEqual(0);
      // the label hangs 14 px under the circle
      expect(n.y + n.r + 18).toBeLessThanOrEqual(model.height);
    }
    const main = model.nodes.filter((n) => !n.side);
    const islands = model.nodes.filter((n) => n.side);
    expect(main).toHaveLength(21);
    expect(islands.map((n) => n.label).sort()).toEqual(['5eyy6y', 'as20ze', 'nhk9ej', 'ppedrr']);
    // the islands' note sits under the lowest label of the main component, the islands under it
    const mainBottom = Math.max(...main.map((n) => n.y + n.r + 18));
    expect(model.islandsNote).not.toBeNull();
    expect(model.islandsNote!.y).toBeGreaterThan(mainBottom);
    for (const n of islands) expect(n.y - n.r - 4).toBeGreaterThan(model.islandsNote!.y);
    // two pairs fit side by side: one row, each pair drawn level
    expect(new Set(islands.map((n) => Math.round(n.y))).size).toBe(1);
    // edges follow their nodes
    const byId = new Map(model.nodes.map((n) => [n.id, n]));
    for (const e of model.edges) {
      expect(e.x1).toBe(byId.get(e.a)!.x);
      expect(e.y2).toBe(byId.get(e.b)!.y);
    }
  });

  it('fits a 608 px column (the 640 viewport) the same way: nothing past the edge', () => {
    const model = fiberGraphModel(network, 608);
    expect(model.width).toBe(608);
    expect(model.sideX).toBeNull();
    for (const n of model.nodes) {
      expect(n.x - reach(n)).toBeGreaterThanOrEqual(0);
      expect(n.x + reach(n)).toBeLessThanOrEqual(608);
    }
  });

  it('keeps the islands in a strip on the right where the column is wide enough', () => {
    const model = fiberGraphModel(network, 880);
    expect(model.width).toBe(880);
    expect(model.sideX).toBe(880 - FIBER_GRAPH.side);
    expect(model.islandsNote).toEqual({ x: 878, y: 48, anchor: 'end' });
    for (const n of model.nodes) expect(n.x + reach(n)).toBeLessThanOrEqual(880);
  });

  it('wraps islands to further rows instead of running past the edge', () => {
    const many = structuredClone(network);
    // eight more separate pairs, each of two fresh parties
    for (let i = 0; i < 8; i++) {
      const a = { ...many.parties[0], lockScriptHash: `0x${'a'.repeat(62)}${i}0`, address: null };
      const b = { ...many.parties[0], lockScriptHash: `0x${'b'.repeat(62)}${i}0`, address: null };
      a.channels = b.channels = 1;
      many.parties.push(a, b);
      many.pairs.push({
        ...many.pairs[0],
        firstLockScriptHash: a.lockScriptHash,
        secondLockScriptHash: b.lockScriptHash,
        channels: 1,
      });
    }
    many.summary = {
      ...many.summary,
      observedParties: many.parties.length,
      observedPairs: many.pairs.length,
      totalChannels: many.summary.totalChannels + 8,
    };
    const model = fiberGraphModel(many, 358);
    const islands = model.nodes.filter((n) => n.side);
    expect(islands).toHaveLength(20);
    expect(new Set(islands.map((n) => Math.round(n.y))).size).toBeGreaterThan(1);
    for (const n of model.nodes) {
      expect(n.x - reach(n)).toBeGreaterThanOrEqual(0);
      expect(n.x + reach(n)).toBeLessThanOrEqual(358);
      expect(n.y + n.r + 18).toBeLessThanOrEqual(model.height);
    }
  });

  it('refuses a width that cannot hold its biggest party, naming the width', () => {
    expect(() => fiberGraphModel(network, 70)).toThrow(/70 px/);
    expect(() => fiberGraphModel(network, Number.NaN)).toThrow(/width/);
  });
});
