import type { FiberNetwork, FiberPair, FiberParty } from '@/lib/api';
import { connectedComponents, kamadaKawaiLayout, type LayoutPoint } from '@/lib/graph-layout';

/**
 * The Fiber network as the chain sees it (Fiber board V65): every party a node, every pair that
 * opened channels an edge. Encodings only — every count and amount is the API's own field.
 */

export const FIBER_GRAPH = {
  /** The canvas height when the islands sit in a strip at the right (the desktop board). */
  minHeight: 560,
  /** The main component's margin inside a desktop canvas. */
  pad: 48,
  /** The right strip for the islands, on a desktop canvas. */
  side: 150,
  /** From this canvas width the islands keep the right strip; narrower, they sink under (R4). */
  sideFrom: 720,
  /** Half of a six-character 11 px label (7 px a character) with its 3 px halo. */
  labelHalf: 23,
  /** A label's baseline under its circle; its descenders reach 4 px further. */
  labelDrop: 14,
} as const;
const SIDE_SLOT = 110;
/** The open-channel ring is drawn at r + 3 with a 1.2 px stroke. */
const RING = 4;
/** Space between two islands in the row under the main component, and between rows. */
const ISLAND_GAP = 16;
/** Two parties of a two-party island sit this far apart. */
const PAIR_SPAN = 60;
/** The box a larger island is laid out in, inside its slot. */
const ISLAND_BOX = { w: 120, h: 70 } as const;

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
  /** Not linked to the main component: in the right strip, or in the row under it. */
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
  /** The measured canvas width the layout was computed for; the SVG is drawn 1:1 at it. */
  width: number;
  height: number;
  /** Left edge of the "not linked to the rest" strip; null when there is no strip. */
  sideX: number | null;
  /** Where the "not linked to the rest" note goes; null when everything is linked. */
  islandsNote: { x: number; y: number; anchor: 'start' | 'end' } | null;
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

/** What the graph draws, validated, before any layout: components, neighbours, unpaired. */
export interface FiberGraphData {
  parties: FiberParty[];
  pairs: FiberPair[];
  byId: Map<string, FiberParty>;
  links: (readonly [string, string])[];
  /** Largest first; the first is the main component. */
  components: string[][];
  neighbours: Map<string, Set<string>>;
  unpairedChannels: number;
}

export function fiberGraphData(network: FiberNetwork): FiberGraphData {
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
  const neighbours = new Map(ids.map((id) => [id, new Set<string>()]));
  for (const [a, b] of links) {
    neighbours.get(a)!.add(b);
    neighbours.get(b)!.add(a);
  }
  return {
    parties,
    pairs,
    byId,
    links,
    components: connectedComponents(ids, links),
    neighbours,
    unpairedChannels: summary.totalChannels - represented,
  };
}

/** 5 + 2.2·√(channels it took part in): the same circle at every width. */
const radius = (party: FiberParty) => 5 + 2.2 * Math.sqrt(party.channels);
/** How far a party reaches sideways from its centre: its ring, or half its label. */
const reachX = (r: number) => Math.max(r + RING, FIBER_GRAPH.labelHalf);
const reachUp = (r: number) => r + RING;
const reachDown = (r: number) => r + FIBER_GRAPH.labelDrop + 4;

/** Space kept between two parties' footprints (ring and label) after separation. */
const FOOTPRINT_GAP = 3;

/**
 * Kamada–Kawai places parties by graph distance, not by size, so in a narrow column a hub's circle
 * covers its neighbours' labels. Each party's footprint (its ring, its label under it) is a box;
 * overlapping boxes are pushed apart along the axis that frees them soonest. A party never leaves
 * [its reach, width − its reach] across; it may move down, which only makes the canvas taller.
 * Deterministic: same input, same picture.
 */
function separate(
  ids: readonly string[],
  positions: Map<string, LayoutPoint>,
  radiusOf: (id: string) => number,
  width: number,
  top: number
): void {
  const boxes = ids.map((id) => {
    const r = radiusOf(id);
    return { id, p: { ...positions.get(id)! }, x: reachX(r), up: reachUp(r), down: reachDown(r) };
  });
  const clampX = (b: (typeof boxes)[number]) => {
    b.p.x = Math.min(width - b.x, Math.max(b.x, b.p.x));
  };
  for (let pass = 0; pass < 2000; pass++) {
    let moved = false;
    for (let i = 0; i < boxes.length; i++)
      for (let j = i + 1; j < boxes.length; j++) {
        const [a, b] = [boxes[i], boxes[j]];
        const ox = Math.min(a.p.x + a.x, b.p.x + b.x) - Math.max(a.p.x - a.x, b.p.x - b.x);
        const oy = Math.min(a.p.y + a.down, b.p.y + b.down) - Math.max(a.p.y - a.up, b.p.y - b.up);
        if (ox + FOOTPRINT_GAP <= 1e-6 || oy + FOOTPRINT_GAP <= 1e-6) continue;
        moved = true;
        const [left, right] = b.p.x >= a.p.x ? [a, b] : [b, a];
        const [upper, lower] = b.p.y >= a.p.y ? [a, b] : [b, a];
        if (ox < oy) {
          const before = right.p.x - left.p.x;
          left.p.x -= (ox + FOOTPRINT_GAP) / 2;
          right.p.x += (ox + FOOTPRINT_GAP) / 2;
          clampX(left);
          clampX(right);
          // a wall stopped the push: the rest goes down instead
          if (right.p.x - left.p.x - before < ox + FOOTPRINT_GAP - 1e-6)
            lower.p.y += oy + FOOTPRINT_GAP;
        } else {
          upper.p.y -= (oy + FOOTPRINT_GAP) / 2;
          lower.p.y += (oy + FOOTPRINT_GAP) / 2;
        }
      }
    for (const b of boxes) b.p.y = Math.max(top + b.up, b.p.y);
    if (!moved) {
      for (const b of boxes) positions.set(b.id, b.p);
      return;
    }
  }
  throw new Error(`Fiber graph: ${ids.length} parties still overlap in a ${width} px canvas`);
}

function layoutComponent(
  members: readonly string[],
  links: FiberGraphData['links']
): Map<string, LayoutPoint> {
  const inGroup = new Set(members);
  return kamadaKawaiLayout(
    members,
    links.filter(([a]) => inGroup.has(a))
  );
}

/**
 * Where every party goes on a canvas `width` px wide (R4): the main component fills the width,
 * and the islands either keep a strip at its right (wide canvases, the desktop board) or sink into
 * a row under it, wrapping to further rows. Nothing is scaled and nothing reaches past the width.
 */
export function fiberGraphLayout(data: FiberGraphData, width: number): FiberGraphModel {
  if (!Number.isFinite(width) || width <= 0)
    throw new Error(`Fiber graph: invalid canvas width ${width}`);
  const { byId, links, components } = data;
  const r = (id: string) => radius(byId.get(id)!);
  const widest = Math.max(0, ...data.parties.map((party) => 2 * reachX(radius(party))));
  if (widest > width)
    throw new Error(
      `Fiber graph: a ${width} px canvas cannot hold its biggest party (${Math.ceil(widest)} px)`
    );
  const { minHeight, pad, side, sideFrom } = FIBER_GRAPH;
  const rest = components.slice(1);
  const strip = width >= sideFrom;
  const sideX = strip && rest.length ? width - side : null;
  const positions = new Map<string, LayoutPoint>();
  let height: number;
  let islandsNote: FiberGraphModel['islandsNote'] = null;
  if (strip) {
    height = Math.max(minHeight, pad + 40 + rest.length * SIDE_SLOT + pad);
    if (components.length) {
      for (const [id, p] of fit(layoutComponent(components[0], links), {
        x0: pad,
        x1: (sideX ?? width) - pad,
        y0: pad,
        y1: minHeight - pad,
      }))
        positions.set(id, p);
      separate(components[0], positions, r, sideX ?? width, 0);
      const bottom = Math.max(
        ...components[0].map((id) => positions.get(id)!.y + reachDown(r(id)))
      );
      height = Math.max(height, Math.ceil(bottom + pad / 2));
    }
    rest.forEach((members, i) => {
      const y0 = pad + 40 + i * SIDE_SLOT;
      const ordered = [...members].sort((a, b) => byId.get(b)!.channels - byId.get(a)!.channels);
      if (ordered.length <= 2) {
        ordered.forEach((id, j) => positions.set(id, { x: sideX! + 30 + j * 80, y: y0 + j * 30 }));
        return;
      }
      for (const [id, p] of fit(layoutComponent(members, links), {
        x0: sideX! + 20,
        x1: width - 20,
        y0,
        y1: y0 + 80,
      }))
        positions.set(id, p);
    });
    if (sideX !== null) islandsNote = { x: width - 2, y: 48, anchor: 'end' };
  } else {
    let bottom = 0;
    if (components.length) {
      const main = components[0];
      const radii = main.map(r);
      const mx = Math.max(...radii.map(reachX));
      const top = Math.max(...radii.map(reachUp));
      const boxW = width - 2 * mx;
      // A Kamada–Kawai picture is round; a phone column is narrow, so the box is a little taller
      // than it is wide (pages scroll down, not across).
      const boxH = Math.round(Math.min(minHeight - 2 * pad, Math.max(280, boxW * 1.2)));
      for (const [id, p] of fit(layoutComponent(main, links), {
        x0: mx,
        x1: width - mx,
        y0: top,
        y1: top + boxH,
      }))
        positions.set(id, p);
      separate(main, positions, r, width, 0);
      bottom = Math.max(...main.map((id) => positions.get(id)!.y + reachDown(r(id))));
    }
    height = Math.ceil(bottom);
    if (rest.length) {
      const noteY = bottom + 14 + 11;
      islandsNote = { x: 0, y: Math.round(noteY), anchor: 'start' };
      // Pack the islands into rows left to right (biggest island first), then
      // draw each row on one centre line so its islands read level.
      const slots = rest.map((members) => {
        const ordered = [...members].sort((a, b) => byId.get(b)!.channels - byId.get(a)!.channels);
        const maxR = Math.max(...ordered.map(r));
        const inner = ordered.length === 1 ? 0 : ordered.length === 2 ? PAIR_SPAN : ISLAND_BOX.w;
        const slot = { members, ordered, maxR, w: 2 * reachX(maxR) + inner };
        if (slot.w > width)
          throw new Error(
            `Fiber graph: a ${width} px canvas cannot hold an island of ${ordered.length} parties (${Math.ceil(slot.w)} px)`
          );
        return slot;
      });
      const rows: (typeof slots)[] = [];
      let used = Infinity;
      for (const slot of slots) {
        if (used + ISLAND_GAP + slot.w > width) {
          rows.push([]);
          used = -ISLAND_GAP;
        }
        rows[rows.length - 1].push(slot);
        used += ISLAND_GAP + slot.w;
      }
      let rowTop = noteY + 10;
      let rowH = 0;
      rows.forEach((row, i) => {
        if (i > 0) rowTop += rowH + ISLAND_GAP;
        const up = Math.max(...row.map((slot) => reachUp(slot.maxR)));
        const big = row.some((slot) => slot.ordered.length > 2);
        rowH = up + (big ? ISLAND_BOX.h : 0) + Math.max(...row.map((slot) => reachDown(slot.maxR)));
        const y0 = rowTop + up;
        let slotX = 0;
        for (const slot of row) {
          const mx = reachX(slot.maxR);
          if (slot.ordered.length <= 2)
            slot.ordered.forEach((id, j) =>
              positions.set(id, { x: slotX + mx + j * PAIR_SPAN, y: y0 })
            );
          else
            for (const [id, p] of fit(layoutComponent(slot.members, links), {
              x0: slotX + mx,
              x1: slotX + mx + ISLAND_BOX.w,
              y0,
              y1: y0 + ISLAND_BOX.h,
            }))
              positions.set(id, p);
          slotX += slot.w + ISLAND_GAP;
        }
      });
      height = Math.ceil(rowTop + rowH);
    }
  }
  const sideIds = new Set(rest.flat());
  const nodes: FiberGraphNode[] = data.parties.map((party) => {
    const p = positions.get(party.lockScriptHash)!;
    return {
      id: party.lockScriptHash,
      party,
      label: partyTail(party),
      x: p.x,
      y: p.y,
      r: radius(party),
      channels: party.channels,
      openChannels: party.openChannels,
      ring: party.openChannels > 0,
      side: sideIds.has(party.lockScriptHash),
    };
  });
  const edges: FiberGraphEdge[] = data.pairs.map((pair) => {
    const a = positions.get(pair.firstLockScriptHash)!;
    const b = positions.get(pair.secondLockScriptHash)!;
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
    islandsNote,
    components,
    nodes,
    edges,
    neighbours: data.neighbours,
    unpairedChannels: data.unpairedChannels,
  };
}

export function fiberGraphModel(network: FiberNetwork, width: number): FiberGraphModel {
  return fiberGraphLayout(fiberGraphData(network), width);
}
