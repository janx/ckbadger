import { describe, expect, it } from 'vitest';
import tailwindConfig from '@/tailwind.config';
import { cn } from '@/lib/utils';
import { NODE_H, PITCH } from '@/components/transaction/what-moved';
import { LONG_PLACEHOLDER_PX } from '@/components/search-bar';
import { DUMP_CHAR_PX } from '@/lib/cell-byte-layout';
import { LABEL_PX as CONCENTRATION_LABEL_PX } from '@/components/ui/concentration-bar';
import { SEGMENT_LABEL_PX } from '@/components/scripts/script-code-evidence';
import {
  LABEL_PX as HOLDER_LABEL_PX,
  TICK_PX as HOLDER_TICK_PX,
  TICK_ROW_PX as HOLDER_TICK_ROW_PX,
} from '@/components/scripts/holder-timeline';
import { LABEL_ADVANCE as FAMILY_LABEL_ADVANCE } from '@/components/scripts/script-family-board';
import { headBudget } from '@/components/scripts/script-family-page';
import { TICK_CHAR_PX } from '@/lib/chart-ticks';
import {
  END_PX as DAO_HISTORY_END_PX,
  NAME_PX as DAO_HISTORY_NAME_PX,
} from '@/components/dao/dao-history';
import { CHAR as FORK_DAYS_CHAR } from '@/components/forks/fork-days-chart';
import {
  CHAR_PX as VM_CHAR_PX,
  EDITION_CHAR_PX as VM_EDITION_CHAR_PX,
  NAME_CHAR_PX as VM_NAME_CHAR_PX,
  PLOT_NAME_LINE as VM_PLOT_NAME_LINE,
} from '@/components/hardforks/vm-lane-timeline';
import { CHAR_10 as TENURE_CHAR_10, CHAR_11 as TENURE_CHAR_11 } from '@/components/ui/tenure-bar';
import { NARROW_COLUMNS } from '@/app/blocks/page';
import { POOL_COLUMNS } from '@/app/transactions/page';
import { HELD_COLUMN } from '@/components/asset/inventory-table';
import { SHARE_FIGURE } from '@/app/scripts/page';
import { ALLELE_GEL } from '@/lib/allele-gel-layout';
import { textPx } from '@/lib/type-metrics';
import { FIBER_GRAPH } from '@/lib/fiber-network-model';
import { NUMBER_TEXT_PX, SMALL_TEXT_PX } from '@/lib/pipeline/layout';
import { AXIS_CHAR_PX } from '@/lib/chart-ticks';
import {
  HOVER_HEAD_Y,
  HOVER_LINE,
  INSIDE_TICK_ROOM,
  PLOT_NAME_BAND,
  PLOT_NAME_Y,
  PLOT_TOP,
  PLOT_UNIT_BAND,
  PLOT_UNIT_Y,
  REFERENCE_KEY_BAND,
  REFERENCE_KEY_Y,
  X_AXIS_BAND,
  X_LABEL_ROW,
} from '@/components/ui/chart';

/**
 * The budgets the readability plan's lane F3 (2026-11-18) computes from the type scale, held to
 * the numbers they were ruled to be. Step 1 replaced every literal that followed from the scale
 * with a computation of lib/type-metrics.ts or lib/chart-type.ts, and step 2 set the scale to
 * 12 / 13 / 14 px (R4) with SVG text by role from a 12 px floor and one advance, 0.6 em a glyph.
 *
 * BUDGETS below is one row per value a drawing file holds: the number it was ruled to be, its
 * arithmetic beside it, against the value imported from that file, so a change of the value in the
 * code, or of the scale under it, fails its row. Where a site writes its size or its advance as an
 * expression of the module (`fontSize={SVG_TEXT.tick}`, `text.length * charPx('meta')`), there is
 * no value of its own to read: the module's numbers are pinned in type-metrics.test.ts and
 * chart-type.test.ts, the SVG gate (chart-type-gate.test.ts) refuses a size of a site's own, and
 * the site's fit is held by its own figure or table test (the commit that took those rows out of
 * this file names each). The list tables' columns are each pinned as literal arrays by their
 * table's test (blocks-page, transactions-page, transaction-list-table and the others); a column
 * held by a module-level constant is read here too.
 *
 * The other blocks: cn() on the named line classes, textPx's arithmetic, and headBudget.
 */

describe('cn() reads the named lines as the arbitrary ones they replaced', () => {
  const lists = [
    ['text-body', 'leading-[18px]'],
    ['leading-[18px]', 'text-body'],
    ['leading-[18px]', 'leading-none'],
    ['leading-none', 'leading-[18px]'],
    ['text-meta', 'leading-[14px]', 'whitespace-nowrap'],
    ['leading-[14px]', 'text-small'],
    ['[&_a]:leading-[18px]', '[&_a]:leading-4'],
  ];
  const named = (c: string) =>
    c.replace('leading-[18px]', 'leading-row').replace('leading-[14px]', 'leading-label');

  it.each(lists)('keeps and drops them alike: %s', (...list) => {
    expect(cn(...list.map(named))).toBe(named(cn(...list)));
  });
});

describe("the pipeline's monoWidth on every label it measures", () => {
  it('gives (n × px × 3) / 5 to the bit, for up to 80 glyphs at both of its sizes', () => {
    for (const px of [11, 12])
      for (let n = 0; n <= 80; n++) expect(textPx(n, px), `${n} at ${px}`).toBe((n * px * 3) / 5);
  });
});

// Re-written (review-fb-tests finding 9): this block compared two formulas both written here; it
// now reads script-family-page's own headBudget.
describe('headBudget (script-family-page) on every head it budgets', () => {
  it('budgets 0.6 em a glyph at the meta step for every head of up to 40 characters', () => {
    // n glyphs at 12 px are 36n / 5 px, rounded up to a whole pixel, and the 16 px gap
    for (let n = 1; n <= 40; n++)
      expect(headBudget('x'.repeat(n)), `${n}`).toBe(Math.ceil((36 * n) / 5) + 16);
    // the two heads it budgets: 9 glyphs, 64.8 → 65 + 16; 10 glyphs, 72 + 16
    expect([headBudget('hash type'), headBudget('live cells')]).toEqual([81, 88]);
  });
});

/** The two lines tailwind.config.ts gives Tailwind (leading-row, leading-label). */
const TAILWIND_LINE = (tailwindConfig.theme as { extend: { lineHeight: Record<string, string> } })
  .extend.lineHeight;

/**
 * Every budget lane F3 computed from the type scale, as the code that draws it holds it
 * (review-fb-tests finding 4, review-fa-tests finding 4): each row is the file, the name of the
 * value, the number it was ruled to be at the scale of R4 (meta 12, small 13, body 14 px; SVG
 * text by role from lib/chart-type.ts: tick, label and note 12, value 12.5, head 13, title 13.5,
 * reading 14; a glyph 0.6 em; a tight line a size and 3 px), its arithmetic in the comment, and
 * the value IMPORTED from that file. A change of the drawing code's value, or of the scale it
 * reads, fails its row.
 */
const BUDGETS: [file: string, name: string, ruled: number, drawn: number][] = [
  // HTML text: the long placeholder, 34 glyphs at the body step: 34 × 8.4
  ['components/search-bar.tsx', 'LONG_PLACEHOLDER_PX', 285.6, LONG_PLACEHOLDER_PX],
  // the cell dump's glyph at the small step: 13 × 0.6
  ['lib/cell-byte-layout.ts', 'DUMP_CHAR_PX', 7.8, DUMP_CHAR_PX],
  // the size an in-segment label's fit is measured at: the small step
  ['components/ui/concentration-bar.tsx', 'LABEL_PX', 13, CONCENTRATION_LABEL_PX],
  ['components/scripts/script-code-evidence.tsx', 'SEGMENT_LABEL_PX', 13, SEGMENT_LABEL_PX],
  ['components/scripts/holder-timeline.tsx', 'LABEL_PX', 13, HOLDER_LABEL_PX],
  // the holder timeline's ticks at the meta step, and their row: 12 and a tight line, 12 + 3
  ['components/scripts/holder-timeline.tsx', 'TICK_PX', 12, HOLDER_TICK_PX],
  ['components/scripts/holder-timeline.tsx', 'TICK_ROW_PX', 15, HOLDER_TICK_ROW_PX],
  // the family board's label glyph at the meta step: 12 × 0.6
  ['components/scripts/script-family-board.tsx', 'LABEL_ADVANCE', 7.2, FAMILY_LABEL_ADVANCE],
  // the row line, the small step's 13 × 1.5 = 19.5 rounded up, and the tight label line, 12 + 3,
  // as Tailwind's theme draws them (leading-row, leading-label)
  ['tailwind.config.ts', 'lineHeight.row', 20, parseFloat(TAILWIND_LINE.row)],
  ['tailwind.config.ts', 'lineHeight.label', 15, parseFloat(TAILWIND_LINE.label)],
  // a node of the tx page's ribbon: two row lines and 4 px, and its pitch, 10 px more
  ['components/transaction/what-moved.tsx', 'NODE_H', 44, NODE_H],
  ['components/transaction/what-moved.tsx', 'PITCH', 54, PITCH],
  // the chart renderer's bands and baselines: the plot's name line, a sentence, a tight line at
  // the sentence size, 13 + 3, its band two of them (re-pinned, the ink and size roles, finished:
  // 15 and 30 at the label size); a plot's unit alone a tight label line, 12 + 3, its band two of
  // them; the plot's top a tick and its 6 px gap; the x axis 5 + 12 + 11; the x labels a tight
  // tick line; the reference key a name and its value at the head size, 13, its band 4 px more
  // (12 and 16 at the tick size); an in-plot tick label 1 px under a label; the hover head a label
  // and 5 px, its lines a tight reading, 14 + 3
  ['components/ui/chart.tsx', 'PLOT_NAME_Y', 16, PLOT_NAME_Y],
  ['components/ui/chart.tsx', 'PLOT_NAME_BAND', 32, PLOT_NAME_BAND],
  ['components/ui/chart.tsx', 'PLOT_UNIT_Y', 15, PLOT_UNIT_Y],
  ['components/ui/chart.tsx', 'PLOT_UNIT_BAND', 30, PLOT_UNIT_BAND],
  ['components/ui/chart.tsx', 'PLOT_TOP', 18, PLOT_TOP],
  ['components/ui/chart.tsx', 'X_AXIS_BAND', 28, X_AXIS_BAND],
  ['components/ui/chart.tsx', 'X_LABEL_ROW', 15, X_LABEL_ROW],
  ['components/ui/chart.tsx', 'REFERENCE_KEY_Y', 13, REFERENCE_KEY_Y],
  ['components/ui/chart.tsx', 'REFERENCE_KEY_BAND', 17, REFERENCE_KEY_BAND],
  ['components/ui/chart.tsx', 'INSIDE_TICK_ROOM', 13, INSIDE_TICK_ROOM],
  ['components/ui/chart.tsx', 'HOVER_HEAD_Y', 17, HOVER_HEAD_Y],
  ['components/ui/chart.tsx', 'HOVER_LINE', 17, HOVER_LINE],
  // the axis's glyphs, a tick's advance: 12 × 0.6
  ['lib/chart-ticks.ts', 'AXIS_CHAR_PX', 7.2, AXIS_CHAR_PX],
  ['lib/chart-ticks.ts', 'TICK_CHAR_PX', 7.2, TICK_CHAR_PX],
  // the DAO history's names at the label role and its end values at the reading role
  ['components/dao/dao-history.tsx', 'NAME_PX', 12, DAO_HISTORY_NAME_PX],
  ['components/dao/dao-history.tsx', 'END_PX', 14, DAO_HISTORY_END_PX],
  // the fork figures: a day chart's glyph at the label role (the tree writes a miner in an id
  // form or by the upper bound for text that is not ours, held by fork-tree.test and
  // type-metrics.test since the review's figures lane)
  ['components/forks/fork-days-chart.tsx', 'CHAR', 7.2, FORK_DAYS_CHAR],
  // the VM lanes: a note's glyph and a name's at 12 × 0.6, an edition's at the title role,
  // 13.5 × 0.6, the plot's name a tight label line, 12 + 3
  ['components/hardforks/vm-lane-timeline.tsx', 'CHAR_PX', 7.2, VM_CHAR_PX],
  ['components/hardforks/vm-lane-timeline.tsx', 'NAME_CHAR_PX', 7.2, VM_NAME_CHAR_PX],
  ['components/hardforks/vm-lane-timeline.tsx', 'EDITION_CHAR_PX', 8.1, VM_EDITION_CHAR_PX],
  ['components/hardforks/vm-lane-timeline.tsx', 'PLOT_NAME_LINE', 15, VM_PLOT_NAME_LINE],
  // the gels: the allele gel's glyph at the tick role and its turned head, 14 glyphs (100.8,
  // rounded up) and 8 px of margin (the sequencing gel measures a gene's name, rule and value by
  // the upper bound for text that is not ours: object-gels.test and type-metrics.test)
  ['lib/allele-gel-layout.ts', 'ALLELE_GEL.charWidth', 7.2, ALLELE_GEL.charWidth],
  ['lib/allele-gel-layout.ts', 'ALLELE_GEL.headRotated', 109, ALLELE_GEL.headRotated],
  // the identity's term bar: a label's glyph and a note's, 12 × 0.6 each
  ['components/ui/tenure-bar.tsx', 'CHAR_11', 7.2, TENURE_CHAR_11],
  ['components/ui/tenure-bar.tsx', 'CHAR_10', 7.2, TENURE_CHAR_10],
  // the Fiber graph's label half: six glyphs at the label role, 43.2, and its 3 px halo, halved
  // and rounded up
  ['lib/fiber-network-model.ts', 'FIBER_GRAPH.labelHalf', 24, FIBER_GRAPH.labelHalf],
  // the home pipeline's text at the meta step
  ['lib/pipeline/layout.ts', 'SMALL_TEXT_PX', 12, SMALL_TEXT_PX],
  ['lib/pipeline/layout.ts', 'NUMBER_TEXT_PX', 12, NUMBER_TEXT_PX],
  // the list tables' columns (lane F3-e: each from the widest value it holds, rounded up to a
  // whole pixel). /blocks' 608 px table: Height, the stone's 24, "#99,999,999" at 14 px, 92.4,
  // and the gap, 132.4 (re-pinned on purpose, readability R6: the stone's 34, 142.4, 143); Miner,
  // "ckb1…4s93hhsj" at 13 px, 101.4, and the gap; Size · cycles, "3,500,000,000" at 13 px, the
  // last column
  ['app/blocks/page.tsx', 'NARROW_COLUMNS height', 133, minWidthOf(NARROW_COLUMNS, 'height')],
  ['app/blocks/page.tsx', 'NARROW_COLUMNS miner', 118, minWidthOf(NARROW_COLUMNS, 'miner')],
  ['app/blocks/page.tsx', 'NARROW_COLUMNS resources', 102, minWidthOf(NARROW_COLUMNS, 'resources')],
  // the pool's list: Transaction, the stone's 40.3, 17 glyphs at 14 px, 142.8, and the gap,
  // 199.1 (re-pinned on purpose, readability R6: the stone's 56.5, 215.3, 216); Status ·
  // waiting, the 7 px dot, 6, "Confirming" at 13 px, 78, 6, "· 99d ago", 70.2, and the gap,
  // 183.2; Fee rate, "1,000,000,000" at 13 px, 101.4, and the gap; Size, "597,000", 54.6, and
  // the gap; Cycles, "3,500,000,000", 101.4, the last column
  ['app/transactions/page.tsx', 'POOL_COLUMNS tx', 200, minWidthOf(POOL_COLUMNS, 'tx')],
  ['app/transactions/page.tsx', 'POOL_COLUMNS status', 184, minWidthOf(POOL_COLUMNS, 'status')],
  ['app/transactions/page.tsx', 'POOL_COLUMNS feeRate', 118, minWidthOf(POOL_COLUMNS, 'feeRate')],
  ['app/transactions/page.tsx', 'POOL_COLUMNS size', 71, minWidthOf(POOL_COLUMNS, 'size')],
  ['app/transactions/page.tsx', 'POOL_COLUMNS cycles', 102, minWidthOf(POOL_COLUMNS, 'cycles')],
  // the inventory's CKB held: its head, 21 glyphs at 12 px, 151.2, wider than its widest amount,
  // and the gap
  [
    'components/asset/inventory-table.tsx',
    'HELD_COLUMN',
    168,
    minWidthOf([HELD_COLUMN], 'capacity'),
  ],
  // the scripts list's share figure, "100.00%" at 13 px, 54.6
  ['app/scripts/page.tsx', 'SHARE_FIGURE', 55, SHARE_FIGURE],
];

/** A list column's minimum width, by its key: it must have one. */
function minWidthOf(columns: readonly { key: string; minWidth?: number }[], key: string): number {
  const column = columns.find((c) => c.key === key);
  if (column?.minWidth === undefined) throw new Error(`no column ${key} with a minimum width`);
  return column.minWidth;
}

describe('every budget lane F3 computed is, in the code that draws it, the number it was ruled to be', () => {
  it.each(BUDGETS)('%s: %s is %s', (_file, _name, ruled, drawn) => {
    expect(drawn).toBe(ruled);
  });
});
