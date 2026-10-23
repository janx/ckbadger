/**
 * Geometry of the transaction pipeline, ported from the approved prototype
 * (docs/superpowers/specs/2026-10-23-transaction-pipeline-prototype.html). All functions are pure.
 *
 * A 7 px grid: a tx is an n×n square of cells (area ∝ bytes), a proposal is a 5 px chip. Cards are
 * top-aligned; the chain runs through the card headers.
 */

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
