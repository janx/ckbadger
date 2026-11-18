import { describe, expect, it } from 'vitest';
import { WIDEST } from '@/__tests__/utils/shell-budget';
import { clockText } from '@/lib/clock';
import { STATUS_KEY_HINTS } from '@/lib/shortcuts';
import {
  CLOCK_FORM,
  STATUS_LINE_INSET_PX,
  fitStatusLine,
  statusLinePx,
  type StatusLineContent,
  type StatusLineForm,
  type StatusLineParts,
} from '@/lib/status-line-fit';

/**
 * The status line's budget per width (readability plan 2026-11-18, lane F3-f; docs/RESPONSIVE_LAYOUT.md
 * "Status line"): at the meta step, 12 px, a glyph 7.2 px; the mode block 20 px of inset, a segment
 * 25 (px-3 each side and its line) and 6 between its runs. The parts measure, for the record: LIVE
 * 48.8, SYNCING / OFFLINE 70.4; mainnet / testnet 75.4; `tip #21,045,159` 131.8 (139 with a ninth
 * digit), the number alone 104.2 (111.4); `ckbadger 0.8.2 · synced` 190.6, `· syncing` 197.8; the
 * key hints 67, 81.4 and 95.8; the clock 82.6. Chrome measured the first five on the new-size build
 * at 49, 75, 132, 191 and 327 (the hints and the clock), and the whole line at 773: the one test
 * that asserts those readings says so in its title; every other number here is the model's.
 */

type Case = { mode: 'LIVE' | 'SYNCING'; digits: 8 | 9 };

/** The line's content: mode, network, an eight- or nine-digit tip, the release as built. */
function content({ mode, digits }: Case, network: string = 'mainnet'): StatusLineContent {
  return {
    mode,
    network,
    tip: digits === 9 ? WIDEST.block : '#21,045,159',
    release: `ckbadger 0.8.2 · ${mode === 'SYNCING' ? 'syncing' : 'synced'}`,
    hints: STATUS_KEY_HINTS,
  };
}

const CASES: Case[] = [
  { mode: 'LIVE', digits: 8 },
  { mode: 'LIVE', digits: 9 },
  { mode: 'SYNCING', digits: 8 },
  { mode: 'SYNCING', digits: 9 },
];

const ALL: StatusLineParts = { network: true, tip: 'labelled', release: true, hints: 3 };
const NARROW: StatusLineParts = { network: true, tip: 'labelled', release: false, hints: 0 };

/** Per width, its form and the parts drawn for each case of CASES, in order. */
const TABLE: [number, StatusLineForm, StatusLineParts[]][] = [
  [1280, 'wide', [ALL, ALL, ALL, ALL]],
  [1024, 'wide', [ALL, ALL, ALL, ALL]],
  // 773.4 / 780.6 / 802.2 / 809.4 px of parts: `g b blocks` leaves (677.6 … 713.6 px)
  [768, 'wide', Array(4).fill({ ...ALL, hints: 2 })],
  [640, 'narrow', [NARROW, NARROW, NARROW, NARROW]],
  [390, 'narrow', [NARROW, NARROW, NARROW, NARROW]],
  // 338.6 / 345.8 hold; 360.2 / 367.4 give up the tip's word (332.6 / 339.8)
  [360, 'narrow', [NARROW, NARROW, { ...NARROW, tip: 'number' }, { ...NARROW, tip: 'number' }]],
  // LIVE: the number alone (311 / 318.2); SYNCING: 332.6 / 339.8 still too wide, the tip leaves
  // (228.4)
  [
    320,
    'narrow',
    [
      { ...NARROW, tip: 'number' },
      { ...NARROW, tip: 'number' },
      { ...NARROW, tip: null },
      { ...NARROW, tip: null },
    ],
  ],
];

describe('fitStatusLine: the parts the line draws at each width', () => {
  it.each(TABLE)('at %i px (%s form)', (width, form, expected) => {
    // one network or two: the line names the active one, and the names the code knows are seven
    // letters each, so the line is the same
    for (const network of WIDEST.networks)
      CASES.forEach((c, i) => {
        const line = content(c, network);
        const parts = fitStatusLine(form, line, width);
        expect(parts, `${c.mode} ${c.digits} digits ${network}`).toEqual(expected[i]);
        expect(statusLinePx(line, parts)).toBeLessThanOrEqual(width);
      });
  });

  // Re-titled (review-fb-tests finding 8): this test was "measures the parts as Chrome measured them
  // on the new-size build" and asserts the model's numbers; the Chrome readings are the next test.
  it('measures each part at the meta step, 7.2 px a glyph, with the insets it is drawn with', () => {
    const live = content({ mode: 'LIVE', digits: 8 });
    const none: StatusLineParts = { network: false, tip: null, release: false, hints: 0 };
    // the mode and the clock alone: 4 × 7.2 + 20 and 8 × 7.2 + 25
    expect(statusLinePx(live, none)).toBe(131.4);
    expect(statusLinePx(live, { ...none, network: true }) - 131.4).toBeCloseTo(75.4, 6);
    expect(statusLinePx(live, { ...none, tip: 'labelled' }) - 131.4).toBeCloseTo(131.8, 6);
    expect(statusLinePx(live, { ...none, tip: 'number' }) - 131.4).toBeCloseTo(104.2, 6);
    expect(statusLinePx(live, { ...none, release: true }) - 131.4).toBeCloseTo(190.6, 6);
    expect(statusLinePx(live, { ...none, hints: 3 }) - 48.8).toBeCloseTo(326.8, 6);
    expect(statusLinePx(live, ALL)).toBe(773.4);
  });

  it('agrees to the whole pixel with what Chrome measured on the new-size build', () => {
    // The literals are the lead's Chrome readings of the line before F3-f (LIVE, mainnet,
    // `tip #21,045,159`, `ckbadger 0.8.2 · synced`, the three hints with the clock, and the whole
    // line in a 768 px window), whole pixels as recorded; the model rounds to each of them.
    const live = content({ mode: 'LIVE', digits: 8 });
    const none: StatusLineParts = { network: false, tip: null, release: false, hints: 0 };
    // a part is what the line grows by when it is drawn; the mode block its word and its insets
    const part = (parts: Partial<StatusLineParts>) =>
      statusLinePx(live, { ...none, ...parts }) - statusLinePx(live, none);
    const mode =
      statusLinePx(live, none) -
      statusLinePx({ ...live, mode: '' }, none) +
      2 * STATUS_LINE_INSET_PX.mode;
    const clock = statusLinePx(live, none) - mode;
    expect(
      [
        mode,
        part({ network: true }),
        part({ tip: 'labelled' }),
        part({ release: true }),
        part({ hints: 3 }) + clock,
        statusLinePx(live, ALL),
      ].map(Math.round)
    ).toEqual([49, 75, 132, 191, 327, 773]);
  });

  it('keeps 54.4 px to spare at 768 with the widest content: room for a release seven characters longer', () => {
    const widest = content({ mode: 'SYNCING', digits: 9 });
    const parts = fitStatusLine('wide', widest, 768);
    expect(768 - statusLinePx(widest, parts)).toBeCloseTo(54.4, 6);
    // the whole wide form from 810 px
    expect(statusLinePx(widest, ALL)).toBe(809.4);
    expect(fitStatusLine('wide', widest, 810)).toEqual(ALL);
  });

  // review-fb-tests finding 8: each edge pinned on both sides and at equality (a line exactly as
  // wide as its parts holds them: `<=`), and the wide form's whole order of leaving.
  it('holds the whole wide form from 809.4 px with the widest content, 773.4 with LIVE', () => {
    const widest = content({ mode: 'SYNCING', digits: 9 });
    const live = content({ mode: 'LIVE', digits: 8 });
    const lessG = { ...ALL, hints: 2 };
    for (const [line, width, parts] of [
      [widest, 810, ALL],
      [widest, 809.4, ALL],
      [widest, 809.39, lessG],
      [widest, 809, lessG],
      [live, 774, ALL],
      [live, 773.4, ALL],
      [live, 773.39, lessG],
      [live, 773, lessG],
    ] as const)
      expect(fitStatusLine('wide', line, width), `${line.mode} at ${width}`).toEqual(parts);
  });

  it('gives the wide form up in its order, each part at the width its line needs', () => {
    // the widest content: SYNCING 70.4, mainnet 75.4, `tip #999,999,999` 139 (the number alone
    // 111.4), `ckbadger 0.8.2 · syncing` 197.8, the hints 67 / 81.4 / 95.8, the clock 82.6
    const widest = content({ mode: 'SYNCING', digits: 9 });
    const steps: [number, StatusLineParts][] = [
      [809.4, ALL],
      [713.6, { ...ALL, hints: 2 }],
      [632.2, { ...ALL, hints: 1 }],
      [565.2, { ...ALL, hints: 0 }],
      [537.6, { ...ALL, hints: 0, tip: 'number' }],
      [426.2, { ...ALL, hints: 0, tip: null }],
      [350.8, { ...ALL, hints: 0, tip: null, network: false }],
      [153, { network: false, tip: null, release: false, hints: 0 }],
    ];
    steps.forEach(([width, parts], i) => {
      expect(statusLinePx(widest, parts), `the line of step ${i}`).toBe(width);
      expect(fitStatusLine('wide', widest, width), `at ${width}`).toEqual(parts);
      // a hundredth under, the next part goes
      if (i + 1 < steps.length)
        expect(fitStatusLine('wide', widest, width - 0.01), `under ${width}`).toEqual(
          steps[i + 1][1]
        );
    });
  });

  it('holds the narrow form at equality and gives the tip’s word up a hundredth under it', () => {
    // SYNCING, eight digits: 360.2 px of parts, the number alone 332.6
    const syncing = content({ mode: 'SYNCING', digits: 8 });
    expect(statusLinePx(syncing, NARROW)).toBe(360.2);
    expect(fitStatusLine('narrow', syncing, 360.2)).toEqual(NARROW);
    expect(fitStatusLine('narrow', syncing, 360.19)).toEqual({ ...NARROW, tip: 'number' });
    expect(fitStatusLine('narrow', syncing, 332.6)).toEqual({ ...NARROW, tip: 'number' });
    expect(fitStatusLine('narrow', syncing, 332.59)).toEqual({ ...NARROW, tip: null });
  });

  it('draws the whole form before the line is measured', () => {
    const widest = content({ mode: 'SYNCING', digits: 9 });
    expect(fitStatusLine('wide', widest, null)).toEqual(ALL);
    expect(fitStatusLine('narrow', widest, null)).toEqual(NARROW);
  });

  it('keeps the mode and the clock however narrow the line', () => {
    const live = content({ mode: 'LIVE', digits: 8 });
    expect(fitStatusLine('wide', live, 100)).toEqual({
      network: false,
      tip: null,
      release: false,
      hints: 0,
    });
  });

  it('measures the clock in its form, two digits a field', () => {
    for (const at of [new Date(2026, 0, 1, 0, 0, 0), new Date(2026, 10, 18, 23, 59, 59)])
      expect(clockText(at)).toHaveLength(CLOCK_FORM.length);
  });
});
