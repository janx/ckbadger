import { describe, expect, it } from 'vitest';
import { CHART_BY_SLUG } from '@/components/charts/chart-catalogue';
import {
  axisTicks,
  categoricalAxis,
  chartModel,
  formatChartReadout,
  shortBucketLabel,
} from '@/components/charts/chart-model';
import { CLASS_COLORS, DATA_COLOR } from '@/lib/design-tokens';
import { chartHead } from '@/components/charts/chart-head';

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
      values: {
        transfer: '100',
        dao: '300',
        udt: '50',
        nftSpore: '40',
        otherContracts: '10',
        // the day's knowledge size, the route's (the D1 hand-back, item 2)
        knowledgeSize: '500',
      },
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
        // lane D1: the API's merged bands, from the capacities (here 10 + 10, 10 + 7 + 5.54)
        '1m6m': '20',
        lt1m: '22.54',
        holderCount: '358415',
      },
    },
  ],
  bands: [
    { key: 'gt3y', label: '3 years or more', members: ['gt3y'] },
    { key: '1y3y', label: '1 to 3 years', members: ['1y3y'] },
    { key: '6m1y', label: '6 to 12 months', members: ['6m1y'] },
    { key: '1m6m', label: '1 to 6 months', members: ['3m6m', '1m3m'] },
    { key: 'lt1m', label: 'under 1 month', members: ['1w1m', '1d1w', '24h'] },
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
      // C, the route's total issuance, more than the bands (the D1 hand-back, item 2)
      data: [
        {
          date: '2026-10-29',
          values: { circulating: '3', nervosdao: '2', burnt: '1', totalIssuance: '7' },
        },
      ],
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
          componentsTotalShannons: '600000000',
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
    // Re-pinned for lane D1 (rule 9, CH-07): the card heads with the page's own head — the largest
    // kind named in words, its CKB, and the sentence its share (was a card headline of its own).
    const head = chartHead(def('common-knowledge-composition'), model);
    expect(head.eyebrow).toBe('the Nervos DAO, the largest kind');
    expect(head.figure).toMatchObject({ display: '300', unit: 'CKB' });
    expect(head.sentence).toBe('The Nervos DAO holds 60.00% of occupied CKB, plain CKB 20.00%.');
    // Re-pinned for CH-14 (UI review 2026-11-14): one percent form, no space before the sign.
    expect(model.card?.legend.map((l) => `${l.label} ${l.value}`)).toEqual([
      'Nervos DAO 60.00%',
      'Plain CKB 20.00%',
      'Tokens 10.00%',
      'Objects 8.00%',
      'Other scripts 2.00%',
    ]);
  });

  // Re-pinned for lane D1 (the chart-hero board, rules 5 and 9): the card and the page draw the
  // same five bands the API names, its merged figures as sent (was four groups the page summed
  // from rounded percentages, and eight bands on the page: CH-07's two sets of names).
  it('draws the HODL wave in the API’s five bands on a fixed 0–100 axis, page and card (CH-07, CH-15)', () => {
    const model = chartModel(def('hodl-wave'), hodl);
    expect(model.card?.series.map((s) => s.label)).toEqual([
      '3 years or more',
      '1 to 3 years',
      '6 to 12 months',
      '1 to 6 months',
      'under 1 month',
    ]);
    expect(model.card?.points[0].values).toEqual({
      gt3y: '27.47',
      '1y3y': '20.01',
      '6m1y': '10',
      '1m6m': '20',
      lt1m: '22.54',
    });
    expect(chartHead(def('hodl-wave'), model).figure).toMatchObject({
      display: '27.47',
      unit: '% of all CKB',
    });
    expect(model.yDomain).toEqual([0, 100]);
    expect(model.series).toEqual(model.card?.series);
    // every band the API returns stays, in the table under the chart
    expect(model.table).toHaveLength(8);
  });

  // Re-pinned for CH-02 (UI review 2026-11-14): the card's bands are the six kinds in their class
  // colours; the merged "Assets" band had no class colour to take. The review's item D: they stand
  // in the page's fixed class order, not by size (the test said "largest first" and its data could
  // not tell); here the largest kind is not first and the shares do not descend.
  it('summarises activity by type in its six kinds, in the page’s fixed class order (CH-07, CH-02)', () => {
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
            transfer: '50',
            dao: '22',
            token: '3',
            object: '4',
            identity: '1',
            scriptCall: '20',
            // the day's kind occurrences, the route's (the D1 hand-back, item 2)
            kindOccurrences: '100',
          },
        },
      ],
    });
    // Re-pinned for lane D1 (rule 9, CH-07): the card's bands are the page's, in the page's class
    // order (was the card's own order, script calls first); still fixed, not by size.
    expect(model.card?.legend.map((l) => `${l.label} ${l.value}`)).toEqual([
      'CKB transfers 50.00%',
      'Nervos DAO 22.00%',
      'Tokens 3.00%',
      'Objects 4.00%',
      'Identities 1.00%',
      'Script calls 20.00%',
    ]);
    // the head names the largest kind in words (was the first kind of the card's order)
    const head = chartHead(def('activity-type-breakdown'), model);
    expect(head.eyebrow).toBe('CKB transfers, the largest kind in a day');
    expect(head.figure).toMatchObject({ display: '50', unit: 'activities' });
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
        { date: '100-1k CKB', value: '300', value2: '1' },
        { date: '>=1m CKB', value: '100', value2: '1' },
      ],
      // the route's wholes (the D1 hand-back, item 2)
      liveCells: '1000',
      occupiedCapacity: '3',
    });
    // Re-pinned for CH-12: the API's buckets only ('100+ CKB' is none of them), named in the
    // axis' words, and a bucket's unit is its symbol ('h', was 'hours'; CH-14).
    // Re-pinned for lane D1 (rules 3 and 9): the page's own head, named in words.
    expect(chartHead(def('cell-size-distribution'), cellSize)).toMatchObject({
      eyebrow: 'live cells of < 100 CKB',
      figure: { display: '60.00', unit: '% of live cells' },
    });
    // Re-pinned for CH-18: no fact restates the head's bucket (was: not "Largest bucket").
    expect(cellSize.facts).toBeUndefined();
    const epochs = chartModel(def('epoch-time-distribution'), {
      title: 'Epochs',
      yAxisLabel: 'Epochs',
      data: [
        // minute 241 in hours, cut (re-pinned for the ruling: half up it was 4.02)
        { date: '3.98', value: '10' },
        { date: '4.01', value: '622' },
        { date: '50.50', value: '1' },
      ],
    });
    expect(chartHead(def('epoch-time-distribution'), epochs)).toMatchObject({
      eyebrow: 'the most common duration',
      figure: { display: '4.01', unit: 'h' },
    });
    const cohort = chartModel(def('address-cohort-retention'), {
      title: 'Cohorts',
      yAxisLabel: '%',
      data: [
        { date: '2024-06', value: '7.901989', value2: '10' },
        { date: '2026-09', value: '1.5', value2: '3' },
      ],
    });
    // Re-pinned for CH-19 (UI review 2026-11-14): the newest cohort is the month still filling;
    // the headline is the last complete cohort, the one before it (was 2026-09).
    expect(chartHead(def('address-cohort-retention'), cohort)).toMatchObject({
      eyebrow: 'occupied share of its balance, the 2024-06 cohort',
      figure: { display: '7.90', unit: '%' },
    });
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
    // Re-pinned for lane D1 (rule 9, CH-07): the card heads with the page's figure, the leading
    // miner's blocks grouped (was its share in a headline of the card's own); its share stays on
    // its row, 42,705 of 67,791 blocks cut, 62.99% (re-pinned for the D1 hand-back, item 1: it was
    // half up, 63.00%).
    expect(chartHead(def('miner-address-distribution'), miners)).toMatchObject({
      at: '2026-10-23 → 2026-10-29',
      figure: { display: '42,705', unit: 'blocks' },
    });
    expect(miners.rankedRows?.[0].share).toBe('62.99%');
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
    // Re-pinned for lane D1: the page's head, at the API's today; its figure cut, never rounded
    // (2.3395 is 2.33, was 2.34); with no today, no figure, never the quarter sample. Its year cut
    // too (re-pinned for the ruling of 2026-11-19: 6.9561234 read year 6.96, it reads 6.95).
    expect(chartHead(def('nominal-apc'), model)).toMatchObject({
      at: 'year 6.95',
      figure: { display: '2.33', unit: '% a year' },
    });
    expect(model.today).toEqual({ label: 'year 6.95', values: { primary: '2.3395' } });
    expect(
      chartHead(def('nominal-apc'), chartModel(def('nominal-apc'), payload)).figure
    ).toBeNull();
  });

  it('marks the consensus uncle-rate target and draws real inflation dashed (CH-12)', () => {
    const uncle = chartModel(def('uncle-rate'), {
      title: 'Uncle rate',
      yAxisLabel: 'Uncle rate',
      data: [{ date: '2026-10-29', value: '0.0257' }],
      orphanRateTarget: { numerator: 1, denominator: 40 },
    });
    // Re-pinned for CH-14 (UI review 2026-11-14): one percent form, no space before the sign.
    expect(uncle.referenceLines).toEqual([{ value: 2.5, label: 'target 2.50%' }]);
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

describe('categorical axes (responsive review C#6)', () => {
  it('names the Cell size buckets as categories, while the many-bin histograms stay numeric', () => {
    expect(categoricalAxis(def('cell-size-distribution'))).toBe(true);
    expect(categoricalAxis(def('block-time-distribution'))).toBe(false);
    expect(categoricalAxis(def('epoch-time-distribution'))).toBe(false);
    expect(categoricalAxis(def('hash-rate'))).toBe(false);
  });
});

describe('visual pass fixes (F2 charts, 2026-10-30 services)', () => {
  it('reads counts under 100,000 whole and grouped, like the board (16,826)', () => {
    expect(formatChartReadout('16826', 'transactions')).toBe('16,826');
    expect(formatChartReadout('563', 'addresses')).toBe('563');
    expect(formatChartReadout('1462472', 'cells')).toBe('1.46M');
  });

  it('draws the consensus epoch-duration target from the API (CH-12, R2b-b)', () => {
    const model = chartModel(def('epoch-time-length'), {
      title: 'Epoch length',
      yAxisLabel: 'Hours',
      y2AxisLabel: 'Blocks',
      data: [{ date: '15204', value: '4.15', value2: '1800' }],
      target: { seconds: 14400, hours: '4.00' },
    });
    expect(model.referenceLines).toEqual([{ value: 4, label: 'target 4.00 h' }]);
  });

  it('links every ranked entity by its series id and tells same names apart (CH-29, R2b-b)', () => {
    const series = (entries: [string, string, string | undefined][]) => ({
      title: 'Used',
      series: entries.map(([key, label, id]) => ({
        key,
        label,
        color: '#000',
        ...(id ? { id } : {}),
      })),
      data: [
        {
          date: '2026-10-29',
          values: {
            ...Object.fromEntries(entries.map(([key], i) => [key, String((9 - i) * 100)])),
            // the day's whole, the route's (the D1 hand-back, item 2)
            total: String(entries.reduce((sum, _, i) => sum + (9 - i) * 100, 0)),
          },
        },
      ],
      // lane D1: five or fewer ranks, each series is its own band
      bands: entries.map(([key, label]) => ({ key, label, members: [key] })),
    });
    const scripts = series([
      ['top0', 'Default Lock', 'family:default-lock'],
      [
        'top1',
        'Type ID (type)',
        'ref:0x00000000000000000000000000000000000000000000000000545950455f4944:type',
      ],
      ['others', 'Others', undefined],
    ]);
    const s = chartModel(def('most-utilized-scripts'), {
      title: 'Scripts',
      usedShare: scripts,
      capacityShare: scripts,
    });
    expect(s.rankedRows?.map((row) => row.href)).toEqual([
      '/scripts/Default%20Lock',
      '/script/0x00000000000000000000000000000000000000000000000000545950455f4944',
      undefined,
    ]);
    const assets = series([
      [
        'top0',
        'RGB++ (token)',
        'token:0xb5fdb4e8ad52f5e3ed4a294c01ca99f7811b06baef2e7fce5b7d4ebbf006717d',
      ],
      [
        'top1',
        'MEMES (token)',
        'token:0x5cb3322a541873340aab6ea96396a728465cebd1cc5cc62b080f6215d76ab4fa',
      ],
      [
        'top2',
        'Nervape (object)',
        'dob:0xd5852c19fa4fa394d64915cafe026cdeb702ce53cf2b839c6ace501e8dead41c',
      ],
      [
        'top3',
        'MEMES (token)',
        'token:0x68c01bf9159fac7f328b11f88085d8f6c1096fcd63f9d0d63899ccfd98169b4b',
      ],
      [
        'top4',
        '.bit (identity)',
        'identity:0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f',
      ],
      ['top5', 'Santa (object)', 'object:0x8f67efedd50c61c9dd332defd4051f08a02d797700000014'],
    ]);
    const a = chartModel(def('most-utilized-assets'), {
      title: 'Assets',
      usedShare: assets,
      capacityShare: assets,
    });
    expect(a.rankedRows?.map((row) => [row.label, row.href])).toEqual([
      ['RGB++', '/tokens/0xb5fdb4e8ad52f5e3ed4a294c01ca99f7811b06baef2e7fce5b7d4ebbf006717d'],
      [
        'MEMES · 0x5cb3…b4fa',
        '/tokens/0x5cb3322a541873340aab6ea96396a728465cebd1cc5cc62b080f6215d76ab4fa',
      ],
      ['Nervape', '/clusters/0xd5852c19fa4fa394d64915cafe026cdeb702ce53cf2b839c6ace501e8dead41c'],
      [
        'MEMES · 0x68c0…9b4b',
        '/tokens/0x68c01bf9159fac7f328b11f88085d8f6c1096fcd63f9d0d63899ccfd98169b4b',
      ],
      ['.bit', '/identities/0x646f746269745f636f6c6c656374696f6e5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f'],
      ['Santa', '/classes/0x8f67efedd50c61c9dd332defd4051f08a02d797700000014'],
    ]);
    expect(a.rankedPreview?.rows[1].label).toBe('MEMES · 0x5cb3…b4fa');
    // The fact names the leader like the rows do, its class as the mark (detail shot: "RGB++ (token)").
    // Re-pinned for CH-18: the name is the fact's bright value, not part of its label.
    expect(a.facts?.[0]).toMatchObject({ label: 'Largest asset', name: 'RGB++' });
    const unknown = series([
      ['top0', 'X', 'nft:0x01'],
      ['top1', 'Y', 'family:y'],
    ]);
    expect(() =>
      chartModel(def('most-utilized-scripts'), {
        title: 'Scripts',
        usedShare: unknown,
        capacityShare: unknown,
      })
    ).toThrow(/unknown ranked series id nft:0x01/);
  });

  it('shortens a bucket label to its range where the subtitle names the unit', () => {
    expect(shortBucketLabel('100-1k CKB')).toBe('100-1k');
    expect(shortBucketLabel('>=1m CKB')).toBe('>=1m');
    expect(shortBucketLabel('4.02')).toBe('4.02');
  });
});

// CH-13 (Charts.dc.html "Deposited per day · CKB, last 60 days"): the card draws the last 60
// complete days exactly as the API gives them; the detail keeps the whole history.
describe('Deposited per day card window (CH-13)', () => {
  const days = Array.from({ length: 75 }, (_, i) => {
    const date = new Date(Date.UTC(2026, 7, 11 + i)).toISOString().slice(0, 10);
    return { date, value: String(1000 + i), value2: String(i) };
  });
  const payload = {
    title: 'Daily deposit',
    yAxisLabel: 'CKB',
    y2AxisLabel: 'deposits',
    data: days,
  };
  it('crops the card to the last 60 days and leaves the detail whole', () => {
    const definition = def('daily-deposit');
    // Re-pinned for lane D1 (rule 9): the caption written to stand with its date on one line
    expect(definition.cardSubtitle).toBe('deposited a day, last 60 d');
    const model = chartModel(definition, payload);
    expect(model.points).toHaveLength(75);
    expect(model.card?.points).toHaveLength(60);
    expect(model.card?.points[0].label).toBe(days[15].date);
    expect(model.card?.points.at(-1)).toEqual(model.points.at(-1));
  });
});

// Review of main..fix/readability (2026-11-18, test finding 8): filling a histogram's regular run
// (readability lane A) fails on what it cannot read, and nothing reached those throws. The grid is
// the API's: a label off it, a label that is not a decimal number and a second series (an empty
// bucket has no value for it) are broken reads, never guessed at. An axis reads the run the model
// filled and fails without it (logic finding 2).
/**
 * Epoch durations as the API labels them: the minute in hours at two decimals, cut (the
 * coordinator's ruling of 2026-11-19; it was half up, `(m / 60).toFixed(2)`: 181 minutes read 3.02,
 * it reads 3.01).
 */
const hours = (m: number) =>
  `${Math.floor(m / 60)}.${String(Math.floor(((m % 60) * 100) / 60)).padStart(2, '0')}`;

describe('a filled histogram fails on what it cannot read', () => {
  const histogram = (slug: string, data: { date: string; value: string; value2?: string }[]) =>
    chartModel(def(slug), { title: slug, yAxisLabel: 'Epochs', data });
  /** Epoch durations as the API labels them: the minute in hours, two decimals, cut. */
  const minutes = (from: number, to: number) =>
    Array.from({ length: to - from + 1 }, (_, i) => ({
      date: hours(from + i),
      value: '1',
    }));
  /** Block intervals: every 0.1 s from 0 to 5 s. */
  const tenths = () =>
    Array.from({ length: 51 }, (_, i) => ({ date: `${Math.floor(i / 10)}.${i % 10}`, value: '1' }));
  /** One label replaced, inside the regular run (an outlier beyond it is never filled). */
  const replaced = (data: { date: string; value: string }[], at: string, by: string) =>
    data.map((point) => (point.date === at ? { ...point, date: by } : point));

  it('fails on a bucket off its grid', () => {
    // 4.17 h is 250.2 minutes: the API writes minute 250 as 4.16, cut (re-pinned for the ruling:
    // half up it wrote 4.17, and 4.16 was the label off the grid)
    expect(() =>
      histogram('epoch-time-distribution', replaced(minutes(180, 300), '4.16', '4.17'))
    ).toThrow('epoch-time-distribution: bucket 4.17 is not one of 1/60 of its unit');
    expect(() => histogram('block-time-distribution', replaced(tenths(), '2.5', '2.55'))).toThrow(
      'block-time-distribution: bucket 2.55 is not one of 1/10 of its unit'
    );
  });

  it('fails on a bucket that is not a decimal number', () => {
    expect(() =>
      histogram('epoch-time-distribution', replaced(minutes(180, 300), '4.16', '4.16e0'))
    ).toThrow('epoch-time-distribution: bucket 4.16e0 is not a number');
  });

  it('fails on a histogram of more than one series', () => {
    expect(() =>
      histogram(
        'epoch-time-distribution',
        minutes(180, 300).map((point) => ({ ...point, value2: '2' }))
      )
    ).toThrow(
      'epoch-time-distribution: a filled histogram has one series; an empty bucket has no other value'
    );
  });

  it('keeps the run it filled, and an axis without that run fails', () => {
    // two outliers (2.05 h, 50.50 h) around a run from 4 to 5 h with minutes missing in it
    const model = histogram(
      'epoch-time-distribution',
      [123, 240, 241, 243, 250, 260, 270, 280, 290, 300, 3030].map((m) => ({
        date: hours(m),
        value: '2',
      }))
    );
    // every minute from 240 to 300 has its slot, a missing one its 0; the outliers keep one each
    expect(model.points).toHaveLength(63);
    expect(model.bucketRun).toEqual({ lo: 1, hi: 61 });
    expect([model.points[0].label, model.points[1].label, model.points[61].label]).toEqual([
      '2.05',
      '4.00',
      '5.00',
    ]);
    expect(model.points[3]).toEqual({ label: '4.03', values: { primary: '0' } });
    expect(model.points.at(-1)!.label).toBe('50.50');
    expect(() => axisTicks(model.points, 'bucket', 6, 'h')).toThrow(
      'bucket axis: a numeric histogram has no regular run from its model'
    );
    expect(() => axisTicks(model.points, 'bucket', 6, 'h', { lo: 1, hi: 63 })).toThrow(
      'bucket axis: run 1..63 is past the 63 points drawn'
    );
  });

  // Review of the fixes (2026-11-19, N1): the run indexes the model's points, while a tile hands
  // its axis a card's points where it has one; no histogram has a card, and a definition that
  // would give one a card window fails by its key instead of reading the run against other points.
  it('fails a histogram that its definition would give a card', () => {
    expect(() =>
      chartModel(
        { ...def('epoch-time-distribution'), cardDays: 60 },
        { title: 'epoch-time-distribution', yAxisLabel: 'Epochs', data: minutes(180, 300) }
      )
    ).toThrow(
      'epoch-time-distribution: a histogram has no card: its bucket run indexes the points it draws'
    );
  });
});
