import { describe, expect, it } from 'vitest';
import { cn } from '@/lib/utils';
import { NODE_H } from '@/components/transaction/what-moved';
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
  // transaction-list-table: 1,230 and 564 px as before)
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

describe('headBudget (script-family-page) on every head it budgets', () => {
  // its formula as written before the module, at the meta step (12 px since R4; 11 px before)
  const old = (label: string) => Math.ceil(label.length * 0.6 * 12) + 16;
  const now = (label: string) => Math.ceil(textPx(label.length, fontPx('meta'))) + 16;

  it('budgets 0.6 em a glyph at the meta step for every head of up to 40 characters', () => {
    for (let n = 1; n <= 40; n++) expect(now('x'.repeat(n)), `${n}`).toBe(old('x'.repeat(n)));
    // re-pinned at 12 px (R4): 76 → 81 and 82 → 88
    expect([now('hash type'), now('live cells')]).toEqual([81, 88]);
  });
});
