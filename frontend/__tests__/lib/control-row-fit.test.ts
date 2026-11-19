import { describe, expect, it } from 'vitest';
import {
  CHIP_COUNT_CHARS,
  chipFormFits,
  chipFormPx,
  chipRowPx,
  chipRowShowsCounts,
  NARROWEST_LINE_PX,
  selectPairPx,
  selectWellUnderLabel,
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

// Lane cd-fixes-controls (L1 NOTE 11): a filter's phone form is decided from what does not change
// with the data: its labels, its marks, and whether it counts its values; a filter that counts
// leaves room for a count of CHIP_COUNT_CHARS glyphs (12 px, 7.2 a glyph, after a 6 px gap: 42 px)
// on every value. Its counts never decide it (they decided it before: the four standards were
// chips at 308.8 px while their counts loaded and a select at 397.6 px when they came).
// Re-pinned on purpose: `chipRowFits(values)`, the row as written, is `chipFormFits(values,
// counted)`; `chipRowPx` still measures the row as written.
describe('a filter’s phone form: chips where its row fits the narrowest phone, else a select', () => {
  it('leaves room for a count of five glyphs, up to 9,999 as written', () => {
    expect(CHIP_COUNT_CHARS).toBe(5);
  });

  it('measures the address page’s three kinds at 200.8 px, and with room for counts 287.2: chips', () => {
    // all 3: 22 + 23.4 + 6 + 7.2 = 58.6; ckb 2: 58.6; dao 1 with its mark: 22 + 7 + 6 + 23.4 +
    // 13.2 = 71.6; two gaps of 6. With room: all and ckb 22 + 23.4 + 42 = 87.4; dao 100.4
    expect(chipRowPx(ADDRESS_KINDS)).toBe(200.8);
    expect(chipFormPx(ADDRESS_KINDS, true)).toBe(287.2);
    expect(chipFormFits(ADDRESS_KINDS, true)).toBe(true);
  });

  it('measures the inventory’s four standards at 397.6 px, with room 476.8: a select, counts or none', () => {
    // all 727: 22 + 23.4 + 6 + 21.6 = 73; xudt 629: 22 + 31.2 + 27.6 = 80.8; sudt 94: 22 + 31.2 +
    // 6 + 14.4 = 73.6; xudt-compatible 4: 22 + 117 + 6 + 7.2 = 152.2; three gaps of 6. With room:
    // 4 × 22 + 202.8 of label + 4 × 42 + 18 = 476.8; with none, 308.8 (the chips it was while its
    // counts loaded)
    expect(chipRowPx(STANDARDS)).toBe(397.6);
    expect(chipFormPx(STANDARDS, true)).toBe(476.8);
    expect(chipFormFits(STANDARDS, true)).toBe(false);
    const loading = STANDARDS.map(({ label }) => ({ label, count: undefined }));
    expect(chipRowPx(loading)).toBe(308.8);
    expect(chipFormPx(loading, true)).toBe(476.8);
    expect(chipFormFits(loading, true)).toBe(false);
  });

  it('measures the six kinds at 783.2 px: a select', () => {
    // all 6,122: 87.4; with their marks (7 + 6): ckb transfers 683, 164; script calls 5,425, 170.6;
    // nervos dao 11, 133.4; tokens 3, 95; objects 0, 102.8; five gaps of 6
    expect(chipRowPx(KINDS)).toBe(783.2);
    // with room: 6 × 22 + 5 marks × 13 + 51 glyphs of label (397.8) + 6 × 42 + 5 × 6
    expect(chipFormPx(KINDS, true)).toBe(876.8);
    expect(chipFormFits(KINDS, true)).toBe(false);
  });

  it('keeps a row with room for its counts inside the line as chips, and one 0.8 px over it a select', () => {
    // three chips with room: 3 × (22 + 42) + 2 × 6 = 204; 15 glyphs of label, 117: 321
    const inside: ChipRowItem[] = [
      { label: 'all', count: 1234 },
      { label: 'tokens' },
      { label: 'domain' },
    ];
    expect(chipFormPx(inside, true)).toBe(321);
    expect(chipFormFits(inside, true)).toBe(true);
    // 16 glyphs, 124.8: 328.8
    const over: ChipRowItem[] = [
      { label: 'all', count: 727 },
      { label: 'tokens' },
      { label: 'domains' },
    ];
    expect(chipFormPx(over, true)).toBe(328.8);
    expect(chipFormFits(over, true)).toBe(false);
    expect(NARROWEST_LINE_PX).toBe(328);
  });

  it('leaves no room on a filter that counts nothing: the collection status at 195 px', () => {
    // all, live, inactive: 3 × 22 + 15 glyphs (117) + 2 × 6
    const status = [{ label: 'all' }, { label: 'live' }, { label: 'inactive' }];
    expect(chipFormPx(status, false)).toBe(195);
    expect(chipFormFits(status, false)).toBe(true);
    // a count on a filter that says it counts nothing is refused: its form left it no room
    expect(() => chipFormPx([{ label: 'all', count: 3 }], false)).toThrow(
      'a count (3) on a filter that does not count its values'
    );
  });

  it('draws the counts while the row as written holds the line; past it they leave the row', () => {
    expect(chipRowShowsCounts(ADDRESS_KINDS)).toBe(true);
    // a count wider than its room: all 123,456,789 (11 glyphs): 22 + 23.4 + 6 + 79.2 = 130.6; ckb
    // 98,765,432 (10): 123.4; dao 1: 71.6; two gaps: 337.6, past the 328 px line. Without its
    // counts the row is 161.2 px, one line, nothing cut
    const wide = [
      { label: 'all', count: 123_456_789 },
      { label: 'ckb', count: 98_765_432 },
      { label: 'dao', count: 1, mark: 'gold' },
    ];
    expect(chipRowPx(wide)).toBe(337.6);
    expect(chipRowShowsCounts(wide)).toBe(false);
    expect(chipRowPx(wide.map(({ label, mark }) => ({ label, mark })))).toBe(161.2);
  });

  it('refuses a count that is not a count', () => {
    expect(() => chipRowPx([{ label: 'all', count: -1 }])).toThrow('Invalid count -1');
  });
});

/**
 * A select never cuts its value (lane c2-finish, item 2): a native select is as wide as its widest
 * option and clips its chosen value, with no mark, where its well is narrower (at 390 an object's
 * 'Version 1 · block #15,064,087' showed as '… #15,064,08', another number). So the well never
 * shrinks: the pair, label and well, stands on one line where it fits its line, the well goes
 * under its label where it does not, and an option wider than the narrowest line even alone is a
 * naming fault the caller sees. The arithmetic: the label at 13 px (7.8 a glyph), the gap 10, the
 * well its 1 px frame each side, the select's 10 px before the value and 25 after it (the
 * chevron), the widest option at 13 px.
 */
describe('a select’s pair stands on one line, or its well goes under its label', () => {
  const VERSION = ['1 · #15,064,087', '12 · #21,016,901 · current'];

  it('measures the object page’s version select at 304.4 px: on one line at 360', () => {
    // version: 7 × 7.8 = 54.6; the widest option, 26 glyphs, 202.8 and 37 of well: 239.8
    expect(selectPairPx('version', VERSION)).toEqual({ label: 54.6, well: 239.8, pair: 304.4 });
    expect(selectWellUnderLabel('version', VERSION, containerContentPx(360))).toBe(false);
  });

  it('puts the well under its label where the pair is wider than its line', () => {
    // cell version: 12 × 7.8 = 93.6, the pair 343.4: past 328, inside 358
    expect(selectPairPx('cell version', VERSION).pair).toBe(343.4);
    expect(selectWellUnderLabel('cell version', VERSION, 328)).toBe(true);
    expect(selectWellUnderLabel('cell version', VERSION, 358)).toBe(false);
    // exactly the line: on it
    expect(selectWellUnderLabel('cell version', VERSION, 343.4)).toBe(false);
  });

  it('stands on one line before its line is measured', () => {
    expect(selectWellUnderLabel('cell version', VERSION, null)).toBe(false);
  });

  it('measures the widest option, and counts a grapheme outside the served Latin at 1.25 em', () => {
    // 'ckb held in its cells', 21 glyphs: 163.8 + 37 = 200.8; sorted by: 70.2
    expect(selectPairPx('sorted by', ['name', 'ckb held in its cells', 'holders'])).toEqual({
      label: 70.2,
      well: 200.8,
      pair: 281,
    });
    // '拯救民主', four graphemes at 1.25 em of 13 px: 65 + 37 = 102 (a Latin run of four: 68.2)
    expect(selectPairPx('class', ['拯救民主']).well).toBe(102);
  });

  it('throws for an option wider than the narrowest line even alone: a naming fault', () => {
    // 37 glyphs: 288.6 + 37 = 325.6, inside 328; 38: 333.4, past it
    const fits = 'x'.repeat(37);
    expect(selectPairPx('state', [fits]).well).toBe(325.6);
    expect(selectWellUnderLabel('state', [fits], 328)).toBe(true);
    const over = 'x'.repeat(38);
    expect(() => selectWellUnderLabel('state', ['all', over], 1232)).toThrow(
      `select "state": its option "${over}" makes a well 333.4 px wide, wider than the 328 px line of a 360 px window`
    );
    expect(() => selectWellUnderLabel('state', [over], null)).toThrow(/333\.4 px/);
  });

  it('refuses a select with no option', () => {
    expect(() => selectPairPx('state', [])).toThrow('select "state": no option to measure');
  });
});
