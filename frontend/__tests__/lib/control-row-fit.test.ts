import { describe, expect, it } from 'vitest';
import {
  chipRowFits,
  chipRowPx,
  NARROWEST_LINE_PX,
  tabRowPx,
  tabRowShowsCounts,
  type ChipRowItem,
  type TabRowItem,
} from '@/lib/control-row-fit';
import { containerContentPx } from '@/__tests__/utils/tailwind-box';

/**
 * The phone rule the owner passed with the controls board (gate D0′, 2026-11-19): when a tab row is
 * wider than its line the counts leave the row; a name is never cut and the row never scrolls or
 * wraps. The board's arithmetic (`tabRowPx` of src/d0/controls.js), each figure written here: a
 * name 0.6 em of 14 px (8.4 a glyph) below sm and of 17 px (10.2) from it, a count 0.6 em of 12 px
 * (7.2) and its 6 px margin, the prompt square 8 px pulled 8 (12 from sm) toward the first name,
 * a gap of 16 px between tabs (22 from sm).
 */
const ADDRESS: TabRowItem[] = [
  { label: 'activities', count: 3 },
  { label: 'transactions', count: 3 },
  { label: 'live cells', count: 3 },
];
const INVENTORY: TabRowItem[] = [
  { label: 'tokens', count: 727 },
  { label: 'objects', count: 5653 },
  { label: 'identities', count: 4 },
];

describe('the tab row’s width (the board’s tabRowPx)', () => {
  it('is the address page’s 356.4 px with its counts and 316.8 without, at the body step', () => {
    expect(tabRowPx(ADDRESS, 'body', true)).toEqual({
      names: 268.8,
      counts: 39.6,
      chrome: 48,
      withCounts: 356.4,
      bare: 316.8,
    });
  });

  it('is the inventory’s 324 px with its counts, at the body step', () => {
    expect(tabRowPx(INVENTORY, 'body', true)).toEqual({
      names: 193.2,
      counts: 82.8,
      chrome: 48,
      withCounts: 324,
      bare: 241.2,
    });
  });

  it('sets the names at 17 px from sm, with the wider gap and pull', () => {
    // 32 glyphs at 10.2 = 326.4; the square 8 − 12 + 22 = 18 and two gaps of 22
    expect(tabRowPx(ADDRESS, 'section', true)).toEqual({
      names: 326.4,
      counts: 39.6,
      chrome: 62,
      withCounts: 428,
      bare: 388.4,
    });
  });

  it('draws no square and no chrome before the first name outside a command line', () => {
    expect(tabRowPx(ADDRESS, 'body', false).chrome).toBe(32);
    expect(tabRowPx([{ label: 'all' }], 'body', false)).toEqual({
      names: 25.2,
      counts: 0,
      chrome: 0,
      withCounts: 25.2,
      bare: 25.2,
    });
  });

  it('measures a count as it is written, grouped, and refuses one that is not a count', () => {
    // `5,653`: five glyphs at 7.2 and the 6 px margin
    expect(tabRowPx([{ label: 'a', count: 5653 }], 'body', false).counts).toBe(42);
    expect(tabRowPx([{ label: 'a', count: 0 }], 'body', false).counts).toBe(13.2);
    expect(() => tabRowPx([{ label: 'a', count: -1 }], 'body', false)).toThrow('Invalid count -1');
    expect(() => tabRowPx([{ label: 'a', count: 1.5 }], 'body', false)).toThrow(
      'Invalid count 1.5'
    );
  });
});

describe('the counts leave a row wider than its line (the owner’s phone rule)', () => {
  // the line a full-width row stands on: the container's content at the window
  const line = (window: number) => containerContentPx(window);

  it('reads the lines of 360, 390 and 640 px windows as 328, 358 and 608', () => {
    expect([360, 390, 640].map(line)).toEqual([328, 358, 608]);
    expect(NARROWEST_LINE_PX).toBe(line(360));
  });

  it('drops the address page’s counts at 360 (356.4 in 328) and keeps them at 390 and 640', () => {
    expect(tabRowShowsCounts(ADDRESS, 'body', true, line(360))).toBe(false);
    expect(tabRowShowsCounts(ADDRESS, 'body', true, line(390))).toBe(true);
    expect(tabRowShowsCounts(ADDRESS, 'section', true, line(640))).toBe(true);
  });

  it('keeps the inventory’s counts at 360 (324 in 328), and drops them one digit later', () => {
    expect(tabRowShowsCounts(INVENTORY, 'body', true, line(360))).toBe(true);
    const more = [INVENTORY[0], { label: 'objects', count: 56530 }, INVENTORY[2]];
    // one more glyph: 324 + 7.2 = 331.2
    expect(tabRowPx(more, 'body', true).withCounts).toBe(331.2);
    expect(tabRowShowsCounts(more, 'body', true, line(360))).toBe(false);
    expect(tabRowShowsCounts(more, 'body', true, line(390))).toBe(true);
  });

  it('draws the counts before the line is measured', () => {
    expect(tabRowShowsCounts(ADDRESS, 'body', true, null)).toBe(true);
  });

  it('throws when the names alone do not fit the narrowest line: a design fault', () => {
    const four = [...ADDRESS, { label: 'names', count: 1 }];
    // 37 glyphs at 8.4 = 310.8, the square 16 and three gaps 48: 374.8 in 328
    expect(() => tabRowShowsCounts(four, 'section', true, line(1280))).toThrow(
      'tab row "activities · transactions · live cells · names" is 374.8 px without its counts at 14 px: wider than the 328 px line of a 360 px window'
    );
    expect(() => tabRowPx(four, 'body', true)).not.toThrow();
  });
});

/**
 * Chips or a select (the controls board's pictures at 390; the coordinator's follow-up of lane C1):
 * below 640 a filter's values are chips when their row fits one line of the narrowest phone, the
 * 328 px line of a 360 px window, and a select after the filter's label when it does not; from 640
 * they are chips. A chip is its 1 px frame and 10 px padding each side (22), its 7 px mark and its
 * 6 px gap where it has one, its label at 13 px (7.8 a glyph), its count at 12 px (7.2) after a
 * 6 px gap; chips stand 6 px apart. Each figure is written here.
 */
const ADDRESS_KINDS: ChipRowItem[] = [
  { label: 'all', count: 3 },
  { label: 'ckb', count: 2 },
  { label: 'dao', count: 1, mark: 'gold' },
];
const STANDARDS: ChipRowItem[] = [
  { label: 'all', count: 727 },
  { label: 'xudt', count: 629 },
  { label: 'sudt', count: 94 },
  { label: 'xudt-compatible', count: 4 },
];
const KINDS: ChipRowItem[] = [
  { label: 'all', count: 6122 },
  { label: 'ckb transfers', count: 683, mark: 'jade' },
  { label: 'script calls', count: 5425, mark: 'slate-ckb' },
  { label: 'nervos dao', count: 11, mark: 'gold' },
  { label: 'tokens', count: 3, mark: 'token' },
  { label: 'objects', count: 0, mark: 'object' },
];

describe('a row of filter chips fits the narrowest phone, or is a select', () => {
  it('measures the address page’s three kinds at 200.8 px: chips', () => {
    // all 3: 22 + 23.4 + 6 + 7.2 = 58.6; ckb 2: 58.6; dao 1 with its mark: 22 + 7 + 6 + 23.4 +
    // 13.2 = 71.6; two gaps of 6
    expect(chipRowPx(ADDRESS_KINDS)).toBe(200.8);
    expect(chipRowFits(ADDRESS_KINDS)).toBe(true);
  });

  it('measures the inventory’s four standards at 397.6 px: a select', () => {
    // all 727: 22 + 23.4 + 6 + 21.6 = 73; xudt 629: 22 + 31.2 + 27.6 = 80.8; sudt 94: 22 + 31.2 +
    // 6 + 14.4 = 73.6; xudt-compatible 4: 22 + 117 + 6 + 7.2 = 152.2; three gaps of 6
    expect(chipRowPx(STANDARDS)).toBe(397.6);
    expect(chipRowFits(STANDARDS)).toBe(false);
  });

  it('measures the six kinds at 783.2 px: a select', () => {
    // all 6,122: 87.4; with their marks (7 + 6): ckb transfers 683, 164; script calls 5,425, 170.6;
    // nervos dao 11, 133.4; tokens 3, 95; objects 0, 102.8; five gaps of 6
    expect(chipRowPx(KINDS)).toBe(783.2);
    expect(chipRowFits(KINDS)).toBe(false);
  });

  it('keeps a row 0.6 px inside the line as chips, and makes one 0.6 px over it a select', () => {
    // four chips: 4 × 22 + 3 × 6 = 106; 23 glyphs of label, 179.4; one count of five glyphs, 42
    const inside: ChipRowItem[] = [
      { label: 'all', count: 1234 },
      { label: 'tokens' },
      { label: 'objects' },
      { label: 'domains' },
    ];
    expect(chipRowPx(inside)).toBe(327.4);
    expect(chipRowFits(inside)).toBe(true);
    // 25 glyphs of label, 195; one count of three glyphs, 27.6
    const over: ChipRowItem[] = [
      { label: 'all', count: 727 },
      { label: 'tokens' },
      { label: 'objects' },
      { label: 'addresses' },
    ];
    expect(chipRowPx(over)).toBe(328.6);
    expect(chipRowFits(over)).toBe(false);
    expect(NARROWEST_LINE_PX).toBe(328);
  });

  it('refuses a count that is not a count', () => {
    expect(() => chipRowPx([{ label: 'all', count: -1 }])).toThrow('Invalid count -1');
  });
});
