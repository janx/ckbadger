import { describe, expect, it } from 'vitest';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import { axisTicks, chartModel } from '@/components/charts/chart-model';
import { CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';

// F2 charts group: the Charts index board's encodings, computed once in the chart model.

const def = (slug: string) => {
  const definition = CHART_BY_SLUG.get(slug);
  if (!definition) throw new Error(`no chart ${slug}`);
  return definition;
};

const composition = {
  title: 'Common Knowledge Bytes Composition',
  series: [
    { key: 'transfer', label: 'CKB Transfer', color: '#22c55e' },
    { key: 'dao', label: 'DAO', color: '#f59e0b' },
    { key: 'udt', label: 'UDT', color: '#3b82f6' },
    { key: 'nftSpore', label: 'Object (Spore)', color: '#ec4899' },
    { key: 'otherContracts', label: 'Other Contracts', color: '#8b5cf6' },
  ],
  data: [
    {
      date: '2026-10-29',
      values: { transfer: '100', dao: '300', udt: '50', nftSpore: '40', otherContracts: '10' },
    },
  ],
};

const hodlSeries = [
  ['gt3y', '> 3y', '27.47'],
  ['1y3y', '1y-3y', '20.01'],
  ['6m1y', '6m-1y', '10'],
  ['3m6m', '3m-6m', '10'],
  ['1m3m', '1m-3m', '10'],
  ['1w1m', '1w-1m', '10'],
  ['1d1w', '1d-1w', '7'],
  ['24h', '24h', '5.54'],
] as const;
const hodl = {
  title: 'CKB HODL Wave',
  series: hodlSeries.map(([key, label]) => ({ key, label, color: '#a78bfa' })),
  data: [
    {
      date: '2026-10-29',
      values: {
        ...Object.fromEntries(hodlSeries.map(([key, , value]) => [key, value])),
        holderCount: '358415',
      },
    },
  ],
};

describe('Charts index encodings (F2 charts)', () => {
  it('colours supply, secondary issuance and knowledge by kind by class, gold only for DAO (CH-06, CH-14)', () => {
    const kind = chartModel(def('common-knowledge-composition'), composition);
    expect(kind.series.map((s) => [s.label, s.color])).toEqual([
      // Largest first: the bottom band is the biggest share today.
      ['Nervos DAO', CLASS_COLORS.dao],
      ['Plain CKB', CLASS_COLORS.ckb],
      ['Tokens', CLASS_COLORS.tokens],
      ['Objects', CLASS_COLORS.objects],
      ['Other scripts', CLASS_COLORS.other],
    ]);
    const supply = chartModel(def('total-supply'), {
      title: 'Total Supply',
      series: [
        { key: 'circulating', label: 'Circulating', color: '#00c389' },
        { key: 'nervosdao', label: 'Nervos DAO', color: '#3b82f6' },
        { key: 'burnt', label: 'Burnt', color: '#6b7280' },
      ],
      data: [{ date: '2026-10-29', values: { circulating: '3', nervosdao: '2', burnt: '1' } }],
    });
    expect(supply.series.map((s) => [s.label, s.color])).toEqual([
      ['Circulating', DATA_COLOR],
      ['Nervos DAO', CLASS_COLORS.dao],
      ['Burnt', CLASS_COLORS.ckb],
    ]);
    const secondary = chartModel(def('secondary-issuance'), {
      title: 'Secondary issuance',
      series: [
        { key: 'compensation', label: 'DAO', color: '#00c389' },
        { key: 'mining', label: 'Mining Reward', color: '#8b5cf6' },
        { key: 'burnt', label: 'Treasury', color: '#6b7280' },
      ],
      data: [
        {
          date: '2026-10-29',
          values: { compensation: '2', mining: '3', burnt: '1' },
          protocolTotalShannons: '600000000',
          phase1CompensationShannons: '0',
        },
      ],
    });
    // The miners' share is jade, never the Objects violet; stacked burnt → compensation → miners.
    expect(secondary.series.map((s) => [s.label, s.color])).toEqual([
      ['Burnt', CLASS_COLORS.ckb],
      ['DAO compensation', CLASS_COLORS.dao],
      ['Miners', DATA_COLOR],
    ]);
    expect(() =>
      chartModel(def('total-supply'), {
        ...composition,
        title: 'Supply',
      })
    ).toThrow(/total-supply: unknown series transfer/);
  });

  it('heads knowledge by kind with the leading share and lists every kind in a legend (CH-07)', () => {
    const model = chartModel(def('common-knowledge-composition'), composition);
    expect(model.headline).toEqual({ pre: 'nervos dao', display: '60.00', unit: '%' });
    expect(model.card?.legend.map((l) => `${l.label} ${l.value}`)).toEqual([
      'Nervos DAO 60.00 %',
      'Plain CKB 20.00 %',
      'Tokens 10.00 %',
      'Objects 8.00 %',
      'Other scripts 2.00 %',
    ]);
  });

  it('summarises the HODL wave in four age groups on a fixed 0–100 axis (CH-07, CH-15)', () => {
    const model = chartModel(def('hodl-wave'), hodl);
    expect(model.card?.series.map((s) => s.label)).toEqual([
      'over 1 y',
      '3 m – 1 y',
      '1 w – 3 m',
      'under 1 w',
    ]);
    expect(model.card?.points[0].values).toEqual({
      over1y: '47.48',
      m3y1: '20',
      w1m3: '20',
      under1w: '12.54',
    });
    expect(model.headline).toEqual({ pre: 'over 1 y', display: '47.48', unit: '%' });
    expect(model.yDomain).toEqual([0, 100]);
    // The detail keeps every band the API returns.
    expect(model.series).toHaveLength(8);
  });

  it('summarises activity by type in the four groups of the board (CH-07)', () => {
    const model = chartModel(def('activity-type-breakdown'), {
      title: 'Activity Type Breakdown',
      series: ['transfer', 'dao', 'token', 'object', 'identity', 'scriptCall'].map((key) => ({
        key,
        label: key,
        color: '#000',
      })),
      data: [
        {
          date: '2026-10-29',
          values: {
            transfer: '10',
            dao: '2',
            token: '3',
            object: '4',
            identity: '1',
            scriptCall: '80',
          },
        },
      ],
    });
    expect(model.card?.legend.map((l) => `${l.label} ${l.value}`)).toEqual([
      'Script calls 80.00 %',
      'Transfers 10.00 %',
      'Assets 8.00 %',
      'DAO 2.00 %',
    ]);
    expect(model.headline).toEqual({ pre: 'script calls', display: '80.00', unit: '%' });
  });

  it('draws live cells as one live count, consumed cells kept apart (CH-08)', () => {
    const model = chartModel(def('cell-count'), {
      title: 'Cell Count',
      series: [
        { key: 'allCells', label: 'All Cells', color: '#6b7280' },
        { key: 'liveCells', label: 'Live Cells', color: '#00c389' },
        { key: 'deadCells', label: 'Dead Cells', color: '#ef4444' },
      ],
      data: [
        {
          date: '2026-10-29',
          values: { allCells: '102408911', liveCells: '1462472', deadCells: '100946439' },
        },
      ],
    });
    expect(model.kind).toBe('line');
    expect(model.series.map((s) => s.key)).toEqual(['liveCells']);
    expect(model.points[0].values).toEqual({ liveCells: '1462472' });
    expect(model.secondary?.title).toBe('Consumed cells, cumulative');
    expect(model.secondary?.points[0].values.secondary).toBe('100946439');
  });

  it('heads distributions by their meaning, not "Largest bucket" (CH-09)', () => {
    const cellSize = chartModel(def('cell-size-distribution'), {
      title: 'Cell size',
      yAxisLabel: 'Cells',
      data: [
        { date: '<100 CKB', value: '600', value2: '1' },
        { date: '100+ CKB', value: '300', value2: '1' },
        { date: '>=1m CKB', value: '100', value2: '1' },
      ],
    });
    expect(cellSize.headline).toEqual({ pre: '<100 CKB', display: '60.00', unit: '%' });
    expect(cellSize.summaryLabel).not.toBe('Largest bucket');
    const epochs = chartModel(def('epoch-time-distribution'), {
      title: 'Epochs',
      yAxisLabel: 'Epochs',
      data: [
        { date: '3.98', value: '10' },
        { date: '4.02', value: '622' },
        { date: '50.50', value: '1' },
      ],
    });
    expect(epochs.headline).toEqual({ pre: 'most common', display: '4.02', unit: 'hours' });
    const cohort = chartModel(def('address-cohort-retention'), {
      title: 'Cohorts',
      yAxisLabel: '%',
      data: [
        { date: '2024-06', value: '7.901989', value2: '10' },
        { date: '2026-09', value: '1.5', value2: '3' },
      ],
    });
    expect(cohort.headline).toEqual({ pre: '2026-09', display: '1.50', unit: '%' });
  });

  it('heads ranked cards with the leader share, grouped like every other number (CH-31)', () => {
    const miners = chartModel(def('miner-address-distribution'), {
      title: 'Miners',
      windowDays: 7,
      totalBlocks: 67791,
      fromDate: '2026-10-23',
      toDate: '2026-10-29',
      data: [
        {
          minerLockHash: `0x${'a'.repeat(64)}`,
          address: 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq93hhsj',
          minerName: null,
          blocksMined: 42705,
          percentage: '62.9950',
        },
      ],
    });
    // 62.9950 rounds half up to 63.00, as every rate reading does.
    expect(miners.headline).toEqual({ pre: 'top', display: '63.00', unit: '%' });
  });

  it("heads nominal APC with the API's today at its exact year, never the quarter sample (CH-03, CH-33)", () => {
    const payload = {
      title: 'Nominal APC',
      yAxisLabel: 'Nominal APC (%)',
      y2AxisLabel: 'nominal-apc-timeline',
      data: [
        { date: '0', value: '4.2', value2: 'past' },
        { date: '6.75', value: '2.3642', value2: 'current' },
        { date: '20', value: '1.59', value2: 'future' },
      ],
    };
    const model = chartModel(def('nominal-apc'), {
      ...payload,
      today: { blockNumber: 20876568, date: '2026-10-30', year: '6.9561234', value: '2.3395' },
    });
    expect(model.headline).toEqual({ pre: 'year 6.96', display: '2.34', unit: '%' });
    expect(model.today).toEqual({ label: 'year 6.96', values: { primary: '2.3395' } });
    expect(chartModel(def('nominal-apc'), payload).headline).toBeNull();
  });

  it('marks the consensus uncle-rate target and draws real inflation dashed (CH-12)', () => {
    const uncle = chartModel(def('uncle-rate'), {
      title: 'Uncle rate',
      yAxisLabel: 'Uncle rate',
      data: [{ date: '2026-10-29', value: '0.0257' }],
      orphanRateTarget: { numerator: 1, denominator: 40 },
    });
    expect(uncle.referenceLines).toEqual([{ value: 2.5, label: 'target 2.50 %' }]);
    const inflation = chartModel(def('inflation-rate'), {
      title: 'Inflation',
      yAxisLabel: 'Nominal',
      y2AxisLabel: 'Real',
      data: [{ date: '2026-10-29', value: '5.5', value2: '1.2' }],
    });
    expect(inflation.series.map((s) => [s.key, !!s.dashed])).toEqual([
      ['primary', false],
      ['companion', true],
    ]);
    const turnover = chartModel(def('capacity-turnover-ratio'), {
      title: 'Turnover',
      yAxisLabel: 'Daily',
      y2AxisLabel: 'Weekly',
      data: [{ date: '2026-10-29', value: '0.5', value2: '1.43' }],
    });
    // The card draws the weekly share alone; the daily share stays on the detail.
    expect(turnover.card?.series.map((s) => s.key)).toEqual(['primary']);
    expect(turnover.series.map((s) => s.key)).toEqual(['primary', 'companion']);
  });
});

describe('axis ticks (CH-24, CH-09, CH-35)', () => {
  const days = (labels: string[]) => labels.map((label) => ({ label, values: {} }));

  it('labels a long daily series at year starts, thinned to the room available', () => {
    const points = days([
      '2019-11-16',
      '2020-01-01',
      '2021-01-01',
      '2022-01-01',
      '2023-01-01',
      '2024-01-01',
      '2025-01-01',
      '2026-01-01',
      '2026-10-29',
    ]);
    expect(axisTicks(points, 'date', 8).map((t) => t.label)).toEqual([
      '2020',
      '2021',
      '2022',
      '2023',
      '2024',
      '2025',
      '2026',
    ]);
    expect(axisTicks(points, 'date', 3).map((t) => t.label)).toEqual(['2020', '2023', '2026']);
  });

  it('labels a short daily series at month starts', () => {
    const points = days(['2026-08-02', '2026-09-01', '2026-09-15', '2026-10-01', '2026-10-29']);
    expect(axisTicks(points, 'date', 8)).toEqual([
      { index: 1, label: '2026-09' },
      { index: 3, label: '2026-10' },
    ]);
  });

  it('labels a span inside one month by day (CH-58: three days of crawler history)', () => {
    const points = days(['2026-10-27', '2026-10-28', '2026-10-29']);
    expect(axisTicks(points, 'date', 6)).toEqual([
      { index: 0, label: '10-27' },
      { index: 1, label: '10-28' },
      { index: 2, label: '10-29' },
    ]);
  });

  it('labels epochs with thousands groups and every bucket of a short distribution', () => {
    const epochs = days(Array.from({ length: 15203 }, (_, i) => String(i)));
    expect(axisTicks(epochs, 'epoch', 4).map((t) => t.label)).toEqual([
      '#0',
      '#5,000',
      '#10,000',
      '#15,000',
    ]);
    const buckets = days(['<100 CKB', '100+ CKB', '>=1m CKB']);
    expect(axisTicks(buckets, 'bucket', 3).map((t) => t.label)).toEqual([
      '<100 CKB',
      '100+ CKB',
      '>=1m CKB',
    ]);
    const years = days(['0', '0.25', '5', '10', '15', '20']);
    expect(axisTicks(years, 'model-year', 3).map((t) => t.label)).toEqual([
      'year 0',
      'year 10',
      'year 20',
    ]);
  });
});
