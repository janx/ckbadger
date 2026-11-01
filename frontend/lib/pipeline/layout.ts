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
 *
 * Below `G.minViewW` (two whole blocks after NEXT) the same lifetime stands on end: the column
 * orientation (`COL`, docs/superpowers/plans/2026-11-01-narrow-widths/SPEC.md) reads it top to bottom
 * as rows that span the canvas, with the same squares, dots, materials and window rules. The
 * measured width picks the orientation; nothing is ever scaled.
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
 * The narrowest one-row canvas: the chain start (381), two whole cards one pitch apart (109 + 89)
 * and the right margin (10) = 589 px, so a row always shows the tip and the block before it.
 */
const MIN_VIEW_W = CHAIN_X0 + PITCH + CARD_W + CHAIN_BASE.marginR;

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
  minViewW: MIN_VIEW_W,
  /** The fewest chain blocks ever shown in a row: the ones that fit the narrowest canvas (2). */
  minBlocks: wholeCards(MIN_VIEW_W),
} as const;

/** An uncle travels inside the block that includes it: its header and proposals, never its txs. */
export const UNC = { top: 4, head: 11, gap: 3, padB: 3, chipRows: 1 } as const;

/** One tx lifetime left to right (`G`), or stood on end, top to bottom (`COL`). */
export type Orientation = 'row' | 'column';

/**
 * The column orientation: mempool, proposed, NEXT, the tip and three older blocks, each one row
 * spanning the canvas from `x0` to `viewW − x0`. x values are absolute canvas coordinates; a row is
 * drawn as a card translated to (`x0`, its top), so its contents use `x − x0`.
 *
 * Row anatomy: the name or number at `nameX`; a well's pool total ending at `countX`; proposal dots
 * from `chipX` (5 per row, two rows, then "+N"); the tx line from `lineX` — the cellbase diamond
 * first, then squares at 10 px per cell, each centred vertically — ending at the row's right
 * padding, with any uncle boxes and the line's "+N" at its end. A row is two cells and 5 px
 * padding tall (30 px) and grows only for a 3×3 or 4×4 square. The chain line runs at `chainX`,
 * visible only in the gaps between the rows.
 */
export const COL = {
  x0: 6,
  /** Above the mempool row. */
  top: 8,
  /** Below the last row: room for the dotted tail, which stops `tailGap` short of the edge. */
  bottom: 20,
  tailGap: 4,
  pad: 5,
  minCells: 2,
  wellGap: 12,
  blockGap: 14,
  /** NEXT stands alone: both of its gaps are wider, as the frontier gaps of a row are. */
  frontierGap: 24,
  chainX: 28,
  /** The window mark runs this far either side of the chain line. */
  markHalf: 6,
  nameX: 12,
  countX: 100,
  /** Left edge of the first proposal dot (its centre is at 60). */
  chipX: 57,
  chipsPerRow: 5,
  chipRows: 2,
  lineX: 108,
  padR: 6,
  /** Between the tx line and what ends it: its "+N", an uncle box, the next uncle box. */
  endGap: 4,
  /** The tip and three older blocks. */
  blocks: 4,
  /** An uncle box at the end of a block's tx line: "uncle", then up to four of its dots. */
  uncle: { h: 18, pad: 4, labelW: 30, chips: 4 },
  /**
   * The narrowest column (a 320 px viewport): the line still holds the cellbase, a 4×4 square, a
   * "+N" and an uncle box. A block with two uncle boxes fits from 310 px.
   */
  minViewW: 288,
} as const;

/** Height of a column row whose tallest square is `side` cells: 30 px for 1×1 and 2×2. */
export function columnRowH(side: number): number {
  return Math.max(COL.minCells, side) * U + 2 * COL.pad;
}

/** Width of a column uncle box: the word, then room for its dots when it carries proposals. */
export function columnUncleW(proposals: number): number {
  const u = COL.uncle;
  return u.pad + u.labelW + u.pad + (proposals ? u.chips * G.chipP - 2 + u.pad : 0);
}

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

export interface LinePos {
  id: string;
  /** First cell on the line. */
  c: number;
  n: number;
}

/**
 * The column's tx line: squares side by side in reading order, each `n` cells wide, first fit (a
 * square that no longer fits is counted as hidden; a later, smaller one may still fit). With
 * `reserveFirst` the cellbase takes cell 0.
 */
export function packLine(
  items: readonly PackItem[],
  cells: number,
  reserveFirst: boolean
): { pos: LinePos[]; hidden: number } {
  const pos: LinePos[] = [];
  let c = reserveFirst ? 1 : 0;
  let hidden = 0;
  for (const t of items) {
    if (c + t.n <= cells) {
      pos.push({ id: t.id, c, n: t.n });
      c += t.n;
    } else hidden++;
  }
  return { pos, hidden };
}

/**
 * A column tx line in `room` px from its start: as many squares as fit, and when any are left over
 * (or `extra` were never listed) whole cells at the end give way to the "+N" that counts them, drawn
 * end-anchored at the end of the room. `hidden` includes `extra`.
 */
export function fitLine(
  items: readonly PackItem[],
  room: number,
  reserveFirst: boolean,
  extra: number,
  what: string
): { pos: LinePos[]; hidden: number } {
  const all = Math.floor((room + 1) / U);
  for (let cells = all; cells >= (reserveFirst ? 1 : 0); cells--) {
    const p = packLine(items, cells, reserveFirst);
    const hidden = p.hidden + extra;
    if (!hidden) return { pos: p.pos, hidden };
    if (cells * U - 1 + COL.endGap + labelWidth(`+${hidden}`) <= room) {
      return { pos: p.pos, hidden };
    }
  }
  throw new Error(`pipeline: ${what} has no room for its tx line in ${room} px`);
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
 * card cut by its edge), at most `maxBlocks`. A row is never narrower than `minViewW`, so at least
 * `minBlocks` fit. A landing block (just mined, still being
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
  const { hc, q, xn, xf, ticks, tailOpen } = windowEdges(slots, H, w, G.gap);
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
  return {
    ticks,
    label,
    hit:
      xn === null
        ? null
        : { x0: hc ? hc.x : xn, x1: q ? q.x + q.w : (xf ?? viewW), xn, xf: xf ?? viewW },
    tailOpen,
  };
}

/**
 * The ends of the window of H along the chain's direction (slot `x`/`w` are positions along it):
 * a tick in the middle of the gap at each end that is on screen, `gap` being the gap to assume
 * past a card whose neighbour is not drawn.
 */
function windowEdges(slots: readonly RulerSlot[], H: number, w: WindowParams, gap: number) {
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
    xn = p ? (p.x + p.w + near.x) / 2 : near.x - gap / 2;
    ticks.push(xn);
  }
  if (far) {
    xf = q ? (far.x + far.w + q.x) / 2 : far.x + far.w + gap / 2;
    ticks.push(xf);
  }
  const mined = slots.filter((s) => s.mined);
  const last = mined.length ? mined.reduce((a, b) => (b.x > a.x ? b : a)) : null;
  return { hc, q, xn, xf, ticks, tailOpen: !!last && inWindow(last.number, H, w) && !far };
}

/**
 * The window of H in a column, along the rows (slot `x`/`w` are a row's top and height): the same
 * ticks, in the middle of the gaps, and the same gold links and tail. There is no lane beside the
 * rows for a dimension line or its label: the gold chain line between the ticks is the dimension
 * line, and the words are in the hover's tooltip (window, NEXT and block tips all name the range).
 */
export function columnWindowRuler(
  slots: readonly RulerSlot[],
  H: number,
  w: WindowParams,
  viewH: number
): RulerGeom {
  const { hc, q, xn, xf, ticks, tailOpen } = windowEdges(slots, H, w, COL.blockGap);
  return {
    ticks,
    label: null,
    hit:
      xn === null
        ? null
        : { x0: hc ? hc.x : xn, x1: q ? q.x + q.w : (xf ?? viewH), xn, xf: xf ?? viewH },
    tailOpen,
  };
}

/** The column window at rest: a short gold mark across the chain line at each tick. */
export function columnWindowMarksPath(r: RulerGeom): string {
  const f = (v: number) => v.toFixed(1);
  const a = f(COL.chainX - COL.markHalf);
  const b = f(COL.chainX + COL.markHalf);
  return r.ticks.map((y) => `M${a} ${f(y)}H${b}`).join('');
}

/** The hovered column window: each tick runs across the full width of the gap it sits in. */
export function columnRulerPath(r: RulerGeom, viewW: number): string {
  const f = (v: number) => v.toFixed(1);
  return r.ticks.map((y) => `M${f(COL.x0)} ${f(y)}H${f(viewW - COL.x0)}`).join('');
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

/**
 * The reward link of a hovered cellbase in a column. It steps left out of the cellbase into a lane
 * just before the tx line (between the dots and the line) and runs down it, past the rows below;
 * with the paid cellbase on screen at `b` it turns back into it, else it ends where the chain's own
 * tail starts and goes on in the same open dotted tail. `tail` is the column tail (`x0`/`x1` along
 * the rows).
 */
export function columnRewardLink(a: Point, b: Point | null, tail: ChainTail): RewardLink {
  const lane = COL.lineX - 6;
  const r = 5;
  const f = (v: number) => v.toFixed(1);
  const down = `M${f(a.X)} ${f(a.Y)}H${f(lane + r)}Q${f(lane)} ${f(a.Y)} ${f(lane)} ${f(a.Y + r)}`;
  if (b) {
    if (b.Y - a.Y < 2 * r) {
      throw new Error(`pipeline: a column reward link runs down, from y ${a.Y} to y ${b.Y}`);
    }
    return {
      d: `${down}V${f(b.Y - r)}Q${f(lane)} ${f(b.Y)} ${f(lane + r)} ${f(b.Y)}H${f(b.X)}`,
      tail: '',
    };
  }
  if (tail.x0 < a.Y + r) {
    throw new Error(
      `pipeline: a reward link from y ${a.Y} cannot end in a tail at ${tail.x0}, before its bend into the lane`
    );
  }
  return {
    d: `${down}V${f(tail.x0)}`,
    tail: `M${f(lane)} ${f(tail.x0)}V${f(tail.x1)}`,
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
  const y = G.chainY + 0.5;
  const f = (v: number) => v.toFixed(1);
  const seg = ([x0, x1]: Span) => `M${f(x0)} ${y}L${f(x1)} ${y}`;
  const s = chainSpans(cards, next, H, w, 2);
  const tail = chainTail(cards, viewW);
  return {
    solid: s.solid.map(seg).join(''),
    pend: s.pend.map(seg).join(''),
    win: s.win.map(seg).join(''),
    tail: tail ? `M${f(tail.x0)} ${y}L${tail.x1} ${y}` : '',
  };
}

/** A stretch of the chain line along its direction: from, to. */
type Span = [number, number];

/**
 * The chain's links along its direction (slot `x`/`w` are positions along it), `inset` px clear of
 * the cards: NEXT ⇢ the first card dashed, then a link wherever the newer card names the older one
 * as its parent — gold between two blocks inside the window of H, dashed from a block still
 * landing, solid otherwise.
 */
function chainSpans(
  cards: readonly ChainSlotState[],
  next: { x: number; w: number } | null,
  H: number,
  w: WindowParams,
  inset: number
): { solid: Span[]; pend: Span[]; win: Span[] } {
  const list = cards.slice().sort((a, b) => a.x - b.x);
  const solid: Span[] = [];
  const pend: Span[] = [];
  const win: Span[] = [];
  if (next && list.length) pend.push([next.x + next.w + inset, list[0].x - inset]);
  for (let i = 0; i + 1 < list.length; i++) {
    const a = list[i];
    const b = list[i + 1];
    if (a.parentHash !== b.hash) continue;
    const x0 = a.x + a.w + inset;
    const x1 = b.x - inset;
    if (x1 - x0 < 6) continue;
    if (a.mined && b.mined && inWindow(a.number, H, w) && inWindow(b.number, H, w)) {
      win.push([x0, x1]);
    } else if (a.solid < 0.5) pend.push([x0, x1]);
    else solid.push([x0, x1]);
  }
  return { solid, pend, win };
}

/**
 * The chain of a column at `COL.chainX`, top to bottom (slot `x`/`w` are a row's top and height):
 * the links fill the gaps between the rows exactly, so the rows, drawn over the line, leave it
 * visible only between them. The tail runs from the last row to `tailGap` above the bottom edge.
 */
export function columnChainPaths(
  cards: readonly ChainSlotState[],
  next: { x: number; w: number } | null,
  H: number,
  w: WindowParams,
  viewH: number
): ChainPaths {
  const f = (v: number) => v.toFixed(1);
  const seg = ([y0, y1]: Span) => `M${COL.chainX} ${f(y0)}V${f(y1)}`;
  const s = chainSpans(cards, next, H, w, 0);
  const tail = columnChainTail(cards, viewH);
  return {
    solid: s.solid.map(seg).join(''),
    pend: s.pend.map(seg).join(''),
    win: s.win.map(seg).join(''),
    tail: tail ? seg([tail.x0, tail.x1]) : '',
  };
}

/** Where a column's chain runs on below its last row: from that row to `tailGap` above the edge. */
export function columnChainTail(
  cards: readonly { x: number; w: number }[],
  viewH: number
): ChainTail | null {
  if (!cards.length) return null;
  const last = cards.reduce((a, b) => (b.x > a.x ? b : a));
  return { x0: last.x + last.w, x1: viewH - COL.tailGap };
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
  /** The uncle box, local to the card. */
  x: number;
  y: number;
  w: number;
  h: number;
  label: string;
  overflow: number;
}

export interface SceneCard {
  /** Block hash. */
  id: string;
  block: PipelineBlock;
  slot: 'landing' | 'chain';
  /** The card's origin: its contents are local to it. y is 0 in a row. */
  x: number;
  y: number;
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
  /** The card's origin: its contents are local to it. y is 0 in a row. */
  x: number;
  y: number;
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
  /** Txs of the stage not drawn (listed or not): a column well's line counts them as "+N". */
  hidden: number;
}

/** A stretch of the canvas from top `y`, `h` tall: one row of a column. */
export interface Band {
  y: number;
  h: number;
}

export interface Scene {
  orientation: Orientation;
  viewW: number;
  /** Canvas height: one 132 px row, or the stacked rows of a column. */
  H: number;
  phase: ScenePhase;
  snapshot: PipelineSnapshot;
  /** The window defaults to the block after this one (NEXT, or the landing block). */
  windowTip: number;
  pool: { pending: ScenePoolGrid; proposed: ScenePoolGrid } | null;
  next: SceneNext | null;
  cards: SceneCard[];
  /**
   * A column's wells and NEXT's row, which hold their place whether or not they have content (NEXT's
   * row holds the landing block while it lands); null in a row, where they sit at fixed x.
   */
  bands: { pending: Band; proposed: Band; next: Band } | null;
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

/** A pool stage ready to place: its listed txs in drawing order, each one's look, and its total. */
interface PoolStage {
  txs: PipelinePoolTx[];
  total: number;
  look: Map<string, TxLook>;
}

function poolStage(
  txs: PipelinePoolTx[],
  total: number,
  lookOf: (t: PipelinePoolTx) => TxLook
): PoolStage {
  if (total < txs.length) {
    throw new Error(`pipeline: pool total ${total} is below the ${txs.length} txs listed`);
  }
  return { txs, total, look: new Map(txs.map((t) => [t.hash, lookOf(t)])) };
}

const lookIn = (stage: PoolStage, hash: string): TxLook => stage.look.get(hash) as TxLook;

function poolGrid(stage: PoolStage, x0: number, cols: number): ScenePoolGrid {
  const { txs, total } = stage;
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
        lookIn(stage, q.id),
        x0 + q.c * U,
        G.poolTop + q.r * U,
        q.n * U - 1,
        'pool'
      )
    ),
    total,
    hidden: total - p.pos.length,
  };
}

/** What a block card shows, checked against the snapshot contract, before it is placed. */
interface BlockFacts {
  block: PipelineBlock;
  own: PipelineProposal[];
  byUncle: PipelineProposal[][];
  /** Its listed txs in drawing order: big squares first, then by fee rate. */
  order: PipelineTx[];
}

interface CardGeom extends BlockFacts {
  uExtra: number;
  pack: PackResult;
}

function blockFacts(b: PipelineBlock): BlockFacts {
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
  return { block: b, own, byUncle, order: b.txs.slice().sort(bySize) };
}

function cardGeom(f: BlockFacts): CardGeom {
  const uExtra = unclesH(f.byUncle.map((u) => u.length));
  const pack = pack2D(
    f.order.map((t) => ({ id: t.hash, n: squareSide(t.size) })),
    G.cardCols,
    G.fieldRows - Math.ceil(uExtra / U),
    true
  );
  return { ...f, uExtra, pack };
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
      x: G.padX,
      y: top,
      w: G.cardW - 2 * G.padX,
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
    y: 0,
    w: G.cardW,
    h: CARD_H,
    chips,
    chipOverflow: own.overflow,
    uncles,
    squares,
    hidden: g.pack.hidden + b.omittedTxs,
  };
}

function checkNext(next: PipelineNext): void {
  if (!Number.isInteger(next.omittedTxs) || next.omittedTxs < 0) {
    throw new Error(`pipeline: next.omittedTxs ${next.omittedTxs} is not a count`);
  }
}

function sceneNext(s: PipelineSnapshot, next: PipelineNext): SceneNext {
  checkNext(next);
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
    y: 0,
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

  let stages: PoolStages | null = null;
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
    // Two pool stages before NEXT: pending, then proposed (oldest proposal first, next to NEXT).
    stages = {
      proposed: poolStage(proposed, p.proposedTotal, (t) => {
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
      pending: poolStage(pending, p.pendingTotal, (t) =>
        txLook({ stage: 'pending', cycles: t.cycles, tooEarly: false })
      ),
    };
  }

  const facts = s.blocks.map(blockFacts);
  // The measured width picks the orientation: a row while two whole blocks fit after NEXT, else
  // the column.
  const laid =
    viewW >= G.minViewW
      ? rowScene(s, viewW, phase, stages, facts)
      : columnScene(s, viewW, phase, stages, facts);
  // Every reward link a hover can draw ends where the snapshot says: checked here, once.
  for (const c of laid.cards) rewardTarget(laid.cards, c);
  return {
    ...laid,
    viewW,
    phase,
    snapshot: s,
    windowTip: phase === 'landing' ? s.tip.number - 1 : s.tip.number,
  };
}

interface PoolStages {
  pending: PoolStage;
  proposed: PoolStage;
}

/** The geometry part of a scene: what the orientation decides. */
type Laid = Pick<Scene, 'orientation' | 'H' | 'pool' | 'next' | 'cards' | 'bands'>;

function rowScene(
  s: PipelineSnapshot,
  viewW: number,
  phase: ScenePhase,
  stages: PoolStages | null,
  facts: readonly BlockFacts[]
): Laid {
  // Two pool wells before NEXT: pending, then proposed.
  const pool = stages
    ? {
        proposed: poolGrid(stages.proposed, G.proposedX + G.padX, G.cardCols),
        pending: poolGrid(stages.pending, G.mempoolX + G.padX, G.cardCols),
      }
    : null;
  const geoms = facts.map(cardGeom);
  const slots = chainLayout(geoms.length, viewW, phase === 'landing');
  const cards = slots.map((slot) => sceneCard(geoms[slot.index], slot.x, slot.slot));
  const next = phase === 'settled' && s.next ? sceneNext(s, s.next) : null;
  return { orientation: 'row', H: G.H, pool, next, cards, bands: null };
}

/** Proposal dots of a column row: 5 per row, at most two rows centred on the row, then "+N". */
function columnDots(total: number, h: number) {
  const per = COL.chipsPerRow;
  const slots = chipSlots(total, per * COL.chipRows);
  const rows = slots.overflow > 0 ? COL.chipRows : Math.max(1, Math.ceil(slots.shown / per));
  const top = h / 2 - (G.chip + G.chipP * (rows - 1)) / 2;
  return {
    ...slots,
    pos: Array.from({ length: slots.shown }, (_, i) => ({
      x: COL.chipX - COL.x0 + (i % per) * G.chipP,
      y: top + Math.floor(i / per) * G.chipP,
    })),
  };
}

/** Where a column row's proposal "+N" is drawn (end-anchored, local): after the second dot row. */
export function columnChipCountAt(h: number): { x: number; y: number } {
  const top = h / 2 - (G.chip + G.chipP) / 2;
  return {
    x: COL.chipX - COL.x0 + COL.chipsPerRow * G.chipP - 2,
    y: top + G.chipP + G.chip / 2 + 3.5,
  };
}

/**
 * Where a column row's tx "+N" is drawn (end-anchored, local): at the end of its tx line, before
 * its uncle boxes.
 */
export function columnCountAt(card: { w: number; h: number; uncles: readonly { x: number }[] }): {
  x: number;
  y: number;
} {
  return {
    x: card.uncles.length ? card.uncles[0].x - COL.endGap : card.w - COL.padR,
    y: card.h / 2 + 3.5,
  };
}

/** Where a column well's "+N" is drawn (end-anchored, absolute). */
export function columnWellCountAt(viewW: number, band: Band): { x: number; y: number } {
  return { x: viewW - COL.x0 - COL.padR, y: band.y + band.h / 2 + 3.5 };
}

const squareTop = (h: number, s: number) => Math.floor((h - s) / 2);
const sideOf = (pos: readonly LinePos[]) => Math.max(1, ...pos.map((p) => p.n));

/**
 * A block as a column row: its own proposal dots, then its tx line — the cellbase and the txs that
 * fit — and at the end of the line its uncle boxes ("uncle" and the uncle's dots), which take their
 * width out of the line as an uncle mini-card takes rows out of a card's field.
 */
function columnCard(f: BlockFacts, slot: SceneCard['slot'], w: number): SceneCard {
  const b = f.block;
  const u = COL.uncle;
  const lineX = COL.lineX - COL.x0;
  const widths = f.byUncle.map((list) => columnUncleW(list.length));
  const txEnd = w - COL.padR - widths.reduce((sum, bw) => sum + bw + COL.endGap, 0);
  const fit = fitLine(
    f.order.map((t) => ({ id: t.hash, n: squareSide(t.size) })),
    txEnd - lineX,
    true,
    b.omittedTxs,
    `block #${b.number}`
  );
  const h = columnRowH(sideOf(fit.pos));
  const mid = h / 2;
  const txByHash = new Map(b.txs.map((t) => [t.hash, t]));
  const squares: SceneSquare[] = [
    square(
      b.cellbase.hash,
      'cellbase',
      txLook({ stage: 'cellbase', cycles: null, tooEarly: false }),
      lineX,
      squareTop(h, U - 1),
      U - 1,
      b.hash
    ),
    ...fit.pos.map((q) => {
      const t = txByHash.get(q.id) as PipelineTx;
      return square(
        q.id,
        'tx',
        txLook({ stage: 'committed', cycles: t.cycles, tooEarly: false }),
        lineX + q.c * U,
        squareTop(h, q.n * U - 1),
        q.n * U - 1,
        b.hash
      );
    }),
  ];
  const own = columnDots(f.own.length, h);
  const chips: SceneChip[] = f.own.slice(0, own.shown).map((p, i) => ({
    key: `${p.shortId}|`,
    proposal: p,
    ...own.pos[i],
  }));
  const uncles: SceneUncle[] = [];
  let x = txEnd + COL.endGap;
  f.byUncle.forEach((list, ui) => {
    const slots = chipSlots(list.length, u.chips);
    uncles.push({
      index: ui,
      ref: b.uncles[ui],
      x,
      y: mid - u.h / 2,
      w: widths[ui],
      h: u.h,
      label: 'uncle',
      overflow: slots.overflow,
    });
    list.slice(0, slots.shown).forEach((p, i) => {
      chips.push({
        key: `${p.shortId}|${ui}`,
        proposal: p,
        x: x + u.pad + u.labelW + u.pad + i * G.chipP,
        y: mid - G.chip / 2,
      });
    });
    x += widths[ui] + COL.endGap;
  });
  return {
    id: b.hash,
    block: b,
    slot,
    x: COL.x0,
    y: 0,
    w,
    h,
    chips,
    chipOverflow: own.overflow,
    uncles,
    squares,
    hidden: fit.hidden,
  };
}

/** NEXT as a column row: hollow dots for the proposals it will carry, then its predicted line. */
function columnNext(s: PipelineSnapshot, next: PipelineNext, w: number): SceneNext {
  checkNext(next);
  const lineX = COL.lineX - COL.x0;
  // Drawn in the backend's order (by size, descending), like a block's txs.
  const fit = fitLine(
    next.txs.map((t) => ({ id: t.hash, n: squareSide(t.size) })),
    w - COL.padR - lineX,
    true,
    next.omittedTxs,
    `next block #${s.tip.number + 1}`
  );
  const h = columnRowH(sideOf(fit.pos));
  const dots = columnDots(next.proposals, h);
  return {
    number: s.tip.number + 1,
    x: COL.x0,
    y: 0,
    w,
    h,
    next,
    ghosts: [
      { id: null, x: lineX, y: squareTop(h, U - 1), s: U - 1, cellbase: true },
      ...fit.pos.map((q) => ({
        id: q.id,
        x: lineX + q.c * U,
        y: squareTop(h, q.n * U - 1),
        s: q.n * U - 1,
        cellbase: false,
      })),
    ],
    ghostChips: dots.pos,
    chipOverflow: dots.overflow,
    hidden: fit.hidden,
  };
}

/**
 * The pipeline stood on end: mempool, proposed, NEXT, the tip and three older blocks, top to
 * bottom, each a row as wide as the canvas. Rows are 30 px unless a 3×3 or 4×4 square makes one
 * taller; the gaps are 12 between the wells, 14 between blocks and 24 on both sides of NEXT. The
 * canvas is as tall as its rows plus the room for the tail.
 */
function columnScene(
  s: PipelineSnapshot,
  viewW: number,
  phase: ScenePhase,
  stages: PoolStages | null,
  facts: readonly BlockFacts[]
): Laid {
  if (viewW < COL.minViewW) {
    throw new Error(`pipeline: a ${viewW} px view is narrower than the ${COL.minViewW} px column`);
  }
  const w = viewW - 2 * COL.x0;
  const landing = phase === 'landing';
  const wellRoom = viewW - COL.x0 - COL.padR - COL.lineX;
  const wellLine = (stage: PoolStage, what: string) => {
    const fit = fitLine(
      stage.txs.map((t) => ({ id: t.hash, n: squareSide(t.size) })),
      wellRoom,
      false,
      stage.total - stage.txs.length,
      what
    );
    return { ...fit, h: columnRowH(sideOf(fit.pos)) };
  };
  const lines = stages
    ? {
        pending: wellLine(stages.pending, 'the mempool well'),
        proposed: wellLine(stages.proposed, 'the proposed well'),
      }
    : null;
  const next = phase === 'settled' && s.next ? columnNext(s, s.next, w) : null;
  const i0 = landing ? 1 : 0;
  const rows = [
    ...(landing ? [columnCard(facts[0], 'landing', w)] : []),
    ...facts.slice(i0, i0 + COL.blocks).map((f) => columnCard(f, 'chain', w)),
  ];

  const base = columnRowH(1);
  let y: number = COL.top;
  const band = (h: number, gap: number): Band => {
    const out = { y, h };
    y += h + gap;
    return out;
  };
  const pending = band(lines ? lines.pending.h : base, COL.wellGap);
  const proposed = band(lines ? lines.proposed.h : base, COL.frontierGap);
  const nextBand = band(landing ? rows[0].h : next ? next.h : base, COL.frontierGap);
  const cards = rows.map((c, i) => {
    if (c.slot === 'landing') return { ...c, y: nextBand.y };
    const at = { ...c, y };
    y += c.h + (i + 1 < rows.length ? COL.blockGap : 0);
    return at;
  });
  const last = cards[cards.length - 1];

  const grid = (stage: PoolStage, line: NonNullable<typeof lines>['pending'], at: Band) => ({
    squares: line.pos.map((q) =>
      square(
        q.id,
        'tx',
        lookIn(stage, q.id),
        COL.lineX + q.c * U,
        at.y + squareTop(at.h, q.n * U - 1),
        q.n * U - 1,
        'pool'
      )
    ),
    total: stage.total,
    hidden: line.hidden,
  });
  return {
    orientation: 'column',
    H: last.y + last.h + COL.bottom,
    pool:
      stages && lines
        ? {
            pending: grid(stages.pending, lines.pending, pending),
            proposed: grid(stages.proposed, lines.proposed, proposed),
          }
        : null,
    next: next ? { ...next, y: nextBand.y } : null,
    cards,
    bands: { pending, proposed, next: nextBand },
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
  const next = scene.next;
  if (next) {
    for (const g of next.ghosts) {
      if (g.id) squares.push({ id: g.id, X: next.x + g.x, Y: next.y + g.y, s: g.s, ghost: true });
    }
  }
  for (const c of scene.cards) {
    for (const q of c.squares) squares.push({ id: q.id, X: c.x + q.x, Y: c.y + q.y, s: q.s });
    for (const k of c.chips) chips.push({ card: c.id, key: k.key, X: c.x + k.x, Y: c.y + k.y });
  }
  return { squares, chips };
}

/**
 * The orientation-dependent drawing of a scene's chain, window and reward link, for the static
 * render and the motion layer alike. Slots are along the chain's direction: `along` maps a card
 * (or a moving card's current values) to its position and length on it.
 */
export interface SceneAxis {
  along(c: { x: number; y: number; w: number; h: number }): { x: number; w: number };
  chainPaths(
    cards: readonly ChainSlotState[],
    next: { x: number; w: number } | null,
    H: number,
    w: WindowParams
  ): ChainPaths;
  ruler(slots: readonly RulerSlot[], H: number, w: WindowParams, text: string): RulerGeom;
  /** The window at rest: marks across the chain line. */
  marks(r: RulerGeom): string;
  /** The hovered window: its ticks (and, in a row, the dimension line). */
  ticks(r: RulerGeom): string;
  tail(cards: readonly { x: number; w: number }[]): ChainTail | null;
  rewardLink(a: Point, b: Point | null, tail: ChainTail): RewardLink;
}

export function axisOf(scene: Pick<Scene, 'orientation' | 'viewW' | 'H'>): SceneAxis {
  const { viewW, H: viewH } = scene;
  if (scene.orientation === 'row') {
    return {
      along: (c) => ({ x: c.x, w: c.w }),
      chainPaths: (cards, next, H, w) => chainPaths(cards, next, H, w, viewW),
      ruler: (slots, H, w, text) => windowRuler(slots, H, w, viewW, text),
      marks: windowMarksPath,
      ticks: rulerPath,
      tail: (cards) => chainTail(cards, viewW),
      rewardLink,
    };
  }
  return {
    along: (c) => ({ x: c.y, w: c.h }),
    chainPaths: (cards, next, H, w) => columnChainPaths(cards, next, H, w, viewH),
    ruler: (slots, H, w) => columnWindowRuler(slots, H, w, viewH),
    marks: columnWindowMarksPath,
    ticks: (r) => columnRulerPath(r, viewW),
    tail: (cards) => columnChainTail(cards, viewH),
    rewardLink: columnRewardLink,
  };
}
