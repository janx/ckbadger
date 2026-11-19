import { describe, expect, it } from 'vitest';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { capNote, chartHead } from '@/components/charts/chart-head';
import {
  axisOutliers,
  chartModel,
  cutShare,
  formatChartHover,
  formatChartReadout,
} from '@/components/charts/chart-model';
import { CHART_FIXTURES } from './chart-hero-fixtures';

/**
 * The D1 hand-back (lane D1, items 1 and 2): a chart page states a percentage in one form, cut and
 * never rounded, and every whole a page states or divides by is the API's figure, never the page's
 * sum of its bands. `cutShare` (components/charts/chart-model.ts) owns a percentage on a chart page:
 * the head's sentence, a band's end, a ranking's rows and its card preview; a rate is cut by
 * `cutDecimal`, the head's function, in the hover readout, the in-plot reading and the card.
 */

const definition = (slug: string) => {
  const found = CHART_BY_SLUG.get(slug);
  if (!found) throw new Error(`no chart ${slug}`);
  return found;
};

/** A fixture of the hero suite, deep-copied so a test can change what the API sends. */
function fixture<T = Record<string, unknown>>(method: string): T {
  return structuredClone(CHART_FIXTURES[method]) as T;
}

type StackedFixture = {
  data: { date: string; values: Record<string, string>; componentsTotalShannons?: string }[];
};

describe('one form of a percentage on a chart page: cut (item 1)', () => {
  it('cuts a rate in the hover readout, the in-plot reading and the card, as the head does', () => {
    // half up these were 3.00, 9.96 and 1,234.57
    expect(formatChartReadout('2.999', '%')).toBe('2.99');
    expect(formatChartHover('9.955', 's')).toBe('9.95');
    expect(formatChartReadout('1234.567', 'P')).toBe('1,234.56');
    expect(formatChartReadout('+1.119', 'PH/s')).toBe('+1.11');
    expect(formatChartReadout('-0.009', '%')).toBe('0.00');
  });

  it('writes a positive share that cuts to nothing as under a hundredth, never a false 0.00%', () => {
    expect(cutShare('1', '1000000', 'tiny')).toBe('<0.01%');
    expect(cutShare('0', '1000000', 'none')).toBe('0.00%');
    // a part below its whole cuts below 100.00, so no bound is needed there
    expect(cutShare('999999', '1000000', 'nearly')).toBe('99.99%');
  });

  it('cuts the miners’ rows and their card preview', () => {
    const model = chartModel(
      definition('miner-address-distribution'),
      fixture('getMinerAddressDistributionChart') as never
    );
    // 9,000 of 60,583 blocks is 14.8556…%: half up 14.86, cut 14.85
    expect(model.rankedRows![1].share).toBe('14.85%');
    expect(model.rankedPreview!.rows[1].share).toBe('14.85%');
  });

  it('cuts a script ranking’s rows, its preview and its leader’s fact', () => {
    const model = chartModel(
      definition('most-utilized-scripts'),
      fixture('getMostUtilizedScriptsChart') as never
    );
    // 500 of the day's 2,200 CKB is 22.7272…%: half up 22.73, cut 22.72
    expect(model.rankedRows![1].share).toBe('22.72%');
    expect(model.rankedPreview!.rows[1].share).toBe('22.72%');
  });
});

describe('a histogram’s bucket labels are cut, as the route writes them (the ruling)', () => {
  it('reads an epoch-duration bucket by its cut label and fills its empty minutes cut', () => {
    // a minute in hours at two decimals, cut, as the route writes it: 181 minutes is 3.01 (half up
    // it was 3.02), 184 is 3.06 (was 3.07); one minute is 1.67 hundredths, so no two share a label
    const hours = (minutes: number) =>
      `${Math.floor(minutes / 60)}.${String(Math.floor(((minutes % 60) * 100) / 60)).padStart(2, '0')}`;
    expect([180, 181, 182, 183, 184].map(hours)).toEqual(['3.00', '3.01', '3.03', '3.05', '3.06']);
    // the route sends the minutes some epoch took: 3 to 4 hours, but 183 minutes
    const minutes = Array.from({ length: 61 }, (_, i) => 180 + i).filter((m) => m !== 183);
    const model = chartModel(definition('epoch-time-distribution'), {
      title: 'Epoch Time Distribution',
      yAxisLabel: 'Epochs',
      data: minutes.map((m) => ({ date: hours(m), value: '1' })),
    } as never);
    // every minute of the run, the empty one drawn as its 0 under its own cut label
    expect(model.points.map((point) => point.label)).toEqual(
      Array.from({ length: 61 }, (_, i) => hours(180 + i))
    );
    expect(model.points[3]).toMatchObject({ label: '3.05', values: { primary: '0' } });
  });
});

describe('a whole a chart page states is the API’s figure (item 2)', () => {
  it('divides knowledge by kind by the day’s knowledge size the route sends', () => {
    const payload = fixture<StackedFixture>('getCommonKnowledgeCompositionChart');
    // a whole other than the parts' sum shows which one the page divides by
    for (const point of payload.data) point.values.knowledgeSize = '200';
    const slug = definition('common-knowledge-composition');
    const model = chartModel(slug, payload as never);
    expect(chartHead(slug, model).sentence).toBe(
      'Other scripts hold 22.00% of occupied CKB, plain CKB 20.00%.'
    );
    expect(model.bandEnds!.otherContracts).toBe('22.00%');
  });

  it('divides secondary issuance by the total of its destinations the route sends', () => {
    const payload = fixture<StackedFixture>('getSecondaryIssuanceChart');
    // 18,835 CKB, twice the three destinations
    for (const point of payload.data) point.componentsTotalShannons = '1883500000000';
    const slug = definition('secondary-issuance');
    const model = chartModel(slug, payload as never);
    expect(chartHead(slug, model).sentence).toBe(
      'By destination, 36.19% is burnt, 9.02% paid to DAO depositors as compensation and 4.77% to miners.'
    );
  });

  it('divides a ranking by the day’s total the route sends', () => {
    const payload = fixture<{ usedShare: StackedFixture; capacityShare: StackedFixture }>(
      'getMostUtilizedScriptsChart'
    );
    for (const scope of [payload.usedShare, payload.capacityShare])
      scope.data.at(-1)!.values.total = '440000000000';
    const slug = definition('most-utilized-scripts');
    const model = chartModel(slug, payload as never);
    // the five largest hold 2,000 of 4,400 CKB
    expect(chartHead(slug, model).sentence).toBe(
      'The five largest scripts hold 45.45% of occupied CKB.'
    );
    expect(model.rankedRows![0].share).toBe('13.63%');
  });

  // lane D1's third round, M2: the five drawn ranks' total and the card's "Others" are the route's
  // figures (drawnTotal, rest), never a sum in the page; a figure that differs from the sum shows
  // which one the page states
  it('states the five largest by the route’s drawn total', () => {
    const payload = fixture<{ usedShare: StackedFixture; capacityShare: StackedFixture }>(
      'getMostUtilizedScriptsChart'
    );
    payload.usedShare.data.at(-1)!.values.drawnTotal = '100000000000';
    const slug = definition('most-utilized-scripts');
    // 1,000 of the day's 2,200 CKB
    expect(chartHead(slug, chartModel(slug, payload as never)).sentence).toBe(
      'The five largest scripts hold 45.45% of occupied CKB.'
    );
  });

  it('writes the card’s "Others" as the route’s rest', () => {
    const payload = fixture<{ usedShare: StackedFixture; capacityShare: StackedFixture }>(
      'getMostUtilizedScriptsChart'
    );
    payload.usedShare.data.at(-1)!.values.rest = '77700000000';
    const model = chartModel(definition('most-utilized-scripts'), payload as never);
    // 777 of 2,200 CKB
    expect(model.rankedPreview!.rows.at(-1)).toMatchObject({ title: 'Others', share: '35.31%' });
  });

  // lane D1's third round, N12: a tie at ranks 5 and 6 is the route's to break; the page keeps
  // its order (it sorted ties by label)
  it('keeps the route’s order of a ranking, ties too', () => {
    const payload = fixture<{
      usedShare: StackedFixture & { series: { key: string; label: string }[] };
      capacityShare: StackedFixture;
    }>('getMostUtilizedScriptsChart');
    const last = payload.usedShare.data.at(-1)!.values;
    // ranks 5 and 6 tie at 200 CKB; the route put "Type ID" (top4) before "Always success" (top5)
    last.top5 = last.top4;
    payload.usedShare.series[4].label = 'Zeta';
    payload.usedShare.series[5].label = 'Alpha';
    const model = chartModel(definition('most-utilized-scripts'), payload as never);
    const labels = model.rankedRows!.map((row) => row.label);
    expect(labels.indexOf('Zeta')).toBeLessThan(labels.indexOf('Alpha'));
  });

  it('divides cell size by the live cells and the occupied CKB the route counts', () => {
    const payload = fixture<{ liveCells: string; occupiedCapacity: string }>(
      'getCellSizeDistributionChart'
    );
    payload.liveCells = '2022560';
    payload.occupiedCapacity = '353949148';
    const slug = definition('cell-size-distribution');
    const head = chartHead(slug, chartModel(slug, payload as never));
    expect(head.figure!.display).toBe('45.05');
    expect(head.figure!.title).toBe('911,281 of 2,022,560 live cells');
    expect(head.sentence).toBe('The most occupied CKB is in cells of 100k – 1M CKB, 16.95% of it.');
  });

  it('refuses a stack whose route sends no whole', () => {
    for (const [slug, method, field] of [
      ['activity-type-breakdown', 'getActivityTypeBreakdownChart', 'kindOccurrences'],
      ['total-supply', 'getTotalSupplyChart', 'totalIssuance'],
      ['common-knowledge-composition', 'getCommonKnowledgeCompositionChart', 'knowledgeSize'],
    ] as const) {
      const payload = fixture<StackedFixture>(method);
      delete payload.data.at(-1)!.values[field];
      expect(() => chartModel(definition(slug), payload as never), slug).toThrow(field);
    }
    const issuance = fixture<StackedFixture>('getSecondaryIssuanceChart');
    delete issuance.data.at(-1)!.componentsTotalShannons;
    expect(() => chartModel(definition('secondary-issuance'), issuance as never)).toThrow(
      'componentsTotalShannons'
    );
  });

  it('names a stack above its scale by the day’s whole the route sends', () => {
    const payload = fixture<StackedFixture>('getActivityTypeBreakdownChart');
    // the spike's kind occurrences, one more than its bands, shows where the note reads its whole
    payload.data[50].values.kindOccurrences = '1000699';
    const model = chartModel(definition('activity-type-breakdown'), payload as never);
    const outliers = axisOutliers(model.points, model.series, model.kind)!;
    expect(outliers.indexes).toEqual([50]);
    expect(capNote(model, model.points, outliers.indexes, (label) => label, 'day')).toBe(
      // re-pinned for lane D1's second follow-up, item 2: the stack's whole is kind counts
      `${model.points[50].label} · 1,000,699 kind counts, above this scale`
    );
  });
});
