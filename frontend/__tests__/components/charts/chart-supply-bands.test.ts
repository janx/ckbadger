import { describe, expect, it } from 'vitest';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { chartModel } from '@/components/charts/chart-model';
import { chartHead } from '@/components/charts/chart-head';
import { CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';
import { PALETTE, color, contrastRatio, type PaletteKey } from '@/lib/palette';
import { fontPx, textPx } from '@/lib/type-metrics';
import { CHART_FIXTURES } from './chart-hero-fixtures';

/**
 * Lane D1, second follow-up, item 1: the Supply chart draws C in four bands that partition it — the
 * CKB that circulates and is not deposited, the DAO's deposits, the DAO compensation not yet claimed
 * and the burnt — so the stack's top is the issue it names, and its head's "circulating" is the
 * protocol circulation (C − genesis burnt − S), the amount the site calls circulating everywhere.
 *
 * Re-pinned for lane exactness, part A2 (M1, the lead's ruling): the bands are named, and keyed, for what they hold. The
 * DAO band is the status-0 deposits, "Deposited in the Nervos DAO", the amount the DAO page and the
 * circulation-ratio chart call deposited; the liquid band is the circulation less those deposits, the
 * CKB in withdrawal requests included, "Circulating, not deposited" (`circulatingNotDeposited`; it
 * was "Outside the DAO", `outsideDao`, and the DAO band "In the Nervos DAO", while a request's
 * principal, inside the DAO, was drawn in the band outside it).
 */

const supply = () =>
  chartModel(CHART_BY_SLUG.get('total-supply')!, CHART_FIXTURES.getTotalSupplyChart as never);

/** The DAO compensation not yet claimed: the DAO's own gold, a step from it (`lib/palette.ts`). */
const UNCLAIMED = 'gold-ramp' as PaletteKey;

describe('the Supply chart’s four bands (lane D1, second follow-up, item 1)', () => {
  it('stacks four bands in the words and colours of their roles, bottom first', () => {
    expect(supply().series.map((s) => [s.key, s.label, s.color])).toEqual([
      ['circulatingNotDeposited', 'Circulating, not deposited', DATA_COLOR],
      ['daoDeposited', 'Deposited in the Nervos DAO', CLASS_COLORS.dao],
      ['unclaimedCompensation', 'Unclaimed DAO compensation', color(UNCLAIMED)],
      ['burnt', 'Burnt', CLASS_COLORS.ckb],
    ]);
  });

  it('ends each band on its share of C, and heads with the protocol circulation', () => {
    const model = supply();
    // 5,000, 3,000, 500 and 3,500 of 12,000, cut
    expect(model.bandEnds).toEqual({
      circulatingNotDeposited: '41.66%',
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
    // re-pinned for lane exactness, part A2 (M1): the longest name is "Deposited in the Nervos DAO", 27 glyphs at
    // 13 px, 210.6 px (it was "Unclaimed DAO compensation", 26, 202.8); the figure 6 glyphs, 46.8
    expect(fontPx('small')).toBe(13);
    expect(longest).toBeCloseTo(210.6, 6);
    expect(figure).toBeCloseTo(46.8, 6);
    // 1280: the leader 12, the name, 16, the figure, inside the two fifths the plot gives up:
    // 12 + 210.6 + 16 + 46.8 = 285.4 ≤ 0.4 × 1232 = 492.8
    expect(12 + longest + 16 + figure).toBeCloseTo(285.4, 6);
    expect(12 + longest + 16 + figure).toBeLessThanOrEqual(0.4 * 1232);
    // 360: the swatch 10, 8, the name, at least 8, the figure, on one line of 328:
    // 10 + 8 + 210.6 + 8 + 46.8 = 283.4 ≤ 328
    expect(10 + 8 + longest + 8 + figure).toBeCloseTo(283.4, 6);
    expect(10 + 8 + longest + 8 + figure).toBeLessThanOrEqual(328);
  });

  it('says the circulation and the deposits of it in the bands’ words (lane exactness, part A2, M1)', () => {
    const definition = CHART_BY_SLUG.get('total-supply')!;
    // 8,000 of 12,000 circulates (66.66 %), 3,000 of the 8,000 deposited (37.50 %), 3,500 burnt
    expect(chartHead(definition, supply()).sentence).toBe(
      'Of the 12,000 CKB issued, 66.66% circulates, 37.50% of it deposited in the Nervos DAO; 29.16% is burnt.'
    );
  });

  it('refuses the bands of an API older than this page, and says so (lane exactness, part A2, N2)', () => {
    const older = {
      title: 'Total Supply',
      series: [
        { key: 'outsideDao', label: 'Outside the DAO', color: '#00c389' },
        { key: 'daoDeposited', label: 'In the Nervos DAO', color: '#3b82f6' },
      ],
      data: [
        {
          date: '2026-11-18',
          values: { outsideDao: '5', daoDeposited: '3', totalIssuance: '8', circulating: '8' },
        },
      ],
    };
    expect(() => chartModel(CHART_BY_SLUG.get('total-supply')!, older as never)).toThrow(
      'total-supply: the API sends the band outsideDao: it is older than this page'
    );
  });
});
