import { describe, expect, it } from 'vitest';
import {
  G,
  U,
  UNC,
  cardCols,
  cardH,
  cardW,
  chainLayout,
  chipSlots,
  chipsPerRow,
  inWindow,
  pack2D,
  pick,
  uncleBoxH,
  uncleChipsPerRow,
  unclesH,
  windowRange,
  windowRuler,
  type PickTargets,
  type RulerSlot,
} from '@/lib/pipeline/layout';

const MAINNET = { proposalWindowClosest: 2, proposalWindowFarthest: 10 };

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

describe('card width', () => {
  it('grows with the number of proposals so they fit in three chip rows', () => {
    const few = cardCols([1], 3);
    const many = cardCols([1], 40);
    expect(few).toBe(G.minCols);
    expect(chipsPerRow(many) * G.propRows).toBeGreaterThanOrEqual(40);
    expect(many).toBeGreaterThan(few);
  });

  it('is at least 11 columns when the block carries an uncle', () => {
    expect(cardCols([1], 0, UNC.minCols)).toBe(11);
  });

  it('never exceeds 14 columns', () => {
    expect(cardCols(Array(400).fill(1), 2000)).toBe(G.maxCols);
    expect(cardW(G.maxCols)).toBe(G.maxCols * U - 1 + 2 * G.padX);
  });
});

describe('uncle box', () => {
  it('is a header only without proposals, one chip row, then two at most', () => {
    const cols = 11;
    const per = uncleChipsPerRow(cols);
    expect(uncleBoxH(cols, 0)).toBe(UNC.head + UNC.padB);
    expect(uncleBoxH(cols, 1)).toBe(UNC.head + G.chipP + UNC.padB);
    expect(uncleBoxH(cols, per)).toBe(UNC.head + G.chipP + UNC.padB);
    expect(uncleBoxH(cols, per + 1)).toBe(UNC.head + 2 * G.chipP + UNC.padB);
    expect(uncleBoxH(cols, per * 5)).toBe(UNC.head + 2 * G.chipP + UNC.padB);
  });

  it('adds its area to the card height', () => {
    const extra = unclesH(11, [2]);
    expect(extra).toBe(UNC.top + uncleBoxH(11, 2) + UNC.gap);
    expect(cardH(3, extra) - cardH(3)).toBe(extra);
    expect(unclesH(11, [])).toBe(0);
  });
});

describe('chainLayout', () => {
  const cards = (n: number, cols = 6) => Array.from({ length: n }, () => ({ cols }));

  it('stops at the view width once 12 cards are shown', () => {
    const slots = chainLayout(cards(24), 1600, false);
    expect(slots.length).toBeGreaterThanOrEqual(12);
    expect(slots.length).toBeLessThan(24);
    const last = slots[slots.length - 1];
    expect(last.x + last.w).toBeLessThanOrEqual(1600 - 10);
    const nextX = last.x + last.w + G.gap;
    expect(nextX + cardW(6)).toBeGreaterThan(1600 - 10);
    expect(slots[0].x).toBe(G.chainX0);
  });

  it('always shows at least 12 cards, even past a narrow view', () => {
    expect(chainLayout(cards(24, 14), 760, false)).toHaveLength(12);
  });

  it('never shows more than 24 cards', () => {
    expect(chainLayout(cards(30, 6), 100_000, false)).toHaveLength(G.maxBlocks);
  });

  it('puts a landing block at the NEXT position and keeps the chain slots of the others', () => {
    const settled = chainLayout(cards(13), 2400, false);
    const landing = chainLayout(cards(14), 2400, true);
    expect(landing[0]).toMatchObject({ index: 0, x: G.nextX, slot: 'landing' });
    expect(landing[1].x).toBe(settled[0].x);
    expect(landing[1].index).toBe(1);
  });

  it('starts the chain past the widest NEXT card', () => {
    expect(G.chainX0).toBe(G.nextX + cardW(G.maxCols) + G.gap);
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
