import { describe, expect, it } from 'vitest';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { chartModel } from '@/components/charts/chart-model';
import { chartHead } from '@/components/charts/chart-head';
import { CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';
import { PALETTE, color, contrastRatio, type PaletteKey } from '@/lib/palette';
import { fontPx, textPx } from '@/lib/type-metrics';
import { CHART_FIXTURES } from './chart-hero-fixtures';

/**
 * The Supply chart draws C in the bands that make it, its head's "circulating" the protocol
 * circulation (C − genesis burnt − S), the amount the site calls circulating everywhere (lane D1,
 * second follow-up, item 1; named for what they hold in lane exactness, part A2, M1).
 *
 * lane exactness, part B1: five bands, the DAO's cells drawn whole. `protocol_deposited`, the capacity of every
 * live DAO cell, is in each day's snapshot, so a withdrawal request's principal, still in a DAO
 * cell, is a band of its own beside the deposits:
 *
 *     C = outside the Nervos DAO            (circulating − protocol_deposited)    outsideDao
 *       + deposited in the Nervos DAO       (total_deposited)                     daoDeposited
 *       + withdrawing from the Nervos DAO   (protocol_deposited − total_deposited) daoWithdrawing
 *       + unclaimed DAO compensation        (still in S)                          unclaimedCompensation
 *       + burnt                                                                   burnt
 *
 * It was four ("Circulating, not deposited" held the requests' principal).
 */

const supply = () =>
  chartModel(CHART_BY_SLUG.get('total-supply')!, CHART_FIXTURES.getTotalSupplyChart as never);

/** The DAO compensation not yet claimed: the DAO's own gold, a step from it (`lib/palette.ts`). */
const UNCLAIMED = 'gold-ramp' as PaletteKey;
/** The CKB in withdrawal requests: the DAO's gold too, another step from it (`lib/palette.ts`). */
const WITHDRAWING = 'gold-mid' as PaletteKey;

describe('the Supply chart’s five bands (lane exactness, part B1)', () => {
  it('stacks five bands in the words and colours of their roles, bottom first', () => {
    expect(supply().series.map((s) => [s.key, s.label, s.color])).toEqual([
      ['outsideDao', 'Outside the Nervos DAO', DATA_COLOR],
      ['daoDeposited', 'Deposited in the Nervos DAO', CLASS_COLORS.dao],
      ['daoWithdrawing', 'Withdrawing from the Nervos DAO', color(WITHDRAWING)],
      ['unclaimedCompensation', 'Unclaimed DAO compensation', color(UNCLAIMED)],
      ['burnt', 'Burnt', CLASS_COLORS.ckb],
    ]);
  });

  it('ends each band on its share of C, and heads with the protocol circulation', () => {
    const model = supply();
    // 4,900, 3,000, 100, 500 and 3,500 of 12,000, cut
    expect(model.bandEnds).toEqual({
      outsideDao: '40.83%',
      daoDeposited: '25.00%',
      daoWithdrawing: '0.83%',
      unclaimedCompensation: '4.16%',
      burnt: '29.16%',
    });
    expect(model.latest?.raw).toBe('8000');
    expect(model.points.at(-1)!.whole).toBe('12000');
  });

  it('draws the DAO’s three golds clear of their neighbours and the ground', () => {
    // the board's floor for a stack's fills, 1.5 : 1, by night and by day; the withdrawing band
    // stands between the deposits and the unclaimed compensation, which stands on the burnt
    const neighbours: [PaletteKey, PaletteKey[]][] = [
      [WITHDRAWING, ['gold', UNCLAIMED, 'base-bg']],
      [UNCLAIMED, [WITHDRAWING, 'gold', 'slate-ckb', 'base-bg']],
    ];
    for (const theme of ['dark', 'light'] as const)
      for (const [ink, beside] of neighbours)
        for (const neighbour of beside)
          expect(
            contrastRatio(PALETTE[theme][ink], PALETTE[theme][neighbour]),
            `${theme}: ${ink} on ${neighbour}`
          ).toBeGreaterThanOrEqual(1.5);
  });

  it('names every band at its right end at 1280 and in the phone key at 360', () => {
    const names = supply().series.map((s) => s.label);
    const figure = textPx('41.66%'.length, fontPx('small'));
    const longest = Math.max(...names.map((name) => textPx(name.length, fontPx('small'))));
    // re-pinned for lane exactness, part B1: the longest name is "Withdrawing from the Nervos DAO", 31 glyphs at
    // 13 px, 241.8 px (it was "Deposited in the Nervos DAO", 27, 210.6); the figure 6 glyphs, 46.8
    expect(fontPx('small')).toBe(13);
    expect(longest).toBeCloseTo(241.8, 6);
    expect(figure).toBeCloseTo(46.8, 6);
    // 1280: the leader 12, the name, 16, the figure, inside the two fifths the plot gives up:
    // 12 + 241.8 + 16 + 46.8 = 316.6 ≤ 0.4 × 1232 = 492.8
    expect(12 + longest + 16 + figure).toBeCloseTo(316.6, 6);
    expect(12 + longest + 16 + figure).toBeLessThanOrEqual(0.4 * 1232);
    // 360: the swatch 10, 8, the name, at least 8, the figure, on one line of 328:
    // 10 + 8 + 241.8 + 8 + 46.8 = 314.6 ≤ 328
    expect(10 + 8 + longest + 8 + figure).toBeCloseTo(314.6, 6);
    expect(10 + 8 + longest + 8 + figure).toBeLessThanOrEqual(328);
  });

  it('says the circulation, and of it the deposits and the withdrawing apart', () => {
    const definition = CHART_BY_SLUG.get('total-supply')!;
    // 8,000 of 12,000 circulates (66.66 %); of the 8,000, 3,000 deposited (37.50 %, the
    // circulation-ratio chart's deposited ÷ circulating) and 100 withdrawing (1.25 %); 3,500 burnt
    expect(chartHead(definition, supply()).sentence).toBe(
      'Of the 12,000 CKB issued, 66.66% circulates, 37.50% of it deposited in the Nervos DAO and 1.25% withdrawing from it; 29.16% is burnt.'
    );
  });

  it('refuses the bands of an API older than this page, and says so', () => {
    const band = (key: string) => ({ key, label: key, color: '#000' });
    const day = (values: Record<string, string>) => [{ date: '2026-11-18', values }];
    // part A2's four bands: the first held the requests' principal
    const a2 = {
      title: 'Total Supply',
      series: ['circulatingNotDeposited', 'daoDeposited', 'unclaimedCompensation', 'burnt'].map(
        band
      ),
      data: day({
        circulatingNotDeposited: '5',
        daoDeposited: '3',
        unclaimedCompensation: '0',
        burnt: '0',
        totalIssuance: '8',
        circulating: '8',
      }),
    };
    expect(() => chartModel(CHART_BY_SLUG.get('total-supply')!, a2 as never)).toThrow(
      'total-supply: the API sends the band circulatingNotDeposited: it is older than this page'
    );
    // lane D1's four bands: outsideDao then held them too, and no band is withdrawing
    const d1 = {
      title: 'Total Supply',
      series: ['outsideDao', 'daoDeposited', 'unclaimedCompensation', 'burnt'].map(band),
      data: day({
        outsideDao: '5',
        daoDeposited: '3',
        unclaimedCompensation: '0',
        burnt: '0',
        totalIssuance: '8',
        circulating: '8',
      }),
    };
    expect(() => chartModel(CHART_BY_SLUG.get('total-supply')!, d1 as never)).toThrow(
      'total-supply: the API sends no band daoWithdrawing: it is older than this page'
    );
  });
});
