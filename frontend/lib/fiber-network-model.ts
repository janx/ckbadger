import type { FiberNetwork, FiberPair, FiberParty } from '@/lib/api';
import { connectedComponents, kamadaKawaiLayout, type LayoutPoint } from '@/lib/graph-layout';

/**
 * The Fiber network as the chain sees it (Fiber board V65): every party a node, every pair that
 * opened channels an edge. Encodings only — every count and amount is the API's own field.
 */

export const FIBER_GRAPH_SIZE = { width: 860, minHeight: 560, pad: 48, side: 150 } as const;
const SIDE_SLOT = 110;

export interface FiberGraphNode {
  id: string;
  party: FiberParty;
  label: string;
  x: number;
  y: number;
  /** 5 + 2.2·√(channels it took part in). */
  r: number;
  channels: number;
  openChannels: number;
  /** A jade ring: it has a channel open now. */
  ring: boolean;
  /** In the right strip: not linked to the main component. */
  side: boolean;
}

export interface FiberGraphEdge {
  id: string;
  a: string;
  b: string;
  pair: FiberPair;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  /** Grey: 0.8 + 1.3·ln(channels ever). */
  greyWidth: number;
  /** Jade overlay: 1.2 + √(CKB locked now)/18; null when nothing is open between them. */
  jadeWidth: number | null;
  /** A dashed offset line: at least one channel of the pair was force-closed. */
  forceClosed: boolean;
  /** The dashed line, offset 5 px to one side. */
  dash: { x1: number; y1: number; x2: number; y2: number };
}

export interface FiberGraphModel {
  width: number;
  height: number;
  /** Left edge of the "not linked to the rest" strip; null when everything is linked. */
  sideX: number | null;
  components: string[][];
  nodes: FiberGraphNode[];
  edges: FiberGraphEdge[];
  neighbours: Map<string, Set<string>>;
  /** Channels whose funding tx did not show exactly two parties: they draw no edge. */
  unpairedChannels: number;
}

export const pairKey = (pair: FiberPair) =>
  `${pair.firstLockScriptHash}:${pair.secondLockScriptHash}`;

/** The last six characters of the party's address, or of its lock hash when none is indexed. */
export const partyTail = (party: FiberParty) => (party.address ?? party.lockScriptHash).slice(-6);

function fit(
  points: Map<string, LayoutPoint>,
  box: { x0: number; x1: number; y0: number; y1: number }
): Map<string, LayoutPoint> {
  const xs = [...points.values()].map((p) => p.x);
  const ys = [...points.values()].map((p) => p.y);
  const [mx0, mx1, my0, my1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  const scale = (v: number, lo: number, hi: number, a: number, b: number) =>
    hi - lo < 1e-9 ? (a + b) / 2 : a + ((v - lo) / (hi - lo)) * (b - a);
  return new Map(
    [...points].map(([id, p]) => [
      id,
      { x: scale(p.x, mx0, mx1, box.x0, box.x1), y: scale(p.y, my0, my1, box.y0, box.y1) },
    ])
  );
}

export function fiberGraphModel(network: FiberNetwork): FiberGraphModel {
  const { parties, pairs, summary } = network;
  if (parties.length !== summary.observedParties || pairs.length !== summary.observedPairs)
    throw new Error(
      `Fiber network lists ${parties.length} parties / ${pairs.length} pairs, summary says ${summary.observedParties} / ${summary.observedPairs}`
    );
  const byId = new Map(parties.map((party) => [party.lockScriptHash, party]));
  for (const pair of pairs)
    for (const hash of [pair.firstLockScriptHash, pair.secondLockScriptHash])
      if (!byId.has(hash))
        throw new Error(`Fiber pair ${pairKey(pair)} names unknown party ${hash}`);
  const represented = pairs.reduce((total, pair) => total + pair.channels, 0);
  if (represented > summary.totalChannels)
    throw new Error(
      `Fiber pairs hold ${represented} channels, more than the ${summary.totalChannels} observed`
    );
  const ids = parties.map((party) => party.lockScriptHash);
  const links = pairs.map((p) => [p.firstLockScriptHash, p.secondLockScriptHash] as const);
  const components = connectedComponents(ids, links);
  const { width, minHeight, pad, side } = FIBER_GRAPH_SIZE;
  const rest = components.slice(1);
  const sideX = rest.length ? width - side : null;
  const height = Math.max(minHeight, pad + 40 + rest.length * SIDE_SLOT + pad);
  const positions = new Map<string, LayoutPoint>();
  if (components.length) {
    const main = components[0];
    const inMain = new Set(main);
    const layout = kamadaKawaiLayout(
      main,
      links.filter(([a]) => inMain.has(a))
    );
    for (const [id, p] of fit(layout, {
      x0: pad,
      x1: (sideX ?? width) - pad,
      y0: pad,
      y1: minHeight - pad,
    }))
      positions.set(id, p);
  }
  rest.forEach((members, i) => {
    const y0 = pad + 40 + i * SIDE_SLOT;
    const ordered = [...members].sort((a, b) => byId.get(b)!.channels - byId.get(a)!.channels);
    if (ordered.length <= 2) {
      ordered.forEach((id, j) => positions.set(id, { x: sideX! + 30 + j * 80, y: y0 + j * 30 }));
      return;
    }
    const inGroup = new Set(members);
    const layout = kamadaKawaiLayout(
      members,
      links.filter(([a]) => inGroup.has(a))
    );
    for (const [id, p] of fit(layout, { x0: sideX! + 20, x1: width - 20, y0, y1: y0 + 80 }))
      positions.set(id, p);
  });
  const sideIds = new Set(rest.flat());
  const nodes: FiberGraphNode[] = parties.map((party) => {
    const p = positions.get(party.lockScriptHash)!;
    return {
      id: party.lockScriptHash,
      party,
      label: partyTail(party),
      x: p.x,
      y: p.y,
      r: 5 + 2.2 * Math.sqrt(party.channels),
      channels: party.channels,
      openChannels: party.openChannels,
      ring: party.openChannels > 0,
      side: sideIds.has(party.lockScriptHash),
    };
  });
  const neighbours = new Map(ids.map((id) => [id, new Set<string>()]));
  const edges: FiberGraphEdge[] = pairs.map((pair) => {
    const a = positions.get(pair.firstLockScriptHash)!;
    const b = positions.get(pair.secondLockScriptHash)!;
    neighbours.get(pair.firstLockScriptHash)!.add(pair.secondLockScriptHash);
    neighbours.get(pair.secondLockScriptHash)!.add(pair.firstLockScriptHash);
    const length = Math.hypot(b.x - a.x, b.y - a.y) || 1;
    const ox = (-(b.y - a.y) / length) * 5;
    const oy = ((b.x - a.x) / length) * 5;
    const lockedCkb = Number(BigInt(pair.openCapacity) / BigInt(100_000_000));
    return {
      id: pairKey(pair),
      a: pair.firstLockScriptHash,
      b: pair.secondLockScriptHash,
      pair,
      x1: a.x,
      y1: a.y,
      x2: b.x,
      y2: b.y,
      greyWidth: 0.8 + 1.3 * Math.log(pair.channels),
      jadeWidth: pair.openChannels > 0 ? 1.2 + Math.sqrt(lockedCkb) / 18 : null,
      forceClosed: pair.forceClosedSettledChannels + pair.forceClosedWaitingChannels > 0,
      dash: { x1: a.x + ox, y1: a.y + oy, x2: b.x + ox, y2: b.y + oy },
    };
  });
  // Thin edges first, so a busy pair's line is drawn on top.
  edges.sort((e1, e2) => e1.pair.channels - e2.pair.channels);
  // Small nodes first, so a hub's label stays on top.
  nodes.sort((n1, n2) => n1.channels - n2.channels);
  return {
    width,
    height,
    sideX,
    components,
    nodes,
    edges,
    neighbours,
    unpairedChannels: summary.totalChannels - represented,
  };
}
