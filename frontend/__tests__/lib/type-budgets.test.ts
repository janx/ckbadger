import { describe, expect, it } from 'vitest';
import { MAP_CHAR_PX } from '@/lib/cell-byte-layout';
import { LABEL_ADVANCE as UPGRADE_LABEL_ADVANCE } from '@/components/scripts/script-family-board';
import { MONO_ADVANCE_EM, charPx, fontPx, snappedAdvancePx, textPx } from '@/lib/type-metrics';

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

describe('headBudget (script-family-page) on every head it budgets', () => {
  const old = (label: string) => Math.ceil(label.length * 0.6 * 11) + 16;
  const now = (label: string) => Math.ceil(textPx(label.length, fontPx('meta'))) + 16;

  it('budgets what it did for every head of up to 40 characters', () => {
    for (let n = 1; n <= 40; n++) expect(now('x'.repeat(n)), `${n}`).toBe(old('x'.repeat(n)));
    expect([now('hash type'), now('live cells')]).toEqual([76, 82]);
  });
});
