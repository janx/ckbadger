import { describe, expect, it } from 'vitest';
import { MAP_CHAR_PX } from '@/lib/cell-byte-layout';
import { LABEL_ADVANCE as UPGRADE_LABEL_ADVANCE } from '@/components/scripts/script-family-board';
import { cn } from '@/lib/utils';
import { NODE_H } from '@/components/transaction/what-moved';
import { ROW_SEAL } from '@/components/ui/row-seal';
import { CHAR_10 as FORK_CHAR_10 } from '@/components/forks/fork-tree';
import {
  LABEL_LINE_PX,
  MONO_ADVANCE_EM,
  ROW_LINE_PX,
  OBSERVED_ADVANCE_EM,
  advancePx,
  charPx,
  columnPx,
  fontPx,
  snappedAdvancePx,
  textPx,
} from '@/lib/type-metrics';
import { SVG_TEXT, centreShift } from '@/lib/chart-type';
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
 * The proof that lane F3's first step moved no pixel (readability plan 2026-11-18): every number
 * that followed from the type scale and was written as a literal is now computed from
 * `lib/type-metrics.ts` or `lib/chart-type.ts`, and each computes, today, exactly the number it
 * replaced. A row is the file, the literal as it was written, its value, and the value now; a
 * literal that kept a hand-tuned margin is now the scale's value plus a named allowance, and its
 * row says which.
 */
const LITERALS: [file: string, literal: string, was: number, now: number][] = [
  // the advance of a step, in HTML text
  ['lib/byte-map.ts', 'META_CHAR_PX = 6.6', 6.6, charPx('meta')],
  ['components/ui/hex-map.tsx', 'META_CHAR_PX (byte-map)', 6.6, charPx('meta')],
  ['components/address/address-holdings.tsx', 'META_CHAR_PX (byte-map)', 6.6, charPx('meta')],
  ['components/search-bar.tsx', 'BODY_CHAR_PX = 7.8', 7.8, charPx('body')],
  [
    'components/search-bar.tsx',
    'LONG_PLACEHOLDER_PX = 34 × BODY_CHAR_PX',
    34 * 7.8,
    textPx(34, fontPx('body')),
  ],
  ['app/network/distributions.tsx', 'LABEL_ADVANCE = 7.2', 7.2, charPx('small')],
  ['lib/cell-byte-layout.ts', 'DUMP_CHAR_PX = 7.2', 7.2, charPx('small')],
  ['lib/cell-byte-layout.ts', 'MAP_CHAR_PX = 6.3 (meta −0.3, allowance)', 6.3, MAP_CHAR_PX],
  // the size a label's fit is measured at
  ['components/ui/concentration-bar.tsx', 'LABEL_PX = 12', 12, fontPx('small')],
  ['components/scripts/script-code-evidence.tsx', 'SEGMENT_LABEL_PX = 12', 12, fontPx('small')],
  ['components/scripts/holder-timeline.tsx', 'LABEL_PX = 12', 12, fontPx('small')],
  ['components/scripts/holder-timeline.tsx', 'TICK_PX = 11', 11, fontPx('meta')],
  ['components/dao/dao-board.tsx', 'monoAdvance(11)', 7, snappedAdvancePx(fontPx('meta'))],
  ['components/dao/dao-board.tsx', 'monoAdvance(12): 12', 12, fontPx('small')],
  [
    'components/scripts/script-family-board.tsx',
    'LABEL_ADVANCE = max(round(0.6 × 11), 0.6 × 11)',
    7,
    UPGRADE_LABEL_ADVANCE,
  ],
  [
    'components/scripts/script-family-page.tsx',
    'parseFloat(TYPE_SCALE.meta[0]) (headBudget)',
    11,
    fontPx('meta'),
  ],
  // the face's advance as a fraction of the size
  ['components/ui/head-id.tsx', 'ADVANCE_EM = 0.6', 0.6, MONO_ADVANCE_EM],
  ['components/ui/title-id.tsx', 'ADVANCE_EM = 0.6', 0.6, MONO_ADVANCE_EM],
  ['components/home/mini-chart.tsx', 'GLYPH_ADVANCE = 0.6', 0.6, MONO_ADVANCE_EM],
  // SVG text by role, in the chart renderer and the charts drawn beside it
  ['components/ui/chart.tsx', 'fontSize="10" (8 sites)', 10, SVG_TEXT.tick],
  ['components/ui/chart.tsx', 'fontSize="11" (3 sites)', 11, SVG_TEXT.label],
  ['components/ui/chart.tsx', 'fontSize="13" (2 sites)', 13, SVG_TEXT.reading],
  ['components/dao/dao-history.tsx', 'fontSize="10"', 10, SVG_TEXT.tick],
  ['components/dao/dao-history.tsx', 'NAME_PX = 11', 11, SVG_TEXT.label],
  ['components/dao/dao-history.tsx', 'END_PX = 13', 13, SVG_TEXT.reading],
  ['components/home/mini-chart.tsx', 'fontSize={10} (3 sites)', 10, SVG_TEXT.tick],
  // the chart renderer's bands and baselines
  ['components/ui/chart.tsx', 'y={14 + keyBand} (the name)', 14, PLOT_NAME_Y],
  ['components/ui/chart.tsx', 'PLOT_NAME_BAND = 28', 28, PLOT_NAME_BAND],
  ['components/ui/chart.tsx', 'top = 16 (no name)', 16, PLOT_TOP],
  ['components/ui/chart.tsx', 'bottom = height - 26', 26, X_AXIS_BAND],
  ['components/ui/chart.tsx', 'X_LABEL_ROW = 12', 12, X_LABEL_ROW],
  ['components/ui/chart.tsx', 'REFERENCE_KEY_Y = 10', 10, REFERENCE_KEY_Y],
  ['components/ui/chart.tsx', 'REFERENCE_KEY_BAND = 14', 14, REFERENCE_KEY_BAND],
  ['components/ui/chart.tsx', 'REFERENCE_KEY_Y - 3 (the swatch)', 3, centreShift(SVG_TEXT.tick)],
  ['components/ui/chart.tsx', 'yTick(v) + 3 (a tick label)', 3, centreShift(SVG_TEXT.tick)],
  ['components/ui/chart.tsx', 'top + 12 (an in-plot tick)', 12, INSIDE_TICK_ROOM],
  ['components/ui/chart.tsx', 'Math.max(13, …) (the end label)', 13, SVG_TEXT.reading],
  ['components/ui/chart.tsx', '× 6.6 (the hover box)', 6.6, textPx(1, SVG_TEXT.label)],
  ['components/ui/chart.tsx', 'by + 16 (the hover head)', 16, HOVER_HEAD_Y],
  ['components/ui/chart.tsx', 'i × 16 (a hover figure)', 16, HOVER_LINE],
  ['components/ui/chart.tsx', 'by + 33 (the first figure)', 33, HOVER_HEAD_Y + 1 + HOVER_LINE],
  ['components/ui/chart.tsx', '22 + rows × 16 (the box)', 22, HOVER_HEAD_Y + 6],
  ['lib/chart-ticks.ts', 'AXIS_CHAR_PX = 6.2 (tick +0.2, allowance)', 6.2, AXIS_CHAR_PX],
  ['lib/chart-ticks.ts', 'TICK_CHAR_PX = 6', 6, advancePx(SVG_TEXT.tick)],
  [
    'components/dao/dao-history.tsx',
    'CHAR_EM = 0.64 (0.6 +0.04, allowance)',
    0.64,
    OBSERVED_ADVANCE_EM,
  ],
  ['components/dao/dao-history.tsx', 'y(v) + 3 (a tick label)', 3, centreShift(SVG_TEXT.tick)],
  ['components/dao/dao-history.tsx', 'y={13} (the end label)', 13, SVG_TEXT.reading],
  ['components/home/mini-chart.tsx', 't.y + 3 (a tick label)', 3, centreShift(SVG_TEXT.tick)],
  ['components/home/mini-chart.tsx', 'text.length * 6 (a clip label)', 6, advancePx(SVG_TEXT.tick)],
  // the row line and the tight label line
  ['components/ui/row-seal.tsx', 'ROW_LINE_PX = 18', 18, ROW_LINE_PX],
  ['components/activity/activity-row.tsx', 'HEAD_LINE_PX = 18', 18, ROW_LINE_PX],
  ['tailwind.config.ts', 'leading-[18px] (37 sites) → leading-row', 18, ROW_LINE_PX],
  ['tailwind.config.ts', 'leading-[14px] (2 sites) → leading-label', 14, LABEL_LINE_PX],
  ['components/scripts/holder-timeline.tsx', 'TICK_ROW_PX = 14', 14, LABEL_LINE_PX],
  ['components/transaction/what-moved.tsx', 'NODE_H = 40 (two 18 px lines)', 40, NODE_H],
  ['components/transaction/what-moved.tsx', 'PITCH = 50', 50, NODE_H + 10],
  // the fork figures
  ['components/forks/fork-tree.tsx', 'fontSize="11" (4 sites)', 11, SVG_TEXT.label],
  ['components/forks/fork-tree.tsx', 'fontSize="10" (5 sites)', 10, SVG_TEXT.tick],
  ['components/forks/fork-tree.tsx', 'fontSize={compact ? 10 : 11}: 10', 10, SVG_TEXT.tick],
  ['components/forks/fork-tree.tsx', 'CHAR_10 = 6.4 (0.64 em, observed)', 6.4, FORK_CHAR_10],
  ['components/forks/fork-days-chart.tsx', 'fontSize="11" (4 sites)', 11, SVG_TEXT.label],
  ['components/forks/fork-days-chart.tsx', 'CHAR = 6.6', 6.6, advancePx(SVG_TEXT.label)],
  // the hand-computed minimum widths of the ListTable columns whose comment records what they
  // hold (the rendered widths are pinned exactly by blocks-page, transactions-page and
  // transaction-list-table: 1,230 and 564 px as before)
  [
    'app/blocks/page.tsx',
    'Block: 195 + ROW_SEAL.block.footprint',
    195 + ROW_SEAL.block.footprint,
    columnPx(
      ROW_SEAL.block.footprint +
        textPx(11, fontPx('body')) +
        charPx('small') +
        textPx(11, fontPx('meta')) +
        16,
      13
    ),
  ],
  ['app/blocks/page.tsx', 'Reward: 118', 118, columnPx(textPx(14, fontPx('small')) + 16, 1)],
  ['app/blocks/page.tsx', 'Fees: 105', 105, columnPx(textPx(12, fontPx('small')) + 16, 2)],
  [
    'app/blocks/page.tsx',
    'Height (608 px table): 104 + ROW_SEAL.block.footprint',
    104 + ROW_SEAL.block.footprint,
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
    columnPx(textPx(13, fontPx('small')) + 16),
  ],
  [
    'app/blocks/page.tsx',
    'Size · cycles (608 px table): 96',
    96,
    columnPx(textPx(13, fontPx('small')), 2),
  ],
  [
    'app/transactions/page.tsx',
    'Transaction (pool): 237',
    237,
    columnPx(ROW_SEAL.tx.footprint + textPx(17, fontPx('body')) + 16, 31),
  ],
  [
    'app/transactions/page.tsx',
    'Status · waiting: 172',
    172,
    columnPx(7 + 6 + textPx(10, fontPx('small')) + 6 + textPx(9, fontPx('small')) + 16),
  ],
  [
    'app/transactions/page.tsx',
    'Fee rate: 123',
    123,
    columnPx(Math.max(textPx(10, fontPx('meta')), textPx(13, fontPx('small'))) + 16, 13),
  ],
  [
    'app/transactions/page.tsx',
    'Size (pool): 68',
    68,
    columnPx(textPx(7, fontPx('small')) + 16, 1),
  ],
  ['app/transactions/page.tsx', 'Cycles (pool): 94', 94, columnPx(textPx(13, fontPx('small')))],
  [
    'components/transaction/transaction-list-table.tsx',
    'Transaction: 208',
    208,
    columnPx(ROW_SEAL.tx.footprint + textPx(17, fontPx('body')) + 16, 2),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'What it did: 246',
    246,
    columnPx(16 + 8 + 8 + 16 + textPx(27, fontPx('small')), 3),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Value: 160',
    160,
    columnPx(textPx(20, fontPx('small')) + 16),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Fee: 96',
    96,
    columnPx(textPx(11, fontPx('small')) + 16),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Cells: 62',
    62,
    columnPx(textPx(6, fontPx('small')) + 16, 2),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Size: 121 (SIZE_TRACK 56)',
    121,
    columnPx(56 + 10 + textPx(5, fontPx('small')) + 16, 3),
  ],
  [
    'components/transaction/transaction-list-table.tsx',
    'Cycles: 162 (CYCLES_TRACK 56)',
    162,
    columnPx(56 + 10 + textPx(11, fontPx('small')) + 16),
  ],
  [
    'components/asset/inventory-table.tsx',
    'CKB held in its cells: 155',
    155,
    columnPx(textPx('CKB held in its cells'.length, fontPx('meta')) + 16),
  ],
  ['app/scripts/page.tsx', 'SHARE_FIGURE = 51', 51, columnPx(textPx(7, fontPx('small')))],
];

describe('every literal lane F3 replaced computes the number it was', () => {
  it.each(LITERALS)('%s: %s', (_file, _literal, was, now) => {
    expect(now).toBe(was);
  });
});

describe('the snapped advance the six fit tests shared', () => {
  // concentration-bar, script-code-evidence, holder-timeline, icicle-partition, dao-board and
  // script-family-board each wrote `Math.max(Math.round(0.6 * px), 0.6 * px)`
  const old = (px: number) => Math.max(Math.round(0.6 * px), 0.6 * px);

  it('gives what each copy gave at every size they measure, to the bit but at 12 px', () => {
    for (const px of [10, 10.5, 11, 13]) expect(snappedAdvancePx(px), `${px} px`).toBe(old(px));
  });

  it('gives 7.2 at 12 px where 0.6 × 12 was one unit in the last place under it', () => {
    expect(old(12)).toBe(7.199999999999999);
    expect(snappedAdvancePx(12)).toBe(7.2);
    expect(snappedAdvancePx(12) - old(12)).toBeLessThan(1e-14);
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

describe('headBudget (script-family-page) on every head it budgets', () => {
  const old = (label: string) => Math.ceil(label.length * 0.6 * 11) + 16;
  const now = (label: string) => Math.ceil(textPx(label.length, fontPx('meta'))) + 16;

  it('budgets what it did for every head of up to 40 characters', () => {
    for (let n = 1; n <= 40; n++) expect(now('x'.repeat(n)), `${n}`).toBe(old('x'.repeat(n)));
    expect([now('hash type'), now('live cells')]).toEqual([76, 82]);
  });
});
