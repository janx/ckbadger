import { describe, expect, it } from 'vitest';
import {
  FIXTURE_TIP,
  buildPipelineSnapshot,
  committedTxHash,
  mineNext,
  poolTxHash,
  rewardOf,
  shortIdOf,
} from '@/__tests__/msw/fixtures/pipeline-snapshot';
import type { PipelineSnapshot, PipelineTx } from '@/lib/api';
import {
  BLOCK_FIELD_CAPACITY,
  CARD_H,
  CHIPS_PER_ROW,
  G,
  POOL_PENDING_CAPACITY,
  POOL_PROPOSED_CAPACITY,
  U,
  UNC,
  UNCLE_CHIPS_PER_ROW,
  buildScene,
  chainLayout,
  chainPaths,
  chainTail,
  chipSlots,
  inWindow,
  pack2D,
  pick,
  rewardLink,
  rewardTarget,
  rulerPath,
  uncleBoxH,
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
/** The card width of the design: 8 columns of the 10 px grid plus the side padding. */
const CARD_W = 89;
/** The page container is 1280 px wide; the canvas inside its padding is 1246 px. */
const CONTAINER_VIEW_W = 1246;

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

describe('geometry of the one-row canvas', () => {
  it('is a 10 px grid with fixed 89 × 98 px cards in a 132 px row', () => {
    expect(U).toBe(10);
    expect(G.cardCols).toBe(8);
    expect(G.cardW).toBe(CARD_W);
    expect(CARD_H).toBe(98);
    expect(G.H).toBe(132);
    // The chain runs through the card headers.
    expect(G.chainY).toBe(37);
    expect(G.chainY).toBeGreaterThan(G.cardTop);
    expect(G.chainY).toBeLessThan(G.cardTop + G.headH);
    expect(G.cardTop + CARD_H).toBeLessThanOrEqual(G.H);
    expect(G.fieldRows).toBe(5);
    expect(G.poolRows).toBe(7);
    expect(UNC.chipRows).toBe(1);
  });

  it('draws proposals as two rows of ten 6 px dots on an 8 px pitch', () => {
    expect(G.propRows).toBe(2);
    expect(G.chip).toBe(6);
    expect(G.chipP).toBe(8);
    expect(CHIPS_PER_ROW).toBe(10);
    // The two rows end above the tx field.
    expect(G.propTop + G.propRows * G.chipP).toBeLessThanOrEqual(G.fieldTop);
  });

  it('runs mempool ⇢ proposed ⇢ NEXT ⇢ tip with a wider gap on both sides of NEXT', () => {
    expect(G.mempoolX).toBe(6);
    expect(G.gap).toBe(20);
    expect(G.frontierGap).toBe(44);
    expect(G.frontierGap).toBeGreaterThan(G.gap);
    expect(G.proposedX).toBe(G.mempoolX + CARD_W + G.gap);
    expect(G.nextX).toBe(G.proposedX + CARD_W + G.frontierGap);
    expect(G.chainX0).toBe(G.nextX + CARD_W + G.frontierGap);
    expect([G.mempoolX, G.proposedX, G.nextX, G.chainX0]).toEqual([6, 115, 248, 381]);
  });
});

describe('grid capacities', () => {
  it('are the 1×1 cells of a pool well (8 × 7) and of a block field (8 × 5)', () => {
    // The backend truncates its lists to these: 56 pending, 56 proposed, and per block the cellbase
    // plus 39 txs.
    expect(POOL_PENDING_CAPACITY).toBe(56);
    expect(POOL_PROPOSED_CAPACITY).toBe(56);
    expect(BLOCK_FIELD_CAPACITY).toBe(40);
    expect(POOL_PENDING_CAPACITY).toBe(G.cardCols * G.poolRows);
    expect(BLOCK_FIELD_CAPACITY).toBe(G.cardCols * G.fieldRows);
  });

  it('packs at most a well of 1×1 pool txs inside each pool well', () => {
    const s = buildPipelineSnapshot();
    const pool = s.pool as NonNullable<PipelineSnapshot['pool']>;
    const many = Array.from({ length: POOL_PENDING_CAPACITY + 4 }, (_, i) => ({
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
    // The well shows the pool total, listed or not.
    expect(wells.pending.total).toBe(many.length + 100);
    for (const [grid, x] of [
      [wells.pending, G.mempoolX],
      [wells.proposed, G.proposedX],
    ] as const) {
      for (const q of grid.squares) {
        expect(q.x).toBeGreaterThanOrEqual(x + G.padX);
        expect(q.x + q.s).toBeLessThanOrEqual(x + CARD_W - G.padX);
        expect(q.y).toBeGreaterThanOrEqual(G.poolTop);
        expect(q.y + q.s).toBeLessThanOrEqual(G.cardTop + CARD_H - G.padB);
      }
    }
  });
});

describe('card size', () => {
  const scene = buildScene(buildPipelineSnapshot(), 1600, 'settled');

  it('is the same width for every card, whatever its txs, proposals and uncles', () => {
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

  it('is one fixed height for every card: quiet, full, carrying uncles, and NEXT', () => {
    const quiet = scene.cards.filter((c) => !c.block.uncles.length && c.block.txs.length <= 2);
    expect(new Set(quiet.map((c) => c.block.txs.length))).toEqual(new Set([0, 1]));
    expect(new Set(scene.cards.map((c) => c.h))).toEqual(new Set([CARD_H]));
    expect(cardOf(scene, T - 5).h).toBe(CARD_H);
    expect(cardOf(scene, T - 1).h).toBe(CARD_H);
    expect(scene.next?.h).toBe(CARD_H);
  });

  it('shows "+N" for the 1×1 txs that do not fit the 8 × 5 field', () => {
    const s = buildPipelineSnapshot();
    const n = T - 8; // no txs, proposals or uncles in the fixture
    const txs = smallTxs(n, 200);
    const blocks = s.blocks.map((b) =>
      b.number === n ? { ...b, txs, txCount: 1 + txs.length, omittedTxs: 0 } : b
    );
    const card = cardOf(buildScene({ ...s, blocks }, 1600, 'settled'), n);
    // 8 × 5 = 40 cells: the cellbase and 39 txs; the other 161 are counted, not drawn.
    expect(card.squares).toHaveLength(BLOCK_FIELD_CAPACITY);
    expect(card.squares.every((q) => q.x + q.s <= card.w - G.padX)).toBe(true);
    expect(card.squares.every((q) => q.y + q.s <= G.cardTop + CARD_H - G.padB)).toBe(true);
    expect(card.hidden).toBe(161);
    expect(card.w).toBe(CARD_W);
  });

  it('orders equal-size transactions by their exact fee rates', () => {
    const s = buildPipelineSnapshot();
    const n = T - 8;
    const txs = smallTxs(n, 2);
    txs[0].feeRate = '9007199254740992';
    txs[1].feeRate = '9007199254740993';
    const blocks = s.blocks.map((b) =>
      b.number === n ? { ...b, txs, txCount: 1 + txs.length, omittedTxs: 0 } : b
    );
    const card = cardOf(buildScene({ ...s, blocks }, 1600, 'settled'), n);
    expect(card.squares.slice(1).map((square) => square.id)).toEqual([txs[1].hash, txs[0].hash]);
  });

  it('counts the txs the backend omitted together with the field overflow', () => {
    // T−5 commits 299 txs; the backend lists the cellbase's 39 companions and omits 260.
    const full = cardOf(scene, T - 5);
    expect(full.block.txs).toHaveLength(BLOCK_FIELD_CAPACITY - 1);
    expect(full.squares).toHaveLength(BLOCK_FIELD_CAPACITY);
    expect(full.hidden).toBe(260);
    expect(cardOf(scene, T - 8).hidden).toBe(0);
  });
});

describe('uncle box', () => {
  it('is a header only without proposals, then one row of dots at most', () => {
    const per = UNCLE_CHIPS_PER_ROW;
    expect(uncleBoxH(0)).toBe(UNC.head + UNC.padB);
    expect(uncleBoxH(1)).toBe(UNC.head + G.chipP + UNC.padB);
    expect(uncleBoxH(per)).toBe(UNC.head + G.chipP + UNC.padB);
    expect(uncleBoxH(per + 1)).toBe(UNC.head + G.chipP + UNC.padB);
    expect(uncleBoxH(per * 5)).toBe(UNC.head + G.chipP + UNC.padB);
  });

  it('takes its area out of the tx field instead of growing the card', () => {
    const extra = unclesH([2]);
    expect(extra).toBe(UNC.top + uncleBoxH(2) + UNC.gap);
    expect(unclesH([])).toBe(0);
    // T−1 carries one uncle with two proposals: its tx field starts below the mini-card and still
    // ends inside the fixed card.
    const s = buildPipelineSnapshot();
    const n = T - 1;
    const txs = smallTxs(n, 60);
    const blocks = s.blocks.map((b) =>
      b.number === n ? { ...b, txs, txCount: 1 + txs.length, omittedTxs: 0 } : b
    );
    const card = cardOf(buildScene({ ...s, blocks }, 1600, 'settled'), n);
    expect(card.h).toBe(CARD_H);
    const fieldTop = G.fieldTop + extra;
    expect(Math.min(...card.squares.map((q) => q.y))).toBe(fieldTop);
    expect(Math.max(...card.squares.map((q) => q.y + q.s))).toBeLessThanOrEqual(
      G.cardTop + CARD_H - G.padB
    );
    const rows = G.fieldRows - Math.ceil(extra / U);
    expect(card.squares).toHaveLength(G.cardCols * rows);
    expect(card.hidden).toBe(1 + txs.length - G.cardCols * rows);
  });

  it('fits the fixed card width, labelled "uncle", with its proposal chips inside', () => {
    const s = buildPipelineSnapshot();
    // T−1 carries the uncle T−2 with two proposals; give that uncle 30 more.
    const extra = Array.from({ length: 30 }, (_, i) => ({
      shortId: shortIdOf(committedTxHash(T - 2, 900 + i)),
      uncleIndex: 0,
      state: 'waiting' as const,
      txHash: null,
      committedBlock: null,
    }));
    const blocks = s.blocks.map((b) =>
      b.number === T - 1 ? { ...b, proposals: [...b.proposals, ...extra] } : b
    );
    const card = cardOf(buildScene({ ...s, blocks }, 1600, 'settled'), T - 1);
    expect(card.w).toBe(CARD_W);
    expect(card.uncles.map((u) => u.label)).toEqual(['uncle']);
    const chips = card.chips.filter((k) => k.proposal.uncleIndex === 0);
    // One row of the mini-card is drawn; the rest is its "+N".
    expect(chips).toHaveLength(UNCLE_CHIPS_PER_ROW);
    expect(card.uncles[0].overflow).toBe(32 - UNCLE_CHIPS_PER_ROW);
    for (const k of chips) {
      expect(k.x).toBeGreaterThanOrEqual(G.padX + 3);
      expect(k.x + G.chip).toBeLessThanOrEqual(card.w - G.padX - 3);
    }
  });
});

describe('chainLayout', () => {
  it('stops at the last whole card that fits the view', () => {
    const slots = chainLayout(24, 1600, false);
    expect(slots.length).toBeGreaterThanOrEqual(G.minBlocks);
    expect(slots.length).toBeLessThan(24);
    const last = slots[slots.length - 1];
    expect(last.x + last.w).toBeLessThanOrEqual(1600 - 10);
    expect(last.x + last.w + G.gap + CARD_W).toBeGreaterThan(1600 - 10);
    expect(slots[0].x).toBe(G.chainX0);
    expect(
      slots.every((sl, i) => sl.w === CARD_W && sl.x === G.chainX0 + i * (CARD_W + G.gap))
    ).toBe(true);
  });

  it.each([
    { width: 760, blocks: 3 },
    { width: 1000, blocks: 5 },
    { width: CONTAINER_VIEW_W, blocks: 8 },
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

  it('never shows fewer blocks than fit the 760 px minimum canvas: 3 by this geometry', () => {
    expect(G.minViewW).toBe(760);
    // Derived from the geometry: the cards that fit from the chain start to the margin.
    expect(G.minBlocks).toBe(
      Math.floor((G.minViewW - G.marginR - G.chainX0 + G.gap) / (CARD_W + G.gap))
    );
    expect(G.minBlocks).toBe(3);
    expect(chainLayout(24, G.minViewW, false)).toHaveLength(G.minBlocks);
    // The landing block sits at NEXT, beside as many chain blocks as fit.
    const landing = chainLayout(24, G.minViewW, true);
    expect(landing.map((sl) => sl.slot)).toEqual(['landing', 'chain', 'chain', 'chain']);
  });

  it('refuses a view narrower than the minimum canvas instead of clipping its cards', () => {
    expect(() => chainLayout(24, G.minViewW - 1, false)).toThrow(/759 px view .* 760 px/);
  });

  it('fits all 8 chain blocks inside the 1280 px page container', () => {
    const slots = chainLayout(24, CONTAINER_VIEW_W, false);
    expect(slots).toHaveLength(8);
    const last = slots[slots.length - 1];
    expect(last.x + last.w).toBeLessThanOrEqual(CONTAINER_VIEW_W - 10);
    // A ninth would not fit.
    expect(last.x + last.w + G.gap + CARD_W).toBeGreaterThan(CONTAINER_VIEW_W - 10);
  });

  it('never shows more than 24 cards', () => {
    expect(chainLayout(30, 100_000, false)).toHaveLength(G.maxBlocks);
  });

  it('puts a landing block at the NEXT position and keeps the chain slots of the others', () => {
    const settled = chainLayout(13, 2400, false);
    const landing = chainLayout(14, 2400, true);
    expect(landing[0]).toMatchObject({ index: 0, x: G.nextX, w: CARD_W, slot: 'landing' });
    expect(landing[1].x).toBe(settled[0].x);
    expect(landing[1].index).toBe(1);
  });

  it('starts the chain one frontier gap past the NEXT card', () => {
    expect(G.chainX0).toBe(G.nextX + CARD_W + G.frontierGap);
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

  it('puts both ticks in the middle of the gap between neighbouring cards', () => {
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

  it('lets the window run off the last card when its far edge is not on screen', () => {
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

  it('marks the window at rest with a short gold mark across the chain line at each tick', () => {
    const ruler = { ticks: [480, 1461], label: null, hit: null, tailOpen: false };
    // chainY 37: each mark runs from 4.5 px above the line to 5.5 px below it.
    expect(windowMarksPath(ruler)).toBe('M480.0 32.5V42.5M1461.0 32.5V42.5');
  });

  it('raises the ticks to the dimension line when hovered and breaks the line around the label', () => {
    const ruler = windowRuler(slots, 101, MAINNET, 2000, textOf(101));
    const d = rulerPath(ruler);
    const [xn, xf] = ruler.ticks;
    // Both ticks drop from above the dimension line onto the header row.
    expect(d).toContain(`M${xn.toFixed(1)} ${G.winY - 5}V${G.cardTop + G.headH}`);
    expect(d).toContain(`M${xf.toFixed(1)} ${G.winY - 5}V${G.cardTop + G.headH}`);
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
  // The 1280 px container: NEXT, then 8 blocks; the window of a block runs right, towards older ones.
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
    // T−3: near tick at 916, far end past the edge; 1246 − 916 = 330 ≥ 294 + 2 · 6.
    const { text, ruler } = place(TIP - 3);
    expect(labelWidth(text)).toBe(294);
    expect(ruler.ticks).toEqual([916]);
    expect(ruler.label).toEqual({
      x: (916 + W) / 2,
      x0: (916 + W) / 2 - 153,
      x1: (916 + W) / 2 + 153,
      at: 'between',
    });
    const [l, r] = textBox(ruler.label as WindowLabelGeom, text);
    expect(l).toBeGreaterThan(916);
    expect(r).toBeLessThanOrEqual(W);
  });

  it('puts it outside the window, before the near tick, when the window in view is too short', () => {
    // T−4 and T−5: near ticks at 1025 and 1134; the label (294 px + padding) no longer fits.
    for (const [H, xn] of [
      [TIP - 4, 1025],
      [TIP - 5, 1134],
    ]) {
      const { text, ruler } = place(H);
      expect(ruler.ticks).toEqual([xn]);
      expect(ruler.label).toEqual({ x: xn - 153, x0: xn - 306, x1: xn, at: 'outside' });
      const [l, r] = textBox(ruler.label as WindowLabelGeom, text);
      expect(l).toBeGreaterThanOrEqual(0);
      expect(r).toBe(xn - 6);
      // The dimension line is then drawn whole, from the tick to the view edge.
      expect(rulerPath(ruler)).toBe(
        `M${xn.toFixed(1)} ${G.winY - 5}V${G.cardTop + G.headH}M${xn.toFixed(1)} ${G.winY}H${W.toFixed(1)}`
      );
    }
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
    // The 1246 px row read through a 760 px view: the window of T−4 starts at 1025, off view.
    const text = windowLabel(TIP - 4, TIP, MAINNET);
    const ruler = windowRuler(slots, TIP - 4, MAINNET, 760, text);
    expect(ruler.ticks).toEqual([1025]);
    expect(ruler.label).toBeNull();
    // The ruler itself is still drawn (and clipped by the view).
    expect(rulerPath(ruler)).toContain('M1025.0 16H760.0');
  });
});

describe('a narrower view: fewer blocks, all of them whole', () => {
  it.each([760, 1000, CONTAINER_VIEW_W])(
    'at %i px: no card past the edge, a forward tail, every window label in view and off its ticks',
    (W) => {
      const s = buildPipelineSnapshot();
      const scene = buildScene(s, W, 'settled');
      const next = scene.next as NonNullable<Scene['next']>;
      for (const c of scene.cards) expect(c.x + c.w).toBeLessThanOrEqual(W - G.marginR);
      const last = scene.cards[scene.cards.length - 1];
      expect(chainTail(scene.cards, W)).toEqual({ x0: last.x + last.w + 2, x1: W - 4 });
      expect(last.x + last.w + 2).toBeLessThan(W - 4);
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
          // Only a window whose near end is past the last card has no label (and no ruler).
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

describe('chainPaths: the chain line', () => {
  // NEXT at 248, then #100 … #85 every 109 px, all linked parent to child.
  const cards: ChainSlotState[] = Array.from({ length: 16 }, (_, i) => ({
    number: 100 - i,
    x: G.chainX0 + i * (CARD_W + G.gap),
    w: CARD_W,
    mined: true,
    hash: `h${100 - i}`,
    parentHash: `h${99 - i}`,
    solid: 1,
  }));
  const next = { x: G.nextX, w: CARD_W };

  it('is plain horizontal segments through the headers: no arrowheads anywhere', () => {
    const paths = chainPaths(cards, next, 101, MAINNET, 2000);
    expect(Object.keys(paths).sort()).toEqual(['pend', 'solid', 'tail', 'win']);
    const y = (G.chainY + 0.5).toFixed(1);
    for (const d of Object.values(paths)) {
      expect(d).toMatch(new RegExp(`^(M[\\d.]+ ${y}L[\\d.]+ ${y})*$`));
    }
  });

  it('is dashed from NEXT to the tip, gold inside the window and solid outside it', () => {
    const paths = chainPaths(cards, next, 101, MAINNET, 2000);
    const y = (G.chainY + 0.5).toFixed(1);
    expect(paths.pend).toBe(
      `M${(G.nextX + CARD_W + 2).toFixed(1)} ${y}L${(G.chainX0 - 2).toFixed(1)} ${y}`
    );
    // The window of #101 is #91 … #99: the eight links between them are gold.
    expect(paths.win.match(/M/g)).toHaveLength(8);
    // #100 → #99 and #91 → #90 … #86 → #85 lie outside it.
    expect(paths.solid.match(/M/g)).toHaveLength(7);
    expect(paths.tail).toMatch(new RegExp(`L1996 ${y}$`));
  });

  it('breaks where the newer card does not name the older one as its parent', () => {
    const forked = cards.map((c) => (c.number === 95 ? { ...c, parentHash: 'elsewhere' } : c));
    const paths = chainPaths(forked, next, 101, MAINNET, 2000);
    expect(paths.win.match(/M/g)).toHaveLength(7);
  });

  it('ends in a dotted tail from just past the last card to 4 px before the view edge', () => {
    const tail = chainTail(cards, 2000);
    const last = cards[cards.length - 1];
    expect(tail).toEqual({ x0: last.x + CARD_W + 2, x1: 1996 });
    const y = (G.chainY + 0.5).toFixed(1);
    expect(chainPaths(cards, next, 101, MAINNET, 2000).tail).toBe(
      `M${(last.x + CARD_W + 2).toFixed(1)} ${y}L1996 ${y}`
    );
    expect(chainTail([], 2000)).toBeNull();
  });
});

describe('rewardLink: a cellbase to the block it pays', () => {
  const a = { X: 390.5, Y: 76.5 };
  const tail = { x0: 1235, x1: 1242 };

  it('drops into the paid cellbase when it is on screen, without a tail', () => {
    const b = { X: 1589.5, Y: 76.5 };
    // Up into the lane at winY (16), a 5 px bend, along the lane, a 5 px bend, down into it.
    expect(rewardLink(a, b, tail)).toEqual({
      d: 'M390.5 76.5V21Q390.5 16 395.5 16H1584.5Q1589.5 16 1589.5 21V76.5',
      tail: '',
    });
  });

  it('runs right along the lane to the chain tail and ends there open, dotted to the edge', () => {
    const link = rewardLink(a, null, tail);
    expect(link.d).toBe('M390.5 76.5V21Q390.5 16 395.5 16H1235.0');
    // It never drops anywhere: the last command stays in the lane.
    expect(link.d).not.toMatch(/H[\d.]+Q|V[\d.]+$/);
    // The open end: the span of the chain line's own off-screen tail, one lane up.
    expect(link.tail).toBe(`M1235.0 ${G.winY}H1242.0`);
  });

  it('refuses a tail that starts before the cellbase has turned into the lane', () => {
    expect(() => rewardLink(a, null, { x0: 392, x1: 1242 })).toThrow(/tail at 392/);
  });
});

describe('rewardTarget: where a reward link ends', () => {
  const s = buildPipelineSnapshot();
  const at = (scene: Scene, n: number) => cardOf(scene, n);

  it('is the paid block on screen in a wide view, and off screen in the 1280 px container', () => {
    const wide = buildScene(s, 2000, 'settled');
    expect(rewardTarget(wide.cards, at(wide, T))).toBe(at(wide, T - 11));
    const fit = buildScene(s, CONTAINER_VIEW_W, 'settled');
    // tip − 7 is the last block on screen: tip − 11 is past the right edge.
    expect(fit.cards[fit.cards.length - 1].block.number).toBe(T - 7);
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

  it('rejects a paid block inside the range on screen that has no card', () => {
    const scene = buildScene(s, CONTAINER_VIEW_W, 'settled');
    const c = at(scene, T);
    const pays = { number: T - 3, hash: at(scene, T - 3).id, reward: rewardOf(T - 3) };
    const payer = { ...c, block: { ...c.block, cellbase: { ...c.block.cellbase, pays } } };
    const gap = scene.cards.filter((k) => k.block.number !== T - 3);
    expect(() => rewardTarget(gap, payer)).toThrow(
      new RegExp(`pays #${T - 3}, which should be on screen \\(down to #${T - 7}\\) but is not`)
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

  it('snaps to a proposal chip within 4 px of its centre', () => {
    expect(pick(targets, 202.5 + 3, 82.5)).toEqual({ kind: 'chip', card: 'blk', key: 'chip0' });
    expect(pick(targets, 202.5 + 5, 82.5)).toBeNull();
  });
});

describe('chipSlots: proposal dots that fit, with room for "+N"', () => {
  it('shows every chip when they fit', () => {
    expect(chipSlots(48, 48)).toEqual({ shown: 48, overflow: 0 });
    expect(chipSlots(0, 48)).toEqual({ shown: 0, overflow: 0 });
  });

  it('frees one dot slot per label glyph at the end of the last row', () => {
    // "+255" is 4 glyphs of 6 px, each under one 8 px pitch.
    expect(chipSlots(299, 48)).toEqual({ shown: 44, overflow: 255 });
    // A one-digit "+N" takes two slots.
    expect(chipSlots(51, 48)).toEqual({ shown: 46, overflow: 5 });
    expect(chipSlots(49, 48)).toEqual({ shown: 46, overflow: 3 });
    const { shown, overflow } = chipSlots(1500, 48);
    expect(shown + overflow).toBe(1500);
    expect(48 - shown).toBeGreaterThanOrEqual(String(overflow).length + 1);
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
      { id: null, x: G.padX, y: G.fieldTop, s: U - 1, cellbase: true },
    ]);
    expect(next.hidden).toBe(0);
  });

  it('draws a predicted tx that is not in the truncated pool list, and counts omitted ones as +N', () => {
    const outside = `0x${'ab'.repeat(32)}`;
    const scene = buildScene(
      withNext({
        txs: [
          { hash: outside, size: 9000 },
          { hash: poolTxHash('w0'), size: 1600 },
        ],
        omittedTxs: 3,
        bytes: 10_600,
        cycles: 5_000_000,
        proposals: 5,
      }),
      1600,
      'settled'
    );
    const next = scene.next as NonNullable<typeof scene.next>;
    const big = next.ghosts.find((g) => g.id === outside);
    expect(big?.s).toBe(4 * U - 1);
    expect(next.ghosts.find((g) => g.id === poolTxHash('w0'))?.s).toBe(2 * U - 1);
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
