import { describe, expect, it } from 'vitest';
import tailwindConfig from '@/tailwind.config';
import {
  FIXTURE_TIP,
  buildPipelineSnapshot,
  committedTxHash,
  mineNext,
  poolTxHash,
  rewardOf,
  shortIdOf,
  uncleHash,
  withSmallTxs,
} from '@/__tests__/msw/fixtures/pipeline-snapshot';
import type { PipelineProposal, PipelineSnapshot, PipelineTx } from '@/lib/api';
import {
  BLOCK_FIELD_CAPACITY,
  CARD_H,
  CELL,
  CHIPS_PER_ROW,
  COL,
  G,
  GUT,
  POOL_PENDING_CAPACITY,
  NUMBER_TEXT_PX,
  POOL_PROPOSED_CAPACITY,
  SMALL_TEXT_PX,
  U,
  UNC,
  UNCLE_CHIPS_PER_ROW,
  axisOf,
  boxRect,
  buildScene,
  cardBox,
  chainLayout,
  chainPaths,
  chainTail,
  chipSlots,
  columnChainPaths,
  columnChainTail,
  columnChipCountAt,
  columnCountAt,
  columnFlowPaths,
  columnHeadAt,
  columnLineH,
  columnRewardLink,
  columnRowH,
  columnWellCountAt,
  columnWellHeadAt,
  columnWellRect,
  columnWellShade,
  fitLine,
  flowPaths,
  packLine,
  pickTargets,
  inWindow,
  pack2D,
  pick,
  rewardLink,
  rewardTarget,
  rowHead,
  side,
  strapBox,
  tail4,
  uncleRowH,
  unclesH,
  wellRect,
  wellShade,
  windowLabel,
  windowLane,
  windowRange,
  labelWidth,
  type BlockSpan,
  type ChainSlotState,
  type WindowLabelGeom,
  type PickTargets,
  type Scene,
} from '@/lib/pipeline/layout';

const MAINNET = { proposalWindowClosest: 2, proposalWindowFarthest: 10 };
const T = FIXTURE_TIP;
/** The plate width of the design: 5 cells at the 17 px pitch (83 px) and 8 px on both sides. */
const CARD_W = 99;
/** The plate pitch: a plate and the 18 px gap after it. */
const PITCH = 117;
/** A 1280 px page container, less its padding: the canvas of the row at desktop width. */
const CONTAINER_VIEW_W = 1246;
/** What the backend lists of a block's txs at most (crates/api/src/routes/pipeline.rs). */
const BACKEND_BLOCK_TXS = 39;
/** What the backend lists of each pool stage at most (crates/api/src/routes/pipeline.rs). */
const BACKEND_POOL_TXS = 56;

function cardOf(scene: Scene, n: number) {
  const c = scene.cards.find((k) => k.block.number === n);
  if (!c) throw new Error(`card #${n} is not on screen`);
  return c;
}

/** `count` 1×1 txs committed in block `n`, each proposed two blocks earlier. */
const smallTxs = (n: number, count: number): PipelineTx[] =>
  Array.from({ length: count }, (_, i) => ({
    hash: committedTxHash(n, i),
    size: 330,
    fee: '330',
    feeRate: '1000',
    cycles: 1_000_000,
    cyclesStatus: 'done',
    proposedIn: { block: n - 2, uncleIndex: null },
  }));

/** `s` with block `n`'s listed txs replaced (and its counts kept consistent). */
function withTxs(s: PipelineSnapshot, n: number, txs: PipelineTx[], omitted = 0): PipelineSnapshot {
  return {
    ...s,
    blocks: s.blocks.map((b) =>
      b.number === n ? { ...b, txs, txCount: 1 + txs.length + omitted, omittedTxs: omitted } : b
    ),
  };
}

/** `count` waiting proposals of uncle `uncleIndex`, carried by a block. */
const uncleProposals = (uncleIndex: number, count: number, seed: number): PipelineProposal[] =>
  Array.from({ length: count }, (_, i) => ({
    shortId: shortIdOf(committedTxHash(T - 30, seed + i)),
    uncleIndex,
    state: 'waiting' as const,
    txHash: null,
    committedBlock: null,
  }));

function occupied(pos: { c: number; r: number; n: number }[]): Set<string> {
  const cells = new Set<string>();
  for (const p of pos) {
    for (let r = p.r; r < p.r + p.n; r++) {
      for (let c = p.c; c < p.c + p.n; c++) {
        const key = `${c},${r}`;
        if (cells.has(key)) throw new Error(`cell ${key} used twice`);
        cells.add(key);
      }
    }
  }
  return cells;
}

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The filled rectangles of a strap's path; anything else in the path fails the test. */
function rects(d: string): Rect[] {
  const out: Rect[] = [];
  let read = 0;
  for (const m of d.matchAll(/M([\d.]+) ([\d.]+)h([\d.]+)v([\d.]+)h-([\d.]+)Z/g)) {
    expect(m[5], `${m[0]} closes on its own width`).toBe(m[3]);
    out.push({ x: Number(m[1]), y: Number(m[2]), w: Number(m[3]), h: Number(m[4]) });
    read += m[0].length;
  }
  expect(read, 'the path is filled rectangles only').toBe(d.length);
  return out;
}

describe('pack2D: first-fit grid packing', () => {
  it('never overlaps squares and keeps (0,0) for the cellbase', () => {
    const items = [4, 1, 3, 1, 2, 2, 1, 1, 1, 4, 1, 2].map((n, i) => ({ id: `t${i}`, n }));
    const { pos, hidden } = pack2D(items, 9, 17, true);
    const cells = occupied(pos);
    expect(cells.has('0,0')).toBe(false);
    expect(hidden).toBe(0);
    expect(pos).toHaveLength(items.length);
    for (const p of pos) {
      expect(p.c + p.n).toBeLessThanOrEqual(9);
      expect(p.r + p.n).toBeLessThanOrEqual(17);
    }
  });

  it('fills holes left by big squares with later small ones', () => {
    const { pos } = pack2D(
      [
        { id: 'big', n: 2 },
        { id: 'a', n: 1 },
      ],
      3,
      4,
      true
    );
    // The 2×2 cannot start at (0,0) (cellbase) → (1,0); the 1×1 drops into the hole at (0,1).
    expect(pos.find((p) => p.id === 'big')).toMatchObject({ c: 1, r: 0 });
    expect(pos.find((p) => p.id === 'a')).toMatchObject({ c: 0, r: 1 });
  });

  it('counts what does not fit as hidden', () => {
    const items = Array.from({ length: 10 }, (_, i) => ({ id: `t${i}`, n: 1 }));
    const { pos, hidden } = pack2D(items, 3, 3, true);
    expect(pos).toHaveLength(8);
    expect(hidden).toBe(2);
  });

  it('clamps a square to the grid width', () => {
    const { pos } = pack2D([{ id: 'x', n: 4 }], 3, 5, false);
    expect(pos[0]).toMatchObject({ c: 0, r: 0, n: 3 });
  });
});

describe('the unit: 15 px cells on a 17 px pitch', () => {
  it('says the gutter once: an n-cell span is n · 17 − 2 px', () => {
    expect([CELL, GUT, U]).toEqual([15, 2, 17]);
    expect([1, 2, 3, 4].map(side)).toEqual([15, 32, 49, 66]);
    expect(side(5)).toBe(83);
    // A span is whole cells: there is no span of no cell.
    expect(() => side(0)).toThrow(/whole number of cells/);
    expect(() => side(1.5)).toThrow(/whole number of cells/);
  });
});

describe('geometry of the one-row canvas', () => {
  it('draws fixed 99 × 122 plates in a 146 px row, under an 18 px lane', () => {
    expect(G.cardCols).toBe(5);
    expect(G.fieldRows).toBe(4);
    expect(G.poolRows).toBe(4);
    expect(G.padX).toBe(8);
    expect(G.cardW).toBe(CARD_W);
    expect(G.cardW).toBe(side(G.cardCols) + 2 * G.padX);
    expect(CARD_H).toBe(122);
    expect(G.cardTop).toBe(18);
    // the lane, a plate and 6 px below it
    expect(G.H).toBe(G.cardTop + CARD_H + 6);
    expect(G.H).toBe(146);
    // The head is 8…28 down a plate: the 14 px stone at (8, 11), the number at x 27.
    expect(G.headH).toBe(28);
    expect(G.headY).toBe(G.cardTop + 18);
    expect([G.stoneX, G.stoneY - G.cardTop, G.stone, G.numberX]).toEqual([8, 11, 14, 27]);
    expect(G.stoneY + G.stone).toBeLessThanOrEqual(G.cardTop + G.headH);
    // The field from 48 down the plate, four rows, then the 8 px bottom padding.
    expect(G.fieldTop - G.cardTop).toBe(48);
    expect(G.padB).toBe(8);
    expect(G.fieldTop + side(G.fieldRows) + G.padB).toBe(G.cardTop + CARD_H);
    expect(UNC.chipRows).toBe(1);
  });

  it('draws proposals as two rows of nine 7 px dots on a 9 px pitch, at 30 and 38 down the plate', () => {
    expect([G.chip, G.chipP, G.chipRowP]).toEqual([7, 9, 8]);
    expect(G.propRows).toBe(2);
    expect(CHIPS_PER_ROW).toBe(9);
    expect(G.propTop - G.cardTop).toBe(30);
    expect(G.propTop + G.chipRowP - G.cardTop).toBe(38);
    // Nine dots fit the plate inside its padding; a tenth would not.
    const rowEnd = (n: number) => G.padX + n * G.chipP - (G.chipP - G.chip);
    expect(rowEnd(CHIPS_PER_ROW)).toBeLessThanOrEqual(G.cardW - G.padX);
    expect(rowEnd(CHIPS_PER_ROW + 1)).toBeGreaterThan(G.cardW - G.padX);
    // The two rows end above the tx field.
    expect(G.propTop + G.chipRowP + G.chip).toBeLessThanOrEqual(G.fieldTop);
  });

  it('runs mempool ⇢ proposed ⇢ NEXT ⇢ tip with the one wide gap between the pool and NEXT', () => {
    expect(G.mempoolX).toBe(0);
    expect(G.gap).toBe(18);
    expect(G.frontierGap).toBe(30);
    expect(G.proposedX).toBe(G.mempoolX + CARD_W + G.gap);
    expect(G.nextX).toBe(G.proposedX + CARD_W + G.frontierGap);
    // NEXT is a plate's gap from the tip: no wide gap on its chain side.
    expect(G.chainX0).toBe(G.nextX + CARD_W + G.gap);
    expect([G.mempoolX, G.proposedX, G.nextX, G.chainX0]).toEqual([0, 117, 246, 363]);
  });

  // Direction G (round 7, Strap.dc.html `.sp`): one strap through the head line of the plates, the
  // head line itself (plate y 8 … 28), so the stone and the number stand on it.
  it('runs the strap through the head line: canvas y 26 … 46, 20 px, the stone and the number on it', () => {
    expect([G.strapY, G.strapH]).toEqual([26, 20]);
    expect(G.strapY - G.cardTop).toBe(8);
    // exactly the head line: it ends where the head does, and the words' middle is its middle
    expect(G.strapY + G.strapH).toBe(G.cardTop + G.headH);
    expect(G.strapY + G.strapH / 2).toBe(G.headY);
    // the 14 px stone stands inside it, 3 px clear of both edges
    expect(G.stoneY - G.strapY).toBe(3);
    expect(G.strapY + G.strapH - (G.stoneY + G.stone)).toBe(3);
    // a plate's own strap, local to its card: across the whole plate, at the strap's y
    expect(strapBox('row', G.cardW, CARD_H)).toEqual({ x: 0, y: 26, width: 99, height: 20 });
  });

  it('keeps the window’s words and the reward link in the 18 px lane above the plates', () => {
    expect(G.winY).toBe(9);
    // the words' baseline sits inside the lane, above the plates
    expect(G.winY + 3.5).toBeLessThan(G.cardTop);
    // The reward link turns along the lane, never into a plate.
    const link = rewardLink({ X: 380, Y: 73.5 }, { X: 500, Y: 73.5 }, { x0: 600, x1: 666 });
    expect(link.d).toContain(`H495.0Q500.0 ${G.winY}`);
  });
});

describe('grid capacities', () => {
  it('draws 20 cells of a pool well (5 × 4) and of a block field (5 × 4)', () => {
    expect(POOL_PENDING_CAPACITY).toBe(20);
    expect(POOL_PROPOSED_CAPACITY).toBe(20);
    expect(BLOCK_FIELD_CAPACITY).toBe(20);
    expect(POOL_PENDING_CAPACITY).toBe(G.cardCols * G.poolRows);
    expect(BLOCK_FIELD_CAPACITY).toBe(G.cardCols * G.fieldRows);
  });

  it('sets a well under its head line, 99 × 94, its marks 7 px inside its 1 px border', () => {
    expect(G.wellTop - G.cardTop).toBe(28);
    expect(G.wellH).toBe(94);
    expect(G.wellTop + G.wellH).toBe(G.cardTop + CARD_H);
    expect(G.poolInset).toBe(1 + 7);
    expect(G.poolTop).toBe(G.wellTop + G.poolInset);
  });

  // Lane R (Plate.dc.html `.wl`, `.tl`): the well's 1 px edge falls on its own outer pixels, its
  // inset a 3 px shade under that edge; the pool's name and total stand 1 px inside its edges.
  it('draws a well as its exact 99 × 94 px with the edge on its outer pixels, the inset under it', () => {
    expect(wellRect(G.proposedX)).toEqual({
      x: G.proposedX + 0.5,
      y: G.wellTop + 0.5,
      width: CARD_W - 1,
      height: G.wellH - 1,
    });
    expect(wellShade(G.proposedX)).toEqual({
      x: G.proposedX + 1,
      y: G.wellTop + 1,
      width: CARD_W - 2,
      height: 3,
    });
    expect(G.poolHeadPad).toBe(1);
  });
});

describe('a box and its 1 px line', () => {
  it('fills exactly its px, or insets by half a line so the line lies on its own outer pixels', () => {
    expect(boxRect(10, 20, 99, 122, false)).toEqual({ x: 10, y: 20, width: 99, height: 122 });
    expect(boxRect(10, 20, 99, 122, true)).toEqual({ x: 10.5, y: 20.5, width: 98, height: 121 });
  });

  it('sets a row card under the lane and a column row at its own top', () => {
    expect(cardBox('row', CARD_W, CARD_H, false)).toEqual({
      x: 0,
      y: G.cardTop,
      width: CARD_W,
      height: CARD_H,
    });
    // NEXT's dashed outline (and a hovered plate's edge): Plate.dc.html's `rect x=.5 y=.5 98 × 121`
    expect(cardBox('row', CARD_W, CARD_H, true)).toEqual({
      x: 0.5,
      y: G.cardTop + 0.5,
      width: 98,
      height: 121,
    });
    expect(cardBox('column', 358, 44, false)).toEqual({ x: 0, y: 0, width: 358, height: 44 });
  });

  it('refuses a box with no extent', () => {
    expect(() => boxRect(0, 0, 0, 10, false)).toThrow(/box of 0 × 10 px has no extent/);
    expect(() => boxRect(0, 0, 10, 1, true)).toThrow(/box of 10 × 1 px has no extent/);
  });

  it('draws 20 of the 56 pool txs the backend lists in a well, and counts the rest', () => {
    const s = buildPipelineSnapshot();
    const pool = s.pool as NonNullable<PipelineSnapshot['pool']>;
    const many = Array.from({ length: BACKEND_POOL_TXS }, (_, i) => ({
      ...pool.pending[0],
      hash: poolTxHash(`many${i}`),
      size: 330,
      arrivedAt: pool.pending[0].arrivedAt + i,
    }));
    const scene = buildScene(
      { ...s, pool: { ...pool, pending: many, pendingTotal: many.length + 100 } },
      1600,
      'settled'
    );
    const wells = scene.pool as NonNullable<Scene['pool']>;
    expect(wells.pending.squares).toHaveLength(POOL_PENDING_CAPACITY);
    // The well shows the pool total, listed or not; drawn and hidden make it up exactly.
    expect(wells.pending.total).toBe(many.length + 100);
    expect(wells.pending.squares.length + wells.pending.hidden).toBe(wells.pending.total);
    for (const [grid, x] of [
      [wells.pending, G.mempoolX],
      [wells.proposed, G.proposedX],
    ] as const) {
      for (const q of grid.squares) {
        expect(q.x).toBeGreaterThanOrEqual(x + G.poolInset);
        expect(q.x + q.s).toBeLessThanOrEqual(x + CARD_W - G.poolInset);
        expect(q.y).toBeGreaterThanOrEqual(G.poolTop);
        expect(q.y + q.s).toBeLessThanOrEqual(G.wellTop + G.wellH - G.poolInset);
      }
    }
    expect(wells.pending.squares[0]).toMatchObject({ x: 8, y: 54, s: 15 });
  });
});

describe('plate size', () => {
  const scene = buildScene(buildPipelineSnapshot(), 1600, 'settled');

  it('is the same width for every plate, whatever its txs, proposals and uncles', () => {
    // On screen: quiet blocks, a block with 299 txs (T−5), one with 299 proposals (T−7) and two
    // that carry an uncle (T−1, T−6).
    for (const n of [T - 5, T - 7, T - 1, T - 6, T - 8]) expect(cardOf(scene, n).w).toBe(CARD_W);
    expect(new Set(scene.cards.map((c) => c.w))).toEqual(new Set([CARD_W]));
    expect(scene.next?.w).toBe(CARD_W);
    expect(G.cardW).toBe(CARD_W);
  });

  it('keeps that width for a just-mined block landing at NEXT', () => {
    const s1 = buildPipelineSnapshot();
    const s2 = mineNext(s1, { commit: [poolTxHash('w0')], propose: [poolTxHash('q0')] });
    const landing = buildScene(s2, 1600, 'landing').cards[0];
    expect(landing.slot).toBe('landing');
    expect(landing.w).toBe(CARD_W);
  });

  it('is one fixed height for every plate: quiet, full, carrying uncles, and NEXT', () => {
    const quiet = scene.cards.filter((c) => !c.block.uncles.length && c.block.txs.length <= 2);
    expect(new Set(quiet.map((c) => c.block.txs.length))).toEqual(new Set([0, 1]));
    expect(new Set(scene.cards.map((c) => c.h))).toEqual(new Set([CARD_H]));
    expect(cardOf(scene, T - 5).h).toBe(CARD_H);
    expect(cardOf(scene, T - 1).h).toBe(CARD_H);
    expect(scene.next?.h).toBe(CARD_H);
  });

  it('shows "+N" for the 1×1 txs that do not fit the 5 × 4 field', () => {
    const n = T - 8; // no txs, proposals or uncles in the fixture
    const card = cardOf(
      buildScene(withTxs(buildPipelineSnapshot(), n, smallTxs(n, 200)), 1600, 'settled'),
      n
    );
    // 5 × 4 = 20 cells: the cellbase and 19 txs; the other 181 are counted, not drawn.
    expect(card.squares).toHaveLength(BLOCK_FIELD_CAPACITY);
    expect(card.squares.every((q) => q.x + q.s <= card.w - G.padX)).toBe(true);
    expect(card.squares.every((q) => q.y + q.s <= G.cardTop + CARD_H - G.padB)).toBe(true);
    expect(card.hidden).toBe(181);
    expect(card.squares.length + card.hidden).toBe(card.block.txCount);
    expect(card.w).toBe(CARD_W);
    // The field: 15 px squares on the 17 px pitch from (8, 48) down the plate.
    expect(card.squares.slice(0, 6).map((q) => [q.x, q.y - G.cardTop, q.s])).toEqual([
      [8, 48, 15],
      [25, 48, 15],
      [42, 48, 15],
      [59, 48, 15],
      [76, 48, 15],
      [8, 65, 15],
    ]);
  });

  it('orders equal-size transactions by their exact fee rates', () => {
    const n = T - 8;
    const txs = smallTxs(n, 2);
    txs[0].feeRate = '9007199254740992';
    txs[1].feeRate = '9007199254740993';
    const card = cardOf(buildScene(withTxs(buildPipelineSnapshot(), n, txs), 1600, 'settled'), n);
    expect(card.squares.slice(1).map((square) => square.id)).toEqual([txs[1].hash, txs[0].hash]);
  });

  it('counts the txs the backend lists but the field cannot hold together with the ones it omitted', () => {
    // T−5 commits 299 txs; the backend lists 39 of them and omits 260; the field draws the
    // cellbase and 19.
    const full = cardOf(scene, T - 5);
    expect(full.block.txs).toHaveLength(BACKEND_BLOCK_TXS);
    expect(full.block.omittedTxs).toBe(260);
    expect(full.squares).toHaveLength(BLOCK_FIELD_CAPACITY);
    expect(full.hidden).toBe(BACKEND_BLOCK_TXS - (BLOCK_FIELD_CAPACITY - 1) + 260);
    expect(full.hidden).toBe(280);
    expect(full.squares.length + full.hidden).toBe(full.block.txCount);
    expect(cardOf(scene, T - 8).hidden).toBe(0);
  });

  it('fits a 4 × 4 tx in the field, beside one column, and counts a second as "+N"', () => {
    const n = T - 8;
    const big = (i: number): PipelineTx => ({ ...smallTxs(n, i + 1)[i], size: 9600 });
    const card = cardOf(
      buildScene(withTxs(buildPipelineSnapshot(), n, [big(0), big(1)]), 1600, 'settled'),
      n
    );
    expect(card.squares.map((q) => q.s)).toEqual([15, 66]);
    expect(card.squares[1]).toMatchObject({ x: 8 + U, y: G.fieldTop });
    expect(card.hidden).toBe(1);
  });
});

// Ruling on lane F: the stone is an accent and gives way; the number and the tx "+N" on a row
// plate's head line are data and stay whole, at least 4 px apart (closer, "…4755+280" reads as one
// token).
describe('a plate’s head line: the stone gives way, never the number or its "+N"', () => {
  const n = T - 8; // no txs, proposals or uncles in the fixture
  /** Block T−8 listing the backend's 39 txs (19 drawn) and omitting more: `hidden` counted. */
  const hiding = (hidden: number) =>
    withTxs(buildPipelineSnapshot(), n, smallTxs(n, BACKEND_BLOCK_TXS), hidden - 20);
  const rowCard = (hidden: number) => cardOf(buildScene(hiding(hidden), 1600, 'settled'), n);
  /** The px between the number's end and the "+N"'s start on the head line. */
  const gap = (h: { numberX: number }, hidden: number) =>
    G.countX - labelWidth(`+${hidden}`) - (h.numberX + (tail4(n).length * NUMBER_TEXT_PX * 3) / 5);

  // re-pinned (readability R4): the number is drawn in text-meta, 12 px since R4 (it was
  // text-small when that was 12 px); the canvas is drawn for a 12 px number
  it('measures the number at the size it is drawn: text-meta, 12 px, 0.6 em a glyph', () => {
    expect(NUMBER_TEXT_PX).toBe(12);
    expect(tailwindConfig.theme.fontSize.meta[0]).toBe(`${NUMBER_TEXT_PX}px`);
    // the stone at 8 (14 px), the number from 27, the "+N" ending at 91, at least 4 px apart
    expect([G.stoneX, G.stone, G.numberX, G.countX, G.headGap]).toEqual([8, 14, 27, 91, 4]);
    expect(tail4(n)).toHaveLength(5);
  });

  it('keeps the stone while the number (27 … 63) and the "+N" stand 4 px apart: up to "+99"', () => {
    expect(rowHead(n, 0)).toEqual({ stone: true, numberX: 27 });
    // "+99" is 21.6 px at 12 px (re-pinned, readability R4: 19.8 at 11): 69.4 … 91, 6.4 px after
    // the number (was 8.2)
    expect(rowHead(n, 99)).toEqual({ stone: true, numberX: 27 });
    expect(gap({ numberX: 27 }, 99)).toBeCloseTo(6.4, 9);
    const card = rowCard(99);
    expect(card).toMatchObject({ hidden: 99, stone: true, numberX: 27 });
    expect(card.squares.length + card.hidden).toBe(card.block.txCount);
  });

  it('presses no stone from "+100": the number starts at the padding, x 8', () => {
    // "+100" … "+999" is 28.8 px at 12 px (re-pinned, readability R4: 26.4 at 11): from 62.2, over
    // a number at 27 by 0.8 px (was 1.6 px after it); at 8 the number ends at 44, 18.2 px clear.
    expect(gap({ numberX: 27 }, 100)).toBeCloseTo(-0.8, 9);
    for (const hidden of [100, 280, 999, 1000]) {
      expect(rowHead(n, hidden), `+${hidden}`).toEqual({ stone: false, numberX: G.padX });
      const card = rowCard(hidden);
      expect(card).toMatchObject({ hidden, stone: false, numberX: 8 });
      expect(gap(card, hidden)).toBeGreaterThanOrEqual(G.headGap);
      expect(card.squares.length + card.hidden).toBe(card.block.txCount);
    }
    // the boundary, exactly
    expect(rowHead(n, 99).stone && !rowHead(n, 100).stone).toBe(true);
  });

  // re-pinned (readability R4): "+2980" is 36 px at 12 px (33 at 11), so 11 px clear (was 14)
  it('holds the widest block’s head at the padding: 3,000 txs, "+2980" 11 px clear of its number', () => {
    const card = rowCard(2980);
    expect(card.block.txCount).toBe(3000);
    expect(card).toMatchObject({ hidden: 2980, stone: false, numberX: 8 });
    expect(card.squares.length + card.hidden).toBe(3000);
    expect(gap(card, 2980)).toBeCloseTo(11, 9);
    // every count a block can hide keeps its number and its "+N" at least 4 px apart
    for (let hidden = 0; hidden <= 2999; hidden++) {
      const head = rowHead(n, hidden);
      if (hidden) expect(gap(head, hidden), `+${hidden}`).toBeGreaterThanOrEqual(G.headGap);
      expect(head.stone, `+${hidden}`).toBe(hidden < 100);
    }
  });

  it('says it in the scene for a landing block too, and leaves the column’s head as it is', () => {
    const s2 = mineNext(buildPipelineSnapshot(), {
      commit: [poolTxHash('w0')],
      propose: [poolTxHash('q0')],
    });
    expect(s2.blocks[0].number).toBe(T + 1);
    const landing = withTxs(s2, T + 1, s2.blocks[0].txs, 980);
    const card = buildScene(landing, 1600, 'landing').cards[0];
    expect(card).toMatchObject({ slot: 'landing', hidden: 980, stone: false, numberX: 8 });
    // and the same block as a chain plate once it has landed
    expect(cardOf(buildScene(landing, 1600, 'settled'), T + 1)).toMatchObject({
      slot: 'chain',
      stone: false,
      numberX: 8,
    });
    // In a column the tx "+N" ends the line, not the head: every row keeps its stone and its
    // number at 27.
    for (const c of buildScene(hiding(2980), 358, 'settled').cards) {
      expect(c).toMatchObject({ stone: true, numberX: COL.numberX });
    }
  });
});

describe('a plate is cut only where a transaction is', () => {
  it.each([0, 1, 19, 60])(
    'cuts exactly one ring per drawn square for a block of %i txs, in a row and in a column',
    (k) => {
      const n = T - 2; // on screen in both orientations
      const s = withTxs(buildPipelineSnapshot(), n, smallTxs(n, k));
      for (const W of [1600, 358]) {
        const card = cardOf(buildScene(s, W, 'settled'), n);
        // The cellbase is always drawn, so a block of no tx still has one square and one cut.
        expect(card.squares.length).toBeGreaterThanOrEqual(1);
        expect(card.cuts).toHaveLength(card.squares.length);
        card.squares.forEach((q, i) => {
          // Each cut is its square grown by the gutter on every side: it encloses the square.
          expect(card.cuts[i]).toEqual({ id: q.id, x: q.x - GUT, y: q.y - GUT, s: q.s + 2 * GUT });
          expect(card.cuts[i].x).toBeLessThan(q.x);
          expect(card.cuts[i].x + card.cuts[i].s).toBeGreaterThan(q.x + q.s);
        });
        // Every cut lies inside its plate.
        const top = W >= G.minViewW ? G.cardTop : 0;
        for (const c of card.cuts) {
          expect(c.x).toBeGreaterThanOrEqual(0);
          expect(c.x + c.s).toBeLessThanOrEqual(card.w);
          expect(c.y).toBeGreaterThanOrEqual(top);
          expect(c.y + c.s).toBeLessThanOrEqual(top + card.h);
        }
      }
    }
  );

  it('cuts every square of the full fixture, every size, the cellbase included', () => {
    for (const W of [1600, 358]) {
      const scene = buildScene(buildPipelineSnapshot(), W, 'settled');
      for (const c of scene.cards) {
        expect(c.cuts.map((k) => k.id)).toEqual(c.squares.map((q) => q.id));
        expect(c.cuts.map((k) => k.s)).toEqual(c.squares.map((q) => q.s + 4));
        expect(c.squares[0].kind).toBe('cellbase');
      }
    }
  });

  it('carries nothing for an empty cell in any scene type', () => {
    const scene = buildScene(buildPipelineSnapshot(), 1600, 'settled');
    const card = cardOf(scene, T - 1);
    expect(Object.keys(card).sort()).toEqual([
      'block',
      'chipOverflow',
      'chips',
      'cuts',
      'h',
      'hidden',
      'id',
      // the head (lane F's ruling): whether a stone is pressed, where the number starts
      'numberX',
      'slot',
      'squares',
      'stone',
      'uncles',
      'w',
      'x',
      'y',
    ]);
    expect(Object.keys(card.cuts[0]).sort()).toEqual(['id', 's', 'x', 'y']);
    expect(Object.keys(card.squares[0]).sort()).toEqual([
      'container',
      'id',
      'kind',
      's',
      'stage',
      'step',
      'style',
      'x',
      'y',
    ]);
    expect(Object.keys(card.uncles[0]).sort()).toEqual([
      'h',
      'index',
      'label',
      'line',
      'overflow',
      'ref',
      'w',
      'x',
      'y',
    ]);
    expect(Object.keys(scene.next as object).sort()).toEqual([
      'chipOverflow',
      'ghostChips',
      'ghosts',
      'h',
      'hidden',
      'next',
      'number',
      'w',
      'x',
      'y',
    ]);
    const pool = scene.pool as NonNullable<Scene['pool']>;
    expect(Object.keys(pool.pending).sort()).toEqual(['hidden', 'squares', 'total']);
  });
});

describe('uncle row: an uncle travels in the block that carries it, taken out of the field', () => {
  it('is its word on a 12 px line, then one row of its dots at most', () => {
    const per = UNCLE_CHIPS_PER_ROW;
    expect(UNC.head).toBe(12);
    expect(uncleRowH(0)).toBe(UNC.head);
    expect(uncleRowH(1)).toBe(UNC.head + G.chip);
    expect(uncleRowH(per)).toBe(UNC.head + G.chip);
    expect(uncleRowH(per + 1)).toBe(UNC.head + G.chip);
    expect(uncleRowH(per * 5)).toBe(UNC.head + G.chip);
    // No box: its dots are a plate's dot row, nine on the 9 px pitch from the padding.
    expect(per).toBe(CHIPS_PER_ROW);
  });

  it('takes its rows out of the tx field instead of growing the plate: one uncle keeps 2 rows', () => {
    expect(UNC.gap).toBe(5);
    expect(unclesH([])).toBe(0);
    expect(unclesH([2])).toBe(uncleRowH(2) + UNC.gap);
    expect(unclesH([2])).toBe(24);
    // An uncle without proposals is its word alone: one row.
    expect(unclesH([0])).toBe(17);
    // T−1 carries one uncle with two proposals: its field starts below the uncle and still ends
    // inside the fixed plate.
    const n = T - 1;
    const txs = smallTxs(n, 60);
    const card = cardOf(buildScene(withTxs(buildPipelineSnapshot(), n, txs), 1600, 'settled'), n);
    expect(card.h).toBe(CARD_H);
    expect(card.uncles).toEqual([
      expect.objectContaining({ index: 0, label: 'uncle', x: 8, y: G.fieldTop, w: 83, h: 19 }),
    ]);
    const fieldTop = G.fieldTop + 24;
    expect(Math.min(...card.squares.map((q) => q.y))).toBe(fieldTop);
    expect(Math.max(...card.squares.map((q) => q.y + q.s))).toBeLessThanOrEqual(
      G.cardTop + CARD_H - G.padB
    );
    const rows = G.fieldRows - Math.ceil(24 / U);
    expect(rows).toBe(2);
    expect(card.squares).toHaveLength(G.cardCols * rows);
    expect(card.hidden).toBe(1 + txs.length - G.cardCols * rows);
  });

  it('keeps a field row for the cellbase under two uncles, and refuses a plate with no row left', () => {
    const n = T - 8;
    const s = withTxs(buildPipelineSnapshot(), n, smallTxs(n, 60));
    const carrying = (count: number): PipelineSnapshot => ({
      ...s,
      blocks: s.blocks.map((b) =>
        b.number === n
          ? {
              ...b,
              uncles: Array.from({ length: count }, (_, i) => ({
                number: n - 1 - i,
                hash: uncleHash(n - 1 - i),
              })),
              proposals: Array.from({ length: count }, (_, i) =>
                uncleProposals(i, 1, 100 * i)
              ).flat(),
            }
          : b
      ),
    });
    expect(unclesH([1, 1])).toBe(48);
    const card = cardOf(buildScene(carrying(2), 1600, 'settled'), n);
    expect(card.uncles.map((u) => u.y)).toEqual([G.fieldTop, G.fieldTop + 24]);
    // 4 − ⌈48 / 17⌉ = 1 row: the cellbase and four txs.
    expect(card.squares).toHaveLength(G.cardCols);
    expect(new Set(card.squares.map((q) => q.y))).toEqual(new Set([G.fieldTop + 48]));
    expect(() => buildScene(carrying(3), 1600, 'settled')).toThrow(
      new RegExp(`block #${n} carries 3 uncles: its plate keeps no field row for the cellbase`)
    );
  });

  it('fits the plate width, labelled "uncle", with one row of its proposal dots', () => {
    const s = buildPipelineSnapshot();
    // T−1 carries the uncle T−2 with two proposals; give that uncle 30 more.
    const blocks = s.blocks.map((b) =>
      b.number === T - 1 ? { ...b, proposals: [...b.proposals, ...uncleProposals(0, 30, 900)] } : b
    );
    const card = cardOf(buildScene({ ...s, blocks }, 1600, 'settled'), T - 1);
    expect(card.w).toBe(CARD_W);
    // on a plate an uncle is a row of its own, after the dot rows
    expect(card.uncles.map((u) => [u.label, u.line])).toEqual([['uncle', 'own']]);
    const chips = card.chips.filter((k) => k.proposal.uncleIndex === 0);
    // One row is drawn; the rest is its "+N", which ends at the uncle's right end.
    expect(chips).toHaveLength(UNCLE_CHIPS_PER_ROW);
    expect(card.uncles[0].overflow).toBe(32 - UNCLE_CHIPS_PER_ROW);
    expect(card.uncles[0].x + card.uncles[0].w).toBe(card.w - G.padX);
    for (const k of chips) {
      expect(k.x).toBeGreaterThanOrEqual(G.padX);
      expect(k.x + G.chip).toBeLessThanOrEqual(card.w - G.padX);
      expect(k.y).toBe(card.uncles[0].y + UNC.head);
    }
    expect(chips.map((k) => k.x)).toEqual([8, 17, 26, 35, 44, 53, 62, 71, 80]);
  });
});

describe('chainLayout', () => {
  it('stops at the last whole plate that fits the view', () => {
    const slots = chainLayout(24, 1600, false);
    expect(slots).toHaveLength(10);
    expect(slots.length).toBeGreaterThanOrEqual(G.minBlocks);
    const last = slots[slots.length - 1];
    expect(last.x + last.w).toBeLessThanOrEqual(1600 - 10);
    expect(last.x + last.w + G.gap + CARD_W).toBeGreaterThan(1600 - 10);
    expect(slots[0].x).toBe(G.chainX0);
    expect(slots.every((sl, i) => sl.w === CARD_W && sl.x === G.chainX0 + i * PITCH)).toBe(true);
  });

  it.each([
    { width: 606, blocks: 2 },
    { width: 760, blocks: 3 },
    { width: 1000, blocks: 5 },
    { width: CONTAINER_VIEW_W, blocks: 7 },
  ])(
    'shows only whole blocks: $blocks at $width px, 10 px clear of the edge',
    ({ width, blocks }) => {
      const slots = chainLayout(24, width, false);
      expect(slots).toHaveLength(blocks);
      for (const sl of slots) expect(sl.x + sl.w).toBeLessThanOrEqual(width - G.marginR);
      // One more would not fit whole.
      const last = slots[slots.length - 1];
      expect(last.x + last.w + G.gap + CARD_W).toBeGreaterThan(width - G.marginR);
      expect(G.marginR).toBe(10);
    }
  );

  it('never shows fewer than two whole blocks in a row: the 589 px minimum canvas', () => {
    // The narrowest row is the chain start, two whole plates one pitch apart, and the margin.
    expect(G.minViewW).toBe(G.chainX0 + PITCH + CARD_W + G.marginR);
    expect(G.minViewW).toBe(589);
    // Derived from the geometry: the plates that fit from the chain start to the margin.
    expect(G.minBlocks).toBe(Math.floor((G.minViewW - G.marginR - G.chainX0 + G.gap) / PITCH));
    expect(G.minBlocks).toBe(2);
    expect(chainLayout(24, G.minViewW, false)).toHaveLength(G.minBlocks);
    // One pixel less and the second plate would be cut.
    const last = chainLayout(24, G.minViewW, false)[G.minBlocks - 1];
    expect(last.x + last.w).toBe(G.minViewW - G.marginR);
    // The landing block sits at NEXT, beside as many chain blocks as fit.
    const landing = chainLayout(24, G.minViewW, true);
    expect(landing.map((sl) => sl.slot)).toEqual(['landing', 'chain', 'chain']);
  });

  it('lays a 606 px view (the 640–767 band) out as exactly two whole plates, nothing cut', () => {
    const scene = buildScene(buildPipelineSnapshot(), 606, 'settled');
    expect(scene.cards.map((c) => c.block.number)).toEqual([T, T - 1]);
    expect(scene.cards.map((c) => c.x)).toEqual([363, 480]);
    for (const c of scene.cards) {
      expect(c.w).toBe(CARD_W);
      expect(c.x + c.w).toBeLessThanOrEqual(606 - G.marginR);
    }
    // A third would be cut by the edge, so it is not drawn at all.
    const last = scene.cards[scene.cards.length - 1];
    expect(last.x + last.w + G.gap + CARD_W).toBeGreaterThan(606 - G.marginR);
    // Pool wells and NEXT keep their places and widths.
    expect(scene.next?.x).toBe(G.nextX);
    expect(scene.next?.w).toBe(CARD_W);
  });

  it('refuses a row narrower than the minimum canvas instead of clipping its plates', () => {
    const w = G.minViewW;
    expect(() => chainLayout(24, w - 1, false)).toThrow(new RegExp(`${w - 1} px view .* ${w} px`));
  });

  it('fits 7 chain blocks in the 1246 px canvas of a 1280 px page container', () => {
    const slots = chainLayout(24, CONTAINER_VIEW_W, false);
    expect(slots).toHaveLength(7);
    const last = slots[slots.length - 1];
    expect(last.x + last.w).toBe(1164);
    expect(last.x + last.w).toBeLessThanOrEqual(CONTAINER_VIEW_W - 10);
    // An eighth would not fit.
    expect(last.x + last.w + G.gap + CARD_W).toBeGreaterThan(CONTAINER_VIEW_W - 10);
  });

  it('never shows more than 24 plates', () => {
    expect(chainLayout(30, 100_000, false)).toHaveLength(G.maxBlocks);
  });

  it('puts a landing block at the NEXT position and keeps the chain slots of the others', () => {
    const settled = chainLayout(13, 2400, false);
    const landing = chainLayout(14, 2400, true);
    expect(landing[0]).toMatchObject({ index: 0, x: G.nextX, w: CARD_W, slot: 'landing' });
    expect(landing[1].x).toBe(settled[0].x);
    expect(landing[1].index).toBe(1);
  });

  it('starts the chain one plate gap past the NEXT outline', () => {
    expect(G.chainX0).toBe(G.nextX + CARD_W + G.gap);
  });
});

describe('proposal window', () => {
  /** The label of the window of `H` when NEXT is #101. */
  const textOf = (H: number) => windowLabel(H, 100, MAINNET);
  /** NEXT #101 and the chain #100 … #85, each 60 px wide with a 20 px gap. */
  const slots: BlockSpan[] = [
    { number: 101, x: 214, w: 60, mined: false },
    ...Array.from({ length: 16 }, (_, i) => ({
      number: 100 - i,
      x: 400 + i * 80,
      w: 60,
      mined: true,
    })),
  ];

  it('is the heights [H − farthest, H − closest]', () => {
    expect(windowRange(100, MAINNET)).toEqual({ from: 90, to: 98 });
    expect(inWindow(90, 100, MAINNET)).toBe(true);
    expect(inWindow(98, 100, MAINNET)).toBe(true);
    expect(inWindow(99, 100, MAINNET)).toBe(false);
    expect(inWindow(89, 100, MAINNET)).toBe(false);
  });

  // Direction G: no tick and no dimension line. The window's one line of words starts at the left
  // edge of its first plate in view (the one nearest NEXT); the lane's hit area runs on the plates'
  // edges.
  it('starts the words at the first window plate in view, and runs the lane on the plates’ edges', () => {
    const lane = windowLane(slots, 101, MAINNET, 2000, textOf(101));
    // The window of #101 is #91 … #99: #99 at 480 is the first in view, #91 ends at 1180.
    const text = textOf(101);
    // re-pinned (readability R4): 52 glyphs at 12 px, 374.4 px (343.2 at 11)
    expect(labelWidth(text)).toBeCloseTo(374.4, 9);
    expect(lane.label).toEqual({ x0: 480, x1: 480 + labelWidth(text) });
    // the lane: from NEXT (H's own plate) to #90 past the far end; the head line from #99's left
    // edge to #91's right edge
    expect(lane.hit).toEqual({ x0: 214, x1: 1260, xn: 480, xf: 1180 });
    expect(lane).not.toHaveProperty('ticks');
    expect(lane).not.toHaveProperty('tailOpen');
  });

  it('lets the lane run to the view’s edge when the window’s far end is not on screen', () => {
    const chain = slots.filter((s) => s.mined).slice(0, 5);
    const lane = windowLane(chain, 101, MAINNET, 1000, textOf(101));
    expect(lane.hit).toEqual({ x0: 480, x1: 1000, xn: 480, xf: 1000 });
  });

  it('has no words and no lane when no plate of the window is in view', () => {
    const lane = windowLane(
      [{ number: 100, x: 400, w: 60, mined: true }],
      120,
      MAINNET,
      1000,
      textOf(120)
    );
    expect(lane).toEqual({ label: null, hit: null });
    // NEXT is never a plate of a window: a window of NEXT alone has none either
    expect(
      windowLane([{ number: 101, x: 214, w: 60, mined: false }], 103, MAINNET, 1000, textOf(103))
    ).toEqual({ label: null, hit: null });
  });

  it('writes one line naming whose window it is and how many blocks it spans', () => {
    expect(windowLabel(101, 100, MAINNET)).toBe(
      'next block can commit txs proposed in these 9 blocks'
    );
    expect(windowLabel(18_734_755, 18_734_800, MAINNET)).toBe(
      '…4755 could commit txs proposed in these 9 blocks'
    );
  });
});

describe('window label placement: from the first window plate in view, inside the view', () => {
  // The 1246 px canvas: NEXT, then 7 blocks (363, 480 … 1065, the last ending at 1164); the window
  // of a block runs right, towards older ones.
  const W = CONTAINER_VIEW_W;
  const TIP = 18_734_755;
  const NEXT = TIP + 1;
  const slots: BlockSpan[] = [
    { number: NEXT, x: G.nextX, w: CARD_W, mined: false },
    ...chainLayout(8, W, false).map((sl, i) => ({
      number: TIP - i,
      x: sl.x,
      w: sl.w,
      mined: true,
    })),
  ];
  const place = (H: number, view = W) => {
    const text = windowLabel(H, TIP, MAINNET);
    return { text, lane: windowLane(slots, H, MAINNET, view, text) };
  };

  // Measured in Chrome (lane F): the label is drawn in text-meta, so it is measured at the meta
  // step, 12 px since readability R4 (re-pinned: 11 px, 6.6 a glyph, before).
  it('measures a label at the size the canvas draws it: text-meta, 12 px, 0.6 em a glyph', () => {
    expect(SMALL_TEXT_PX).toBe(12);
    expect(tailwindConfig.theme.fontSize.meta[0]).toBe(`${SMALL_TEXT_PX}px`);
    // 49 glyphs of 7.2 px: 352.8 px, and the label's extent is exactly its text, no padding
    const { text, lane } = place(TIP - 2);
    expect(text).toHaveLength(49);
    expect(labelWidth(text)).toBe(352.8);
    const label = lane.label as WindowLabelGeom;
    expect(label.x1 - label.x0).toBeCloseTo(352.8, 9);
    expect([labelWidth('uncle'), labelWidth('+2999'), labelWidth('+3')]).toEqual([36, 36, 14.4]);
  });

  it('starts the label at the left edge of the first window plate in view', () => {
    // NEXT's window is T−9 … T−1: T−1 at 480 is its first plate; 52 glyphs, 374.4 px at 12 px
    // (re-pinned from 343.2), to 854.4
    const next = place(NEXT);
    expect(next.text).toHaveLength(52);
    expect(next.lane.label).toEqual({ x0: 480, x1: 480 + 374.4 });
    // T−2's window is T−12 … T−4: T−4 at 831, and 831 + 352.8 = 1183.8 stays inside 1246
    expect(place(TIP - 2).lane.label).toEqual({ x0: 831, x1: 831 + 352.8 });
  });

  it('ends it at the view’s edge instead when it would pass that edge', () => {
    // T−3's window is T−13 … T−5: T−5 at 948 is its first plate, and 948 + 352.8 = 1300.8 > 1246
    // (re-pinned at 12 px from 323.4 and 922.6)
    const t3 = place(TIP - 3).lane.label as WindowLabelGeom;
    expect(t3.x0).toBeCloseTo(W - 352.8, 9);
    expect(t3.x0).toBeCloseTo(893.2, 9);
    expect(t3.x1).toBe(W);
    // T−4's window starts at T−6 (1065): the same end
    expect((place(TIP - 4).lane.label as WindowLabelGeom).x1).toBe(W);
  });

  it('draws no label when no plate of the window is in view', () => {
    // T−5's window is T−15 … T−7: the row ends at T−6
    expect(place(TIP - 5).lane).toEqual({ label: null, hit: null });
    // The 1246 px row read through a 760 px view: T−4's only plate in it (T−6 at 1065) is past it
    expect(place(TIP - 4, 760).lane).toEqual({ label: null, hit: null });
  });

  it('keeps every window label of the row inside [0, viewW]', () => {
    for (const s of slots) {
      const { text, lane } = place(s.number);
      if (!lane.label) continue;
      expect(lane.label.x0).toBeGreaterThanOrEqual(0);
      expect(lane.label.x1).toBeLessThanOrEqual(W);
      expect(lane.label.x1 - lane.label.x0).toBeCloseTo(labelWidth(text), 9);
    }
  });
});

describe('a narrower view: fewer blocks, all of them whole', () => {
  it.each([589, 606, 760, 1000, CONTAINER_VIEW_W])(
    'at %i px: no plate past the edge, a forward tail, every window label in view',
    (W) => {
      const s = buildPipelineSnapshot();
      const scene = buildScene(s, W, 'settled');
      const next = scene.next as NonNullable<Scene['next']>;
      for (const c of scene.cards) expect(c.x + c.w).toBeLessThanOrEqual(W - G.marginR);
      const last = scene.cards[scene.cards.length - 1];
      // The tail starts at the last plate's edge and runs 66 px, or to the view edge.
      const x0 = last.x + last.w;
      expect(chainTail(scene.cards, W)).toEqual({ x0, x1: x0 + Math.min(66, W - x0) });
      expect(x0).toBeLessThan(W);
      const slots: BlockSpan[] = [
        { number: next.number, x: next.x, w: next.w, mined: false },
        ...scene.cards.map((c) => ({ number: c.block.number, x: c.x, w: c.w, mined: true })),
      ];
      for (const sl of slots) {
        const text = windowLabel(sl.number, scene.windowTip, s.consensus);
        const lane = windowLane(slots, sl.number, s.consensus, W, text);
        if (!lane.label) {
          // Only a window whose near end is past the last plate has no words (and no lane).
          expect(lane.hit).toBeNull();
          expect(sl.number - s.consensus.proposalWindowClosest).toBeLessThan(last.block.number);
          continue;
        }
        expect(lane.label.x0).toBeGreaterThanOrEqual(0);
        expect(lane.label.x1).toBeLessThanOrEqual(W);
        // it starts at its first plate unless that would carry it past the view's edge
        const first = Math.min(
          ...scene.cards
            .filter((c) => inWindow(c.block.number, sl.number, s.consensus))
            .map((c) => c.x)
        );
        expect(lane.label.x0).toBe(Math.min(first, W - labelWidth(text)));
      }
    }
  );
});

describe('chainPaths: the strap', () => {
  // NEXT at 246, then #100 … #85 every 117 px, all linked parent to child.
  const cards: ChainSlotState[] = Array.from({ length: 16 }, (_, i) => ({
    number: 100 - i,
    x: G.chainX0 + i * PITCH,
    w: CARD_W,
    mined: true,
    hash: `h${100 - i}`,
    parentHash: `h${99 - i}`,
    plate: true,
  }));
  const next = { x: G.nextX, w: CARD_W };
  const VIEW = 2400;
  /** The strap across the gap after a plate ending at `end`: the whole 18 px, 26 … 46. */
  const gapStrap = (end: number) => `M${end.toFixed(1)} 26.0h18.0v20.0h-18.0Z`;
  /** The two dashed lines of a strap not yet a block, from `x0` to `x1`. */
  const dashedPair = (x0: number, x1: number) =>
    `M${x0.toFixed(1)} 26.5H${x1.toFixed(1)}M${x0.toFixed(1)} 45.5H${x1.toFixed(1)}`;

  it('fills each link across the whole gap at the strap’s y and height: no tick, no inset', () => {
    const paths = chainPaths(cards, next, 101, MAINNET, VIEW);
    expect(Object.keys(paths).sort()).toEqual(['pend', 'solid', 'tail', 'tailWin', 'win']);
    for (const name of ['solid', 'win', 'tail', 'tailWin'] as const) {
      for (const r of rects(paths[name])) {
        expect([r.y, r.h], name).toEqual([G.strapY, G.strapH]);
      }
    }
    // the tip (363 … 462) → #99 (480 …): one rect from the tip's right edge to #99's left edge
    expect(paths.solid.startsWith(gapStrap(G.chainX0 + CARD_W))).toBe(true);
    expect(rects(paths.solid)[0]).toEqual({ x: 462, y: 26, w: 18, h: 20 });
  });

  it('is two dashed lines from NEXT to the tip, on the strap’s top and bottom pixel rows', () => {
    const paths = chainPaths(cards, next, 101, MAINNET, VIEW);
    // NEXT ends at 345, the tip starts at 363
    expect(paths.pend).toBe('M345.0 26.5H363.0M345.0 45.5H363.0');
    expect(paths.pend).toBe(dashedPair(G.nextX + CARD_W, G.chainX0));
  });

  it('is gilt between two plates inside the window and the plate’s tone outside it', () => {
    const paths = chainPaths(cards, next, 101, MAINNET, VIEW);
    // The window of #101 is #91 … #99: the eight joints between them are gilt.
    expect(rects(paths.win)).toHaveLength(8);
    expect(rects(paths.win).map((r) => r.x)).toEqual(
      [99, 98, 97, 96, 95, 94, 93, 92].map((n) => G.chainX0 + (100 - n) * PITCH + CARD_W)
    );
    // #100 → #99 and #91 → #90 … #86 → #85 lie outside it.
    expect(rects(paths.solid)).toHaveLength(7);
    for (const r of [...rects(paths.win), ...rects(paths.solid)]) expect(r.w).toBe(18);
  });

  it('breaks where the newer plate does not name the older one as its parent', () => {
    const forked = cards.map((c) => (c.number === 95 ? { ...c, parentHash: 'elsewhere' } : c));
    const paths = chainPaths(forked, next, 101, MAINNET, VIEW);
    expect(rects(paths.win)).toHaveLength(7);
    expect(rects(paths.solid)).toHaveLength(7);
    // nothing across the gap after #95
    const after95 = G.chainX0 + 5 * PITCH + CARD_W;
    for (const r of [...rects(paths.win), ...rects(paths.solid)]) expect(r.x).not.toBe(after95);
  });

  it('is dashed from a block still landing, until its plate is set, even inside the window', () => {
    const landing = cards.map((c) => (c.number === 100 ? { ...c, plate: false } : c));
    // no NEXT while a block lands: the landing block's link is the only dashed one
    expect(chainPaths(landing, null, 101, MAINNET, VIEW).pend).toBe(
      dashedPair(G.chainX0 + CARD_W, G.chainX0 + PITCH)
    );
    // with #100 inside the drawn window (that of #103) it is still dashed, never gilt
    const inside = chainPaths(landing, null, 103, MAINNET, VIEW);
    expect(inside.pend).toBe(dashedPair(G.chainX0 + CARD_W, G.chainX0 + PITCH));
    expect(rects(inside.win).map((r) => r.x)).not.toContain(G.chainX0 + CARD_W);
  });

  it('draws nothing across a gap that two moving plates have closed', () => {
    const closing = cards.slice(0, 3).map((c, i) => (i === 2 ? { ...c, x: c.x - PITCH + 50 } : c));
    // #98 slid onto #99: their gap is gone, the tip → #99 link stays
    const paths = chainPaths(closing, null, 200, MAINNET, VIEW);
    expect(rects(paths.solid)).toEqual([{ x: 462, y: 26, w: 18, h: 20 }]);
  });

  it('runs on 66 px past the last plate, from its edge, or to the view edge', () => {
    const last = cards[cards.length - 1];
    // #85 ends at 2217: the tail runs from there for 66 px.
    expect(chainTail(cards, VIEW)).toEqual({ x0: 2217, x1: 2283 });
    const paths = chainPaths(cards, next, 101, MAINNET, VIEW);
    // the window of #101 ends on screen (#91): the tail is the plate's tone
    expect(paths.tail).toBe('M2217.0 26.0h66.0v20.0h-66.0Z');
    expect(paths.tailWin).toBe('');
    expect(rects(paths.tail)[0].x).toBe(last.x + CARD_W);
    // With 23 px of room it stops at the edge.
    expect(chainTail(cards, 2240)).toEqual({ x0: 2217, x1: 2240 });
    expect(chainPaths(cards, next, 101, MAINNET, 2240).tail).toBe('M2217.0 26.0h23.0v20.0h-23.0Z');
    // a plate sliding out past the edge leaves no room: no tail
    expect(chainPaths(cards, next, 101, MAINNET, 2200).tail).toBe('');
    expect(chainTail([], 2000)).toBeNull();
  });

  it('gilds the tail while the window runs on past the last plate on screen', () => {
    // #100 … #96 on screen, NEXT's window running on to #91 past them
    const paths = chainPaths(cards.slice(0, 5), next, 101, MAINNET, VIEW);
    expect(paths.tail).toBe('');
    expect(paths.tailWin).toBe(
      `M${(G.chainX0 + 4 * PITCH + CARD_W).toFixed(1)} 26.0h66.0v20.0h-66.0Z`
    );
    // the window of #96 (#86 … #94) does not reach the last plate (#96): plain
    const plain = chainPaths(cards.slice(0, 5), next, 96, MAINNET, VIEW);
    expect(plain.tailWin).toBe('');
    expect(rects(plain.tail)).toHaveLength(1);
  });

  it('points arrowheads through each gap of the pool: 5 × 8 on a 7 px pitch, centred', () => {
    const flows = flowPaths();
    // mempool (0 … 99) ⇢ proposed (117 …): an 18 px gap holds two, 102 and 109, 5 px long
    expect(flows.toProposed).toBe(
      'M102.0 32.0L107.0 36.0L102.0 40.0ZM109.0 32.0L114.0 36.0L109.0 40.0Z'
    );
    // proposed (117 … 216) ⇢ NEXT (246 …): a 30 px gap holds four, 218 … 244
    const tips = [...flows.toNext.matchAll(/M([\d.]+) 32\.0L([\d.]+) 36\.0L([\d.]+) 40\.0Z/g)];
    expect(tips.map((m) => Number(m[1]))).toEqual([218, 225, 232, 239]);
    expect(tips.every((m) => Number(m[2]) - Number(m[1]) === 5 && m[3] === m[1])).toBe(true);
    expect(tips.map((m) => m[0]).join('')).toBe(flows.toNext);
    // centred on the head line, the strap's middle
    expect(G.headY).toBe(36);
  });
});

describe('rewardLink: a cellbase to the block it pays', () => {
  const a = { X: 390.5, Y: 73.5 };
  const tail = { x0: 1235, x1: 1242 };

  it('drops into the paid cellbase when it is on screen, without a tail', () => {
    const b = { X: 1589.5, Y: 73.5 };
    // Up into the lane at winY (9), a 5 px bend, along the lane, a 5 px bend, down into it.
    expect(rewardLink(a, b, tail)).toEqual({
      d: 'M390.5 73.5V14Q390.5 9 395.5 9H1584.5Q1589.5 9 1589.5 14V73.5',
      tail: '',
    });
  });

  it('runs right along the lane to the chain tail and ends there open', () => {
    const link = rewardLink(a, null, tail);
    expect(link.d).toBe('M390.5 73.5V14Q390.5 9 395.5 9H1235.0');
    // It never drops anywhere: the last command stays in the lane.
    expect(link.d).not.toMatch(/H[\d.]+Q|V[\d.]+$/);
    // The open end: the span of the chain's own tail, one lane up.
    expect(link.tail).toBe(`M1235.0 ${G.winY}H1242.0`);
  });

  it('refuses a tail that starts before the cellbase has turned into the lane', () => {
    expect(() => rewardLink(a, null, { x0: 392, x1: 1242 })).toThrow(/tail at 392/);
  });
});

describe('rewardTarget: where a reward link ends', () => {
  const s = buildPipelineSnapshot();
  const at = (scene: Scene, n: number) => cardOf(scene, n);

  it('is the paid block on screen in a wide view, and off screen in the 1246 px canvas', () => {
    const wide = buildScene(s, 2000, 'settled');
    expect(rewardTarget(wide.cards, at(wide, T))).toBe(at(wide, T - 11));
    const fit = buildScene(s, CONTAINER_VIEW_W, 'settled');
    // tip − 6 is the last block on screen: tip − 11 is past the right edge.
    expect(fit.cards[fit.cards.length - 1].block.number).toBe(T - 6);
    expect(rewardTarget(fit.cards, at(fit, T))).toBe('off');
  });

  it('is off screen too when the paid block is not in the snapshot at all', () => {
    const scene = buildScene(s, CONTAINER_VIEW_W, 'settled');
    const c = at(scene, T);
    const pays = { number: T - 40, hash: '0xolder', reward: rewardOf(T - 40) };
    const far = { ...c, block: { ...c.block, cellbase: { ...c.block.cellbase, pays } } };
    expect(rewardTarget(scene.cards, far)).toBe('off');
  });

  it('is nothing for a cellbase that pays no block yet', () => {
    const scene = buildScene(s, CONTAINER_VIEW_W, 'settled');
    const c = at(scene, T);
    const none = { ...c, block: { ...c.block, cellbase: { ...c.block.cellbase, pays: null } } };
    expect(rewardTarget(scene.cards, none)).toBeNull();
  });

  it('rejects a paid block that should be on screen but is not', () => {
    const blocks = s.blocks.map((b) =>
      b.number === T
        ? {
            ...b,
            cellbase: {
              ...b.cellbase,
              pays: { number: T - 3, hash: '0xelsewhere', reward: rewardOf(T - 3) },
            },
          }
        : b
    );
    expect(() => buildScene({ ...s, blocks }, CONTAINER_VIEW_W, 'settled')).toThrow(
      /pays 0xelsewhere as #\d+, but #\d+ on screen is 0x/
    );
  });

  it('rejects a paid block inside the range on screen that has no plate', () => {
    const scene = buildScene(s, CONTAINER_VIEW_W, 'settled');
    const c = at(scene, T);
    const pays = { number: T - 3, hash: at(scene, T - 3).id, reward: rewardOf(T - 3) };
    const payer = { ...c, block: { ...c.block, cellbase: { ...c.block.cellbase, pays } } };
    const gap = scene.cards.filter((k) => k.block.number !== T - 3);
    expect(() => rewardTarget(gap, payer)).toThrow(
      new RegExp(`pays #${T - 3}, which should be on screen \\(down to #${T - 6}\\) but is not`)
    );
  });

  it('rejects a paid block on screen under another number', () => {
    const blocks = s.blocks.map((b) =>
      b.number === T
        ? {
            ...b,
            cellbase: {
              ...b.cellbase,
              pays: { number: T - 30, hash: b.parentHash, reward: rewardOf(T - 30) },
            },
          }
        : b
    );
    expect(() => buildScene({ ...s, blocks }, CONTAINER_VIEW_W, 'settled')).toThrow(
      /pays 0x\w+ as #\d+, but that block is #\d+/
    );
  });

  it('rejects a cellbase that pays a block that is not older than its own', () => {
    const blocks = s.blocks.map((b) =>
      b.number === T - 1
        ? {
            ...b,
            cellbase: {
              ...b.cellbase,
              pays: { number: T, hash: s.blocks[0].hash, reward: rewardOf(T) },
            },
          }
        : b
    );
    expect(() => buildScene({ ...s, blocks }, CONTAINER_VIEW_W, 'settled')).toThrow(
      /pays #\d+, which is not an earlier block/
    );
  });
});

describe('pick: magnetic hit testing', () => {
  const targets: PickTargets = {
    squares: [
      { id: 'a', X: 100, Y: 100, s: 6 },
      { id: 'b', X: 104, Y: 100, s: 13 },
      { id: 'far', X: 300, Y: 100, s: 6 },
    ],
    chips: [{ card: 'blk', key: 'chip0', X: 200, Y: 80 }],
  };

  it('prefers the square under the pointer (2 px margin), topmost first', () => {
    expect(pick(targets, 105, 103)).toEqual({ kind: 'tx', id: 'b' });
    expect(pick(targets, 99, 101)).toEqual({ kind: 'tx', id: 'a' });
    expect(pick(targets, 301 - 2.5, 101)).toEqual({ kind: 'tx', id: 'far' });
  });

  it('falls back to the nearest square within 8 px', () => {
    expect(pick(targets, 300 + 6 + 7, 103)).toEqual({ kind: 'tx', id: 'far' });
    expect(pick(targets, 300 + 6 + 9, 103)).toBeNull();
  });

  it('reports a predicted (ghost) square as a ghost hit', () => {
    const withGhost: PickTargets = {
      squares: [...targets.squares, { id: 'g', X: 400, Y: 100, s: 6, ghost: true }],
      chips: [],
    };
    expect(pick(withGhost, 403, 103)).toEqual({ kind: 'ghost', id: 'g' });
    expect(pick(withGhost, 400 + 6 + 5, 103)).toEqual({ kind: 'ghost', id: 'g' });
  });

  it('snaps to a proposal dot within 4 px of its centre', () => {
    const cx = 200 + G.chip / 2;
    const cy = 80 + G.chip / 2;
    expect(pick(targets, cx + 3, cy)).toEqual({ kind: 'chip', card: 'blk', key: 'chip0' });
    expect(pick(targets, cx + 5, cy)).toBeNull();
  });
});

describe('chipSlots: proposal dots that fit, with room for "+N"', () => {
  it('shows every chip when they fit', () => {
    expect(chipSlots(48, 48)).toEqual({ shown: 48, overflow: 0 });
    expect(chipSlots(0, 48)).toEqual({ shown: 0, overflow: 0 });
  });

  it('frees one dot slot per label glyph at the end of the last row', () => {
    // "+255" is 4 glyphs of 6.6 px, each under one dot pitch.
    expect(chipSlots(299, 48)).toEqual({ shown: 44, overflow: 255 });
    // A one-digit "+N" takes two slots.
    expect(chipSlots(51, 48)).toEqual({ shown: 46, overflow: 5 });
    expect(chipSlots(49, 48)).toEqual({ shown: 46, overflow: 3 });
    const { shown, overflow } = chipSlots(1500, 48);
    expect(shown + overflow).toBe(1500);
    expect(48 - shown).toBeGreaterThanOrEqual(String(overflow).length + 1);
    // A plate's two rows of nine: T−7's 299 proposals show 14 and "+285".
    expect(chipSlots(299, CHIPS_PER_ROW * G.propRows)).toEqual({ shown: 14, overflow: 285 });
  });
});

describe('NEXT card from next.txs', () => {
  const withNext = (next: NonNullable<PipelineSnapshot['next']>): PipelineSnapshot => ({
    ...buildPipelineSnapshot(),
    next,
  });

  it('draws one ghost per predicted tx, sized by its own bytes, in the backend order', () => {
    const s = buildPipelineSnapshot();
    const next = buildScene(s, 1600, 'settled').next as NonNullable<
      ReturnType<typeof buildScene>['next']
    >;
    const ghosts = next.ghosts.filter((g) => !g.cellbase);
    expect(ghosts.map((g) => g.id).sort()).toEqual(
      (s.next as NonNullable<PipelineSnapshot['next']>).txs.map((t) => t.hash).sort()
    );
    expect(next.ghosts.filter((g) => g.cellbase)).toEqual([
      { id: null, x: G.padX, y: G.fieldTop, s: side(1), cellbase: true },
    ]);
    expect(next.hidden).toBe(0);
    // Its predicted proposals are drawn where a plate's dots are.
    expect(next.ghostChips[0]).toEqual({ x: G.padX, y: G.propTop });
  });

  it('draws a predicted tx that is not in the truncated pool list, and counts omitted ones as +N', () => {
    const outside = `0x${'ab'.repeat(32)}`;
    const scene = buildScene(
      withNext({
        txs: [
          { hash: outside, size: 9000 },
          { hash: poolTxHash('w0'), size: 330 },
        ],
        omittedTxs: 3,
        bytes: 9330,
        cycles: 5_000_000,
        proposals: 5,
      }),
      1600,
      'settled'
    );
    const next = scene.next as NonNullable<typeof scene.next>;
    const big = next.ghosts.find((g) => g.id === outside);
    // A 4 × 4 fills the field's four rows and leaves one column, where the 1×1 drops in.
    expect(big).toMatchObject({ x: G.padX + U, y: G.fieldTop, s: side(4) });
    expect(next.ghosts.find((g) => g.id === poolTxHash('w0'))).toMatchObject({
      x: G.padX,
      y: G.fieldTop + U,
      s: side(1),
    });
    expect(next.hidden).toBe(3);
  });

  it('rejects a negative omittedTxs', () => {
    expect(() =>
      buildScene(
        withNext({ txs: [], omittedTxs: -1, bytes: 0, cycles: 0, proposals: 0 }),
        1600,
        'settled'
      )
    ).toThrow(/omittedTxs/);
  });
});

describe('the column: the pipeline stood on end below the 589 px row', () => {
  /** The phone reference: a 390 px viewport less its 16 px gutters (no panel frame). */
  const PHONE_W = 358;
  const quiet = withSmallTxs(buildPipelineSnapshot());
  const scene = buildScene(quiet, PHONE_W, 'settled');
  const bands = scene.bands as NonNullable<Scene['bands']>;
  /** Slots along the rows: a row's top and height. */
  const slotsOf = (sc: Scene): ChainSlotState[] =>
    sc.cards.map((c) => ({
      number: c.block.number,
      x: c.y,
      w: c.h,
      mined: true,
      hash: c.block.hash,
      parentHash: c.block.parentHash,
      plate: true,
    }));
  const nextSlot = (sc: Scene): BlockSpan => {
    const n = sc.next as NonNullable<Scene['next']>;
    return { number: n.number, x: n.y, w: n.h, mined: false };
  };
  /** The strap down a column gap [y0, y1]: x 5 … 25, the whole gap. */
  const colStrap = (y0: number, y1: number) =>
    `M5.0 ${y0.toFixed(1)}h20.0v${(y1 - y0).toFixed(1)}h-20.0Z`;

  it('is picked by the measured width: the column below G.minViewW, the row from it', () => {
    expect(buildScene(quiet, G.minViewW - 1, 'settled').orientation).toBe('column');
    expect(buildScene(quiet, G.minViewW, 'settled').orientation).toBe('row');
    expect(buildScene(quiet, G.minViewW, 'settled').H).toBe(G.H);
    expect(buildScene(quiet, G.minViewW, 'settled').bands).toBeNull();
  });

  it('lays 358 px out as seven 44 px rows: mempool, proposed, NEXT, the tip and three older', () => {
    expect(scene.orientation).toBe('column');
    expect([COL.x0, COL.top]).toEqual([0, 0]);
    // Gaps: 18 between the wells, 30 between the pool and NEXT, 18 from NEXT on (the board).
    expect([COL.wellGap, COL.frontierGap, COL.blockGap]).toEqual([18, 30, 18]);
    expect(bands.pending).toEqual({ y: 0, h: 44 });
    expect(bands.proposed).toEqual({ y: 62, h: 44 });
    expect(bands.next).toEqual({ y: 136, h: 44 });
    expect(scene.next).toMatchObject({ number: T + 1, x: 0, y: 136, w: 358, h: 44 });
    expect(scene.cards.map((c) => c.block.number)).toEqual([T, T - 1, T - 2, T - 3]);
    expect(scene.cards).toHaveLength(COL.blocks);
    expect(scene.cards.map((c) => c.y)).toEqual([198, 260, 322, 384]);
    for (const c of scene.cards) expect(c).toMatchObject({ x: 0, w: 358, h: 44, slot: 'chain' });
    // The last row ends at 428; the 34 px below it hold the tail's two bands.
    expect(COL.bottom).toBe(34);
    expect(scene.H).toBe(462);
  });

  it('grows a row only for a 3×3 or 4×4 square, and the canvas with it', () => {
    // Two cells and 6 px above and below: 44, then 61 and 78.
    expect([1, 2, 3, 4].map(columnRowH)).toEqual([44, 44, 61, 78]);
    // The full fixture's tip commits a 9.6 KB tx: a 4×4 square, so its row is 78 px.
    const full = buildScene(buildPipelineSnapshot(), PHONE_W, 'settled');
    expect(full.cards.map((c) => c.h)).toEqual([78, 44, 44, 44]);
    expect(full.cards.map((c) => c.y)).toEqual([198, 294, 356, 418]);
    expect(full.H).toBe(496);
    const big = cardOf(full, T).squares.find((q) => q.s === side(4));
    // Centred in its row: 6 px above and below.
    expect(big?.y).toBe(6);
  });

  it('draws the tx line from x 126: the cellbase, then squares at 17 px per cell, centred', () => {
    for (const c of scene.cards) {
      const [cb, ...txs] = c.squares;
      expect(cb).toMatchObject({ kind: 'cellbase', x: COL.lineX - COL.x0, y: 14, s: side(1) });
      expect(c.x + cb.x).toBe(126);
      let x = COL.lineX - COL.x0 + U;
      for (const q of txs) {
        // Side by side, each taking its own cells, each centred on the row.
        expect(q.x).toBe(x);
        expect(q.y).toBe(Math.floor((c.h - q.s) / 2));
        x += q.s + GUT;
      }
    }
    // The tip: three 2×2 then a 1×1, biggest first.
    expect(cardOf(scene, T).squares.map((q) => q.s)).toEqual([15, 32, 32, 32, 15]);
    // 13 cells to a line at 358 px: from 126 to 8 px before the edge.
    expect(Math.floor((PHONE_W - COL.padR - COL.lineX + GUT) / U)).toBe(13);
  });

  it('sets the stone at x 8 and the number at x 27, a pool name at x 8 and its total ending at 108', () => {
    expect([COL.stoneX, COL.numberX, COL.nameX, COL.countX]).toEqual([8, 27, 8, 108]);
  });

  // Lane P (PlatePhone.dc.html `.st`, `.no`: centred on the row): the head from the library.
  it('centres a row’s stone and words on its tx line, never on a row grown by its uncles’ line', () => {
    // a 44 px row: the 14 px stone at (8, 15), its words on the middle, 22, + 3.5
    expect(columnHeadAt(cardOf(scene, T))).toEqual({ stoneX: 8, stoneY: 15, y: 25.5 });
    // a 3 × 3 or 4 × 4 line: the stone on the whole pixel, as a square is
    expect(columnHeadAt({ h: 61, uncles: [] })).toEqual({ stoneX: 8, stoneY: 23, y: 34 });
    expect(columnHeadAt({ h: 78, uncles: [] })).toEqual({ stoneX: 8, stoneY: 32, y: 42.5 });
    // Two uncles on a line of their own: the row is 60 px, its head stays on the 44 px tx line,
    // beside the cellbase (within half a pixel: a 15 px mark on a 44 px line sits on 14).
    const two = cardOf(
      buildScene(withSecondUncle(hostWith(smallTxs(T - 1, 2), 0, 10), 3), PHONE_W, 'settled'),
      T - 1
    );
    expect(two.h).toBe(60);
    const head = columnHeadAt(two);
    expect(head).toEqual(columnHeadAt({ h: 44, uncles: [] }));
    const cb = two.squares[0];
    expect(Math.abs(head.stoneY + G.stone / 2 - (cb.y + cb.s / 2))).toBeLessThanOrEqual(0.5);
    expect(head.y).toBe(columnCountAt(two).y);
  });

  it('sets a well across the canvas: square, its 1 px edge on its outer pixels, the inset under it', () => {
    expect(columnWellRect(PHONE_W, bands.pending)).toEqual({
      x: 0.5,
      y: 0.5,
      width: 357,
      height: 43,
    });
    expect(columnWellRect(PHONE_W, bands.proposed)).toEqual({
      x: 0.5,
      y: 62.5,
      width: 357,
      height: 43,
    });
    // the theme's inset: 3 px inside the edge, under its top
    expect(columnWellShade(PHONE_W, bands.proposed)).toEqual({
      x: 1,
      y: 63,
      width: 356,
      height: G.wellShade,
    });
    expect(G.wellShade).toBe(3);
    // its name from x 8 and its total ending at 108, on the middle of its band
    expect(columnWellHeadAt(bands.proposed)).toEqual({ nameX: 8, countX: 108, y: 62 + 22 + 3.5 });
    // at the narrowest column too
    expect(columnWellRect(COL.minViewW, bands.pending).width).toBe(COL.minViewW - 1);
  });

  it('puts the proposal dots at 74, 83, 92 … on the row centre, five a row, two rows, then +N', () => {
    const tip = cardOf(scene, T);
    expect(tip.chips.map((k) => tip.x + k.x)).toEqual([74, 83, 92]);
    for (const k of tip.chips) expect(k.y + G.chip / 2).toBe(tip.h / 2);
    // Many proposals: two rows of five on the 9 px pitch, centred together, and "+N" at the end of
    // the second row giving up one slot per glyph.
    const many = Array.from({ length: 40 }, (_, i) => ({
      shortId: shortIdOf(committedTxHash(T - 2, 500 + i)),
      uncleIndex: null,
      state: 'waiting' as const,
      txHash: null,
      committedBlock: null,
    }));
    const blocks = quiet.blocks.map((b) => (b.number === T - 2 ? { ...b, proposals: many } : b));
    const card = cardOf(buildScene({ ...quiet, blocks }, PHONE_W, 'settled'), T - 2);
    expect(card.chips).toHaveLength(10 - 3);
    expect(card.chipOverflow).toBe(33);
    // 7 + 2 + 7 = 16 px of dots, centred on the 44 px row: rows at 14 and 23.
    expect(COL.chipRowP).toBe(9);
    expect(new Set(card.chips.map((k) => k.y))).toEqual(new Set([14, 23]));
    expect(card.chips.slice(0, 5).map((k) => card.x + k.x)).toEqual([74, 83, 92, 101, 110]);
    const at = columnChipCountAt(card);
    expect(card.x + at.x).toBe(117);
    expect(at.y).toBe(23 + G.chip / 2 + 3.5);
    // The label's three glyphs fit the freed slots, never over a dot.
    const lastDot = Math.max(...card.chips.filter((k) => k.y === 23).map((k) => k.x + G.chip));
    expect(at.x - labelWidth('+33')).toBeGreaterThanOrEqual(lastDot);
  });

  it('ends the tx line with an uncle: its word, then its dots, no box, out of the line’s room', () => {
    // T−1 carries the uncle T−2 and two of its proposals.
    const host = cardOf(scene, T - 1);
    // The word is as wide as it is drawn: "uncle" in the canvas's small text, 36 px at 12 px
    // (re-pinned, readability R4: 33 at 11).
    expect(COL.uncle).toEqual({ labelW: 36, chips: 4 });
    expect(COL.uncle.labelW).toBe(labelWidth('uncle'));
    // The word, 4 px, then four dot slots (34 px): 74 px (`w` below; 71 at 11 px). Room is left
    // after the line's reservation (224 − (83 + 4 + 14.4) = 122.6 ≥ 4 + 74): it follows the tx
    // line, as on the phone board.
    expect(host.uncles).toEqual([
      expect.objectContaining({
        index: 0,
        label: 'uncle',
        line: 'tx',
        x: 276,
        y: 16,
        w: 74,
        h: 12,
        overflow: 0,
      }),
    ]);
    const un = host.uncles[0];
    expect(host.x + un.x + un.w).toBe(PHONE_W - COL.padR);
    const dots = host.chips.filter((k) => k.proposal.uncleIndex === 0);
    expect(dots.map((k) => k.x)).toEqual([un.x + 40, un.x + 49]);
    for (const k of dots) expect(k.y + G.chip / 2).toBe(host.h / 2);
    // Its own two proposals stay in the dot zone; no square reaches the uncle.
    expect(host.chips.filter((k) => k.proposal.uncleIndex === null).map((k) => k.x)).toEqual([
      74, 83,
    ]);
    for (const q of host.squares) expect(q.x + q.s).toBeLessThanOrEqual(un.x - COL.endGap);
    expect(columnCountAt(host).x).toBe(un.x - COL.endGap);
  });

  /** T−1 of the quiet fixture with `txs` listed and `omitted` more, its uncle given `more` proposals. */
  const hostWith = (txs: PipelineTx[], omitted: number, more: number): PipelineSnapshot => {
    const n = T - 1;
    const s = withTxs(quiet, n, txs, omitted);
    return {
      ...s,
      blocks: s.blocks.map((b) =>
        b.number === n ? { ...b, proposals: [...b.proposals, ...uncleProposals(0, more, 3000)] } : b
      ),
    };
  };
  /** A 4×4 tx of block `n`, then `small` 1×1 ones. */
  const bigAnd = (n: number, small: number): PipelineTx[] => [
    { ...smallTxs(n, small + 1)[small], hash: committedTxHash(n, 777), size: 9600 },
    ...smallTxs(n, small),
  ];
  /** `s` with a second uncle on T−1, carrying `count` proposals. */
  const withSecondUncle = (s: PipelineSnapshot, count: number): PipelineSnapshot => ({
    ...s,
    blocks: s.blocks.map((b) =>
      b.number === T - 1
        ? {
            ...b,
            uncles: [...b.uncles, { number: T - 3, hash: uncleHash(T - 3) }],
            proposals: [...b.proposals, ...uncleProposals(1, count, 5000)],
          }
        : b
    ),
  });
  /**
   * The widest block the chain allows on T−1: 3,000 txs (the backend lists a 4×4 and 38 more, its
   * "+N" at most "+2999") and two uncles of 1,500 proposals each ("+1500").
   */
  const widest = withSecondUncle(hostWith(bigAnd(T - 1, 38), 2960, 1498), 1500);
  /** Each uncle's proposals: drawn as dots and counted as "+N", against what the block carries. */
  const uncleTally = (sc: Scene) =>
    sc.cards.flatMap((c) =>
      c.uncles.map((u) => ({
        drawn: c.chips.filter((k) => k.proposal.uncleIndex === u.index).length,
        counted: u.overflow,
        total: c.block.proposals.filter((p) => p.uncleIndex === u.index).length,
      }))
    );

  it('keeps the cellbase, a 4 × 4’s room and the widest "+N" on the line; the uncles follow it only if all fit', () => {
    // T−1: a 4×4, 30 more txs (its "+N" at most "+31") and its uncle with 12 proposals. The line
    // keeps side(5) + 4 + 21.6 = 108.6 px before any uncle (re-pinned, readability R4: the canvas's small text is 12 px, "uncle" and "+1500" 36 px, was 33).
    const busy = hostWith(bigAnd(T - 1, 30), 0, 10);
    // At 358 the line has 224 px: 115.4 are left after the reservation, room for the uncle
    // (4 + 74), which follows the line as on the phone board; the row stays the 4×4's 78 px.
    const wide = cardOf(buildScene(busy, PHONE_W, 'settled'), T - 1);
    expect(wide.uncles).toEqual([
      expect.objectContaining({ line: 'tx', x: 276, y: 39 - 6, w: 74, h: 12, overflow: 11 }),
    ]);
    expect(wide.h).toBe(78);
    expect(wide.squares.map((q) => q.s)).toEqual([15, 66, 15, 15]);
    expect(wide.hidden).toBe(28);
    expect(columnCountAt(wide).x).toBe(276 - COL.endGap);
    // At 288 only 45.4 px are left: the uncle moves under the line, from x 126, and the row grows
    // by its 12 px line and a 4 px gap. The line keeps its 4 × 4.
    expect(COL.uncleGap).toBe(4);
    const narrow = cardOf(buildScene(busy, COL.minViewW, 'settled'), T - 1);
    expect(narrow.uncles).toEqual([
      expect.objectContaining({ line: 'own', x: 126, y: 6 + 66 + 4, w: 74, h: 12, overflow: 11 }),
    ]);
    expect(narrow.h).toBe(78 + 4 + 12);
    expect(columnLineH(narrow)).toBe(78);
    expect(narrow.squares.map((q) => q.s)).toEqual([15, 66, 15, 15]);
    expect(narrow.hidden).toBe(28);
    // its dot sits on its own line, centred on the word's
    expect(narrow.chips.filter((k) => k.proposal.uncleIndex === 0)).toEqual([
      expect.objectContaining({ x: 126 + 40, y: 76 + (12 - G.chip) / 2 }),
    ]);
    // the line, its "+N", the head and the block's own dots keep the line's middle (39)
    expect(columnCountAt(narrow)).toEqual({ x: COL.minViewW - COL.padR, y: 39 + 3.5 });
    for (const k of narrow.chips.filter((c) => c.proposal.uncleIndex === null))
      expect(k.y + G.chip / 2).toBe(39);
    expect(columnChipCountAt(narrow).y).toBe(columnChipCountAt({ h: 78, uncles: [] }).y);
    for (const q of narrow.squares) expect(q.y + q.s / 2).toBeLessThanOrEqual(39 + 0.5);
  });

  it('keeps an uncle’s place whatever txs are listed: the reservation depends on the width and the count', () => {
    // Two listings of one 32-tx block: with a 4×4, and without one.
    const withBig = hostWith(bigAnd(T - 1, 30), 0, 10);
    const smallOnly = hostWith(smallTxs(T - 1, 31), 0, 10);
    for (const W of [COL.minViewW, PHONE_W]) {
      const a = cardOf(buildScene(withBig, W, 'settled'), T - 1).uncles[0];
      const b = cardOf(buildScene(smallOnly, W, 'settled'), T - 1).uncles[0];
      expect([b.line, b.x, b.w]).toEqual([a.line, a.x, a.w]);
    }
    // At 288 the uncle is under the line in both, though the second lists no 4 × 4.
    const lone = cardOf(buildScene(smallOnly, COL.minViewW, 'settled'), T - 1);
    expect(lone.uncles[0]).toMatchObject({ line: 'own', x: 126, y: 6 + 32 + 4 });
    expect(lone.h).toBe(44 + 16);
  });

  it('draws an uncle as its word and its dots, or its word and "+N" when four slots cannot hold the label', () => {
    // The word (36 px (re-pinned, readability R4: the canvas's small text is 12 px, "uncle" and "+1500" 36 px, was 33)), 4 px, then four dot slots (34 px); "+999" still fits the
    // slots, "+1500" needs five, so it follows the word, 4 px after it. Never the word alone while
    // it has proposals.
    const uncleOf = (s: PipelineSnapshot, n: number) =>
      cardOf(buildScene(s, PHONE_W, 'settled'), n);
    // two proposals: both dots in the four slots
    expect(uncleOf(quiet, T - 1).uncles[0]).toMatchObject({ w: 36 + 4 + 34, overflow: 0 });
    // no proposals: the word alone (an uncle T−2 carries, which proposed nothing)
    const bare: PipelineSnapshot = {
      ...quiet,
      blocks: quiet.blocks.map((b) =>
        b.number === T - 2 ? { ...b, uncles: [{ number: T - 3, hash: uncleHash(T - 3) }] } : b
      ),
    };
    expect(uncleOf(bare, T - 2).uncles[0]).toMatchObject({ w: 36, overflow: 0 });
    // 999 proposals: "+999" in the four slots, no dot
    const nines = uncleOf(hostWith(smallTxs(T - 1, 2), 0, 997), T - 1);
    expect(nines.uncles[0]).toMatchObject({ w: 36 + 4 + 34, overflow: 999 });
    expect(nines.chips.filter((k) => k.proposal.uncleIndex === 0)).toEqual([]);
    expect(labelWidth('+1500')).toBe(36);
    const crowd = cardOf(
      buildScene(hostWith(smallTxs(T - 1, 2), 0, 1498), PHONE_W, 'settled'),
      T - 1
    );
    expect(crowd.uncles).toEqual([
      expect.objectContaining({ line: 'tx', x: 274, w: 36 + 4 + 36, overflow: 1500 }),
    ]);
    expect(crowd.chips.filter((k) => k.proposal.uncleIndex === 0)).toEqual([]);
  });

  it('puts two uncles on a line of their own under the tx line, and the row grows by 16 px', () => {
    // T−1 carries two uncles: 12 proposals and 3. Two uncles with dots need 2 · (4 + 74) = 156 px
    // after the line's 101.4 (re-pinned, readability R4: the canvas's small text is 12 px, "uncle" and "+1500" 36 px, was 33): more than a phone has, so they go under the line at 288
    // and at 358, both on one line (74 + 4 + 74 = 152 of the 154 at 288).
    const two = withSecondUncle(hostWith(smallTxs(T - 1, 2), 0, 10), 3);
    for (const W of [COL.minViewW, PHONE_W]) {
      const sc = buildScene(two, W, 'settled');
      const card = cardOf(sc, T - 1);
      expect(card.uncles).toEqual([
        expect.objectContaining({
          index: 0,
          line: 'own',
          x: 126,
          y: 42,
          w: 74,
          h: 12,
          overflow: 11,
        }),
        expect.objectContaining({
          index: 1,
          line: 'own',
          x: 204,
          y: 42,
          w: 74,
          h: 12,
          overflow: 0,
        }),
      ]);
      expect(card.h).toBe(60);
      // the tx line holds everything it held without uncles
      expect(card.squares.map((q) => q.s)).toEqual([15, 15, 15]);
      expect(card.hidden).toBe(0);
      expect(card.chips.filter((k) => k.proposal.uncleIndex === 1).map((k) => k.x)).toEqual([
        244, 253, 262,
      ]);
      // the rows below step down by the 16 px
      expect(sc.cards.map((c) => c.y)).toEqual([198, 260, 338, 400]);
      expect(sc.H).toBe(400 + 44 + 34);
    }
    // A wrapped row is the line's height and 16 px: 60, 77 or 78 + 16 = 94.
    const tall = (size: number) =>
      cardOf(
        buildScene(
          withSecondUncle(hostWith([{ ...smallTxs(T - 1, 1)[0], size }], 0, 10), 3),
          COL.minViewW,
          'settled'
        ),
        T - 1
      ).h;
    expect([tall(330), tall(5400), tall(9600)]).toEqual([60, 77, 94]);
  });

  it('holds at 288 px with the widest block: the cellbase, a 4 × 4 and "+N" on the line, two uncles under it', () => {
    // "+2999" and "+1500" are 36 px (re-pinned, readability R4: the canvas's small text is 12 px, "uncle" and "+1500" 36 px, was 33); an uncle of 1,500 proposals is its word, 4 px and
    // "+1500": 76 px.
    expect(COL.minViewW).toBe(288);
    // (its 76 px: `w` below)
    expect([labelWidth('+2999'), labelWidth('+1500')]).toEqual([36, 36]);
    const card = cardOf(buildScene(widest, COL.minViewW, 'settled'), T - 1);
    expect(card.squares[0].kind).toBe('cellbase');
    // re-pinned (readability R4): its "+N" is 36 px at 12 px (33 at 11), so the line holds one
    // small square fewer (was 15, 66, 15, 15 and 2,996 counted)
    expect(card.squares.map((q) => q.s)).toEqual([15, 66, 15]);
    expect(card.hidden).toBe(2997);
    expect(card.squares.length + card.hidden).toBe(card.block.txCount);
    // The line ends in its "+N", which never touches a square.
    const count = columnCountAt(card);
    expect(count.x).toBe(COL.minViewW - COL.padR);
    const end = Math.max(...card.squares.map((q) => q.x + q.s));
    expect(end + COL.endGap).toBeLessThanOrEqual(count.x - labelWidth(`+${card.hidden}`));
    // Both uncles, word and "+1500", under the line: together they need 76 + 4 + 76 = 156 px,
    // more than the 154 the narrowest column's line has (re-pinned, readability R4: at 11 px they
    // stood on one line, 126 … 196 and 200 … 270), so each takes a line of its own, 126 … 202,
    // and the row grows by two 16 px lines (was one).
    expect(card.uncles).toEqual([
      expect.objectContaining({ line: 'own', x: 126, w: 76, overflow: 1500 }),
      expect.objectContaining({ line: 'own', x: 126, w: 76, overflow: 1500 }),
    ]);
    expect(card.uncles[1].x + card.uncles[1].w).toBeLessThanOrEqual(COL.minViewW - COL.padR);
    expect(card.h).toBe(78 + 16 + 16);
    expect(card.uncles[1].y - card.uncles[0].y).toBe(16);
    expect(card.uncles[1].y + card.uncles[1].h).toBe(card.h - COL.pad);
    // With their dots (two uncles of 999 proposals) they fit the same line: 126 … 278.
    const dotted = cardOf(
      buildScene(
        withSecondUncle(hostWith(bigAnd(T - 1, 38), 2960, 997), 999),
        COL.minViewW,
        'settled'
      ),
      T - 1
    );
    expect(dotted.uncles.map((u) => [u.line, u.x, u.w])).toEqual([
      ['own', 126, 74],
      ['own', 204, 74],
    ]);
  });

  it('never refuses a valid block from 288 px up: every row whole, every uncle inside it', () => {
    const snaps = [widest, hostWith(bigAnd(T - 1, 38), 2960, 1498), buildPipelineSnapshot(), quiet];
    for (const snap of snaps)
      for (let W = COL.minViewW; W < G.minViewW; W++) {
        const sc = buildScene(snap, W, 'settled');
        for (const c of sc.cards) {
          for (const u of c.uncles) {
            expect(u.x).toBeGreaterThanOrEqual(COL.lineX - COL.x0);
            expect(u.x + u.w).toBeLessThanOrEqual(c.w - COL.padR);
            expect(u.y).toBeGreaterThanOrEqual(COL.pad);
            expect(u.y + u.h).toBeLessThanOrEqual(c.h - COL.pad);
            if (u.line === 'tx')
              for (const q of c.squares) expect(q.x + q.s).toBeLessThanOrEqual(u.x - COL.endGap);
            else
              for (const q of c.squares) expect(q.y + q.s).toBeLessThanOrEqual(u.y - COL.uncleGap);
          }
        }
      }
  });

  it('draws or counts every proposal of every uncle: drawn + counted = total, in a row and a column', () => {
    const snaps = [
      buildPipelineSnapshot(),
      quiet,
      hostWith(bigAnd(T - 1, 30), 0, 40),
      hostWith(smallTxs(T - 1, 2), 0, 1498),
      withSecondUncle(hostWith(smallTxs(T - 1, 2), 0, 10), 3),
      widest,
    ];
    for (const snap of snaps)
      for (const W of [COL.minViewW, 300, PHONE_W, G.minViewW - 1, 1600]) {
        const tally = uncleTally(buildScene(snap, W, 'settled'));
        expect(tally.length).toBeGreaterThan(0);
        for (const t of tally) {
          expect(t.drawn + t.counted, `${W} px`).toBe(t.total);
          // an uncle with proposals always shows them: some as dots, or its "+N"
          if (t.total) expect(t.drawn > 0 || t.counted > 0).toBe(true);
        }
      }
  });

  it('counts what a block line cannot hold as +N at its end, never over a square', () => {
    const n = T - 2;
    const txs = smallTxs(n, 60);
    const card = cardOf(buildScene(withTxs(quiet, n, txs, 7), PHONE_W, 'settled'), n);
    const drawn = card.squares.length - 1;
    // 224 px: the cellbase and 11 txs (202 px), 4 px and "+56" (19.8 px) would take 225.8 px.
    expect(drawn).toBe(10);
    // Hidden: the listed txs the line could not hold plus the ones the backend omitted.
    expect(card.hidden).toBe(60 - drawn + 7);
    const at = columnCountAt(card);
    expect(at).toEqual({ x: 358 - COL.padR, y: 22 + 3.5 });
    const end = Math.max(...card.squares.map((q) => q.x + q.s));
    expect(end + COL.endGap).toBeLessThanOrEqual(at.x - labelWidth(`+${card.hidden}`));
  });

  it('draws the wells as rows: squares on the line from x 126 and "+N" for the rest of the pool', () => {
    const pool = scene.pool as NonNullable<Scene['pool']>;
    expect(pool.pending.squares.map((q) => q.x)).toEqual([126, 143, 160, 177, 194]);
    for (const q of pool.pending.squares) expect(q.y).toBe(14);
    expect(pool.pending).toMatchObject({ total: 5, hidden: 0 });
    // w0 is 2×2: it takes two cells and sits 6 px below the row top.
    const w0 = pool.proposed.squares.find((q) => q.id === poolTxHash('w0'));
    expect(w0).toMatchObject({ s: 32, y: 62 + 6 });
    expect(pool.proposed).toMatchObject({ total: 9, hidden: 0 });
    // A busy pool: 56 listed, 300 in the node's pool.
    const p0 = quiet.pool as NonNullable<PipelineSnapshot['pool']>;
    const many = Array.from({ length: BACKEND_POOL_TXS }, (_, i) => ({
      ...p0.pending[0],
      hash: poolTxHash(`busy${i}`),
      size: 330,
      arrivedAt: p0.pending[0].arrivedAt + i,
    }));
    const busy = buildScene(
      { ...quiet, pool: { ...p0, pending: many, pendingTotal: 300 } },
      PHONE_W,
      'settled'
    );
    const grid = (busy.pool as NonNullable<Scene['pool']>).pending;
    expect(grid.squares).toHaveLength(11);
    expect(grid.squares.length + grid.hidden).toBe(300);
    const at = columnWellCountAt(PHONE_W, (busy.bands as NonNullable<Scene['bands']>).pending);
    expect(at).toEqual({ x: 350, y: 0 + 22 + 3.5 });
    const end = Math.max(...grid.squares.map((q) => q.x + q.s));
    expect(end + COL.endGap).toBeLessThanOrEqual(at.x - labelWidth(`+${grid.hidden}`));
  });

  // Direction G, turned (StrapPhone.dc.html `.vs`, `.vd`): the strap runs down through the stones,
  // 20 px wide, 3 px of it on each side of the 14 px stone.
  it('runs the strap down through the stones, x 5 … 25: dashed from NEXT, plate, gilt, the tail', () => {
    expect([COL.strapX, COL.strapW]).toEqual([5, 20]);
    expect(COL.stoneX - COL.strapX).toBe(3);
    expect(COL.strapX + COL.strapW - (COL.stoneX + G.stone)).toBe(3);
    // the same breadth as the row's strap, turned
    expect(COL.strapW).toBe(G.strapH);
    // a row's own strap, local to it: down its whole height
    expect(strapBox('column', PHONE_W, 44)).toEqual({ x: 5, y: 0, width: 20, height: 44 });
    expect(strapBox('column', PHONE_W, 61)).toEqual({ x: 5, y: 0, width: 20, height: 61 });
    const paths = columnChainPaths(slotsOf(scene), nextSlot(scene), T + 1, MAINNET, scene.H);
    // NEXT (136 … 180) ⇢ the tip (198 …): two dashed lines down the strap's outer pixel columns
    expect(paths.pend).toBe('M5.5 180.0V198.0M24.5 180.0V198.0');
    // the tip → tip−1 in the plate's tone (the tip is outside NEXT's window); tip−1 … tip−3 gilt
    expect(paths.solid).toBe(colStrap(242, 260));
    expect(paths.win).toBe(colStrap(304, 322) + colStrap(366, 384));
    // The window of NEXT runs on below the last row: the tail is gilt, 30 px from its bottom edge.
    expect(columnChainTail(slotsOf(scene), scene.H)).toEqual({ x0: 428, x1: 458 });
    expect(paths.tailWin).toBe(colStrap(428, 458));
    expect(paths.tail).toBe('');
    // A link never runs over a row: each fills exactly the gap it crosses.
    const rows = [...scene.cards, scene.next as NonNullable<Scene['next']>];
    for (const name of ['solid', 'win'] as const) {
      for (const r of rects(paths[name])) {
        expect([r.x, r.w]).toEqual([5, 20]);
        expect(rows.some((c) => c.y + c.h === r.y)).toBe(true);
        expect(rows.some((c) => c.y === r.y + r.h)).toBe(true);
      }
    }
  });

  it('points arrowheads down each gap of the pool, 8 × 5 on a 7 px pitch, on the strap’s axis', () => {
    const flows = columnFlowPaths(bands);
    // mempool (0 … 44) ⇢ proposed (62 …): an 18 px gap holds two, from 47 and 54, tips at x 15
    expect(flows.toProposed).toBe('M11.0 47.0L19.0 47.0L15.0 52.0ZM11.0 54.0L19.0 54.0L15.0 59.0Z');
    // proposed (62 … 106) ⇢ NEXT (136 …): a 30 px gap holds four
    const tops = [...flows.toNext.matchAll(/M11\.0 ([\d.]+)L19\.0 ([\d.]+)L15\.0 ([\d.]+)Z/g)];
    expect(tops.map((m) => Number(m[1]))).toEqual([108, 115, 122, 129]);
    expect(tops.every((m) => m[2] === m[1] && Number(m[3]) - Number(m[1]) === 5)).toBe(true);
    expect(tops.map((m) => m[0]).join('')).toBe(flows.toNext);
    expect(COL.strapX + COL.strapW / 2).toBe(15);
  });

  // A column has no lane beside its rows: the gilt says the window, the tips name its range.
  it('has no lane and no words for the window in a column', () => {
    const axis = axisOf(scene);
    const text = windowLabel(T + 1, T, MAINNET);
    expect(axis.lane([nextSlot(scene), ...slotsOf(scene)], T + 1, MAINNET, text)).toEqual({
      label: null,
      hit: null,
    });
    expect(COL).not.toHaveProperty('markHalf');
    expect(COL).not.toHaveProperty('linkX');
    expect(COL).not.toHaveProperty('flowX');
  });

  it("lands a mined block in NEXT's row, beside the four older rows, then steps them down", () => {
    const s2 = mineNext(quiet, { commit: [poolTxHash('w0')], propose: [poolTxHash('q0')] });
    const landing = buildScene(s2, PHONE_W, 'landing');
    expect(landing.next).toBeNull();
    expect(landing.cards.map((c) => [c.block.number, c.slot, c.y])).toEqual([
      [T + 1, 'landing', 136],
      [T, 'chain', 198],
      [T - 1, 'chain', 260],
      [T - 2, 'chain', 322],
      [T - 3, 'chain', 384],
    ]);
    expect(landing.bands?.next).toEqual({ y: 136, h: 44 });
    const settled = buildScene(s2, PHONE_W, 'settled');
    expect(settled.cards.map((c) => [c.block.number, c.y])).toEqual([
      [T + 1, 198],
      [T, 260],
      [T - 1, 322],
      [T - 2, 384],
    ]);
  });

  it("picks in absolute canvas coordinates: a row's squares and dots add its origin", () => {
    const targets = pickTargets(scene);
    const tip = cardOf(scene, T);
    const q = tip.squares[2];
    expect(pick(targets, tip.x + q.x + q.s / 2, tip.y + q.y + q.s / 2)).toEqual({
      kind: 'tx',
      id: q.id,
    });
    const k = tip.chips[0];
    expect(pick(targets, tip.x + k.x + G.chip / 2, tip.y + k.y + G.chip / 2)).toEqual({
      kind: 'chip',
      card: tip.id,
      key: k.key,
    });
    const next = scene.next as NonNullable<Scene['next']>;
    const g = next.ghosts.find((x) => x.id) as NonNullable<Scene['next']>['ghosts'][number];
    expect(pick(targets, next.x + g.x + g.s / 2, next.y + g.y + g.s / 2)).toEqual({
      kind: 'ghost',
      id: g.id,
    });
  });

  it("runs a cellbase's reward link down a lane before the tx line, into the tail", () => {
    const tip = cardOf(scene, T);
    const cb = tip.squares[0];
    const a = { X: tip.x + cb.x + cb.s / 2, Y: tip.y + cb.y + cb.s / 2 };
    expect(a).toEqual({ X: 133.5, Y: 219.5 });
    const tail = columnChainTail(slotsOf(scene), scene.H) as NonNullable<
      ReturnType<typeof columnChainTail>
    >;
    expect(columnRewardLink(a, null, tail)).toEqual({
      d: 'M133.5 219.5H125.0Q120.0 219.5 120.0 224.5V428.0',
      tail: 'M120.0 428.0V458.0',
    });
    // A paid block on screen below: the link turns back into its cellbase.
    expect(columnRewardLink(a, { X: 133.5, Y: 343.5 }, tail)).toEqual({
      d: 'M133.5 219.5H125.0Q120.0 219.5 120.0 224.5V338.5Q120.0 343.5 125.0 343.5H133.5',
      tail: '',
    });
  });

  it('lays every phone width out without a cut row or a square past its row', () => {
    for (const W of [COL.minViewW, 300, 326, PHONE_W, G.minViewW - 1]) {
      for (const sc of [
        buildScene(buildPipelineSnapshot(), W, 'settled'),
        buildScene(quiet, W, 'settled'),
      ]) {
        expect(sc.orientation).toBe('column');
        const rows = [...sc.cards, sc.next as NonNullable<Scene['next']>];
        for (const c of rows) {
          expect(c.x).toBe(COL.x0);
          expect(c.x + c.w).toBe(W - COL.x0);
          for (const q of 'squares' in c ? c.squares : c.ghosts) {
            expect(q.x).toBeGreaterThanOrEqual(COL.lineX - COL.x0);
            expect(q.x + q.s).toBeLessThanOrEqual(c.w - COL.padR);
            expect(q.y).toBeGreaterThanOrEqual(COL.pad);
            expect(q.y + q.s).toBeLessThanOrEqual(c.h - COL.pad);
          }
        }
        const last = sc.cards[sc.cards.length - 1];
        expect(sc.H).toBe(last.y + last.h + COL.bottom);
      }
    }
  });

  it('refuses a view narrower than the narrowest column instead of cutting its rows', () => {
    expect(COL.minViewW).toBe(288);
    expect(() => buildScene(quiet, COL.minViewW - 1, 'settled')).toThrow(
      new RegExp(`${COL.minViewW - 1} px view .* ${COL.minViewW} px column`)
    );
  });

  it('packs a line side by side, first fit, and gives cells back for the "+N" label', () => {
    expect(
      packLine(
        [
          { id: 'a', n: 2 },
          { id: 'b', n: 4 },
          { id: 'c', n: 1 },
        ],
        5,
        true
      )
    ).toEqual({
      pos: [
        { id: 'a', c: 1, n: 2 },
        { id: 'c', c: 3, n: 1 },
      ],
      hidden: 1,
    });
    // 168 px of room hold 10 cells (10 · 17 − 2); 12 txs: the label "+N" takes the last cells it
    // needs (9 cells, 4 px and "+3" would take 151 + 4 + 14.4 = 169.4; 8 cells, 4 px and "+4":
    // 134 + 4 + 14.4 = 152.4; re-pinned at 12 px from 13.2 a "+N").
    const items = Array.from({ length: 12 }, (_, i) => ({ id: `t${i}`, n: 1 }));
    const fit = fitLine(items, 168, false, 0, 'test line');
    expect(fit.pos.length + fit.hidden).toBe(12);
    expect(fit.pos).toHaveLength(8);
    expect(side(fit.pos.length) + COL.endGap + labelWidth(`+${fit.hidden}`)).toBeLessThanOrEqual(
      168
    );
    // A line with room for its "+N" but not for a cell and the "+N" is the "+N" alone: "+12" is
    // 21.6 px at 12 px (re-pinned: the room was 20 for 19.8 px at 11).
    expect(fitLine(items, 22, false, 0, 'a label only')).toEqual({ pos: [], hidden: 12 });
    expect(() => fitLine(items, 4, true, 0, 'a tiny line')).toThrow(/a tiny line has no room/);
  });

  it('gives the panel and the motion layer one axis per orientation', () => {
    const box = { x: 0, y: 198, w: 358, h: 44 };
    expect(axisOf(scene).along(box)).toEqual({ x: 198, w: 44 });
    const row = buildScene(quiet, 1000, 'settled');
    expect(axisOf(row).along(box)).toEqual({ x: 0, w: 358 });
  });
});
