import { describe, expect, it } from 'vitest';
import {
  FIXTURE_TIP,
  buildPipelineSnapshot,
  committedTxHash,
  mineNext,
  poolTxHash,
  shortIdOf,
} from '@/__tests__/msw/fixtures/pipeline-snapshot';
import type { PipelineSnapshot, PipelineTx } from '@/lib/api';
import {
  BLOCK_FIELD_CAPACITY,
  G,
  POOL_PENDING_CAPACITY,
  POOL_PROPOSED_CAPACITY,
  U,
  UNC,
  UNCLE_CHIPS_PER_ROW,
  buildScene,
  cardH,
  chainLayout,
  chipSlots,
  inWindow,
  pack2D,
  pick,
  uncleBoxH,
  unclesH,
  windowRange,
  windowRuler,
  type PickTargets,
  type RulerSlot,
  type Scene,
} from '@/lib/pipeline/layout';

const MAINNET = { proposalWindowClosest: 2, proposalWindowFarthest: 10 };
const T = FIXTURE_TIP;
/** The card width of the design: 9 columns of the 8 px grid plus the side padding. */
const CARD_W = 81;

function cardOf(scene: Scene, n: number) {
  const c = scene.cards.find((k) => k.block.number === n);
  if (!c) throw new Error(`card #${n} is not on screen`);
  return c;
}

/** A card's height for a tx field of `rows` rows and no uncles. */
const heightForRows = (rows: number) => G.fieldTop - G.cardTop + rows * U - 1 + G.padB;

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
    const { pos, hidden, rows } = pack2D(items, 9, 17, true);
    const cells = occupied(pos);
    expect(cells.has('0,0')).toBe(false);
    expect(hidden).toBe(0);
    expect(pos).toHaveLength(items.length);
    for (const p of pos) {
      expect(p.c + p.n).toBeLessThanOrEqual(9);
      expect(p.r + p.n).toBeLessThanOrEqual(rows);
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

describe('grid capacities', () => {
  it('are the 1×1 cells of the pool grids and of a block field on the 8 px grid', () => {
    expect(U).toBe(8);
    // The backend truncates its lists to these: pending 10 × 8, proposed 10 × 12, block 9 × 14.
    expect(POOL_PENDING_CAPACITY).toBe(80);
    expect(POOL_PROPOSED_CAPACITY).toBe(120);
    expect(BLOCK_FIELD_CAPACITY).toBe(126);
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

  it('gives a block with 0–2 txs a 3-row tx field', () => {
    const quiet = scene.cards.filter((c) => !c.block.uncles.length && c.block.txs.length <= 2);
    expect(new Set(quiet.map((c) => c.block.txs.length))).toEqual(new Set([0, 1, 2]));
    for (const c of quiet) expect(c.h).toBe(heightForRows(3));
  });

  it('grows the tx field past 3 rows as far as the field allows', () => {
    expect(cardOf(scene, T - 5).h).toBe(heightForRows(G.fieldRows));
  });

  it('shows "+N" for the 1×1 txs that do not fit the 9 × 14 field', () => {
    const s = buildPipelineSnapshot();
    const n = T - 8; // no txs, proposals or uncles in the fixture
    const txs: PipelineTx[] = Array.from({ length: 200 }, (_, i) => ({
      hash: committedTxHash(n, i),
      size: 330,
      fee: '330',
      feeRate: '1000',
      cycles: 1_000_000,
      cyclesStatus: 'done',
      proposedIn: { block: n - 2, uncleIndex: null },
    }));
    const blocks = s.blocks.map((b) =>
      b.number === n ? { ...b, txs, txCount: 1 + txs.length, omittedTxs: 0 } : b
    );
    const card = cardOf(buildScene({ ...s, blocks }, 1600, 'settled'), n);
    // 9 × 14 = 126 cells: the cellbase and 125 txs; the other 75 are counted, not drawn.
    expect(card.squares).toHaveLength(126);
    expect(card.squares.every((q) => q.x + q.s <= card.w - G.padX)).toBe(true);
    expect(card.hidden).toBe(75);
    expect(card.w).toBe(CARD_W);
  });
});

describe('uncle box', () => {
  it('is a header only without proposals, one chip row, then two at most', () => {
    const per = UNCLE_CHIPS_PER_ROW;
    expect(uncleBoxH(0)).toBe(UNC.head + UNC.padB);
    expect(uncleBoxH(1)).toBe(UNC.head + G.chipP + UNC.padB);
    expect(uncleBoxH(per)).toBe(UNC.head + G.chipP + UNC.padB);
    expect(uncleBoxH(per + 1)).toBe(UNC.head + 2 * G.chipP + UNC.padB);
    expect(uncleBoxH(per * 5)).toBe(UNC.head + 2 * G.chipP + UNC.padB);
  });

  it('adds its area to the card height', () => {
    const extra = unclesH([2]);
    expect(extra).toBe(UNC.top + uncleBoxH(2) + UNC.gap);
    expect(cardH(3, extra) - cardH(3)).toBe(extra);
    expect(unclesH([])).toBe(0);
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
    // Two rows of the mini-card are drawn; the rest is its "+N".
    expect(chips).toHaveLength(2 * UNCLE_CHIPS_PER_ROW);
    expect(card.uncles[0].overflow).toBe(32 - 2 * UNCLE_CHIPS_PER_ROW);
    for (const k of chips) {
      expect(k.x).toBeGreaterThanOrEqual(G.padX + 3);
      expect(k.x + G.chip).toBeLessThanOrEqual(card.w - G.padX - 3);
    }
  });
});

describe('chainLayout', () => {
  it('stops at the view width once at least 11 cards are shown', () => {
    const slots = chainLayout(24, 1600, false);
    expect(slots.length).toBeGreaterThanOrEqual(11);
    expect(slots.length).toBeLessThan(24);
    const last = slots[slots.length - 1];
    expect(last.x + last.w).toBeLessThanOrEqual(1600 - 10);
    expect(last.x + last.w + G.gap + CARD_W).toBeGreaterThan(1600 - 10);
    expect(slots[0].x).toBe(G.chainX0);
    expect(
      slots.every((sl, i) => sl.w === CARD_W && sl.x === G.chainX0 + i * (CARD_W + G.gap))
    ).toBe(true);
  });

  it('always shows at least 11 cards, even past a narrow view', () => {
    expect(chainLayout(24, 760, false)).toHaveLength(11);
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

  it('starts the chain one gap past the NEXT card', () => {
    expect(G.chainX0).toBe(G.nextX + CARD_W + G.gap);
  });
});

describe('proposal window', () => {
  it('is the heights [H − farthest, H − closest]', () => {
    expect(windowRange(100, MAINNET)).toEqual({ from: 90, to: 98 });
    expect(inWindow(90, 100, MAINNET)).toBe(true);
    expect(inWindow(98, 100, MAINNET)).toBe(true);
    expect(inWindow(99, 100, MAINNET)).toBe(false);
    expect(inWindow(89, 100, MAINNET)).toBe(false);
  });

  it('puts both ticks in the middle of the gap between neighbouring cards', () => {
    // NEXT is #101, then the chain #100 … #85, each 60 px wide with a 20 px gap.
    const slots: RulerSlot[] = [{ number: 101, x: 214, w: 60, mined: false }];
    for (let i = 0; i < 16; i++)
      slots.push({ number: 100 - i, x: 400 + i * 80, w: 60, mined: true });
    const ruler = windowRuler(slots, 101, MAINNET, 2000);
    const at = (n: number) => slots.find((s) => s.number === n) as RulerSlot;
    // Near edge: between #100 (H−1) and #99 (H−2). Far edge: between #91 (H−10) and #90 (H−11).
    const xn = (at(100).x + at(100).w + at(99).x) / 2;
    const xf = (at(91).x + at(91).w + at(90).x) / 2;
    expect(ruler.ticks).toEqual([xn, xf]);
    expect(ruler.labelX).toBe(xn + 3);
    expect(ruler.tailOpen).toBe(false);
  });

  it('lets the window run off the last card when its far edge is not on screen', () => {
    const slots: RulerSlot[] = [];
    for (let i = 0; i < 5; i++)
      slots.push({ number: 100 - i, x: 400 + i * 80, w: 60, mined: true });
    const ruler = windowRuler(slots, 101, MAINNET, 1000);
    expect(ruler.ticks).toHaveLength(1);
    expect(ruler.tailOpen).toBe(true);
    expect(ruler.hit?.xf).toBe(1000);
  });

  it('has no ruler when the near edge is not on screen', () => {
    const ruler = windowRuler([{ number: 100, x: 400, w: 60, mined: true }], 120, MAINNET, 1000);
    expect(ruler.ticks).toEqual([]);
    expect(ruler.labelX).toBeNull();
    expect(ruler.hit).toBeNull();
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

describe('chipSlots: proposal chips that fit, with room for "+N"', () => {
  it('shows every chip when they fit', () => {
    expect(chipSlots(48, 48)).toEqual({ shown: 48, overflow: 0 });
    expect(chipSlots(0, 48)).toEqual({ shown: 0, overflow: 0 });
  });

  it('frees one chip slot per label glyph at the end of the last row', () => {
    // "+255" is 4 glyphs of ~6 px = 4 chip pitches.
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
