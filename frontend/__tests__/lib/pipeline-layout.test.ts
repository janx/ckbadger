import { describe, expect, it } from 'vitest';
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
  BAND,
  BLOCK_FIELD_CAPACITY,
  CARD_H,
  CELL,
  CHIPS_PER_ROW,
  COL,
  G,
  GUT,
  POOL_PENDING_CAPACITY,
  POOL_PROPOSED_CAPACITY,
  U,
  UNC,
  UNCLE_CHIPS_PER_ROW,
  axisOf,
  buildScene,
  chainLayout,
  chainPaths,
  chainTail,
  chipSlots,
  columnChainPaths,
  columnChainTail,
  columnChipCountAt,
  columnCountAt,
  columnFlowPaths,
  columnRewardLink,
  columnRowH,
  columnRulerPath,
  columnUncleW,
  columnWellCountAt,
  columnWindowMarksPath,
  columnWindowRuler,
  fitLine,
  flowPaths,
  packLine,
  pickTargets,
  inWindow,
  pack2D,
  pick,
  rewardLink,
  rewardTarget,
  rulerPath,
  side,
  uncleRowH,
  unclesH,
  windowLabel,
  windowMarksPath,
  windowRange,
  windowRuler,
  labelWidth,
  type ChainSlotState,
  type WindowLabelGeom,
  type PickTargets,
  type RulerSlot,
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

/** The filled rectangles of a tick band's path; anything else in the path fails the test. */
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

  it('keeps the window bracket, its label and the reward link in the 18 px lane above the plates', () => {
    // The dimension line runs along the lane's middle; its ticks start 5 px above it, in the lane.
    expect(G.winY).toBe(9);
    expect(G.winY - 5).toBeGreaterThanOrEqual(0);
    const ruler = { ticks: [400], label: null, hit: null, tailOpen: false };
    expect(rulerPath(ruler)).toBe(`M400.0 4V${G.cardTop + G.headH}`);
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
      'slot',
      'squares',
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
      'dots',
      'h',
      'index',
      'label',
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
    expect(card.uncles.map((u) => [u.label, u.dots])).toEqual([['uncle', true]]);
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
  const slots: RulerSlot[] = [
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

  it('puts both ticks in the middle of the gap between neighbouring plates', () => {
    const ruler = windowRuler(slots, 101, MAINNET, 2000, textOf(101));
    const at = (n: number) => slots.find((s) => s.number === n) as RulerSlot;
    // Near edge: between #100 (H−1) and #99 (H−2). Far edge: between #91 (H−10) and #90 (H−11).
    const xn = (at(100).x + at(100).w + at(99).x) / 2;
    const xf = (at(91).x + at(91).w + at(90).x) / 2;
    expect(ruler.ticks).toEqual([xn, xf]);
    // The label fits between them: centred, with the line broken around it.
    expect(ruler.label?.at).toBe('between');
    expect(ruler.label?.x).toBe((xn + xf) / 2);
    expect(ruler.tailOpen).toBe(false);
  });

  it('lets the window run off the last plate when its far edge is not on screen', () => {
    const chain = slots.filter((s) => s.mined).slice(0, 5);
    const ruler = windowRuler(chain, 101, MAINNET, 1000, textOf(101));
    expect(ruler.ticks).toHaveLength(1);
    expect(ruler.tailOpen).toBe(true);
    expect(ruler.hit?.xf).toBe(1000);
  });

  it('has no ruler when the near edge is not on screen', () => {
    const ruler = windowRuler(
      [{ number: 100, x: 400, w: 60, mined: true }],
      120,
      MAINNET,
      1000,
      textOf(120)
    );
    expect(ruler.ticks).toEqual([]);
    expect(ruler.label).toBeNull();
    expect(ruler.hit).toBeNull();
    expect(windowMarksPath(ruler)).toBe('');
  });

  it('marks the window at rest with a short gold mark across the head line at each tick', () => {
    const ruler = { ticks: [480, 1461], label: null, hit: null, tailOpen: false };
    // The head line's middle is 36: each mark runs from 4.5 px above it to 5.5 px below it.
    expect(windowMarksPath(ruler)).toBe('M480.0 31.5V41.5M1461.0 31.5V41.5');
  });

  it('raises the ticks to the dimension line when hovered and breaks the line around the label', () => {
    const ruler = windowRuler(slots, 101, MAINNET, 2000, textOf(101));
    const d = rulerPath(ruler);
    const [xn, xf] = ruler.ticks;
    // Both ticks drop from above the dimension line onto the head line (28 down the plate).
    expect(d).toContain(`M${xn.toFixed(1)} 4V46`);
    expect(d).toContain(`M${xf.toFixed(1)} 4V46`);
    // Two segments of the dimension line, from each tick to the label's gap.
    const segs = [...d.matchAll(/M([\d.]+) (\d+)H([\d.]+)/g)].map((m) => m.slice(1).map(Number));
    expect(segs).toHaveLength(2);
    expect(segs.every((g) => g[1] === G.winY)).toBe(true);
    expect(segs[0][0]).toBeCloseTo(xn, 1);
    expect(segs[1][2]).toBeCloseTo(xf, 1);
    // The break is exactly the label's room.
    expect(segs[0][2]).toBeCloseTo(ruler.label?.x0 as number, 1);
    expect(segs[1][0]).toBeCloseTo(ruler.label?.x1 as number, 1);
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

describe('window label placement: inside the view, never across a tick', () => {
  // The 1246 px canvas: NEXT, then 7 blocks; the window of a block runs right, towards older ones.
  const W = CONTAINER_VIEW_W;
  const TIP = 18_734_755;
  const NEXT = TIP + 1;
  const slots: RulerSlot[] = [
    { number: NEXT, x: G.nextX, w: CARD_W, mined: false },
    ...chainLayout(8, W, false).map((sl, i) => ({
      number: TIP - i,
      x: sl.x,
      w: sl.w,
      mined: true,
    })),
  ];
  const place = (H: number) => {
    const text = windowLabel(H, TIP, MAINNET);
    return { text, ruler: windowRuler(slots, H, MAINNET, W, text) };
  };
  const textBox = (label: WindowLabelGeom, text: string) => [
    label.x - labelWidth(text) / 2,
    label.x + labelWidth(text) / 2,
  ];

  it('centres the label between the ticks when it fits there, even with the far end off screen', () => {
    // T−3: near tick at 939 (between …4751 ending at 930 and …4750 at 948), far end past the
    // edge; 1246 − 939 = 307 ≥ 294 + 2 · 6.
    const { text, ruler } = place(TIP - 3);
    expect(labelWidth(text)).toBe(294);
    expect(ruler.ticks).toEqual([939]);
    expect(ruler.label).toEqual({
      x: (939 + W) / 2,
      x0: (939 + W) / 2 - 153,
      x1: (939 + W) / 2 + 153,
      at: 'between',
    });
    const [l, r] = textBox(ruler.label as WindowLabelGeom, text);
    expect(l).toBeGreaterThan(939);
    expect(r).toBeLessThanOrEqual(W);
  });

  it('puts it outside the window, before the near tick, when the window in view is too short', () => {
    // T−4: near tick at 1056; the label (294 px + padding) no longer fits after it.
    const xn = 1056;
    const { text, ruler } = place(TIP - 4);
    expect(ruler.ticks).toEqual([xn]);
    expect(ruler.label).toEqual({ x: xn - 153, x0: xn - 306, x1: xn, at: 'outside' });
    const [l, r] = textBox(ruler.label as WindowLabelGeom, text);
    expect(l).toBeGreaterThanOrEqual(0);
    expect(r).toBe(xn - 6);
    // The dimension line is then drawn whole, from the tick to the view edge.
    expect(rulerPath(ruler)).toBe(`M1056.0 4V46M1056.0 ${G.winY}H${W.toFixed(1)}`);
  });

  it('keeps every hovered window label of the row inside [0, viewW] and clear of its ticks', () => {
    for (const s of slots) {
      const { text, ruler } = place(s.number);
      if (!ruler.label) {
        // Only a window whose near end is off screen has no label (and no ruler at all).
        expect(ruler.hit).toBeNull();
        continue;
      }
      const [l, r] = textBox(ruler.label, text);
      expect(l).toBeGreaterThanOrEqual(0);
      expect(r).toBeLessThanOrEqual(W);
      for (const tx of ruler.ticks) expect(tx < l || tx > r).toBe(true);
    }
  });

  it('draws no label when the near tick itself is past the view edge', () => {
    // The 1246 px row read through a 760 px view: the window of T−4 starts at 1056, off view.
    const text = windowLabel(TIP - 4, TIP, MAINNET);
    const ruler = windowRuler(slots, TIP - 4, MAINNET, 760, text);
    expect(ruler.ticks).toEqual([1056]);
    expect(ruler.label).toBeNull();
    // The ruler itself is still drawn (and clipped by the view).
    expect(rulerPath(ruler)).toContain(`M1056.0 ${G.winY}H760.0`);
  });
});

describe('a narrower view: fewer blocks, all of them whole', () => {
  it.each([589, 606, 760, 1000, CONTAINER_VIEW_W])(
    'at %i px: no plate past the edge, a forward tail, every window label in view and off its ticks',
    (W) => {
      const s = buildPipelineSnapshot();
      const scene = buildScene(s, W, 'settled');
      const next = scene.next as NonNullable<Scene['next']>;
      for (const c of scene.cards) expect(c.x + c.w).toBeLessThanOrEqual(W - G.marginR);
      const last = scene.cards[scene.cards.length - 1];
      // The tail starts 2 px past the last plate and runs 66 px, or to the view edge.
      const x0 = last.x + last.w + 2;
      expect(chainTail(scene.cards, W)).toEqual({ x0, x1: x0 + Math.min(66, W - x0) });
      expect(x0).toBeLessThan(W);
      const slots: RulerSlot[] = [
        { number: next.number, x: next.x, w: next.w, mined: false },
        ...scene.cards.map((c) => ({ number: c.block.number, x: c.x, w: c.w, mined: true })),
      ];
      for (const sl of slots) {
        const text = windowLabel(sl.number, scene.windowTip, s.consensus);
        const ruler = windowRuler(slots, sl.number, s.consensus, W, text);
        for (const tx of ruler.ticks) {
          expect(tx).toBeGreaterThanOrEqual(0);
          expect(tx).toBeLessThanOrEqual(W);
        }
        if (!ruler.label) {
          // Only a window whose near end is past the last plate has no label (and no ruler).
          expect(ruler.hit).toBeNull();
          expect(sl.number - s.consensus.proposalWindowClosest).toBeLessThan(last.block.number);
          continue;
        }
        const half = labelWidth(text) / 2;
        expect(ruler.label.x - half).toBeGreaterThanOrEqual(0);
        expect(ruler.label.x + half).toBeLessThanOrEqual(W);
        for (const tx of ruler.ticks) {
          expect(tx < ruler.label.x - half || tx > ruler.label.x + half).toBe(true);
        }
      }
    }
  );
});

describe('chainPaths: the links, two bands of ticks', () => {
  // NEXT at 246, then #100 … #85 every 117 px, all linked parent to child.
  const cards: ChainSlotState[] = Array.from({ length: 16 }, (_, i) => ({
    number: 100 - i,
    x: G.chainX0 + i * PITCH,
    w: CARD_W,
    mined: true,
    hash: `h${100 - i}`,
    parentHash: `h${99 - i}`,
    solid: 1,
  }));
  const next = { x: G.nextX, w: CARD_W };
  const VIEW = 2400;
  /** The two bands of one 18 px gap after a plate ending at `end`: 4 ticks each. */
  const gapBands = (end: number) =>
    [50, 98]
      .map((y) => [0, 4, 8, 12].map((k) => `M${(end + 2 + k).toFixed(1)} ${y}.0h2v10h-2Z`).join(''))
      .join('');

  it('is filled tick rectangles only, at 32 and 80 down the plate: no stroke, no arrowheads', () => {
    expect(BAND.inset).toBe(2);
    expect(BAND.link).toEqual({ tick: 2, pitch: 4, depth: 10 });
    expect(G.linkY.map((y) => y - G.cardTop)).toEqual([32, 80]);
    const paths = chainPaths(cards, next, 101, MAINNET, VIEW);
    expect(Object.keys(paths).sort()).toEqual(['pend', 'solid', 'tail', 'win']);
    for (const d of Object.values(paths)) {
      for (const r of rects(d)) {
        expect([r.w, r.h]).toEqual([2, 10]);
        expect([50, 98]).toContain(r.y);
      }
    }
  });

  it('crosses an 18 px gap in 4 ticks of 2 px on a 4 px pitch, 2 px inside it', () => {
    const paths = chainPaths(cards, next, 101, MAINNET, VIEW);
    // NEXT ends at 345, the tip starts at 363: ticks at 347, 351, 355, 359, the last ending at 361.
    expect(paths.pend).toBe(gapBands(G.nextX + CARD_W));
    expect(paths.pend).toBe(
      'M347.0 50.0h2v10h-2ZM351.0 50.0h2v10h-2ZM355.0 50.0h2v10h-2ZM359.0 50.0h2v10h-2Z' +
        'M347.0 98.0h2v10h-2ZM351.0 98.0h2v10h-2ZM355.0 98.0h2v10h-2ZM359.0 98.0h2v10h-2Z'
    );
  });

  it('is pending from NEXT to the tip, gold inside the window and plain outside it', () => {
    const paths = chainPaths(cards, next, 101, MAINNET, VIEW);
    // The window of #101 is #91 … #99: the eight links between them are gold, 8 ticks each.
    expect(rects(paths.win)).toHaveLength(8 * 8);
    // #100 → #99 and #91 → #90 … #86 → #85 lie outside it.
    expect(rects(paths.solid)).toHaveLength(7 * 8);
    expect(paths.solid.startsWith(gapBands(G.chainX0 + CARD_W))).toBe(true);
  });

  it('breaks where the newer plate does not name the older one as its parent', () => {
    const forked = cards.map((c) => (c.number === 95 ? { ...c, parentHash: 'elsewhere' } : c));
    const paths = chainPaths(forked, next, 101, MAINNET, VIEW);
    expect(rects(paths.win)).toHaveLength(7 * 8);
    expect(rects(paths.solid)).toHaveLength(7 * 8);
  });

  it('ends in the two bands continued 66 px past the last plate, or to the view edge', () => {
    const last = cards[cards.length - 1];
    // #85 ends at 2217: the tail runs from 2219 for 66 px.
    expect(chainTail(cards, VIEW)).toEqual({ x0: 2219, x1: 2285 });
    const tail = rects(chainPaths(cards, next, 101, MAINNET, VIEW).tail);
    expect(tail).toHaveLength(2 * 17);
    expect(Math.min(...tail.map((r) => r.x))).toBe(last.x + CARD_W + 2);
    expect(Math.max(...tail.map((r) => r.x + r.w))).toBe(2285);
    // With 21 px of room it stops at the edge.
    expect(chainTail(cards, 2240)).toEqual({ x0: 2219, x1: 2240 });
    expect(rects(chainPaths(cards, next, 101, MAINNET, 2240).tail)).toHaveLength(2 * 5);
    expect(chainTail([], 2000)).toBeNull();
  });

  it('runs one faint band of 1 px ticks on a 3 px pitch through each gap of the pool', () => {
    expect(BAND.flow).toEqual({ tick: 1, pitch: 3, depth: 8 });
    expect(G.flowY - G.cardTop).toBe(14);
    const flows = flowPaths();
    // mempool (0…99) ⇢ proposed (117…): 101 … 115, five ticks.
    expect(rects(flows.toProposed)).toEqual(
      [101, 104, 107, 110, 113].map((x) => ({ x, y: 32, w: 1, h: 8 }))
    );
    // proposed (117…216) ⇢ NEXT (246…): 218 … 244, nine ticks.
    expect(rects(flows.toNext).map((r) => r.x)).toEqual([
      218, 221, 224, 227, 230, 233, 236, 239, 242,
    ]);
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
    // "+255" is 4 glyphs of 6 px, each under one dot pitch.
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
      solid: 1,
    }));
  const nextSlot = (sc: Scene): RulerSlot => {
    const n = sc.next as NonNullable<Scene['next']>;
    return { number: n.number, x: n.y, w: n.h, mined: false };
  };
  /** The two link bands of a column gap [y0, y1], 4 ticks of 2 px each, at x 27 and 75. */
  const colBands = (y0: number, y1: number) => {
    const ys: number[] = [];
    for (let y = y0; y + 2 <= y1; y += 4) ys.push(y);
    return [27, 75].map((x) => ys.map((y) => `M${x}.0 ${y}.0h10v2h-10Z`).join('')).join('');
  };

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
    const at = columnChipCountAt(card.h);
    expect(card.x + at.x).toBe(117);
    expect(at.y).toBe(23 + G.chip / 2 + 3.5);
    // The label's three glyphs fit the freed slots, never over a dot.
    const lastDot = Math.max(...card.chips.filter((k) => k.y === 23).map((k) => k.x + G.chip));
    expect(at.x - labelWidth('+33')).toBeGreaterThanOrEqual(lastDot);
  });

  it('ends the tx line with an uncle: its word, then its dots, no box, out of the line’s room', () => {
    // T−1 carries the uncle T−2 and two of its proposals.
    const host = cardOf(scene, T - 1);
    expect(COL.uncle).toEqual({ labelW: 30, chips: 4 });
    expect(columnUncleW(2, true)).toBe(30 + 4 + 34);
    expect(columnUncleW(0, true)).toBe(30);
    expect(columnUncleW(2, false)).toBe(30);
    expect(host.uncles).toEqual([
      expect.objectContaining({
        index: 0,
        label: 'uncle',
        x: 282,
        y: 16,
        w: 68,
        h: 12,
        dots: true,
      }),
    ]);
    const un = host.uncles[0];
    expect(host.x + un.x + un.w).toBe(PHONE_W - COL.padR);
    const dots = host.chips.filter((k) => k.proposal.uncleIndex === 0);
    expect(dots.map((k) => k.x)).toEqual([un.x + 34, un.x + 43]);
    for (const k of dots) expect(k.y + G.chip / 2).toBe(host.h / 2);
    // Its own two proposals stay in the dot zone; no square reaches the uncle.
    expect(host.chips.filter((k) => k.proposal.uncleIndex === null).map((k) => k.x)).toEqual([
      74, 83,
    ]);
    for (const q of host.squares) expect(q.x + q.s).toBeLessThanOrEqual(un.x - COL.endGap);
    expect(columnCountAt(host).x).toBe(un.x - COL.endGap);
  });

  it("lets an uncle's dots give way first: at 288 px the line still holds the cellbase, a 4 × 4, its +N and the uncle's word", () => {
    const n = T - 1;
    const big: PipelineTx = { ...smallTxs(n, 31)[30], hash: committedTxHash(n, 777), size: 9600 };
    const s = withTxs(quiet, n, [big, ...smallTxs(n, 30)]);
    const blocks = s.blocks.map((b) =>
      b.number === n ? { ...b, proposals: [...b.proposals, ...uncleProposals(0, 10, 300)] } : b
    );
    const busy = { ...s, blocks };
    expect(COL.keepCells).toBe(5);
    // At 288 the uncle with its dots would leave the line 82 px, short of the cellbase, a 4 × 4
    // and "+31" (83 + 4 + 18): the dots give way and the uncle is its word.
    const narrow = cardOf(buildScene(busy, COL.minViewW, 'settled'), n);
    expect(narrow.uncles).toEqual([
      expect.objectContaining({ x: 250, w: 30, dots: false, overflow: 12 }),
    ]);
    expect(narrow.chips.filter((k) => k.proposal.uncleIndex === 0)).toEqual([]);
    expect(narrow.squares.map((q) => q.s)).toEqual([side(1), side(4)]);
    expect(narrow.hidden).toBe(30);
    const count = columnCountAt(narrow);
    expect(count.x).toBe(246);
    const end = Math.max(...narrow.squares.map((q) => q.x + q.s));
    expect(end + COL.endGap).toBeLessThanOrEqual(count.x - labelWidth(`+${narrow.hidden}`));
    // At 358 the dots are back: one dot and "+11" in the uncle's four slots.
    const wide = cardOf(buildScene(busy, PHONE_W, 'settled'), n);
    expect(wide.uncles).toEqual([
      expect.objectContaining({ x: 282, w: 68, dots: true, overflow: 11 }),
    ]);
    expect(wide.chips.filter((k) => k.proposal.uncleIndex === 0)).toHaveLength(1);
  });

  it('counts what a block line cannot hold as +N at its end, never over a square', () => {
    const n = T - 2;
    const txs = smallTxs(n, 60);
    const card = cardOf(buildScene(withTxs(quiet, n, txs, 7), PHONE_W, 'settled'), n);
    const drawn = card.squares.length - 1;
    expect(drawn).toBe(11);
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

  it('runs the two link bands down the gaps only, 2 px clear of the rows: pending, plain, gold, tail', () => {
    expect(COL.linkX).toEqual([27, 75]);
    const paths = columnChainPaths(slotsOf(scene), nextSlot(scene), T + 1, MAINNET, scene.H);
    // NEXT ⇢ the tip pending; tip → tip−1 plain (the tip is outside NEXT's window); tip−1 … tip−3
    // gold inside it; then the tail's 30 px.
    expect(paths.pend).toBe(colBands(182, 196));
    expect(paths.pend.startsWith('M27.0 182.0h10v2h-10ZM27.0 186.0h10v2h-10Z')).toBe(true);
    expect(paths.solid).toBe(colBands(244, 258));
    expect(paths.win).toBe(colBands(306, 320) + colBands(368, 382));
    expect(paths.tail).toBe(colBands(430, 460));
    expect(rects(paths.tail)).toHaveLength(2 * 8);
    expect(columnChainTail(slotsOf(scene), scene.H)).toEqual({ x0: 430, x1: 460 });
    // A link never runs under a row.
    for (const d of Object.values(paths)) {
      for (const r of rects(d)) {
        expect([r.w, r.h]).toEqual([10, 2]);
        for (const c of [...scene.cards, scene.next as NonNullable<Scene['next']>]) {
          expect(r.y + r.h <= c.y - 2 || r.y >= c.y + c.h + 2).toBe(true);
        }
      }
    }
  });

  it('runs one faint band 8 wide at x 28 down each gap of the pool', () => {
    const flows = columnFlowPaths(bands);
    // mempool (0…44) ⇢ proposed (62…): 46 … 60, five 1 px ticks on the 3 px pitch.
    expect(rects(flows.toProposed)).toEqual(
      [46, 49, 52, 55, 58].map((y) => ({ x: 28, y, w: 8, h: 1 }))
    );
    // proposed (62…106) ⇢ NEXT (136…): 108 … 134, nine ticks.
    expect(rects(flows.toNext).map((r) => r.y)).toEqual([
      108, 111, 114, 117, 120, 123, 126, 129, 132,
    ]);
  });

  it("marks NEXT's window with a short horizontal tick across the first band between tip and tip − 1", () => {
    const ruler = columnWindowRuler([nextSlot(scene), ...slotsOf(scene)], T + 1, MAINNET, scene.H);
    // In the middle of the gap between the tip (198 + 44) and tip − 1 (260).
    expect(ruler.ticks).toEqual([251]);
    expect(ruler.label).toBeNull();
    // The far end (tip − 9) is below the last row: the window runs on in the gold tail.
    expect(ruler.tailOpen).toBe(true);
    // Centred on the first band (27…37), 6 px either side.
    expect(columnWindowMarksPath(ruler)).toBe('M26.0 251.0H38.0');
    // Hovered, the tick runs across the whole gap.
    expect(columnRulerPath(ruler, PHONE_W)).toBe('M0.0 251.0H358.0');
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
      d: 'M133.5 219.5H125.0Q120.0 219.5 120.0 224.5V430.0',
      tail: 'M120.0 430.0V460.0',
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
    // needs (9 cells, 4 px and "+3": 151 + 4 + 12 = 167).
    const items = Array.from({ length: 12 }, (_, i) => ({ id: `t${i}`, n: 1 }));
    const fit = fitLine(items, 168, false, 0, 'test line');
    expect(fit.pos.length + fit.hidden).toBe(12);
    expect(fit.pos).toHaveLength(9);
    expect(side(fit.pos.length) + COL.endGap + labelWidth(`+${fit.hidden}`)).toBeLessThanOrEqual(
      168
    );
    // A line with room for its "+N" but not for a cell and the "+N" is the "+N" alone.
    expect(fitLine(items, 20, false, 0, 'a label only')).toEqual({ pos: [], hidden: 12 });
    expect(() => fitLine(items, 4, true, 0, 'a tiny line')).toThrow(/a tiny line has no room/);
  });

  it('gives the panel and the motion layer one axis per orientation', () => {
    const box = { x: 0, y: 198, w: 358, h: 44 };
    expect(axisOf(scene).along(box)).toEqual({ x: 198, w: 44 });
    const row = buildScene(quiet, 1000, 'settled');
    expect(axisOf(row).along(box)).toEqual({ x: 0, w: 358 });
  });
});
