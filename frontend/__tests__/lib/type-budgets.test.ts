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
import { MINER_CHAR } from '@/components/forks/fork-tree';
import { CHAR as FORK_DAYS_CHAR } from '@/components/forks/fork-days-chart';
import {
  CHAR_PX as VM_CHAR_PX,
  EDITION_CHAR_PX as VM_EDITION_CHAR_PX,
  NAME_CHAR_PX as VM_NAME_CHAR_PX,
  PLOT_NAME_LINE as VM_PLOT_NAME_LINE,
} from '@/components/hardforks/vm-lane-timeline';
import { CHAR_115 as GEL_CHAR_115, CHAR_12 as GEL_CHAR_12 } from '@/components/ui/sequencing-gel';
import { CHAR_10 as TENURE_CHAR_10, CHAR_11 as TENURE_CHAR_11 } from '@/components/ui/tenure-bar';
import { NARROW_COLUMNS } from '@/app/blocks/page';
import { POOL_COLUMNS } from '@/app/transactions/page';
import { HELD_COLUMN } from '@/components/asset/inventory-table';
import { SHARE_FIGURE } from '@/app/scripts/page';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { ALLELE_GEL } from '@/lib/allele-gel-layout';
import {
  LABEL_LINE_PX,
  MONO_ADVANCE_EM,
  ROW_LINE_PX,
  advancePx,
  charPx,
  columnPx,
  fontPx,
  textPx,
  tightLinePx,
} from '@/lib/type-metrics';
import { SVG_TEXT, centreShift } from '@/lib/chart-type';
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
  REFERENCE_KEY_BAND,
  REFERENCE_KEY_Y,
  X_AXIS_BAND,
  X_LABEL_ROW,
} from '@/components/ui/chart';

/**
 * Every number lane F3 found written as a literal that followed from the type scale, and what it
 * is now that it is computed from `lib/type-metrics.ts` or `lib/chart-type.ts` (readability plan
 * 2026-11-18). Step 1 proved each computed exactly the literal it replaced at 11 / 12 / 13 px
 * (`was`, kept as the record); step 2 set the scale to 12 / 13 / 14 px, the SVG text to a 12 px
 * floor and the advance to one model, 0.6 em a glyph as measured in Chrome, with no allowance a
 * glyph wider: `now` is the number each budget computes at the ruled scale. A row is the file, the
 * literal as it was written, its value then, its value now, and the computation.
 */
const LITERALS: [file: string, literal: string, was: number, now: number, value: number][] = [
  // the advance of a step, in HTML text
  ['lib/byte-map.ts', 'META_CHAR_PX = 6.6', 6.6, 7.2, charPx('meta')],
  ['components/ui/hex-map.tsx', 'META_CHAR_PX (byte-map)', 6.6, 7.2, charPx('meta')],
  ['components/address/address-holdings.tsx', 'META_CHAR_PX (byte-map)', 6.6, 7.2, charPx('meta')],
  ['components/search-bar.tsx', 'BODY_CHAR_PX = 7.8', 7.8, 8.4, charPx('body')],
  [
    'components/search-bar.tsx',
    'LONG_PLACEHOLDER_PX = 34 × BODY_CHAR_PX',
    34 * 7.8,
    285.6,
    textPx(34, fontPx('body')),
  ],
  ['app/network/distributions.tsx', 'LABEL_ADVANCE = 7.2', 7.2, 7.8, charPx('small')],
  ['lib/cell-byte-layout.ts', 'DUMP_CHAR_PX = 7.2', 7.2, 7.8, charPx('small')],
  ['lib/cell-byte-layout.ts', 'MAP_CHAR_PX = 6.3 (meta −0.3, allowance)', 6.3, 7.2, charPx('meta')],
  // the size a label's fit is measured at
  ['components/ui/concentration-bar.tsx', 'LABEL_PX = 12', 12, 13, fontPx('small')],
  ['components/scripts/script-code-evidence.tsx', 'SEGMENT_LABEL_PX = 12', 12, 13, fontPx('small')],
  ['components/scripts/holder-timeline.tsx', 'LABEL_PX = 12', 12, 13, fontPx('small')],
  ['components/scripts/holder-timeline.tsx', 'TICK_PX = 11', 11, 12, fontPx('meta')],
  ['components/dao/dao-board.tsx', 'monoAdvance(11)', 7, 7.2, charPx('meta')],
  ['components/dao/dao-board.tsx', 'monoAdvance(12): 12', 12, 13, fontPx('small')],
  [
    'components/scripts/script-family-board.tsx',
    'LABEL_ADVANCE = max(round(0.6 × 11), 0.6 × 11)',
    7,
    7.2,
    charPx('meta'),
  ],
  [
    'components/scripts/script-family-page.tsx',
    'parseFloat(TYPE_SCALE.meta[0]) (headBudget)',
    11,
    12,
    fontPx('meta'),
  ],
  // the face's advance as a fraction of the size
  ['components/ui/head-id.tsx', 'ADVANCE_EM = 0.6', 0.6, 0.6, MONO_ADVANCE_EM],
  ['components/ui/title-id.tsx', 'ADVANCE_EM = 0.6', 0.6, 0.6, MONO_ADVANCE_EM],
  ['components/home/mini-chart.tsx', 'GLYPH_ADVANCE = 0.6', 0.6, 0.6, MONO_ADVANCE_EM],
  // SVG text by role, in the chart renderer and the charts drawn beside it
  ['components/ui/chart.tsx', 'fontSize="10" (8 sites)', 10, 12, SVG_TEXT.tick],
  ['components/ui/chart.tsx', 'fontSize="11" (3 sites)', 11, 12, SVG_TEXT.label],
  ['components/ui/chart.tsx', 'fontSize="13" (2 sites)', 13, 14, SVG_TEXT.reading],
  ['components/dao/dao-history.tsx', 'fontSize="10"', 10, 12, SVG_TEXT.tick],
  ['components/dao/dao-history.tsx', 'NAME_PX = 11', 11, 12, SVG_TEXT.label],
  ['components/dao/dao-history.tsx', 'END_PX = 13', 13, 14, SVG_TEXT.reading],
  ['components/home/mini-chart.tsx', 'fontSize={10} (3 sites)', 10, 12, SVG_TEXT.tick],
  // the chart renderer's bands and baselines
  ['components/ui/chart.tsx', 'y={14 + keyBand} (the name)', 14, 15, PLOT_NAME_Y],
  ['components/ui/chart.tsx', 'PLOT_NAME_BAND = 28', 28, 30, PLOT_NAME_BAND],
  ['components/ui/chart.tsx', 'top = 16 (no name)', 16, 18, PLOT_TOP],
  ['components/ui/chart.tsx', 'bottom = height - 26', 26, 28, X_AXIS_BAND],
  ['components/ui/chart.tsx', 'X_LABEL_ROW = 12 (a tight tick line now)', 12, 15, X_LABEL_ROW],
  ['components/ui/chart.tsx', 'REFERENCE_KEY_Y = 10', 10, 12, REFERENCE_KEY_Y],
  ['components/ui/chart.tsx', 'REFERENCE_KEY_BAND = 14', 14, 16, REFERENCE_KEY_BAND],
  [
    'components/ui/chart.tsx',
    'REFERENCE_KEY_Y - 3 (the swatch)',
    3,
    3.6,
    centreShift(SVG_TEXT.tick),
  ],
  ['components/ui/chart.tsx', 'yTick(v) + 3 (a tick label)', 3, 3.6, centreShift(SVG_TEXT.tick)],
  ['components/ui/chart.tsx', 'top + 12 (an in-plot tick)', 12, 13, INSIDE_TICK_ROOM],
  ['components/ui/chart.tsx', 'Math.max(13, …) (the end label)', 13, 14, SVG_TEXT.reading],
  ['components/ui/chart.tsx', '× 6.6 (the hover box)', 6.6, 7.2, textPx(1, SVG_TEXT.label)],
  ['components/ui/chart.tsx', 'by + 16 (the hover head)', 16, 17, HOVER_HEAD_Y],
  ['components/ui/chart.tsx', 'i × 16 (a hover figure)', 16, 17, HOVER_LINE],
  ['components/ui/chart.tsx', 'by + 33 (the first figure)', 33, 35, HOVER_HEAD_Y + 1 + HOVER_LINE],
  ['components/ui/chart.tsx', '22 + rows × 16 (the box)', 22, 23, HOVER_HEAD_Y + 6],
  ['lib/chart-ticks.ts', 'AXIS_CHAR_PX = 6.2 (tick +0.2, allowance)', 6.2, 7.2, AXIS_CHAR_PX],
  ['lib/chart-ticks.ts', 'TICK_CHAR_PX = 6', 6, 7.2, advancePx(SVG_TEXT.tick)],
  [
    'components/dao/dao-history.tsx',
    'CHAR_EM = 0.64 (0.6 +0.04, allowance)',
    0.64,
    0.6,
    MONO_ADVANCE_EM,
  ],
  ['components/dao/dao-history.tsx', 'y(v) + 3 (a tick label)', 3, 3.6, centreShift(SVG_TEXT.tick)],
  ['components/dao/dao-history.tsx', 'y={13} (the end label)', 13, 14, SVG_TEXT.reading],
  ['components/home/mini-chart.tsx', 't.y + 3 (a tick label)', 3, 3.6, centreShift(SVG_TEXT.tick)],
  [
    'components/home/mini-chart.tsx',
    'text.length * 6 (a clip label)',
    6,
    7.2,
    advancePx(SVG_TEXT.tick),
  ],
  // the row line and the tight label line
  ['components/ui/row-seal.tsx', 'ROW_LINE_PX = 18', 18, 20, ROW_LINE_PX],
  ['components/activity/activity-row.tsx', 'HEAD_LINE_PX = 18', 18, 20, ROW_LINE_PX],
  ['tailwind.config.ts', 'leading-[18px] (37 sites) → leading-row', 18, 20, ROW_LINE_PX],
  ['tailwind.config.ts', 'leading-[14px] (2 sites) → leading-label', 14, 15, LABEL_LINE_PX],
  ['components/scripts/holder-timeline.tsx', 'TICK_ROW_PX = 14', 14, 15, LABEL_LINE_PX],
  ['components/transaction/what-moved.tsx', 'NODE_H = 40 (two 18 px lines)', 40, 44, NODE_H],
  ['components/transaction/what-moved.tsx', 'PITCH = 50', 50, 54, NODE_H + 10],
  // the fork figures
  ['components/forks/fork-tree.tsx', 'fontSize="11" (4 sites)', 11, 12, SVG_TEXT.label],
  ['components/forks/fork-tree.tsx', 'fontSize="10" (5 sites)', 10, 12, SVG_TEXT.tick],
  ['components/forks/fork-tree.tsx', 'fontSize={compact ? 10 : 11}: 10', 10, 12, SVG_TEXT.tick],
  [
    'components/forks/fork-tree.tsx',
    'CHAR_10 = 6.4 (0.64 em, observed)',
    6.4,
    7.2,
    advancePx(SVG_TEXT.tick),
  ],
  ['components/forks/fork-days-chart.tsx', 'fontSize="11" (4 sites)', 11, 12, SVG_TEXT.label],
  ['components/forks/fork-days-chart.tsx', 'CHAR = 6.6', 6.6, 7.2, advancePx(SVG_TEXT.label)],
  // the VM lanes (hardforks)
  [
    'components/hardforks/vm-lane-timeline.tsx',
    'fontSize="11" (3) and NAME_PX = 11',
    11,
    12,
    SVG_TEXT.label,
  ],
  [
    'components/hardforks/vm-lane-timeline.tsx',
    'fontSize="12.5" (2 sites)',
    12.5,
    13.5,
    SVG_TEXT.title,
  ],
  [
    'components/hardforks/vm-lane-timeline.tsx',
    'fontSize="10.5" (8 sites)',
    10.5,
    12,
    SVG_TEXT.note,
  ],
  ['components/hardforks/vm-lane-timeline.tsx', 'fontSize="12" (2 sites)', 12, 13, SVG_TEXT.head],
  ['components/hardforks/vm-lane-timeline.tsx', 'fontSize="10"', 10, 12, SVG_TEXT.tick],
  [
    'components/hardforks/vm-lane-timeline.tsx',
    'CHAR_PX = 6.5 (note +0.2, allowance)',
    6.5,
    7.2,
    advancePx(SVG_TEXT.note),
  ],
  [
    'components/hardforks/vm-lane-timeline.tsx',
    'NAME_CHAR_PX = 11 × 0.64',
    11 * 0.64,
    7.2,
    advancePx(SVG_TEXT.label),
  ],
  [
    'components/hardforks/vm-lane-timeline.tsx',
    'EDITION_CHAR_PX = 7.5',
    7.5,
    8.1,
    advancePx(SVG_TEXT.title),
  ],
  [
    'components/hardforks/vm-lane-timeline.tsx',
    'PLOT_NAME_LINE = 14',
    14,
    15,
    tightLinePx(SVG_TEXT.label),
  ],
  // the gels (objects)
  ['components/ui/sequencing-gel.tsx', 'fontSize="10" (8 sites)', 10, 12, SVG_TEXT.tick],
  ['components/ui/sequencing-gel.tsx', 'fontSize="12" (3 sites)', 12, 13, SVG_TEXT.head],
  ['components/ui/sequencing-gel.tsx', 'fontSize="11.5" (2 sites)', 11.5, 12.5, SVG_TEXT.value],
  [
    'components/ui/sequencing-gel.tsx',
    'CHAR_12 = 7.64 (head +0.44, allowance)',
    7.64,
    7.8,
    advancePx(SVG_TEXT.head),
  ],
  [
    'components/ui/sequencing-gel.tsx',
    'CHAR_115 = 7.32 (value +0.42, allowance)',
    7.32,
    7.5,
    advancePx(SVG_TEXT.value),
  ],
  ['components/ui/allele-gel.tsx', 'fontSize="10" (4 sites)', 10, 12, SVG_TEXT.tick],
  [
    'lib/allele-gel-layout.ts',
    'charWidth: 6.4 (0.64 em, observed)',
    6.4,
    7.2,
    ALLELE_GEL.charWidth,
  ],
  [
    'lib/allele-gel-layout.ts',
    'headRotated: 98 (14 characters and margin)',
    98,
    109,
    ALLELE_GEL.headRotated,
  ],
  // the identity's term bar and the DAO's icicle
  ['components/ui/tenure-bar.tsx', 'fontSize="11" (4 sites)', 11, 12, SVG_TEXT.label],
  ['components/ui/tenure-bar.tsx', 'fontSize="10.5" (2 sites)', 10.5, 12, SVG_TEXT.note],
  [
    'components/ui/tenure-bar.tsx',
    'CHAR_11 = 6.7 (label +0.1, allowance)',
    6.7,
    7.2,
    advancePx(SVG_TEXT.label),
  ],
  [
    'components/ui/tenure-bar.tsx',
    'CHAR_10 = 6.4 (note +0.1, allowance)',
    6.4,
    7.2,
    advancePx(SVG_TEXT.note),
  ],
  ['components/ui/icicle-partition.tsx', 'px = h >= 40 ? 13 : …', 13, 14, SVG_TEXT.reading],
  ['components/ui/icicle-partition.tsx', 'px = … : 11', 11, 12, SVG_TEXT.label],
  ['components/ui/icicle-partition.tsx', 'fontSize="12"', 12, 13, SVG_TEXT.head],
  ['components/ui/icicle-partition.tsx', 'fontSize="11" (2 sites)', 11, 12, SVG_TEXT.label],
  ['components/ui/icicle-partition.tsx', 'fontSize="11.5"', 11.5, 12.5, SVG_TEXT.value],
  ['components/ui/icicle-partition.tsx', 'fontSize="10.5" (2 sites)', 10.5, 12, SVG_TEXT.note],
  // the network map and the Fiber figures
  ['app/network/network-map.tsx', 'fontSize="12" (the key)', 12, 13, SVG_TEXT.head],
  ['app/network/network-map.tsx', 'fontSize="11" (the name)', 11, 12, SVG_TEXT.label],
  ['app/network/network-map.tsx', 'fontSize="10" (a place)', 10, 12, SVG_TEXT.tick],
  [
    'app/network/network-map.tsx',
    '× 6.2 (a place, kept clear)',
    6.2,
    7.2,
    advancePx(SVG_TEXT.tick),
  ],
  [
    'components/fiber/fiber-channel-timeline.tsx',
    'fontSize="11" (4 sites)',
    11,
    12,
    SVG_TEXT.label,
  ],
  ['components/fiber/fiber-channel-timeline.tsx', 'fontSize="10.5"', 10.5, 12, SVG_TEXT.note],
  [
    'components/fiber/fiber-channel-timeline.tsx',
    'fontSize="11.5" (2 sites)',
    11.5,
    12.5,
    SVG_TEXT.value,
  ],
  ['components/fiber/fiber-network-graph.tsx', 'fontSize="11" (4 sites)', 11, 12, SVG_TEXT.label],
  ['lib/fiber-network-model.ts', 'labelHalf: 23', 23, 24, FIBER_GRAPH.labelHalf],
  // the block's position diagram and the proposal ruler
  ['components/blocks/block-position.tsx', 'fontSize="11" (10 sites)', 11, 12, SVG_TEXT.label],
  ['components/blocks/block-position.tsx', 'fontSize="12" (a tile number)', 12, 13, SVG_TEXT.head],
  ['components/transaction/proposal-ruler.tsx', 'fontSize={10}', 10, 12, SVG_TEXT.tick],
  // the home pipeline's text (its geometry is not redrawn in this lane)
  ['lib/pipeline/layout.ts', 'SMALL_TEXT_PX = 11', 11, 12, SMALL_TEXT_PX],
  ['lib/pipeline/layout.ts', 'NUMBER_TEXT_PX = 12', 12, 12, NUMBER_TEXT_PX],
  // the hand-computed minimum widths of the ListTable columns whose comment records what they
  // hold (the rendered widths are pinned exactly by blocks-page, transactions-page and
  // transaction-list-table: 1,230 and 564 px as before). The record of step 2: lane F3-e then
  // budgeted every column of the list tables from the widest value it holds (lib/list-bounds.ts),
  // the spares kept here going; those budgets are pinned by each table's own test.
  [
    'app/blocks/page.tsx',
    'Block: 195 + ROW_SEAL.block.footprint',
    195 + ROW_SEAL.block.footprint,
    243,
    columnPx(
      ROW_SEAL.block.footprint +
        textPx(11, fontPx('body')) +
        charPx('small') +
        textPx(11, fontPx('meta')) +
        16,
      13
    ),
  ],
  ['app/blocks/page.tsx', 'Reward: 118', 118, 127, columnPx(textPx(14, fontPx('small')) + 16, 1)],
  ['app/blocks/page.tsx', 'Fees: 105', 105, 112, columnPx(textPx(12, fontPx('small')) + 16, 2)],
  [
    'app/blocks/page.tsx',
    'Height (608 px table): 104 + ROW_SEAL.block.footprint',
    104 + ROW_SEAL.block.footprint,
    145,
    columnPx(
      ROW_SEAL.block.footprint +
        Math.max(textPx(11, fontPx('body')), textPx(11, fontPx('meta'))) +
        16,
      2
    ),
  ],
  [
    'app/blocks/page.tsx',
    'Miner (608 px table): 110',
    110,
    118,
    columnPx(textPx(13, fontPx('small')) + 16),
  ],
  [
    'app/blocks/page.tsx',
    'Size · cycles (608 px table): 96',
    96,
    104,
    columnPx(textPx(13, fontPx('small')), 2),
  ],
  [
    'app/transactions/page.tsx',
    'Transaction (pool): 237',
    237,
    247,
    columnPx(ROW_SEAL.tx.footprint + textPx(17, fontPx('body')) + 16, 31),
  ],
  [
    'app/transactions/page.tsx',
    'Status · waiting: 172',
    172,
    184,
    columnPx(7 + 6 + textPx(10, fontPx('small')) + 6 + textPx(9, fontPx('small')) + 16),
  ],
  [
    'app/transactions/page.tsx',
    'Fee rate: 123',
    123,
    131,
    columnPx(Math.max(textPx(10, fontPx('meta')), textPx(13, fontPx('small'))) + 16, 13),
  ],
  [
    'app/transactions/page.tsx',
    'Size (pool): 68',
    68,
    72,
    columnPx(textPx(7, fontPx('small')) + 16, 1),
  ],
  [
    'app/transactions/page.tsx',
    'Cycles (pool): 94',
    94,
    102,
    columnPx(textPx(13, fontPx('small'))),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Transaction: 208',
    208,
    218,
    columnPx(ROW_SEAL.tx.footprint + textPx(17, fontPx('body')) + 16, 2),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'What it did: 246',
    246,
    262,
    columnPx(16 + 8 + 8 + 16 + textPx(27, fontPx('small')), 3),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Value: 160',
    160,
    172,
    columnPx(textPx(20, fontPx('small')) + 16),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Fee: 96',
    96,
    102,
    columnPx(textPx(11, fontPx('small')) + 16),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Cells: 62',
    62,
    65,
    columnPx(textPx(6, fontPx('small')) + 16, 2),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Size: 121 (SIZE_TRACK 56)',
    121,
    124,
    columnPx(56 + 10 + textPx(5, fontPx('small')) + 16, 3),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Cycles: 162 (CYCLES_TRACK 56)',
    162,
    168,
    columnPx(56 + 10 + textPx(11, fontPx('small')) + 16),
  ],
  [
    'components/asset/inventory-table.tsx',
    'CKB held in its cells: 155',
    155,
    168,
    columnPx(textPx('CKB held in its cells'.length, fontPx('meta')) + 16),
  ],
  ['app/scripts/page.tsx', 'SHARE_FIGURE = 51', 51, 55, columnPx(textPx(7, fontPx('small')))],
];

describe('every literal lane F3 replaced computes its number at the ruled scale', () => {
  it.each(LITERALS)('%s: %s (was %s, now %s)', (_file, _literal, _was, now, value) => {
    expect(value).toBe(now);
  });
});

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
  // the chart renderer's bands and baselines: the plot's name a tight label line, 12 + 3, its
  // band two of them; the plot's top a tick and its 6 px gap; the x axis 5 + 12 + 11; the x
  // labels a tight tick line; the reference key on a tick, its band 4 px more; an in-plot tick
  // label 1 px under a label; the hover head a label and 5 px, its lines a tight reading, 14 + 3
  ['components/ui/chart.tsx', 'PLOT_NAME_Y', 15, PLOT_NAME_Y],
  ['components/ui/chart.tsx', 'PLOT_NAME_BAND', 30, PLOT_NAME_BAND],
  ['components/ui/chart.tsx', 'PLOT_TOP', 18, PLOT_TOP],
  ['components/ui/chart.tsx', 'X_AXIS_BAND', 28, X_AXIS_BAND],
  ['components/ui/chart.tsx', 'X_LABEL_ROW', 15, X_LABEL_ROW],
  ['components/ui/chart.tsx', 'REFERENCE_KEY_Y', 12, REFERENCE_KEY_Y],
  ['components/ui/chart.tsx', 'REFERENCE_KEY_BAND', 16, REFERENCE_KEY_BAND],
  ['components/ui/chart.tsx', 'INSIDE_TICK_ROOM', 13, INSIDE_TICK_ROOM],
  ['components/ui/chart.tsx', 'HOVER_HEAD_Y', 17, HOVER_HEAD_Y],
  ['components/ui/chart.tsx', 'HOVER_LINE', 17, HOVER_LINE],
  // the axis's glyphs, a tick's advance: 12 × 0.6
  ['lib/chart-ticks.ts', 'AXIS_CHAR_PX', 7.2, AXIS_CHAR_PX],
  ['lib/chart-ticks.ts', 'TICK_CHAR_PX', 7.2, TICK_CHAR_PX],
  // the DAO history's names at the label role and its end values at the reading role
  ['components/dao/dao-history.tsx', 'NAME_PX', 12, DAO_HISTORY_NAME_PX],
  ['components/dao/dao-history.tsx', 'END_PX', 14, DAO_HISTORY_END_PX],
  // the fork figures: a miner's glyph at the tick role, a day chart's at the label role
  ['components/forks/fork-tree.tsx', 'MINER_CHAR', 7.2, MINER_CHAR],
  ['components/forks/fork-days-chart.tsx', 'CHAR', 7.2, FORK_DAYS_CHAR],
  // the VM lanes: a note's glyph and a name's at 12 × 0.6, an edition's at the title role,
  // 13.5 × 0.6, the plot's name a tight label line, 12 + 3
  ['components/hardforks/vm-lane-timeline.tsx', 'CHAR_PX', 7.2, VM_CHAR_PX],
  ['components/hardforks/vm-lane-timeline.tsx', 'NAME_CHAR_PX', 7.2, VM_NAME_CHAR_PX],
  ['components/hardforks/vm-lane-timeline.tsx', 'EDITION_CHAR_PX', 8.1, VM_EDITION_CHAR_PX],
  ['components/hardforks/vm-lane-timeline.tsx', 'PLOT_NAME_LINE', 15, VM_PLOT_NAME_LINE],
  // the gels: a head's glyph, 13 × 0.6, a value's, 12.5 × 0.6; the allele gel's glyph at the tick
  // role and its turned head, 14 glyphs (100.8, rounded up) and 8 px of margin
  ['components/ui/sequencing-gel.tsx', 'CHAR_12', 7.8, GEL_CHAR_12],
  ['components/ui/sequencing-gel.tsx', 'CHAR_115', 7.5, GEL_CHAR_115],
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
  // whole pixel). /blocks' 608 px table: Height, the stone's 34, "#99,999,999" at 14 px, 92.4,
  // and the gap, 142.4; Miner, "ckb1…4s93hhsj" at 13 px, 101.4, and the gap; Size · cycles,
  // "3,500,000,000" at 13 px, the last column
  ['app/blocks/page.tsx', 'NARROW_COLUMNS height', 143, minWidthOf(NARROW_COLUMNS, 'height')],
  ['app/blocks/page.tsx', 'NARROW_COLUMNS miner', 118, minWidthOf(NARROW_COLUMNS, 'miner')],
  ['app/blocks/page.tsx', 'NARROW_COLUMNS resources', 102, minWidthOf(NARROW_COLUMNS, 'resources')],
  // the pool's list: Transaction, the stone's 56.5, 17 glyphs at 14 px, 142.8, and the gap,
  // 215.3; Status · waiting, the 7 px dot, 6, "Confirming" at 13 px, 78, 6, "· 99d ago", 70.2,
  // and the gap, 183.2; Fee rate, "1,000,000,000" at 13 px, 101.4, and the gap; Size, "597,000",
  // 54.6, and the gap; Cycles, "3,500,000,000", 101.4, the last column
  ['app/transactions/page.tsx', 'POOL_COLUMNS tx', 216, minWidthOf(POOL_COLUMNS, 'tx')],
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
