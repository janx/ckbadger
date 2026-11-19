import { describe, expect, it } from 'vitest';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { chartModel } from '@/components/charts/chart-model';
import { CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';
import { PALETTE, color, contrastRatio, type PaletteKey } from '@/lib/palette';
import { fontPx, textPx } from '@/lib/type-metrics';
import { CHART_FIXTURES } from './chart-hero-fixtures';

/**
 * Lane D1, second follow-up, item 1: the Supply chart draws C in four bands that partition it — the
 * CKB circulating outside the DAO, the DAO's deposits, the DAO compensation not yet claimed and the
 * burnt — so the stack's top is the issue it names, and its head's "circulating" is the protocol
 * circulation (C − genesis burnt − S), the amount the site calls circulating everywhere.
 */

const supply = () =>
  chartModel(CHART_BY_SLUG.get('total-supply')!, CHART_FIXTURES.getTotalSupplyChart as never);

/** The DAO compensation not yet claimed: the DAO's own gold, a step from it (`lib/palette.ts`). */
const UNCLAIMED = 'gold-ramp' as PaletteKey;

describe('the Supply chart’s four bands (lane D1, second follow-up, item 1)', () => {
  it('stacks four bands in the words and colours of their roles, bottom first', () => {
    expect(supply().series.map((s) => [s.key, s.label, s.color])).toEqual([
      ['outsideDao', 'Outside the DAO', DATA_COLOR],
      ['daoDeposited', 'In the Nervos DAO', CLASS_COLORS.dao],
      ['unclaimedCompensation', 'Unclaimed DAO compensation', color(UNCLAIMED)],
      ['burnt', 'Burnt', CLASS_COLORS.ckb],
    ]);
  });

  it('ends each band on its share of C, and heads with the protocol circulation', () => {
    const model = supply();
    // 5,000, 3,000, 500 and 3,500 of 12,000, cut
    expect(model.bandEnds).toEqual({
      outsideDao: '41.66%',
      daoDeposited: '25.00%',
      unclaimedCompensation: '4.16%',
      burnt: '29.16%',
    });
    expect(model.latest?.raw).toBe('8000');
    expect(model.points.at(-1)!.whole).toBe('12000');
  });

  it('draws the unclaimed compensation clear of its neighbours and the ground', () => {
    for (const theme of ['dark', 'light'] as const) {
      const ink = PALETTE[theme][UNCLAIMED];
      // the board's floor for a stack's fills, 1.5 : 1, by night and by day
      for (const neighbour of ['gold', 'slate-ckb', 'base-bg'] as const)
        expect(
          contrastRatio(ink, PALETTE[theme][neighbour]),
          `${theme}: ${UNCLAIMED} on ${neighbour}`
        ).toBeGreaterThanOrEqual(1.5);
    }
  });

  it('names every band at its right end at 1280 and in the phone key at 360', () => {
    const names = supply().series.map((s) => s.label);
    const figure = textPx('41.66%'.length, fontPx('small'));
    const longest = Math.max(...names.map((name) => textPx(name.length, fontPx('small'))));
    // 1280: the leader 12, the name, 16, the figure, inside the two fifths the plot gives up
    expect(12 + longest + 16 + figure).toBeLessThanOrEqual(0.4 * 1232);
    // 360: the swatch 10, 8, the name, at least 8, the figure, on one line of 328
    expect(10 + 8 + longest + 8 + figure).toBeLessThanOrEqual(328);
  });
});
