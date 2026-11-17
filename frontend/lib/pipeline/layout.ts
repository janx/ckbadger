/**
 * Geometry of the transaction pipeline. It started as a port of the approved prototype
 * (docs/superpowers/specs/2026-10-23-transaction-pipeline-prototype.html) and now deliberately
 * differs from it: the canvas is one 146 px row holding one tx lifetime, left to right (mempool well
 * ⇢ proposed well ⇢ NEXT ⇢ the tip → older blocks), and every block is one fixed solid plate
 * (99 × 122) instead of growing with its content. All functions are pure.
 *
 * Direction F (docs/superpowers/plans/2026-11-17-pipeline-finish-plan.md): one unit, a 15 px cell
 * on a 17 px pitch. A tx is an n×n square of cells (area ∝ bytes, `side(n)` px), set in a cut of
 * the gutter's width; a proposal is a 7 px dot on a 9 px pitch. A plate is cut only where a
 * transaction is: the scene carries one cut per drawn square and nothing for an empty cell. A link
 * from a block to its parent is two bands of filled ticks across the gap between their plates.
 *
 * Below `G.minViewW` (two whole blocks after NEXT) the same lifetime stands on end: the column
 * orientation (`COL`) reads it top to bottom as rows that span the canvas, with the same unit,
 * squares, dots, materials and window rules. The measured width picks the orientation; nothing is
 * ever scaled.
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

/** A cell: the side of a 1×1 transaction. */
export const CELL = 15;
/** The gutter between two cells, and the width of the cut round every mark set in a plate. */
export const GUT = 2;
/** The pipeline's one unit, in both orientations: a cell and its gutter. */
export const U = CELL + GUT;

/**
 * The canvas's small text, in px: every word the layout measures ("uncle", every "+N", the
 * window's label) is drawn in the `text-meta` step of the type scale (components/pipeline/
 * svg-text.ts), 11 px of JetBrains Mono, whose every glyph advances 0.6 em. `labelWidth` reads it.
 */
export const SMALL_TEXT_PX = 11;

/**
 * `n` cells side by side, in px: n pitches less the one gutter after the last. The side of an n×n
 * square (15, 32, 49, 66) and the width of an n-cell field; the gutter is said here only.
 */
export function side(n: number): number {
  if (!Number.isInteger(n) || n < 1) {
    throw new Error(`pipeline: a span is a whole number of cells (at least one), got ${n}`);
  }
  return n * U - GUT;
}

/**
 * The bands of ticks that link a block to its parent, and a pool stage to the next: filled
 * rectangles, starting `inset` px inside the gap they cross. A link's band is 2 px ticks on a
 * 4 px pitch, 10 px deep (4 ticks in an 18 px gap); a flow's is 1 px ticks on a 3 px pitch, 8 deep.
 */
export const BAND = {
  inset: 2,
  link: { tick: 2, pitch: 4, depth: 10 },
  flow: { tick: 1, pitch: 3, depth: 8 },
} as const;

interface Ticks {
  tick: number;
  pitch: number;
  depth: number;
}

/**
 * A band of whole ticks along [from, to] in the chain's direction (`x` in a row, `y` in a column),
 * its side `across` px from the canvas edge: one filled rectangle per tick, from `from` on, as path
 * data. A stretch shorter than one tick has none.
 */
function tickBand(along: 'x' | 'y', from: number, to: number, across: number, t: Ticks): string {
  const f = (v: number) => v.toFixed(1);
  const n = to - from >= t.tick ? Math.floor((to - from - t.tick) / t.pitch) + 1 : 0;
  let d = '';
  for (let k = 0; k < n; k++) {
    const a = from + k * t.pitch;
    d +=
      along === 'x'
        ? `M${f(a)} ${f(across)}h${t.tick}v${t.depth}h-${t.tick}Z`
        : `M${f(across)} ${f(a)}h${t.depth}v${t.tick}h-${t.depth}Z`;
  }
  return d;
}

/**
 * The lane above the plates, empty until hovered: the proposal window's bracket and its label and
 * the reward link run in it.
 */
const LANE = 18;
/** Columns and rows of a plate's tx field and of a pool well (5 × 4 = 20 cells). */
const COLS = 5;
const ROWS = 4;
/** Inside a plate, on its left and right and below its field. */
const PAD = 8;

/**
 * Down a plate, from its top edge (the board's numbers, Plate.dc.html): the head line 8…28 (the
 * 14 px stone at 11, the number beside it), two rows of dots at 30 and 38, the field from 48 with
 * 8 px below it; a pool's well from 28, under its head line; a link's two bands at 32 and 80; a
 * pool's flow band at 14.
 */
const DOWN = {
  head: 8,
  headLine: 20,
  stone: 11,
  dots: 30,
  field: 48,
  well: 28,
  links: [32, 80],
  flow: 14,
} as const;

/** A proposal dot: `size` px on a `pitch` px pitch along its row; a plate's rows `rowPitch` apart. */
const DOT = { size: 7, pitch: 9, rowPitch: 8 } as const;

/** A plate: 99 × 122. */
const CARD_W = side(COLS) + 2 * PAD;
const PLATE_H = DOWN.field + side(ROWS) + PAD;

const CHAIN_BASE = {
  /** The lane, a plate, and 6 px below it. */
  H: LANE + PLATE_H + 6,
  /** The window's dimension line and the reward link: along the middle of the lane. */
  winY: LANE / 2,
  cardTop: LANE,
  /** The plate's head, from its top edge to the bottom of its head line. */
  headH: DOWN.head + DOWN.headLine,
  /** The middle of the head line: the number, a pool's name and total, NEXT's "+N". */
  headY: LANE + DOWN.head + DOWN.headLine / 2,
  /** The block's stone: `stone` px square at (`stoneX`, `stoneY`); its number from `numberX`. */
  stone: 14,
  stoneX: 8,
  stoneY: LANE + DOWN.stone,
  numberX: 27,
  padX: PAD,
  propTop: LANE + DOWN.dots,
  /** Two rows of proposal dots: `chip` px dots on a `chipP` px pitch, the rows `chipRowP` apart. */
  propRows: 2,
  chip: DOT.size,
  chipP: DOT.pitch,
  chipRowP: DOT.rowPitch,
  /** One below the bottom of the second dot row, whose "+N" stands on that bottom (`sepY − 1`). */
  sepY: LANE + DOWN.dots + DOT.rowPitch + DOT.size + 1,
  fieldTop: LANE + DOWN.field,
  /**
   * Every plate (and each pool well and NEXT) is this many columns wide whatever it holds; what
   * does not fit is counted as "+N" (dots at the end of their rows, txs on the head line).
   */
  cardCols: COLS,
  fieldRows: ROWS,
  padB: PAD,
  /** A pool's well: from under its head line to the plates' bottom, `wellH` px tall. */
  wellTop: LANE + DOWN.well,
  wellH: PLATE_H - DOWN.well,
  /** A well's marks start 7 px inside its 1 px border, on its left and its top. */
  poolInset: 1 + 7,
  poolTop: LANE + DOWN.well + 1 + 7,
  poolRows: ROWS,
  /** A pool's name and its total stand on the head line 1 px inside its well's edges. */
  poolHeadPad: 1,
  /** The theme's inset under a well's top edge: a shade this many px deep, fading to nothing. */
  wellShade: 3,
  /** The tops of a link's two bands, and of a pool's flow band. */
  linkY: [LANE + DOWN.links[0], LANE + DOWN.links[1]],
  flowY: LANE + DOWN.flow,
  x0: 0,
  gap: 18,
  /** The pool stands apart from NEXT: the one wider gap of the row, on the pool side only. */
  frontierGap: 30,
  /** Kept free between the last chain plate and the right edge of the view. */
  marginR: 10,
  maxBlocks: 24,
  /** The chain's tail past the last plate on screen: at most this long. */
  tail: 66,
} as const;

const PITCH = CARD_W + CHAIN_BASE.gap;

const NEXT_X = CHAIN_BASE.x0 + PITCH + CARD_W + CHAIN_BASE.frontierGap;
const CHAIN_X0 = NEXT_X + CARD_W + CHAIN_BASE.gap;

/**
 * The narrowest one-row canvas: the chain start (363), two whole plates one pitch apart (117 + 99)
 * and the right margin (10) = 589 px, so a row always shows the tip and the block before it.
 */
const MIN_VIEW_W = CHAIN_X0 + PITCH + CARD_W + CHAIN_BASE.marginR;

/**
 * How many whole chain plates fit a view `viewW` px wide: plate k ends at chainX0 + k·pitch +
 * cardW, which must stay `marginR` clear of the right edge.
 */
function wholeCards(viewW: number): number {
  return Math.floor((viewW - CHAIN_BASE.marginR - CHAIN_X0 + CHAIN_BASE.gap) / PITCH);
}

/**
 * One row, one tx lifetime, left to right: mempool (pending) ⇢ proposed ⇢ NEXT ⇢ the tip → older
 * blocks, plates and wells one size. NEXT is a plate's gap from the tip; the pool stands a wider
 * `frontierGap` from NEXT, and the change of material marks that boundary.
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

/** An SVG rect's place and size. */
export interface BoxRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * A box of `w` × `h` px whose top-left pixel is at (x, y), as an SVG rect: exactly its px, or,
 * drawn with a 1 px line round it (`line`), inset by half the line, so the line falls on the box's
 * own outer pixels and the box stays exactly `w` × `h` on screen, crisp at 1×.
 */
export function boxRect(x: number, y: number, w: number, h: number, line: boolean): BoxRect {
  const inset = line ? 0.5 : 0;
  if (!(w - 2 * inset > 0 && h - 2 * inset > 0)) {
    throw new Error(`pipeline: a box of ${w} × ${h} px has no extent`);
  }
  return { x: x + inset, y: y + inset, width: w - 2 * inset, height: h - 2 * inset };
}

/**
 * A card's box (a plate, NEXT's dashed outline, a landing block's) in the card's own coordinates:
 * a row card's starts under the lane, a column row's at its own top. `line`: drawn with a 1 px line
 * (a dashed outline, a hovered plate's edge).
 */
export function cardBox(o: Orientation, w: number, h: number, line: boolean): BoxRect {
  return boxRect(0, o === 'row' ? G.cardTop : 0, w, h, line);
}

/** A row's pool well from `x`: 99 × 94 under the head line, its 1 px edge on its outer pixels. */
export function wellRect(x: number): BoxRect {
  return boxRect(x, G.wellTop, G.cardW, G.wellH, true);
}

/** The theme's inset under a well's top edge: a `wellShade` px band inside its 1 px edge. */
export function wellShade(x: number): BoxRect {
  return boxRect(x + 1, G.wellTop + 1, G.cardW - 2, G.wellShade, false);
}

/**
 * An uncle travels inside the block that carries it: its header and proposals, never its txs. On a
 * plate it is a row of its own after the dot rows, taken out of the field: its word on a `head` px
 * line, then one row of its dots, then `gap` px before what follows. No box and no stone (a block's
 * stone says its tx count, which the pipeline does not know of an uncle).
 */
export const UNC = { head: 12, gap: 5, chipRows: 1 } as const;

/** One tx lifetime left to right (`G`), or stood on end, top to bottom (`COL`). */
export type Orientation = 'row' | 'column';

/**
 * The column orientation (PlatePhone.dc.html): mempool, proposed, NEXT, the tip and three older
 * blocks, each one row spanning the canvas from `x0` to `viewW − x0`. x values are absolute canvas
 * coordinates; a row is drawn as a card translated to (`x0`, its top), so its contents use
 * `x − x0`.
 *
 * Row anatomy: a block's stone at `stoneX` and its number at `numberX` (a pool's name and NEXT's
 * number at `nameX`); a well's pool total ending at `countX`; proposal dots from `chipX` (5 per
 * row, two rows `chipRowP` apart, then "+N"); the tx line from `lineX` — the cellbase first, then
 * squares at 17 px per cell, each centred vertically and set in its cut — ending at the row's
 * right padding, its "+N" and then its uncles (each its word, then its dots or its "+N"), `endGap`
 * apart — or, when they do not all fit after what the line keeps, the uncles on a line of their
 * own under it, from `lineX`, the row growing by `uncleGap` and the word's line. A row is two cells and 6 px padding tall (44 px) and grows only for a 3×3 or 4×4 square.
 * A link is two bands of ticks running down the gap between two rows, at `linkX`; the pool's flow
 * one band at `flowX`.
 */
export const COL = {
  x0: 0,
  /** Above the mempool row. */
  top: 0,
  /** Below the last row: 2 px, the tail's 30 px bands, 2 px. */
  bottom: 34,
  pad: 6,
  minCells: 2,
  wellGap: 18,
  /** The pool stands apart from NEXT, as in a row: the one wider gap, on the pool side only. */
  frontierGap: 30,
  /** NEXT ⇢ the tip, and block to block. */
  blockGap: 18,
  stoneX: 8,
  nameX: 8,
  numberX: 27,
  countX: 108,
  /** Left edge of the first proposal dot. */
  chipX: 74,
  chipsPerRow: 5,
  chipRows: 2,
  chipRowP: 9,
  lineX: 126,
  padR: 8,
  /** Between the tx line and what ends it: its "+N", an uncle, its dots, the next uncle. */
  endGap: 4,
  /** Between the tx line's marks and the uncles' own line under it, when they have one. */
  uncleGap: 4,
  /** The tip and three older blocks. */
  blocks: 4,
  /**
   * An uncle, no box: "uncle", then up to four of its dots with "+N" for the rest in their slots,
   * or, when those slots cannot hold that "+N", "+N" for all its proposals after the word.
   */
  uncle: { labelW: labelWidth('uncle'), chips: 4 },
  /** The left edges of a link's two bands, and of the pool's flow band. */
  linkX: [27, 75],
  flowX: 28,
  /** The chain's tail below the last row. */
  tail: 30,
  /**
   * The narrowest column (a 320 px viewport): a line still holds the cellbase, a 4×4 square and
   * the widest "+N" ("+2999"), and two uncles of the widest kind ("+1500") fit a line of their own
   * under it.
   */
  minViewW: 288,
} as const;

/** Height of a column row whose tallest square is `cells` cells: 44 px for 1×1 and 2×2. */
export function columnRowH(cells: number): number {
  return side(Math.max(COL.minCells, cells)) + 2 * COL.pad;
}

/** How a column uncle is drawn: its width, its dots drawn, and its proposals counted as "+N". */
interface ColumnUncle {
  w: number;
  shown: number;
  overflow: number;
}

/**
 * A column uncle with `proposals`: its word, then its four dot slots with "+N" for the rest in
 * their place; or, when the slots cannot hold that "+N" (past "+999"), its word, `endGap` and "+N"
 * for all of them. Never its word alone while it has proposals: what is not drawn is counted.
 */
function columnUncle(proposals: number): ColumnUncle {
  const u = COL.uncle;
  if (!proposals) return { w: u.labelW, shown: 0, overflow: 0 };
  const slots = slotsFor(proposals, u.chips);
  if (slots) return { w: u.labelW + COL.endGap + dotsW(u.chips), ...slots };
  return { w: u.labelW + COL.endGap + labelWidth(`+${proposals}`), shown: 0, overflow: proposals };
}

/** `n` dots side by side, in px: n pitches less the space after the last. */
function dotsW(n: number): number {
  return n * DOT.pitch - (DOT.pitch - DOT.size);
}

/**
 * Cells a plate's field and a pool well draw (5 × 4 = 20 each; the cellbase takes a block's first
 * cell). The backend lists more than this (39 txs a block, 56 a pool stage:
 * crates/api/src/routes/pipeline.rs); what is not drawn is counted as "+N", so drawn and hidden
 * always make up the total.
 */
export const POOL_PENDING_CAPACITY = G.cardCols * G.poolRows;
export const POOL_PROPOSED_CAPACITY = G.cardCols * G.poolRows;
export const BLOCK_FIELD_CAPACITY = G.cardCols * G.fieldRows;

/** Proposal dots per row of a plate's dot rows: as many as fit inside its padding (9). */
export const CHIPS_PER_ROW = Math.floor((side(G.cardCols) + DOT.pitch - DOT.size) / DOT.pitch);
/** An uncle's dot row is a plate's dot row: no box, no inset. */
export const UNCLE_CHIPS_PER_ROW = CHIPS_PER_ROW;

/** Height of an uncle's row on a plate: its word line, then its dots (one row at most). */
export function uncleRowH(nProposals: number): number {
  const rows = nProposals ? Math.min(UNC.chipRows, Math.ceil(nProposals / UNCLE_CHIPS_PER_ROW)) : 0;
  return UNC.head + (rows ? (rows - 1) * G.chipRowP + G.chip : 0);
}

/** Height a plate's uncles take out of its field (0 without uncles); `counts` = proposals per uncle. */
export function unclesH(counts: readonly number[]): number {
  return counts.reduce((h, n) => h + uncleRowH(n) + UNC.gap, 0);
}

/**
 * How many of `total` dots to draw in `cap` slots. When they do not fit, the last row gives up one
 * slot per glyph of the "+N" label drawn at its end (a glyph of the canvas's small text is 6.6 px,
 * under one 9 px pitch), so the label never covers a dot.
 */
export function chipSlots(total: number, cap: number): { shown: number; overflow: number } {
  const slots = slotsFor(total, cap);
  if (!slots) throw new Error(`pipeline: no room for a +N label in ${cap} chip slots`);
  return slots;
}

/** `chipSlots`, or null when `cap` slots cannot hold the "+N" of what does not fit. */
function slotsFor(total: number, cap: number): { shown: number; overflow: number } | null {
  if (total <= cap) return { shown: total, overflow: 0 };
  for (let r = 1; r <= cap; r++) {
    const overflow = total - (cap - r);
    if (r >= String(overflow).length + 1) return { shown: cap - r, overflow };
  }
  return null;
}

/**
 * Every plate is one fixed height: the head, the dot rows and a `fieldRows`-tall tx field. An
 * uncle takes its rows out of the field, so a plate never grows; txs that do not fit are counted
 * as "+N".
 */
export const CARD_H = PLATE_H;

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
  const all = Math.floor((room + GUT) / U);
  for (let cells = all; cells >= (reserveFirst ? 1 : 0); cells--) {
    const p = packLine(items, cells, reserveFirst);
    const hidden = p.hidden + extra;
    if (!hidden) return { pos: p.pos, hidden };
    // the "+N" follows the line `endGap` after its last cell, or stands alone on an empty line
    const before = cells ? side(cells) + COL.endGap : 0;
    if (before + labelWidth(`+${hidden}`) <= room) return { pos: p.pos, hidden };
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
  /** The label lane spans x0 … x1; the plates' head line spans xn … xf. */
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
  /** The window runs past the last plate on screen; the chain's tail carries it on. */
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
 * rows for a dimension line or its label: the gold links between the ticks are the dimension
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

/** The hovered column window: each tick runs across the full width of the gap it sits in. */
export function columnRulerPath(r: RulerGeom, viewW: number): string {
  const f = (v: number) => v.toFixed(1);
  return r.ticks.map((y) => `M${f(COL.x0)} ${f(y)}H${f(viewW - COL.x0)}`).join('');
}

/**
 * The hovered window: the ticks rise from above the dimension line at `winY` (the lane above the
 * plates) to the bottom of the head line, and the line joins them, broken around a label that sits between them. One path, shared by the
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
 * Width of a label in the canvas's small text (`SMALL_TEXT_PX`): every glyph advances 0.6 em,
 * 6.6 px (written as 3/5 so a whole number of glyphs comes out as the nearest double: 49 → 323.4).
 */
export function labelWidth(text: string): number {
  return (text.length * SMALL_TEXT_PX * 3) / 5;
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

/** The chain's tail past the last plate on screen: from `x0` to `x1` along the chain. */
export interface ChainTail {
  x0: number;
  x1: number;
}

/**
 * Where the chain runs on past the last plate on screen: from `BAND.inset` past that plate for
 * `G.tail` px, or to the view edge when that is nearer (a plate sliding out past the edge leaves
 * no room: the span is then empty, x1 ≤ x0). The chain's tail bands and an off-screen reward link
 * both end in this one span.
 */
export function chainTail(
  cards: readonly { x: number; w: number }[],
  viewW: number
): ChainTail | null {
  if (!cards.length) return null;
  const last = cards.reduce((a, b) => (b.x > a.x ? b : a));
  const x0 = last.x + last.w + BAND.inset;
  return { x0, x1: x0 + Math.min(G.tail, viewW - x0) };
}

export interface RewardLink {
  /** From the paying cellbase up into the lane, along it, and down into the paid one if shown. */
  d: string;
  /** The open end when the paid cellbase is off screen; '' when the link drops into it. */
  tail: string;
}

/**
 * The reward link of a hovered cellbase at `a`. It rises into the lane above the plates (the
 * window's lane: the ruler steps back while a link is shown) and runs right. With the paid block's
 * cellbase on screen at `b` it drops into it. Without (`b` null: the paid block is past the right
 * edge) it runs on to where the chain's own tail starts and ends there in an open tail over the
 * same span, one lane up; it never drops anywhere.
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
 * tail starts and goes on in an open tail over the same span. `tail` is the column tail (`x0`/`x1` along
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
  /** 0 = predicted … 1 = mined and settled. */
  solid: number;
}

/**
 * The chain's links as path data of filled tick rectangles (to be filled, never stroked): each
 * link two bands of ticks across the gap between two plates.
 */
export interface ChainPaths {
  /** Links between settled blocks. */
  solid: string;
  /** NEXT ⇢ the tip, and links from a block that is still landing. */
  pend: string;
  /** Links between two blocks inside the window of H: gold. */
  win: string;
  /** The two bands continued past the last plate on screen (`chainTail`). */
  tail: string;
}

/**
 * The chain from newest (left) to older (right): each link two bands of ticks across the gap
 * between two plates, at 32 and 80 px down them, no arrowheads. A link is drawn only where the
 * newer plate's parentHash is the older plate's hash, so uncles and reorgs break the chain
 * honestly. Inside the window of H the links are gold.
 */
export function chainPaths(
  cards: readonly ChainSlotState[],
  next: { x: number; w: number } | null,
  H: number,
  w: WindowParams,
  viewW: number
): ChainPaths {
  const link = ([x0, x1]: Span) => G.linkY.map((y) => tickBand('x', x0, x1, y, BAND.link)).join('');
  const s = chainSpans(cards, next, H, w, BAND.inset);
  const tail = chainTail(cards, viewW);
  return {
    solid: s.solid.map(link).join(''),
    pend: s.pend.map(link).join(''),
    win: s.win.map(link).join(''),
    tail: tail ? link([tail.x0, tail.x1]) : '',
  };
}

/** The pool's two flow links: from the mempool to the proposed stage, and from it to NEXT. */
export interface FlowPaths {
  toProposed: string;
  toNext: string;
}

/**
 * The pool's flow links in a row: one faint band of ticks in each gap before NEXT, `BAND.inset`
 * inside it, 14 px down the plates.
 */
export function flowPaths(): FlowPaths {
  const band = (x0: number, x1: number) =>
    tickBand('x', x0 + BAND.inset, x1 - BAND.inset, G.flowY, BAND.flow);
  return {
    toProposed: band(G.mempoolX + G.cardW, G.proposedX),
    toNext: band(G.proposedX + G.cardW, G.nextX),
  };
}

/** The pool's flow links in a column: one faint band at `COL.flowX` down each gap below a well. */
export function columnFlowPaths(bands: { pending: Band; proposed: Band; next: Band }): FlowPaths {
  const band = (above: Band, below: Band) =>
    tickBand('y', above.y + above.h + BAND.inset, below.y - BAND.inset, COL.flowX, BAND.flow);
  return {
    toProposed: band(bands.pending, bands.proposed),
    toNext: band(bands.proposed, bands.next),
  };
}

/** A stretch of the chain along its direction: from, to. */
type Span = [number, number];

/**
 * The chain's links along its direction (slot `x`/`w` are positions along it), `inset` px clear of
 * the plates: NEXT ⇢ the first plate pending, then a link wherever the newer plate names the older
 * one as its parent — gold between two blocks inside the window of H, pending from a block still
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
 * The chain of a column, top to bottom (slot `x`/`w` are a row's top and height): each link the
 * two bands of ticks turned to run down the gap between two rows, at `COL.linkX`, `BAND.inset` px
 * clear of both rows. The tail is the two bands continued below the last row.
 */
export function columnChainPaths(
  cards: readonly ChainSlotState[],
  next: { x: number; w: number } | null,
  H: number,
  w: WindowParams,
  viewH: number
): ChainPaths {
  const link = ([y0, y1]: Span) =>
    COL.linkX.map((x) => tickBand('y', y0, y1, x, BAND.link)).join('');
  const s = chainSpans(cards, next, H, w, BAND.inset);
  const tail = columnChainTail(cards, viewH);
  return {
    solid: s.solid.map(link).join(''),
    pend: s.pend.map(link).join(''),
    win: s.win.map(link).join(''),
    tail: tail ? link([tail.x0, tail.x1]) : '',
  };
}

/**
 * Where a column's chain runs on below its last row: from `BAND.inset` below that row for
 * `COL.tail` px, or to the bottom edge when that is nearer.
 */
export function columnChainTail(
  cards: readonly { x: number; w: number }[],
  viewH: number
): ChainTail | null {
  if (!cards.length) return null;
  const last = cards.reduce((a, b) => (b.x > a.x ? b : a));
  const x0 = last.x + last.w + BAND.inset;
  return { x0, x1: x0 + Math.min(COL.tail, viewH - x0) };
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

/** Magnetic picking: inside a square (2 px margin) wins, else the nearest within 8 px; dots 4 px. */
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

/**
 * Where an uncle is drawn: at the end of the tx line (`tx`, a column), or on a line of its own
 * (`own`: under the tx line in a column, its own row after the dot rows on a plate).
 */
export type UncleLine = 'tx' | 'own';

/**
 * An uncle carried by a block: its extent, local to the card. Nothing is drawn for it but its word
 * at `x`, its dots (the card's chips of its index) and its "+N"; no box.
 */
export interface SceneUncle {
  index: number;
  ref: PipelineBlockRef;
  line: UncleLine;
  x: number;
  y: number;
  w: number;
  h: number;
  label: string;
  /**
   * Its proposals not drawn as dots, counted as "+N" ending at `x + w` on its word's line: its
   * dots drawn (the chips of its index) and `overflow` make up all its proposals.
   */
  overflow: number;
}

/**
 * The cut a mark is set in: its square grown by the gutter on every side. One per drawn square, the
 * cellbase included, and none anywhere else: a plate is cut only where a transaction is.
 */
export interface SceneCut {
  /** The square's id. */
  id: string;
  x: number;
  y: number;
  s: number;
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
  /** One per square, in the same order: drawn under the squares, on the plate. */
  cuts: SceneCut[];
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
  /** The stage's total in the node's pool (listed or not): the well shows it on its head line. */
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
  /** Canvas height: one 146 px row, or the stacked rows of a column. */
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

/** The cuts of a plate's squares: each square grown by the gutter on every side. */
const cutsOf = (squares: readonly SceneSquare[]): SceneCut[] =>
  squares.map((q) => ({ id: q.id, x: q.x - GUT, y: q.y - GUT, s: q.s + 2 * GUT }));

/** A pool well's marks, from `x0` (inside its border) down from `G.poolTop`. */
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
      square(q.id, 'tx', lookIn(stage, q.id), x0 + q.c * U, G.poolTop + q.r * U, side(q.n), 'pool')
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
  /** Height the uncles take out of the field. */
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
  const rows = G.fieldRows - Math.ceil(uExtra / U);
  if (rows < 1) {
    throw new Error(
      `pipeline: block #${f.block.number} carries ${f.byUncle.length} uncles: its plate keeps no field row for the cellbase`
    );
  }
  const pack = pack2D(
    f.order.map((t) => ({ id: t.hash, n: squareSide(t.size) })),
    G.cardCols,
    rows,
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
    y: G.propTop + Math.floor(i / per) * G.chipRowP,
  }));
  // The uncles' rows, after the dot rows, where the field would start; the field moves down.
  const uncles: SceneUncle[] = [];
  let top: number = G.fieldTop;
  const per2 = UNCLE_CHIPS_PER_ROW;
  const cap2 = per2 * UNC.chipRows;
  g.byUncle.forEach((list, ui) => {
    const h = uncleRowH(list.length);
    uncles.push({
      index: ui,
      ref: b.uncles[ui],
      line: 'own',
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
        x: G.padX + (i % per2) * G.chipP,
        y: top + UNC.head + Math.floor(i / per2) * G.chipRowP,
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
      side(1),
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
        side(q.n),
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
    cuts: cutsOf(squares),
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
    y: G.propTop + Math.floor(i / per) * G.chipRowP,
  }));
  return {
    number: s.tip.number + 1,
    x: G.nextX,
    y: 0,
    w: G.cardW,
    h: CARD_H,
    next,
    ghosts: [
      { id: null, x: G.padX, y: G.fieldTop, s: side(1), cellbase: true },
      ...fp.pos.map((q) => ({
        id: q.id,
        x: G.padX + q.c * U,
        y: G.fieldTop + q.r * U,
        s: side(q.n),
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
        proposed: poolGrid(stages.proposed, G.proposedX + G.poolInset, G.cardCols),
        pending: poolGrid(stages.pending, G.mempoolX + G.poolInset, G.cardCols),
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
  const top = h / 2 - (G.chip + COL.chipRowP * (rows - 1)) / 2;
  return {
    ...slots,
    pos: Array.from({ length: slots.shown }, (_, i) => ({
      x: COL.chipX - COL.x0 + (i % per) * G.chipP,
      y: top + Math.floor(i / per) * COL.chipRowP,
    })),
  };
}

/** A column row as its helpers read it: its height and where its uncles are. */
interface ColumnRowShape {
  w: number;
  h: number;
  uncles: readonly { x: number; line: UncleLine }[];
}

/**
 * Height of a column row's tx line: the whole row, less the uncles' own line under it when they
 * have one. The head (stone, number, dots) and the tx line are centred on it.
 */
export function columnLineH(row: Pick<ColumnRowShape, 'h' | 'uncles'>): number {
  return row.uncles.some((u) => u.line === 'own') ? row.h - COL.uncleGap - UNC.head : row.h;
}

/**
 * Where a column row's head is drawn (local to the row), on the middle of its tx line
 * (`columnLineH`), never of a row grown by its uncles' own line: a block's stone, `G.stone` px at
 * `COL.stoneX`, set on the whole pixel as a square is, and the baseline of the words on that line
 * (a block's number at `COL.numberX`, NEXT's at `COL.nameX`), as `columnCountAt`'s.
 */
export function columnHeadAt(row: Pick<ColumnRowShape, 'h' | 'uncles'>): {
  stoneX: number;
  stoneY: number;
  y: number;
} {
  const lineH = columnLineH(row);
  return { stoneX: COL.stoneX - COL.x0, stoneY: squareTop(lineH, G.stone), y: lineH / 2 + 3.5 };
}

/** Where a column row's proposal "+N" is drawn (end-anchored, local): after the second dot row. */
export function columnChipCountAt(row: Pick<ColumnRowShape, 'h' | 'uncles'>): {
  x: number;
  y: number;
} {
  const top = columnLineH(row) / 2 - (G.chip + COL.chipRowP) / 2;
  return {
    x: COL.chipX - COL.x0 + dotsW(COL.chipsPerRow),
    y: top + COL.chipRowP + G.chip / 2 + 3.5,
  };
}

/**
 * Where a column row's tx "+N" is drawn (end-anchored, local): at the end of its tx line, before
 * its uncles when they follow it.
 */
export function columnCountAt(card: ColumnRowShape): { x: number; y: number } {
  const first = card.uncles[0];
  return {
    x: first && first.line === 'tx' ? first.x - COL.endGap : card.w - COL.padR,
    y: columnLineH(card) / 2 + 3.5,
  };
}

/** Where a column well's "+N" is drawn (end-anchored, absolute). */
export function columnWellCountAt(viewW: number, band: Band): { x: number; y: number } {
  return { x: viewW - COL.x0 - COL.padR, y: band.y + band.h / 2 + 3.5 };
}

/**
 * Where a column well's words are drawn (absolute), on the middle of its band: its name from
 * `COL.nameX`, its pool total ending at `COL.countX`; and NEXT's "unavailable" in NEXT's band.
 */
export function columnWellHeadAt(band: Band): { nameX: number; countX: number; y: number } {
  return { nameX: COL.nameX, countX: COL.countX, y: band.y + band.h / 2 + 3.5 };
}

/**
 * A column well (PlatePhone.dc.html `.rw.wl`): its band across the canvas, square, its 1 px edge
 * on its own outer pixels, as a row's well (`wellRect`).
 */
export function columnWellRect(viewW: number, band: Band): BoxRect {
  return boxRect(COL.x0, band.y, viewW - 2 * COL.x0, band.h, true);
}

/** The theme's inset under a column well's top edge: a `G.wellShade` px band inside its 1 px edge. */
export function columnWellShade(viewW: number, band: Band): BoxRect {
  return boxRect(COL.x0 + 1, band.y + 1, viewW - 2 * COL.x0 - 2, G.wellShade, false);
}

const squareTop = (h: number, s: number) => Math.floor((h - s) / 2);
/** The tallest square on a line, in cells. */
const tallest = (pos: readonly LinePos[]) => Math.max(1, ...pos.map((p) => p.n));

/**
 * What a block's tx line always keeps, whatever uncles it carries: the cellbase, room for the
 * largest square (a 4×4) and the widest "+N" the block can need (`+(txCount − 1)`). It depends on
 * the block's count alone, never on which txs are listed, so an uncle's place does not jump as
 * they change.
 */
const lineKeeps = (b: PipelineBlock) =>
  side(1 + 4) + (b.txCount > 1 ? COL.endGap + labelWidth(`+${b.txCount - 1}`) : 0);

/**
 * A block as a column row: its own proposal dots, then its tx line — the cellbase and the txs that
 * fit, then its "+N" — and its uncles. The uncles follow the tx line, `endGap` apart, when all of
 * them fit in what is left after `lineKeeps`; otherwise all of them take a line of their own under
 * the tx line, from `lineX`, and the row grows by `uncleGap` and the word's line. Nothing is left
 * uncounted: a square that does not fit is in the line's "+N", an uncle's proposals are its dots
 * and its "+N".
 */
function columnCard(f: BlockFacts, slot: SceneCard['slot'], w: number): SceneCard {
  const b = f.block;
  const u = COL.uncle;
  const lineX = COL.lineX - COL.x0;
  const lineEnd = w - COL.padR;
  const forms = f.byUncle.map((list) => columnUncle(list.length));
  const unclesW = forms.reduce((sum, un) => sum + COL.endGap + un.w, 0);
  const line: UncleLine = lineEnd - lineX - lineKeeps(b) >= unclesW ? 'tx' : 'own';
  if (line === 'own' && unclesW - COL.endGap > lineEnd - lineX) {
    throw new Error(
      `pipeline: the ${forms.length} uncles of block #${b.number} need ${unclesW - COL.endGap} px, more than a ${lineEnd - lineX} px line of their own`
    );
  }
  const txEnd = line === 'tx' ? lineEnd - unclesW : lineEnd;
  const fit = fitLine(
    f.order.map((t) => ({ id: t.hash, n: squareSide(t.size) })),
    txEnd - lineX,
    true,
    b.omittedTxs,
    `block #${b.number}`
  );
  const lineH = columnRowH(tallest(fit.pos));
  const wrapped = line === 'own' && forms.length > 0;
  const h = lineH + (wrapped ? COL.uncleGap + UNC.head : 0);
  // An uncle's word line: centred on the tx line, or under it.
  const uncleY = wrapped ? lineH - COL.pad + COL.uncleGap : lineH / 2 - UNC.head / 2;
  const txByHash = new Map(b.txs.map((t) => [t.hash, t]));
  const squares: SceneSquare[] = [
    square(
      b.cellbase.hash,
      'cellbase',
      txLook({ stage: 'cellbase', cycles: null, tooEarly: false }),
      lineX,
      squareTop(lineH, side(1)),
      side(1),
      b.hash
    ),
    ...fit.pos.map((q) => {
      const t = txByHash.get(q.id) as PipelineTx;
      return square(
        q.id,
        'tx',
        txLook({ stage: 'committed', cycles: t.cycles, tooEarly: false }),
        lineX + q.c * U,
        squareTop(lineH, side(q.n)),
        side(q.n),
        b.hash
      );
    }),
  ];
  const own = columnDots(f.own.length, lineH);
  const chips: SceneChip[] = f.own.slice(0, own.shown).map((p, i) => ({
    key: `${p.shortId}|`,
    proposal: p,
    ...own.pos[i],
  }));
  const uncles: SceneUncle[] = [];
  let x = line === 'tx' ? txEnd + COL.endGap : lineX;
  f.byUncle.forEach((list, ui) => {
    const un = forms[ui];
    uncles.push({
      index: ui,
      ref: b.uncles[ui],
      line,
      x,
      y: uncleY,
      w: un.w,
      h: UNC.head,
      label: 'uncle',
      overflow: un.overflow,
    });
    list.slice(0, un.shown).forEach((p, i) => {
      chips.push({
        key: `${p.shortId}|${ui}`,
        proposal: p,
        x: x + u.labelW + COL.endGap + i * G.chipP,
        y: uncleY + (UNC.head - G.chip) / 2,
      });
    });
    x += un.w + COL.endGap;
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
    cuts: cutsOf(squares),
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
  const h = columnRowH(tallest(fit.pos));
  const dots = columnDots(next.proposals, h);
  return {
    number: s.tip.number + 1,
    x: COL.x0,
    y: 0,
    w,
    h,
    next,
    ghosts: [
      { id: null, x: lineX, y: squareTop(h, side(1)), s: side(1), cellbase: true },
      ...fit.pos.map((q) => ({
        id: q.id,
        x: lineX + q.c * U,
        y: squareTop(h, side(q.n)),
        s: side(q.n),
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
 * bottom, each a row as wide as the canvas. Rows are 44 px unless a 3×3 or 4×4 square makes one
 * taller; the gaps are 18 between the wells, 30 between the pool and NEXT, and 18 from NEXT on.
 * The canvas is as tall as its rows plus the room for the tail.
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
    return { ...fit, h: columnRowH(tallest(fit.pos)) };
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
  const nextBand = band(landing ? rows[0].h : next ? next.h : base, COL.blockGap);
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
        at.y + squareTop(at.h, side(q.n)),
        side(q.n),
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
  /**
   * The hovered window: its ticks (and, in a row, the dimension line). At rest the window has no
   * mark of its own: its gold bands say it.
   */
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
      ticks: rulerPath,
      tail: (cards) => chainTail(cards, viewW),
      rewardLink,
    };
  }
  return {
    along: (c) => ({ x: c.y, w: c.h }),
    chainPaths: (cards, next, H, w) => columnChainPaths(cards, next, H, w, viewH),
    ruler: (slots, H, w) => columnWindowRuler(slots, H, w, viewH),
    ticks: (r) => columnRulerPath(r, viewW),
    tail: (cards) => columnChainTail(cards, viewH),
    rewardLink: columnRewardLink,
  };
}
