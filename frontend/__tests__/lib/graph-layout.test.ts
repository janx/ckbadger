import { describe, expect, it } from 'vitest';
import { connectedComponents, kamadaKawaiLayout } from '@/lib/graph-layout';

const dist = (a: { x: number; y: number }, b: { x: number; y: number }) =>
  Math.hypot(a.x - b.x, a.y - b.y);

describe('connectedComponents', () => {
  it('splits a graph into components, largest first, members in input order', () => {
    const components = connectedComponents(
      ['a', 'b', 'c', 'd', 'e', 'f'],
      [
        ['e', 'f'],
        ['a', 'b'],
        ['b', 'c'],
      ]
    );
    expect(components).toEqual([['a', 'b', 'c'], ['e', 'f'], ['d']]);
  });

  it('refuses an edge to a node it was not given', () => {
    expect(() => connectedComponents(['a'], [['a', 'z']])).toThrow('unknown node z');
  });
});

describe('kamadaKawaiLayout', () => {
  it('places graph distance as layout distance: a path stays a line, ends farthest apart', () => {
    const pos = kamadaKawaiLayout(
      ['a', 'b', 'c', 'd'],
      [
        ['a', 'b'],
        ['b', 'c'],
        ['c', 'd'],
      ]
    );
    const ab = dist(pos.get('a')!, pos.get('b')!);
    const ad = dist(pos.get('a')!, pos.get('d')!);
    expect(ad).toBeGreaterThan(2.7 * ab);
    expect(dist(pos.get('b')!, pos.get('c')!)).toBeCloseTo(ab, 1);
  });

  it('pulls a hub to the middle of its star', () => {
    const leaves = ['l1', 'l2', 'l3', 'l4', 'l5'];
    const pos = kamadaKawaiLayout(
      ['l1', 'hub', 'l2', 'l3', 'l4', 'l5'],
      leaves.map((leaf) => ['hub', leaf] as const)
    );
    const hub = pos.get('hub')!;
    const cx = leaves.reduce((sum, leaf) => sum + pos.get(leaf)!.x, 0) / leaves.length;
    const cy = leaves.reduce((sum, leaf) => sum + pos.get(leaf)!.y, 0) / leaves.length;
    expect(Math.hypot(hub.x - cx, hub.y - cy)).toBeLessThan(0.08);
    for (const leaf of leaves) expect(dist(hub, pos.get(leaf)!)).toBeGreaterThan(0.2);
  });

  it('is deterministic: the same input gives the same picture on every load', () => {
    const nodes = ['p', 'q', 'r', 's', 't'];
    const edges = [
      ['p', 'q'],
      ['q', 'r'],
      ['r', 'p'],
      ['r', 's'],
      ['s', 't'],
    ] as const;
    expect([...kamadaKawaiLayout(nodes, edges)]).toEqual([...kamadaKawaiLayout(nodes, edges)]);
  });

  it('only lays out one connected component', () => {
    expect(() => kamadaKawaiLayout(['a', 'b', 'c'], [['a', 'b']])).toThrow('not connected');
  });
});
