/**
 * Geometry of the transaction pipeline. It started as a port of the approved prototype
 * (docs/superpowers/specs/2026-10-23-transaction-pipeline-prototype.html) and now deliberately
 * differs from it: the panel sits in the page container like the other homepage rows, so the canvas
 * is one 132 px row holding one tx lifetime, left to right (mempool well ⇢ proposed well ⇢ NEXT ⇢
 * the tip → older blocks), and every card is one fixed size (8 columns × 98 px) instead of growing
 * with its content. All functions are pure.
 *
 * A 10 px grid: a tx is an n×n square of cells (area ∝ bytes), a proposal is a 6 px dot on an 8 px
 * pitch. Cards are top-aligned; the chain runs through the card headers.
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

export const U = 10;

const CHAIN_BASE = {
  H: 132,
  /**
   * One lane above the cards, empty until hovered: the proposal window's bracket and the reward
   * link both run here.
   */
  winY: 16,
  cardTop: 28,
  headH: 18,
  chainY: 37,
  padX: 5,
  propTop: 50,
  /** Two rows of proposal dots: `chip` px dots on a `chipP` px pitch. */
  propRows: 2,
  chip: 6,
  chipP: 8,
  /** The bottom of the proposal rows: the baseline of their "+N". */
  sepY: 68,
  fieldTop: 72,
  /**
   * Every card (the two pool wells, NEXT, a block) is this many columns wide whatever it holds;
   * what does not fit is counted as "+N" (dots at the end of their rows, txs in the header).
   */
  cardCols: 8,
  fieldRows: 5,
  padB: 5,
  /** A pool well's body: tx squares from under its header row to its bottom. */
  poolTop: 50,
  poolRows: 7,
  x0: 6,
  gap: 20,
  /** NEXT stands alone between the pool and the chain: the gaps on both of its sides are wider. */
  frontierGap: 44,
  /** The narrowest canvas: below it the panel scrolls instead of laying out a narrower row. */
  minViewW: 760,
  /** Kept free between the last chain card and the right edge of the view. */
  marginR: 10,
  maxBlocks: 24,
} as const;

/** Card width in px: `cardCols` cells and the padding on both sides. */
const CARD_W = CHAIN_BASE.cardCols * U - 1 + 2 * CHAIN_BASE.padX;
const PITCH = CARD_W + CHAIN_BASE.gap;

const NEXT_X = CHAIN_BASE.x0 + PITCH + CARD_W + CHAIN_BASE.frontierGap;
const CHAIN_X0 = NEXT_X + CARD_W + CHAIN_BASE.frontierGap;

/**
 * How many whole chain cards fit a view `viewW` px wide: card k ends at chainX0 + k·pitch + cardW,
 * which must stay `marginR` clear of the right edge.
 */
function wholeCards(viewW: number): number {
  return Math.floor((viewW - CHAIN_BASE.marginR - CHAIN_X0 + CHAIN_BASE.gap) / PITCH);
}

/**
 * One row, one tx lifetime, left to right: mempool (pending) ⇢ proposed ⇢ NEXT ⇢ the tip → older
 * blocks. Every card is the same size. NEXT stands alone between the pool and the chain: the
 * `frontierGap` on both of its sides and the change of material mark that boundary, so there is no
 * band or background behind it.
 */
export const G = {
  ...CHAIN_BASE,
  cardW: CARD_W,
  mempoolX: CHAIN_BASE.x0,
  proposedX: CHAIN_BASE.x0 + PITCH,
  nextX: NEXT_X,
  chainX0: CHAIN_X0,
  /** The fewest chain blocks ever shown: the ones that fit the narrowest canvas. */
  minBlocks: wholeCards(CHAIN_BASE.minViewW),
} as const;

/** An uncle travels inside the block that includes it: its header and proposals, never its txs. */
export const UNC = { top: 4, head: 11, gap: 3, padB: 3, chipRows: 1 } as const;

/**
 * Grid capacities in 1×1 cells. The backend truncates the pool lists and a block's tx list to these
 * (crates/api/src/routes/pipeline.rs cross-references them): each pool well's body is 8 × 7 = 56,
 * a block field 8 × 5 = 40, of which the cellbase takes the first cell (so 39 listed txs).
 */
export const POOL_PENDING_CAPACITY = G.cardCols * G.poolRows;
export const POOL_PROPOSED_CAPACITY = G.cardCols * G.poolRows;
export const BLOCK_FIELD_CAPACITY = G.cardCols * G.fieldRows;

/** Proposal dots per row of a card's proposal area. */
export const CHIPS_PER_ROW = Math.floor((G.cardCols * U) / G.chipP);
/** Proposal dots per row inside an uncle mini-card, inset 3 px on both sides. */
export const UNCLE_CHIPS_PER_ROW = Math.floor((G.cardCols * U - 1 - 6) / G.chipP);

export function uncleBoxH(nProposals: number): number {
  const rows = nProposals ? Math.min(UNC.chipRows, Math.ceil(nProposals / UNCLE_CHIPS_PER_ROW)) : 0;
  return UNC.head + rows * G.chipP + UNC.padB;
}

/** Height of the uncle area of a card (0 without uncles); `counts` = proposals per uncle. */
export function unclesH(counts: readonly number[]): number {
  if (!counts.length) return 0;
  return UNC.top + counts.reduce((h, n) => h + uncleBoxH(n) + UNC.gap, 0);
}

/**
 * How many of `total` dots to draw in `cap` slots. When they do not fit, the last row gives up one
 * slot per glyph of the "+N" label drawn at its end (a 10 px mono glyph is 6 px, under one 8 px
 * pitch), so the label never covers a dot.
 */
export function chipSlots(total: number, cap: number): { shown: number; overflow: number } {
  if (total <= cap) return { shown: total, overflow: 0 };
  for (let r = 1; r <= cap; r++) {
    const overflow = total - (cap - r);
    if (r >= String(overflow).length + 1) return { shown: cap - r, overflow };
  }
  throw new Error(`pipeline: no room for a +N label in ${cap} chip slots`);
}

/**
 * Every card is one fixed height: the header, the proposal rows and a `fieldRows`-tall tx field. An
 * uncle mini-card takes its rows out of the field, so a card never grows; txs that do not fit are
 * counted as "+N".
 */
export const CARD_H = G.fieldTop + G.fieldRows * U - 1 + G.padB - G.cardTop;

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
        done = true;
      }
    }
    if (!done) hidden++;
  }
  return { pos, hidden };
}

export interface ChainSlot {
  /** Index into the cards passed in (newest first). */
  index: number;
  x: number;
  w: number;
  slot: 'landing' | 'chain';
}

/**
 * x positions of `count` cards, newest first: as many whole chain cards as fit the view (never a
 * card cut by its edge), at most `maxBlocks`. The view is never narrower than `minViewW` (the panel
 * scrolls below it), so at least `minBlocks` fit. A landing block (just mined, still being
 * settled) sits at the NEXT position.
 */
export function chainLayout(count: number, viewW: number, landing: boolean): ChainSlot[] {
  if (viewW < G.minViewW) {
    throw new Error(`pipeline: a ${viewW} px view is narrower than the ${G.minViewW} px canvas`);
  }
  const out: ChainSlot[] = [];
  const w = G.cardW;
  let i0 = 0;
  if (landing) {
    if (!count) throw new Error('pipeline: a landing layout needs the landing block');
    out.push({ index: 0, x: G.nextX, w, slot: 'landing' });
    i0 = 1;
  }
  const fit = Math.min(G.maxBlocks, wholeCards(viewW));
  for (let n = 0; n < fit && i0 + n < count; n++) {
    out.push({ index: i0 + n, x: G.chainX0 + n * PITCH, w, slot: 'chain' });
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

/** Where the window's one line of text goes (drawn with text-anchor middle at `x`). */
export interface WindowLabelGeom {
  /** Centre of the text. */
  x: number;
  /** The label's room: the text plus `LABEL_PAD` on both sides. */
  x0: number;
  x1: number;
  /** Between the ticks (the dimension line breaks around it), or outside, before the near tick. */
  at: 'between' | 'outside';
}

export interface RulerGeom {
  ticks: number[];
  /** null when the label fits nowhere in view (the near tick itself is outside the view). */
  label: WindowLabelGeom | null;
  hit: WindowHit | null;
  /** The window runs past the last card on screen; the dotted tail carries it on. */
  tailOpen: boolean;
}

/** The room the dimension line leaves on each side of its label. */
const LABEL_PAD = 6;

/**
 * The proposal window of H as a dimension line: a tick in the middle of the gap at each end of the
 * window that is on screen (the far end is often past the last card), and where `label` goes.
 *
 * The label follows the drafting rule for dimension text, in this order:
 * 1. Between the ticks, centred on the part of the window in view (from the near tick to the far
 *    tick, or to the view edge when the far end is off screen), when the text and its padding fit
 *    there. The dimension line breaks around it.
 * 2. Otherwise outside the window, its padding ending at the near tick (the newer side, towards
 *    NEXT). The dimension line is then drawn whole.
 * 3. Otherwise no label: only when the near tick is itself outside the view.
 * So the text is always fully inside [0, viewW] and never crosses a tick. The static render and the
 * motion layer both draw the ticks, the line (`rulerPath`) and the label from this one result.
 */
export function windowRuler(
  slots: readonly RulerSlot[],
  H: number,
  w: WindowParams,
  viewW: number,
  text: string
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
  let label: WindowLabelGeom | null = null;
  if (xn !== null && xn >= 0 && xn <= viewW) {
    const room = labelWidth(text) + 2 * LABEL_PAD;
    const end = Math.min(xf ?? viewW, viewW);
    if (room <= end - xn) {
      const x = (xn + end) / 2;
      label = { x, x0: x - room / 2, x1: x + room / 2, at: 'between' };
    } else if (room <= xn) {
      label = { x: xn - room / 2, x0: xn - room, x1: xn, at: 'outside' };
    }
  }
  const mined = slots.filter((s) => s.mined);
  const last = mined.length ? mined.reduce((a, b) => (b.x > a.x ? b : a)) : null;
  return {
    ticks,
    label,
    hit:
      xn === null
        ? null
        : { x0: hc ? hc.x : xn, x1: q ? q.x + q.w : (xf ?? viewW), xn, xf: xf ?? viewW },
    tailOpen: !!last && inWindow(last.number, H, w) && !far,
  };
}

/**
 * The hovered window: the ticks rise from above the dimension line at `winY` onto the header row,
 * and the line joins them, broken around a label that sits between them. One path, shared by the
 * static render and the motion layer.
 */
export function rulerPath(r: RulerGeom): string {
  const f = (v: number) => v.toFixed(1);
  const ticks = r.ticks.map((x) => `M${f(x)} ${G.winY - 5}V${G.cardTop + G.headH}`).join('');
  if (!r.hit) return ticks;
  const { xn, xf } = r.hit;
  const y = G.winY;
  if (r.label?.at !== 'between') return `${ticks}M${f(xn)} ${y}H${f(xf)}`;
  return `${ticks}M${f(xn)} ${y}H${f(r.label.x0)}M${f(r.label.x1)} ${y}H${f(xf)}`;
}

/**
 * The window at rest: a short gold mark across the chain line at each end in view. Hovered, the
 * same ticks rise to the dimension line (`rulerPath`).
 */
export function windowMarksPath(r: RulerGeom): string {
  return r.ticks.map((x) => `M${x.toFixed(1)} ${G.chainY - 4.5}V${G.chainY + 5.5}`).join('');
}

/** Width of a 10 px JetBrains Mono label: every glyph advances 0.6 em. */
export function labelWidth(text: string): number {
  return text.length * 6;
}

/**
 * The window's one line of text: whose window it is, and what the ticks bound. `windowTip` is the
 * scene's: the window of `windowTip + 1` is the one of the block being formed.
 */
export function windowLabel(H: number, windowTip: number, w: WindowParams): string {
  const n = w.proposalWindowFarthest - w.proposalWindowClosest + 1;
  const whose = H === windowTip + 1 ? 'next block can' : `${tail4(H)} could`;
  return `${whose} commit txs proposed in these ${n} blocks`;
}

export interface Point {
  X: number;
  Y: number;
}

/** The chain's off-screen tail: dotted from `x0` to `x1`. */
export interface ChainTail {
  x0: number;
  x1: number;
}

/**
 * Where the chain runs on past the last card on screen: from just after that card to 4 px before
 * the view edge. The chain line and an off-screen reward link both end in this one span.
 */
export function chainTail(
  cards: readonly { x: number; w: number }[],
  viewW: number
): ChainTail | null {
  if (!cards.length) return null;
  const last = cards.reduce((a, b) => (b.x > a.x ? b : a));
  return { x0: last.x + last.w + 2, x1: viewW - 4 };
}

export interface RewardLink {
  /** From the paying cellbase up into the lane, along it, and down into the paid one if shown. */
  d: string;
  /** The open dotted end when the paid cellbase is off screen; '' when the link drops into it. */
  tail: string;
}

/**
 * The reward link of a hovered cellbase at `a`. It rises into the lane above the cards (the
 * window's lane: the ruler steps back while a link is shown) and runs right. With the paid block's
 * cellbase on screen at `b` it drops into it. Without (`b` null: the paid block is past the right
 * edge) it runs on to where the chain's own tail starts and ends there in the same open dotted
 * tail, one lane up; it never drops anywhere.
 */
export function rewardLink(a: Point, b: Point | null, tail: ChainTail): RewardLink {
  const top = G.winY;
  const r = 5;
  const f = (v: number) => v.toFixed(1);
  if (b) {
    const dir = Math.sign(b.X - a.X) || 1;
    return {
      d: `M${f(a.X)} ${f(a.Y)}V${top + r}Q${f(a.X)} ${top} ${f(a.X + dir * r)} ${top}H${f(b.X - dir * r)}Q${f(b.X)} ${top} ${f(b.X)} ${top + r}V${f(b.Y)}`,
      tail: '',
    };
  }
  if (tail.x0 < a.X + r) {
    throw new Error(
      `pipeline: a reward link from x ${a.X} cannot end in a tail at ${tail.x0}, before its bend into the lane`
    );
  }
  return {
    d: `M${f(a.X)} ${f(a.Y)}V${top + r}Q${f(a.X)} ${top} ${f(a.X + r)} ${top}H${f(tail.x0)}`,
    tail: `M${f(tail.x0)} ${top}H${f(tail.x1)}`,
  };
}

export interface ChainSlotState extends RulerSlot {
  hash: string;
  parentHash: string;
  /** 0 = predicted/dashed … 1 = mined and settled. */
  solid: number;
}

export interface ChainPaths {
  /** Links between settled blocks. */
  solid: string;
  /** Dashed: NEXT ⇢ the tip, and links from a block that is still landing. */
  pend: string;
  /** Links between two blocks inside the window of H: gold. */
  win: string;
  /** Dotted, from the last card to the view edge. */
  tail: string;
}

/**
 * The chain through the card headers, newest (left) to older (right): plain segments, no
 * arrowheads. A link is drawn only where the newer card's parentHash is the older card's hash, so
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
  let pend = '';
  let win = '';
  if (next && list.length) pend = `M${f(next.x + next.w + 2)} ${y}L${f(list[0].x - 2)} ${y}`;
  for (let i = 0; i + 1 < list.length; i++) {
    const a = list[i];
    const b = list[i + 1];
    if (a.parentHash !== b.hash) continue;
    const x0 = a.x + a.w + 2;
    const x1 = b.x - 2;
    if (x1 - x0 < 6) continue;
    const seg = `M${f(x0)} ${y}L${f(x1)} ${y}`;
    if (a.mined && b.mined && inWindow(a.number, H, w) && inWindow(b.number, H, w)) win += seg;
    else if (a.solid < 0.5) pend += seg;
    else solid += seg;
  }
  const tail = chainTail(list, viewW);
  return {
    solid,
    pend,
    win,
    tail: tail ? `M${f(tail.x0)} ${y}L${tail.x1} ${y}` : '',
  };
}

export interface PickSquare {
  id: string;
  /** Absolute SVG coordinates of the top-left corner. */
  X: number;
  Y: number;
  s: number;
  /** A predicted tx on the NEXT card. */
  ghost?: boolean;
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
  | { kind: 'ghost'; id: string }
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
  const hitOf = (n: PickSquare): PickHit => ({ kind: n.ghost ? 'ghost' : 'tx', id: n.id });
  if (inside) return hitOf(inside);
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
  return near ? hitOf(near) : null;
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
  chips: SceneChip[];
  /** Own proposals that do not fit the two rows of dots. */
  chipOverflow: number;
  uncles: SceneUncle[];
  squares: SceneSquare[];
  /** Txs not drawn: the field's overflow plus the ones the backend omitted. */
  hidden: number;
}

export interface SceneGhost {
  /** The predicted tx; null for the predicted cellbase. */
  id: string | null;
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
  next: PipelineNext;
  ghosts: SceneGhost[];
  ghostChips: { x: number; y: number }[];
  chipOverflow: number;
  /** Predicted txs not drawn: field overflow plus the ones the backend omitted. */
  hidden: number;
}

export interface ScenePoolGrid {
  squares: SceneSquare[];
  /** The stage's total in the node's pool (listed or not): the well shows it on its header row. */
  total: number;
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
const bySize = (a: Sized, b: Sized) => {
  const sizeOrder = squareSide(b.size) - squareSide(a.size);
  if (sizeOrder !== 0) return sizeOrder;
  const aRate = BigInt(a.feeRate);
  const bRate = BigInt(b.feeRate);
  return bRate > aRate ? 1 : bRate < aRate ? -1 : 0;
};

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
  x0: number,
  cols: number,
  lookOf: (t: PipelinePoolTx) => TxLook
): ScenePoolGrid {
  if (total < txs.length) {
    throw new Error(`pipeline: pool total ${total} is below the ${txs.length} txs listed`);
  }
  const byHash = new Map(txs.map((t) => [t.hash, t]));
  const p = pack2D(
    txs.map((t) => ({ id: t.hash, n: squareSide(t.size) })),
    cols,
    G.poolRows,
    false
  );
  return {
    squares: p.pos.map((q) =>
      square(
        q.id,
        'tx',
        lookOf(byHash.get(q.id) as PipelinePoolTx),
        x0 + q.c * U,
        G.poolTop + q.r * U,
        q.n * U - 1,
        'pool'
      )
    ),
    total,
  };
}

interface CardGeom {
  block: PipelineBlock;
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
  const uExtra = unclesH(byUncle.map((u) => u.length));
  const pack = pack2D(
    order.map((t) => ({ id: t.hash, n: squareSide(t.size) })),
    G.cardCols,
    G.fieldRows - Math.ceil(uExtra / U),
    true
  );
  return { block: b, uExtra, pack, own, byUncle };
}

function sceneCard(g: CardGeom, x: number, slot: SceneCard['slot']): SceneCard {
  const b = g.block;
  const per = CHIPS_PER_ROW;
  const own = chipSlots(g.own.length, per * G.propRows);
  const chips: SceneChip[] = g.own.slice(0, own.shown).map((p, i) => ({
    key: `${p.shortId}|`,
    proposal: p,
    x: G.padX + (i % per) * G.chipP,
    y: G.propTop + Math.floor(i / per) * G.chipP,
  }));
  const uncles: SceneUncle[] = [];
  let top: number = G.fieldTop - 1;
  const per2 = UNCLE_CHIPS_PER_ROW;
  const cap2 = per2 * UNC.chipRows;
  g.byUncle.forEach((list, ui) => {
    const h = uncleBoxH(list.length);
    const ref = b.uncles[ui];
    uncles.push({
      index: ui,
      ref,
      y: top,
      h,
      label: 'uncle',
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
    w: G.cardW,
    h: CARD_H,
    chips,
    chipOverflow: own.overflow,
    uncles,
    squares,
    hidden: g.pack.hidden + b.omittedTxs,
  };
}

function sceneNext(s: PipelineSnapshot, next: PipelineNext): SceneNext {
  if (!Number.isInteger(next.omittedTxs) || next.omittedTxs < 0) {
    throw new Error(`pipeline: next.omittedTxs ${next.omittedTxs} is not a count`);
  }
  // Drawn in the backend's order (by size, descending), like a block's txs.
  const fp = pack2D(
    next.txs.map((t) => ({ id: t.hash, n: squareSide(t.size) })),
    G.cardCols,
    G.fieldRows,
    true
  );
  const per = CHIPS_PER_ROW;
  const slots = chipSlots(next.proposals, per * G.propRows);
  const ghostChips = Array.from({ length: slots.shown }, (_, i) => ({
    x: G.padX + (i % per) * G.chipP,
    y: G.propTop + Math.floor(i / per) * G.chipP,
  }));
  return {
    number: s.tip.number + 1,
    x: G.nextX,
    w: G.cardW,
    h: CARD_H,
    next,
    ghosts: [
      { id: null, x: G.padX, y: G.fieldTop, s: U - 1, cellbase: true },
      ...fp.pos.map((q) => ({
        id: q.id,
        x: G.padX + q.c * U,
        y: G.fieldTop + q.r * U,
        s: q.n * U - 1,
        cellbase: false,
      })),
    ],
    ghostChips,
    chipOverflow: slots.overflow,
    hidden: fp.hidden + next.omittedTxs,
  };
}

/**
 * Where the reward link of `card`'s cellbase ends: the card of the block it pays when that block
 * is on screen, 'off' when it is older than every card on screen (past the right edge), null when
 * the cellbase pays no block yet. A paid block that should be on screen but is not, or that is on
 * screen under another number or hash, contradicts the snapshot and throws.
 */
export function rewardTarget(
  cards: readonly SceneCard[],
  card: SceneCard
): SceneCard | 'off' | null {
  const b = card.block;
  const pays = b.cellbase.pays;
  if (!pays) return null;
  const at = `pipeline: cellbase of #${b.number} pays`;
  if (pays.number >= b.number) {
    throw new Error(`${at} #${pays.number}, which is not an earlier block`);
  }
  const byHash = cards.find((k) => k.id === pays.hash);
  const byNumber = cards.find((k) => k.block.number === pays.number);
  if (byHash && byHash !== byNumber) {
    throw new Error(
      `${at} ${pays.hash} as #${pays.number}, but that block is #${byHash.block.number}`
    );
  }
  if (byNumber && byNumber !== byHash) {
    throw new Error(
      `${at} ${pays.hash} as #${pays.number}, but #${pays.number} on screen is ${byNumber.id}`
    );
  }
  if (byNumber) return byNumber;
  const oldest = Math.min(...cards.map((k) => k.block.number));
  if (pays.number >= oldest) {
    throw new Error(
      `${at} #${pays.number}, which should be on screen (down to #${oldest}) but is not`
    );
  }
  return 'off';
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
          (a.proposedIn as PipelineProposedIn).block - (b.proposedIn as PipelineProposedIn).block ||
          a.arrivedAt - b.arrivedAt
      );
    for (const t of pending) {
      if (t.proposedIn !== null) throw new Error(`pipeline: pending tx ${t.hash} has a proposedIn`);
    }
    // Two pool wells before NEXT: pending, then proposed (oldest proposal first, next to NEXT).
    const proposedGrid = poolGrid(
      proposed,
      p.proposedTotal,
      G.proposedX + G.padX,
      G.cardCols,
      (t) => {
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
      }
    );
    pool = {
      proposed: proposedGrid,
      pending: poolGrid(pending, p.pendingTotal, G.mempoolX + G.padX, G.cardCols, (t) =>
        txLook({ stage: 'pending', cycles: t.cycles, tooEarly: false })
      ),
    };
  }

  const geoms = s.blocks.map(cardGeom);
  const slots = chainLayout(geoms.length, viewW, phase === 'landing');
  const cards = slots.map((slot) => sceneCard(geoms[slot.index], slot.x, slot.slot));
  // Every reward link a hover can draw ends where the snapshot says: checked here, once.
  for (const c of cards) rewardTarget(cards, c);
  const next = phase === 'settled' && s.next ? sceneNext(s, s.next) : null;
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
  if (scene.next) {
    for (const g of scene.next.ghosts) {
      if (g.id) squares.push({ id: g.id, X: scene.next.x + g.x, Y: g.y, s: g.s, ghost: true });
    }
  }
  for (const c of scene.cards) {
    for (const q of c.squares) squares.push({ id: q.id, X: c.x + q.x, Y: q.y, s: q.s });
    for (const k of c.chips) chips.push({ card: c.id, key: k.key, X: c.x + k.x, Y: k.y });
  }
  return { squares, chips };
}
