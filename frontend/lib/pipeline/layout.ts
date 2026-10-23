/**
 * Geometry of the transaction pipeline, ported from the approved prototype
 * (docs/superpowers/specs/2026-10-23-transaction-pipeline-prototype.html). All functions are pure.
 *
 * A 7 px grid: a tx is an n×n square of cells (area ∝ bytes), a proposal is a 5 px chip. Cards are
 * top-aligned; the chain runs through the card headers.
 */

import type {
  PipelineBlock,
  PipelineBlockRef,
  PipelineNext,
  PipelinePoolTx,
  PipelineProposal,
  PipelineProposedIn,
  PipelineSnapshot,
  PipelineTx,
} from '@/lib/api';
import {
  squareSide,
  txLook,
  type ShadeStep,
  type SquareStage,
  type SquareStyle,
  type TxLook,
} from '@/lib/pipeline/encoding';

export const U = 7;

const CHAIN_BASE = {
  H: 240,
  cardTop: 54,
  headH: 18,
  chainY: 63,
  padX: 5,
  propTop: 76,
  propRows: 3,
  chip: 5,
  chipP: 6,
  sepY: 97,
  fieldTop: 102,
  fieldRows: 17,
  padB: 5,
  memX0: 14,
  memCols: 17,
  pendLblY: 48,
  pendTop: 56,
  pendRows: 8,
  propLblY: 128,
  propPoolTop: 136,
  propPoolRows: 12,
  divX: 146,
  nextLblX: 152,
  nextX: 214,
  gap: 20,
  minCols: 6,
  maxCols: 14,
  minBlocks: 12,
  maxBlocks: 24,
} as const;

export const G = {
  ...CHAIN_BASE,
  // The chain starts past the widest card NEXT can grow to, so a busy NEXT never moves the chain.
  chainX0: CHAIN_BASE.nextX + CHAIN_BASE.maxCols * U - 1 + 2 * CHAIN_BASE.padX + CHAIN_BASE.gap,
} as const;

/** An uncle travels inside the block that includes it: its header and proposals, never its txs. */
export const UNC = { top: 4, head: 11, gap: 3, padB: 3, minCols: 11, chipRows: 2 } as const;

/**
 * Grid capacities in 1×1 cells. The backend truncates the pool lists and a block's tx list to these
 * (crates/api/src/routes/pipeline.rs cross-references them): pending 17 × 8, proposed 17 × 12,
 * block field 14 × 17.
 */
export const POOL_PENDING_CAPACITY = G.memCols * G.pendRows;
export const POOL_PROPOSED_CAPACITY = G.memCols * G.propPoolRows;
export const BLOCK_FIELD_CAPACITY = G.maxCols * G.fieldRows;

const clamp = (lo: number, hi: number, v: number) => Math.min(hi, Math.max(lo, v));

export const cardW = (cols: number) => cols * U - 1 + 2 * G.padX;
export const chipsPerRow = (cols: number) => Math.floor((cols * U) / G.chipP);
export const uncleChipsPerRow = (cols: number) => Math.floor((cols * U - 1 - 6) / G.chipP);

export function uncleBoxH(cols: number, nProposals: number): number {
  const rows = nProposals
    ? Math.min(UNC.chipRows, Math.ceil(nProposals / uncleChipsPerRow(cols)))
    : 0;
  return UNC.head + rows * G.chipP + UNC.padB;
}

/** Height of the uncle area of a card (0 without uncles); `counts` = proposals per uncle. */
export function unclesH(cols: number, counts: readonly number[]): number {
  if (!counts.length) return 0;
  return UNC.top + counts.reduce((h, n) => h + uncleBoxH(cols, n) + UNC.gap, 0);
}

/** Card columns: room for the tx area, the widest square and the block's own proposals. */
export function cardCols(sides: readonly number[], nProps: number, minCols: number = G.minCols) {
  const area = sides.reduce((s, n) => s + n * n, 0) + 1;
  let cols = Math.max(Math.ceil(Math.sqrt(area * 1.6)), ...sides);
  const needPerRow = Math.ceil(nProps / G.propRows);
  while (chipsPerRow(cols) < needPerRow && cols < G.maxCols) cols++;
  return clamp(minCols, G.maxCols, cols);
}

export const cardH = (rows: number, extra = 0) =>
  G.fieldTop + extra + Math.max(1, rows) * U - 1 + G.padB - G.cardTop;

export interface PackItem {
  id: string;
  n: number;
}

export interface PackPos {
  id: string;
  c: number;
  r: number;
  n: number;
}

export interface PackResult {
  pos: PackPos[];
  hidden: number;
  rows: number;
}

/** First-fit grid packing in reading order: later small squares fill the holes. */
export function pack2D(
  items: readonly PackItem[],
  cols: number,
  maxRows: number,
  reserveFirst: boolean
): PackResult {
  const occ: Uint8Array[] = [];
  const pos: PackPos[] = [];
  let hidden = 0;
  let rows = reserveFirst ? 1 : 0;
  if (reserveFirst) {
    occ[0] = new Uint8Array(cols);
    occ[0][0] = 1;
  }
  const free = (c: number, r: number, n: number) => {
    for (let j = r; j < r + n; j++) {
      const row = occ[j];
      if (!row) continue;
      for (let i = c; i < c + n; i++) if (row[i]) return false;
    }
    return true;
  };
  for (const t of items) {
    const n = Math.min(t.n, cols);
    let done = false;
    for (let r = 0; r + n <= maxRows && !done; r++) {
      for (let c = 0; c + n <= cols && !done; c++) {
        if (!free(c, r, n)) continue;
        for (let j = r; j < r + n; j++) {
          if (!occ[j]) occ[j] = new Uint8Array(cols);
          for (let i = c; i < c + n; i++) occ[j][i] = 1;
        }
        pos.push({ id: t.id, c, r, n });
        rows = Math.max(rows, r + n);
        done = true;
      }
    }
    if (!done) hidden++;
  }
  return { pos, hidden, rows };
}

export interface ChainSlot {
  /** Index into the cards passed in (newest first). */
  index: number;
  x: number;
  w: number;
  slot: 'landing' | 'chain';
}

/**
 * Card x positions, newest first. At least `minBlocks` cards, at most `maxBlocks`, otherwise cut at
 * the view width. A landing block (just mined, still being settled) sits at the NEXT position.
 */
export function chainLayout(
  cards: readonly { cols: number }[],
  viewW: number,
  landing: boolean
): ChainSlot[] {
  const out: ChainSlot[] = [];
  let x: number = G.chainX0;
  let n = 0;
  let i0 = 0;
  if (landing) {
    if (!cards.length) throw new Error('pipeline: a landing layout needs the landing block');
    out.push({ index: 0, x: G.nextX, w: cardW(cards[0].cols), slot: 'landing' });
    i0 = 1;
  }
  for (let i = i0; i < cards.length; i++) {
    const w = cardW(cards[i].cols);
    if (n >= G.maxBlocks || (n >= G.minBlocks && x + w > viewW - 10)) break;
    out.push({ index: i, x, w, slot: 'chain' });
    x += w + G.gap;
    n++;
  }
  return out;
}

export interface WindowParams {
  proposalWindowClosest: number;
  proposalWindowFarthest: number;
}

/** The proposal window of block H: the heights whose proposals H may commit. */
export function windowRange(H: number, w: WindowParams): { from: number; to: number } {
  return { from: H - w.proposalWindowFarthest, to: H - w.proposalWindowClosest };
}

export function inWindow(num: number, H: number, w: WindowParams): boolean {
  return num >= H - w.proposalWindowFarthest && num <= H - w.proposalWindowClosest;
}

export interface RulerSlot {
  number: number;
  x: number;
  w: number;
  /** false for the NEXT (predicted) card. */
  mined: boolean;
}

export interface WindowHit {
  /** The label lane spans x0 … x1; the header row spans xn … xf. */
  x0: number;
  x1: number;
  xn: number;
  xf: number;
}

export interface RulerGeom {
  ticks: number[];
  labelX: number | null;
  hit: WindowHit | null;
  /** The window runs past the last card on screen; the dotted tail carries it on. */
  tailOpen: boolean;
}

/** Two ticks in the middle of the gaps at both ends of the window of H, and one word of label. */
export function windowRuler(
  slots: readonly RulerSlot[],
  H: number,
  w: WindowParams,
  viewW: number
): RulerGeom {
  const at = (num: number) => slots.find((s) => s.number === num) ?? null;
  const hc = at(H);
  const near = at(H - w.proposalWindowClosest);
  const far = at(H - w.proposalWindowFarthest);
  const p = at(H - w.proposalWindowClosest + 1);
  const q = at(H - w.proposalWindowFarthest - 1);
  const ticks: number[] = [];
  let xn: number | null = null;
  let xf: number | null = null;
  if (near) {
    xn = p ? (p.x + p.w + near.x) / 2 : near.x - G.gap / 2;
    ticks.push(xn);
  }
  if (far) {
    xf = q ? (far.x + far.w + q.x) / 2 : far.x + far.w + G.gap / 2;
    ticks.push(xf);
  }
  const mined = slots.filter((s) => s.mined);
  const last = mined.length ? mined.reduce((a, b) => (b.x > a.x ? b : a)) : null;
  return {
    ticks,
    labelX: xn === null ? null : xn + 3,
    hit:
      xn === null
        ? null
        : { x0: hc ? hc.x : xn, x1: q ? q.x + q.w : (xf ?? viewW), xn, xf: xf ?? viewW },
    tailOpen: !!last && inWindow(last.number, H, w) && !far,
  };
}

export interface ChainSlotState extends RulerSlot {
  hash: string;
  parentHash: string;
  /** 0 = predicted/dashed … 1 = mined and settled. */
  solid: number;
}

export interface ChainPaths {
  solid: string;
  arrows: string;
  pend: string;
  win: string;
  winArrows: string;
  tail: string;
}

/**
 * The chain through the card headers, newest (left) to older (right); each gap carries an arrow to
 * the parent. A link is drawn only where the newer card's parentHash is the older card's hash, so
 * uncles and reorgs break the line honestly. Inside the window of H the line is gold.
 */
export function chainPaths(
  cards: readonly ChainSlotState[],
  next: { x: number; w: number } | null,
  H: number,
  w: WindowParams,
  viewW: number
): ChainPaths {
  const list = cards.slice().sort((a, b) => a.x - b.x);
  const y = G.chainY + 0.5;
  const f = (v: number) => v.toFixed(1);
  let solid = '';
  let arrows = '';
  let pend = '';
  let win = '';
  let winArrows = '';
  if (next && list.length) pend = `M${f(next.x + next.w + 2)} ${y}L${f(list[0].x - 2)} ${y}`;
  for (let i = 0; i + 1 < list.length; i++) {
    const a = list[i];
    const b = list[i + 1];
    if (a.parentHash !== b.hash) continue;
    const x0 = a.x + a.w + 2;
    const x1 = b.x - 2;
    if (x1 - x0 < 6) continue;
    const seg = `M${f(x0)} ${y}L${f(x1)} ${y}`;
    const arrow = `M${f(x1 - 4)} ${y - 3}L${f(x1)} ${y}L${f(x1 - 4)} ${y + 3}`;
    if (a.mined && b.mined && inWindow(a.number, H, w) && inWindow(b.number, H, w)) {
      win += seg;
      winArrows += arrow;
      continue;
    }
    if (a.solid < 0.5) pend += seg;
    else solid += seg;
    arrows += arrow;
  }
  const last = list[list.length - 1];
  return {
    solid,
    arrows,
    pend,
    win,
    winArrows,
    tail: last ? `M${f(last.x + last.w + 2)} ${y}L${viewW - 4} ${y}` : '',
  };
}

export interface PickSquare {
  id: string;
  /** Absolute SVG coordinates of the top-left corner. */
  X: number;
  Y: number;
  s: number;
}

export interface PickChip {
  card: string;
  key: string;
  X: number;
  Y: number;
}

export interface PickTargets {
  /** In drawing order: later squares are on top. */
  squares: readonly PickSquare[];
  chips: readonly PickChip[];
}

export type PickHit =
  | { kind: 'tx'; id: string }
  | { kind: 'chip'; card: string; key: string }
  | null;

/** Magnetic picking: inside a square (2 px margin) wins, else the nearest within 8 px; chips 4 px. */
export function pick(targets: PickTargets, x: number, y: number): PickHit {
  let inside: PickSquare | null = null;
  let near: PickSquare | null = null;
  let nearD = 8;
  for (const n of targets.squares) {
    const m = 2;
    if (x >= n.X - m && x <= n.X + n.s + m && y >= n.Y - m && y <= n.Y + n.s + m) inside = n;
    const d = Math.hypot(n.X + n.s / 2 - x, n.Y + n.s / 2 - y) - n.s / 2;
    if (d < nearD) {
      nearD = d;
      near = n;
    }
  }
  if (inside) return { kind: 'tx', id: inside.id };
  let chip: PickChip | null = null;
  let chipD = 4;
  for (const k of targets.chips) {
    const d = Math.hypot(k.X + G.chip / 2 - x, k.Y + G.chip / 2 - y);
    if (d < chipD) {
      chipD = d;
      chip = k;
    }
  }
  if (chip && (!near || chipD < nearD)) return { kind: 'chip', card: chip.card, key: chip.key };
  return near ? { kind: 'tx', id: near.id } : null;
}

// ---------- Scene: one snapshot → the target geometry of everything drawn ----------

export type ScenePhase = 'settled' | 'landing';

export interface SceneSquare {
  /** tx hash (the cellbase hash for the cellbase). */
  id: string;
  kind: 'tx' | 'cellbase';
  stage: SquareStage;
  style: SquareStyle;
  step: ShadeStep;
  /** Local to the container: the pool (absolute) or the card group (translated by the card x). */
  x: number;
  y: number;
  s: number;
  /** 'pool' or the block hash of the card. */
  container: string;
}

export interface SceneChip {
  key: string;
  proposal: PipelineProposal;
  /** Local to the card. */
  x: number;
  y: number;
}

export interface SceneUncle {
  index: number;
  ref: PipelineBlockRef;
  y: number;
  h: number;
  label: string;
  overflow: number;
}

export interface SceneCard {
  /** Block hash. */
  id: string;
  block: PipelineBlock;
  slot: 'landing' | 'chain';
  x: number;
  w: number;
  h: number;
  cols: number;
  chips: SceneChip[];
  /** Own proposals that do not fit the three chip rows. */
  chipOverflow: number;
  uncles: SceneUncle[];
  squares: SceneSquare[];
  /** Txs not drawn: the field's overflow plus the ones the backend omitted. */
  hidden: number;
}

export interface SceneGhost {
  x: number;
  y: number;
  s: number;
  cellbase: boolean;
}

export interface SceneNext {
  number: number;
  x: number;
  w: number;
  h: number;
  cols: number;
  next: PipelineNext;
  ghosts: SceneGhost[];
  ghostChips: { x: number; y: number }[];
  chipOverflow: number;
  /** Predicted txs not drawn: field overflow plus hashes outside the (truncated) proposed list. */
  hidden: number;
}

export interface ScenePoolGrid {
  squares: SceneSquare[];
  total: number;
  hidden: number;
}

export interface Scene {
  viewW: number;
  phase: ScenePhase;
  snapshot: PipelineSnapshot;
  /** The window defaults to the block after this one (NEXT, or the landing block). */
  windowTip: number;
  pool: { pending: ScenePoolGrid; proposed: ScenePoolGrid } | null;
  next: SceneNext | null;
  cards: SceneCard[];
}

export const tail4 = (n: number) => `…${String(n).slice(-4)}`;

interface Sized {
  hash: string;
  size: number;
  feeRate: string;
}

/** Big squares first, then by fee rate (prototype `bySize`). */
const bySize = (a: Sized, b: Sized) =>
  squareSide(b.size) - squareSide(a.size) || Number(b.feeRate) - Number(a.feeRate);

const square = (
  id: string,
  kind: SceneSquare['kind'],
  look: TxLook,
  x: number,
  y: number,
  s: number,
  container: string
): SceneSquare => ({
  id,
  kind,
  stage: look.stage,
  style: look.style,
  step: look.step,
  x,
  y,
  s,
  container,
});

function poolGrid(
  txs: readonly PipelinePoolTx[],
  total: number,
  top: number,
  rows: number,
  lookOf: (t: PipelinePoolTx) => TxLook
): ScenePoolGrid {
  if (total < txs.length) {
    throw new Error(`pipeline: pool total ${total} is below the ${txs.length} txs listed`);
  }
  const byHash = new Map(txs.map((t) => [t.hash, t]));
  const p = pack2D(
    txs.map((t) => ({ id: t.hash, n: squareSide(t.size) })),
    G.memCols,
    rows,
    false
  );
  return {
    squares: p.pos.map((q) =>
      square(
        q.id,
        'tx',
        lookOf(byHash.get(q.id) as PipelinePoolTx),
        G.memX0 + q.c * U,
        top + q.r * U,
        q.n * U - 1,
        'pool'
      )
    ),
    total,
    hidden: p.hidden + (total - txs.length),
  };
}

interface CardGeom {
  block: PipelineBlock;
  cols: number;
  uExtra: number;
  pack: PackResult;
  own: PipelineProposal[];
  byUncle: PipelineProposal[][];
}

function cardGeom(b: PipelineBlock): CardGeom {
  if (b.omittedTxs !== b.txCount - 1 - b.txs.length) {
    throw new Error(
      `pipeline: block #${b.number} omittedTxs ${b.omittedTxs} != txCount ${b.txCount} − 1 − ${b.txs.length} listed`
    );
  }
  const own: PipelineProposal[] = [];
  const byUncle: PipelineProposal[][] = b.uncles.map(() => []);
  for (const p of b.proposals) {
    if (p.uncleIndex === null) own.push(p);
    else if (p.uncleIndex >= 0 && p.uncleIndex < byUncle.length) byUncle[p.uncleIndex].push(p);
    else {
      throw new Error(
        `pipeline: block #${b.number} proposal ${p.shortId} points at uncle ${p.uncleIndex} of ${b.uncles.length}`
      );
    }
  }
  const order = b.txs.slice().sort(bySize);
  const sides = order.map((t) => squareSide(t.size));
  const cols = cardCols(sides, own.length, b.uncles.length ? UNC.minCols : G.minCols);
  const uExtra = unclesH(
    cols,
    byUncle.map((u) => u.length)
  );
  const pack = pack2D(
    order.map((t, i) => ({ id: t.hash, n: sides[i] })),
    cols,
    G.fieldRows - Math.ceil(uExtra / U),
    true
  );
  return { block: b, cols, uExtra, pack, own, byUncle };
}

function sceneCard(g: CardGeom, x: number, slot: SceneCard['slot']): SceneCard {
  const b = g.block;
  const w = cardW(g.cols);
  const per = chipsPerRow(g.cols);
  const cap = per * G.propRows;
  const chips: SceneChip[] = g.own.slice(0, cap).map((p, i) => ({
    key: `${p.shortId}|`,
    proposal: p,
    x: G.padX + (i % per) * G.chipP,
    y: G.propTop + Math.floor(i / per) * G.chipP,
  }));
  const uncles: SceneUncle[] = [];
  let top: number = G.fieldTop - 1;
  const per2 = uncleChipsPerRow(g.cols);
  const cap2 = per2 * UNC.chipRows;
  g.byUncle.forEach((list, ui) => {
    const h = uncleBoxH(g.cols, list.length);
    const ref = b.uncles[ui];
    uncles.push({
      index: ui,
      ref,
      y: top,
      h,
      label: `uncle ${tail4(ref.number)}`,
      overflow: Math.max(0, list.length - cap2),
    });
    list.slice(0, cap2).forEach((p, i) => {
      chips.push({
        key: `${p.shortId}|${ui}`,
        proposal: p,
        x: G.padX + 3 + (i % per2) * G.chipP,
        y: top + UNC.head + Math.floor(i / per2) * G.chipP,
      });
    });
    top += h + UNC.gap;
  });
  const ft = G.fieldTop + g.uExtra;
  const txByHash = new Map(b.txs.map((t) => [t.hash, t]));
  const squares: SceneSquare[] = [
    square(
      b.cellbase.hash,
      'cellbase',
      txLook({ stage: 'cellbase', cycles: null, tooEarly: false }),
      G.padX,
      ft,
      U - 1,
      b.hash
    ),
    ...g.pack.pos.map((q) => {
      const t = txByHash.get(q.id) as PipelineTx;
      return square(
        q.id,
        'tx',
        txLook({ stage: 'committed', cycles: t.cycles, tooEarly: false }),
        G.padX + q.c * U,
        ft + q.r * U,
        q.n * U - 1,
        b.hash
      );
    }),
  ];
  return {
    id: b.hash,
    block: b,
    slot,
    x,
    w,
    h: cardH(g.pack.rows, g.uExtra),
    cols: g.cols,
    chips,
    chipOverflow: Math.max(0, g.own.length - cap),
    uncles,
    squares,
    hidden: g.pack.hidden + b.omittedTxs,
  };
}

function sceneNext(s: PipelineSnapshot, next: PipelineNext, proposed: readonly PipelinePoolTx[]) {
  const byHash = new Map(proposed.map((t) => [t.hash, t]));
  const known: PipelinePoolTx[] = [];
  let unknown = 0;
  for (const h of next.txHashes) {
    const t = byHash.get(h);
    if (t) known.push(t);
    else unknown++;
  }
  const order = known.sort(bySize);
  const sides = order.map((t) => squareSide(t.size));
  const cols = cardCols(sides, next.proposals);
  const fp = pack2D(
    order.map((t, i) => ({ id: t.hash, n: sides[i] })),
    cols,
    G.fieldRows,
    true
  );
  const per = chipsPerRow(cols);
  const cap = per * G.propRows;
  const ghostChips = Array.from({ length: Math.min(next.proposals, cap) }, (_, i) => ({
    x: G.padX + (i % per) * G.chipP,
    y: G.propTop + Math.floor(i / per) * G.chipP,
  }));
  const scene: SceneNext = {
    number: s.tip.number + 1,
    x: G.nextX,
    w: cardW(cols),
    h: cardH(fp.rows),
    cols,
    next,
    ghosts: [
      { x: G.padX, y: G.fieldTop, s: U - 1, cellbase: true },
      ...fp.pos.map((q) => ({
        x: G.padX + q.c * U,
        y: G.fieldTop + q.r * U,
        s: q.n * U - 1,
        cellbase: false,
      })),
    ],
    ghostChips,
    chipOverflow: Math.max(0, next.proposals - cap),
    hidden: fp.hidden + unknown,
  };
  return scene;
}

/**
 * Lay out one snapshot. Every chain fact comes from the snapshot; this only places it. Contract
 * violations the layout depends on throw with context instead of being drawn around.
 */
export function buildScene(s: PipelineSnapshot, viewW: number, phase: ScenePhase): Scene {
  if ((s.status === 'ok') !== (s.pool !== null) || (s.pool === null) !== (s.next === null)) {
    throw new Error(
      `pipeline: status ${s.status} with pool ${s.pool ? 'present' : 'null'} and next ${s.next ? 'present' : 'null'}`
    );
  }
  if (!s.blocks.length || s.blocks[0].hash !== s.tip.hash) {
    throw new Error(`pipeline: newest block is not the tip #${s.tip.number} ${s.tip.hash}`);
  }
  const byNumber = new Map(s.blocks.map((b) => [b.number, b]));

  let pool: Scene['pool'] = null;
  if (s.pool) {
    const p = s.pool;
    const pending = p.pending.slice().sort((a, b) => a.arrivedAt - b.arrivedAt);
    const proposed = p.proposed
      .slice()
      .sort(
        (a, b) =>
          (b.proposedIn as PipelineProposedIn).block - (a.proposedIn as PipelineProposedIn).block ||
          a.arrivedAt - b.arrivedAt
      );
    for (const t of pending) {
      if (t.proposedIn !== null) throw new Error(`pipeline: pending tx ${t.hash} has a proposedIn`);
    }
    pool = {
      pending: poolGrid(pending, p.pendingTotal, G.pendTop, G.pendRows, (t) =>
        txLook({ stage: 'pending', cycles: t.cycles, tooEarly: false })
      ),
      proposed: poolGrid(proposed, p.proposedTotal, G.propPoolTop, G.propPoolRows, (t) => {
        const at = t.proposedIn;
        if (!at) throw new Error(`pipeline: proposed tx ${t.hash} has no proposedIn`);
        const blk = byNumber.get(at.block);
        const proposal = blk?.proposals.find(
          (q) => q.txHash === t.hash && q.uncleIndex === at.uncleIndex
        );
        if (!proposal) {
          throw new Error(
            `pipeline: proposed tx ${t.hash} names #${at.block} (uncle ${at.uncleIndex}) but that block has no proposal for it`
          );
        }
        // "Too early" is the proposal's own state: the backend decides it once.
        return txLook({
          stage: 'proposed',
          cycles: t.cycles,
          tooEarly: proposal.state === 'early',
        });
      }),
    };
  }

  const geoms = s.blocks.map(cardGeom);
  const slots = chainLayout(geoms, viewW, phase === 'landing');
  const cards = slots.map((slot) => sceneCard(geoms[slot.index], slot.x, slot.slot));
  const next =
    phase === 'settled' && s.next && s.pool ? sceneNext(s, s.next, s.pool.proposed) : null;
  return {
    viewW,
    phase,
    snapshot: s,
    windowTip: phase === 'landing' ? s.tip.number - 1 : s.tip.number,
    pool,
    next,
    cards,
  };
}

/** Absolute pick targets of a scene, in drawing order. */
export function pickTargets(scene: Scene): PickTargets {
  const squares: PickSquare[] = [];
  const chips: PickChip[] = [];
  if (scene.pool) {
    for (const q of [...scene.pool.pending.squares, ...scene.pool.proposed.squares]) {
      squares.push({ id: q.id, X: q.x, Y: q.y, s: q.s });
    }
  }
  for (const c of scene.cards) {
    for (const q of c.squares) squares.push({ id: q.id, X: c.x + q.x, Y: q.y, s: q.s });
    for (const k of c.chips) chips.push({ card: c.id, key: k.key, X: c.x + k.x, Y: k.y });
  }
  return { squares, chips };
}
