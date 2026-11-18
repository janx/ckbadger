import { describe, expect, it } from 'vitest';
import { WIDEST } from '@/__tests__/utils/shell-budget';
import { clockText } from '@/lib/clock';
import { STATUS_KEY_HINTS } from '@/lib/shortcuts';
import {
  CLOCK_FORM,
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
 * at 49, 75, 132, 191 and 327 (the hints and the clock).
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

  it('measures the parts as Chrome measured them on the new-size build', () => {
    const live = content({ mode: 'LIVE', digits: 8 });
    const none: StatusLineParts = { network: false, tip: null, release: false, hints: 0 };
    // the mode and the clock alone
    expect(statusLinePx(live, none)).toBe(131.4);
    expect(statusLinePx(live, { ...none, network: true }) - 131.4).toBeCloseTo(75.4, 6);
    expect(statusLinePx(live, { ...none, tip: 'labelled' }) - 131.4).toBeCloseTo(131.8, 6);
    expect(statusLinePx(live, { ...none, tip: 'number' }) - 131.4).toBeCloseTo(104.2, 6);
    expect(statusLinePx(live, { ...none, release: true }) - 131.4).toBeCloseTo(190.6, 6);
    // the three hints and the clock: 327 in Chrome
    expect(statusLinePx(live, { ...none, hints: 3 }) - 48.8).toBeCloseTo(326.8, 6);
    // the whole line: 773 in Chrome, in a 768 px window
    expect(statusLinePx(live, ALL)).toBe(773.4);
  });

  it('keeps 54.4 px to spare at 768 with the widest content: room for a release seven characters longer', () => {
    const widest = content({ mode: 'SYNCING', digits: 9 });
    const parts = fitStatusLine('wide', widest, 768);
    expect(768 - statusLinePx(widest, parts)).toBeCloseTo(54.4, 6);
    // the whole wide form from 810 px
    expect(statusLinePx(widest, ALL)).toBe(809.4);
    expect(fitStatusLine('wide', widest, 810)).toEqual(ALL);
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
