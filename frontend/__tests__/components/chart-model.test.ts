import { describe, expect, it } from 'vitest';
import {
  CHART_BY_SLUG,
  CHARTS,
  INDEX_CHARTS,
  chartApiPath,
  type ChartDefinition,
  type ChartPayload,
} from '@/components/charts/chart-catalogue';
import {
  chartModel,
  chartUnitValue,
  formatChartExact,
  formatChartReadout,
} from '@/components/charts/chart-model';
import { chartHardforkMarkers, rangePoints } from '@/components/charts/chart-detail';
import { chartHead } from '@/components/charts/chart-head';
import supplyProducer from '@/__tests__/fixtures/chart-api-producer/total-supply.json';
import secondaryProducer from '@/__tests__/fixtures/chart-api-producer/secondary-issuance.json';

const simple = {
  title: 'Indexed',
  yAxisLabel: 'Value',
  data: [
    { date: '2024-01-01', value: '1', value2: '2' },
    { date: '2024-01-03', value: '3', value2: '4' },
  ],
};
const stacked = {
  title: 'Indexed',
  series: [
    { key: 'liveCells', label: 'Live Cells', color: '#ffb900' },
    { key: 'deadCells', label: 'Dead Cells', color: '#ffb900' },
  ],
  data: [
    { date: '2024-01-01', values: { liveCells: '1', deadCells: '2' } },
    { date: '2024-01-03', values: { liveCells: '3', deadCells: '4' } },
  ],
};
const utilized = {
  title: 'Used',
  usedShare: {
    title: 'Used',
    series: [
      { key: 'top0', label: 'Alpha', color: '#a' },
      { key: 'others', label: 'Others', color: '#b' },
    ],
    data: [
      {
        date: '2024-01-03',
        values: {
          top0: '900719925474099300',
          others: '900719925474099300',
          // the day's whole, the route's (the D1 hand-back, item 2)
          total: '1801439850948198600',
          // the ranks drawn as their own bands, their total, the route's (lane D1's third round, M2)
          drawnTotal: '900719925474099300',
        },
      },
    ],
    // lane D1: the API names the bands a history is drawn in (re-pinned: it sends them always)
    bands: [
      { key: 'top0', label: 'Alpha', members: ['top0'] },
      { key: 'others', label: 'Others', members: ['others'] },
    ],
  },
  capacityShare: {
    title: 'Capacity',
    series: [{ key: 'top0', label: 'Alpha', color: '#a' }],
    data: [{ date: '2024-01-03', values: { top0: '1', others: '0', total: '1', drawnTotal: '1' } }],
    bands: [{ key: 'top0', label: 'Alpha', members: ['top0'] }],
  },
};
const miners = {
  title: 'Miners',
  windowDays: 7,
  totalBlocks: 100,
  // the route's: the blocks past the five largest (lane exactness, part A, A1)
  restBlocks: 50,
  fromDate: '2024-01-01',
  toDate: '2024-01-07',
  data: [
    {
      minerLockHash: `0x${'a'.repeat(64)}`,
      address: null,
      minerName: 'Pool',
      blocksMined: 50,
      percentage: '50',
    },
  ],
};

// F2 charts: classed and grouped charts read the API's own series keys (fail fast on others).
const keyed = (keys: string[], extra: Record<string, string> = {}) => ({
  title: 'Indexed',
  series: keys.map((key) => ({ key, label: key, color: '#000' })),
  data: ['2024-01-01', '2024-01-03'].map((date, d) => ({
    date,
    values: { ...Object.fromEntries(keys.map((key, i) => [key, String(i + d + 1)])), ...extra },
  })),
});
/**
 * The route's whole at each point, as a split stack's route sends it (the D1 hand-back, item 2):
 * its parts' total and `more` (total supply's C is more than its bands).
 */
const withWhole = (payload: ReturnType<typeof keyed>, field: string, more = 0) => ({
  ...payload,
  data: payload.data.map((point) => ({
    ...point,
    values: {
      ...point.values,
      [field]: String(Object.values(point.values).reduce((sum, v) => sum + Number(v), more)),
    },
  })),
});
const HODL_KEYS = ['gt3y', '1y3y', '6m1y', '3m6m', '1m3m', '1w1m', '1d1w', '24h'];
/**
 * Lane D1: the HODL wave as the API sends it now — its five drawn bands named, the two it merges
 * (1 to 6 months, under 1 month) in every point beside the eight.
 */
const HODL_BANDS = [
  { key: 'gt3y', label: '3 years or more', members: ['gt3y'] },
  { key: '1y3y', label: '1 to 3 years', members: ['1y3y'] },
  { key: '6m1y', label: '6 to 12 months', members: ['6m1y'] },
  { key: '1m6m', label: '1 to 6 months', members: ['3m6m', '1m3m'] },
  { key: 'lt1m', label: 'under 1 month', members: ['1w1m', '1d1w', '24h'] },
];
const banded = <T extends { data: { values: Record<string, string> }[] }>(
  payload: T,
  merged: { '1m6m': string; lt1m: string }
) => ({
  ...payload,
  bands: HODL_BANDS,
  data: payload.data.map((point) => ({ ...point, values: { ...point.values, ...merged } })),
});

function payloadFor(definition: ChartDefinition): ChartPayload {
  if (definition.slug === 'secondary-issuance') return secondaryProducer;
  if (definition.slug === 'hodl-wave')
    return banded(keyed(HODL_KEYS, { holderCount: '123' }), { '1m6m': '9', lt1m: '21' });
  // re-pinned for the four bands (lane D1, second follow-up, item 1): four bands that make C,
  // and the protocol circulation beside them; re-keyed for lane exactness, part A2 (M1)
  if (definition.slug === 'total-supply') {
    const bands = withWhole(
      keyed(['circulatingNotDeposited', 'daoDeposited', 'unclaimedCompensation', 'burnt']),
      'totalIssuance'
    );
    return {
      ...bands,
      data: bands.data.map((point) => ({
        ...point,
        values: {
          ...point.values,
          circulating: String(
            Number(point.values.circulatingNotDeposited) + Number(point.values.daoDeposited)
          ),
        },
      })),
    };
  }
  if (definition.slug === 'common-knowledge-composition')
    return withWhole(
      keyed(['transfer', 'dao', 'udt', 'nftSpore', 'otherContracts']),
      'knowledgeSize'
    );
  if (definition.slug === 'activity-type-breakdown')
    return withWhole(
      keyed(['transfer', 'dao', 'token', 'object', 'identity', 'scriptCall', 'protocol']),
      'activityCount'
    );
  if (definition.slug === 'epoch-time-length')
    return {
      ...simple,
      data: simple.data.map((point, i) => ({ ...point, date: String(15201 + i) })),
    };
  if (definition.slug === 'most-utilized-scripts' || definition.slug === 'most-utilized-assets')
    return utilized;
  if (definition.slug === 'miner-address-distribution') return miners;
  // The API's own size buckets: an unknown bucket fails (re-pinned for CH-12).
  if (definition.slug === 'cell-size-distribution')
    return {
      ...simple,
      data: simple.data.map((point, i) => ({ ...point, date: ['<100 CKB', '>=1m CKB'][i] })),
      // the route's wholes (the D1 hand-back, item 2)
      liveCells: '4',
      occupiedCapacity: '6',
    };
  if (
    [
      'cell-count',
      'common-knowledge-composition',
      'hodl-wave',
      'activity-type-breakdown',
      'total-supply',
      'secondary-issuance',
    ].includes(definition.slug)
  )
    return stacked;
  // A second value belongs only to a chart that names it (a second unit, or two named series of
  // one); any other chart takes the value alone (re-pinned for CH-21: an unnamed companion fails,
  // where it was drawn as "Companion").
  const single = { ...simple, data: simple.data.map(({ date, value }) => ({ date, value })) };
  // A numeric histogram's labels are buckets of its grid (0.1 s; a minute in hours); a date is
  // none of them and fails (re-pinned for the readability review 2026-11-18: the model fills a
  // histogram's regular run on its grid, so it reads each label, where only the axis did).
  // the window the route counts, its epochs and intervals (lane D1's third round, S4)
  if (definition.slug === 'block-time-distribution')
    return {
      ...single,
      data: single.data.map((p, i) => ({ ...p, date: ['7.8', '7.9'][i] })),
      epochs: 42,
      intervals: 100,
      beyondRange: 0,
      // each bucket's count, the route's (lane exactness, part A2, N5)
      bucketCounts: [1, 3],
    };
  if (definition.slug === 'epoch-time-distribution')
    return { ...single, data: single.data.map((p, i) => ({ ...p, date: ['4.00', '4.02'][i] })) };
  // each day's operands, the route's (lane exactness, part A, A2)
  if (definition.slug === 'uncle-rate')
    return {
      ...single,
      orphanRateTarget: { numerator: 1, denominator: 40 },
      counts: single.data.map((point) => ({ date: point.date, uncles: 1, blocks: 10 })),
    };
  return definition.secondary || definition.seriesNames ? simple : single;
}

describe('29 chart variants', () => {
  // Re-pinned for CH-22 (UI review 2026-11-14): the index lists every chart, circulation ratio too.
  it('keeps every existing route and a preview for each of the 29', () => {
    expect(CHARTS).toHaveLength(29);
    expect(INDEX_CHARTS).toHaveLength(29);
    expect(INDEX_CHARTS.map((chart) => chart.slug)).toContain('circulation-ratio');
    expect(new Set(CHARTS.map((chart) => chart.slug)).size).toBe(29);
    for (const chart of CHARTS)
      expect(chart.related.every((slug) => CHART_BY_SLUG.has(slug))).toBe(true);
  });

  it.each(INDEX_CHARTS.map((definition) => [definition.slug, definition] as const))(
    '%s builds a distinct visual and exact latest value',
    (_slug, definition) => {
      const payload = payloadFor(definition);
      const model = chartModel(definition, payload);
      expect(model.kind).toBe(definition.visual === 'ranked' ? 'ranked' : definition.visual);
      expect(model.latest).not.toBeNull();
      expect(model.unit).toBe(definition.unit);
      if (model.kind === 'ranked') expect(model.rankedRows?.length).toBeGreaterThan(0);
      else if ('data' in payload) expect(model.points).toHaveLength(payload.data.length);
    }
  );

  it('scales wide chain integers without passing exact display through Number', () => {
    expect(chartUnitValue('900719925474099300', 'shannons', 'wide CKB')).toBe('9007199254.740993');
    expect(chartUnitValue('900719925474099300000', 'hash-rate', 'wide H/s')).toBe(
      '900719.9254740993'
    );
    expect(chartUnitValue('0.025', 'fraction-percent', 'uncle fraction')).toBe('2.5');
  });

  it('rounds card readouts the way the design shows them, never faking an exact value (CH-02)', () => {
    expect(formatChartReadout('600', 'addresses')).toBe('600');
    expect(formatChartReadout('622', 'epochs')).toBe('622');
    // A whole count under 100,000 reads exactly, grouped, as charts.json does ("16,826"; F2 charts).
    expect(formatChartReadout('1234', 'transactions')).toBe('1,234');
    // re-pinned for the coordinator's ruling of 2026-11-19, a shortened figure is cut (was half up, 123.46k)
    expect(formatChartReadout('123456', 'transactions')).toBe('123.45k');
    // A CKB amount is cut, never rounded (readability R5, re-pinned on purpose: was 9.01B).
    expect(formatChartReadout('9007199254.740993', 'CKB')).toBe('9.00B');
    // A negative takes the minus sign (re-pinned for CH-21: was the hyphen).
    expect(formatChartReadout('-1234.5', 'CKB')).toBe('−1.23k');
    // Rates and durations at two decimals, as charts.json shows them (91.34 PH/s, 1.34%).
    expect(formatChartReadout('12.5', '%')).toBe('12.50');
    // re-pinned for the D1 hand-back, item 1: a rate is cut, never rounded (was 93.69)
    expect(formatChartReadout('93.685105781336746', 'PH/s')).toBe('93.68');
    expect(formatChartReadout('815.691763309692958', 'P')).toBe('815.69');
    expect(formatChartReadout('+1.114235248953059', 'PH/s')).toBe('+1.11');
    expect(formatChartReadout('+208801027.17121727', 'CKB')).toBe('+208.80M');
    // A non-whole amount keeps its two decimals: "99" would read as an exact whole value.
    expect(formatChartReadout('99.00000001', 'CKB')).toBe('99.00');
    expect(formatChartReadout('600.5', 'addresses')).toBe('600.50');
  });

  it('previews the top five ranks plus an exact Others for every remaining rank (CH-01)', () => {
    // 30.000 / 10.005 ×4 / 29.980: rounded shares would leave 29.96 for Others.
    const amounts = ['30000', '10005', '10005', '10005', '10005', '9994', '9993', '9993'];
    const keys = amounts.map((_, i) => `s${i}`);
    const share = {
      title: 'Used',
      series: keys.map((key) => ({ key, label: key, color: '#a' })),
      data: [
        {
          date: '2024-01-03',
          values: {
            ...Object.fromEntries(keys.map((key, i) => [key, `${amounts[i]}000`])),
            // lane D1: the API's rest band, ranks six to eight (9,994 + 9,993 + 9,993)
            rest: '29980000',
            // the five drawn, the route's (lane D1's third round, M2): 30,000 + 4 × 10,005
            drawnTotal: '70020000',
            // the day's whole, the route's (the D1 hand-back, item 2)
            total: '100000000',
          },
        },
      ],
      bands: [
        ...keys.slice(0, 5).map((key) => ({ key, label: key, members: [key] })),
        { key: 'rest', label: 'Others', members: keys.slice(5) },
      ],
    };
    const model = chartModel(CHART_BY_SLUG.get('most-utilized-scripts')!, {
      title: 'Used',
      usedShare: share,
      capacityShare: share,
    });
    // re-pinned for the D1 hand-back, item 1: 10.005% is cut, 10.00% (was half up, 10.01%)
    expect(model.rankedPreview?.rows.map((row) => `${row.label} ${row.share}`)).toEqual([
      's0 30.00%',
      's1 10.00%',
      's2 10.00%',
      's3 10.00%',
      's4 10.00%',
      'Others 29.98%',
    ]);
    // The API's own remainder bucket is ranks beyond its named list, not the card's Others.
    const withBucket = chartModel(CHART_BY_SLUG.get('most-utilized-scripts')!, utilized);
    expect(withBucket.rankedRows?.map((row) => row.label)).toEqual([
      'Alpha',
      'Others beyond the top 1',
    ]);
    expect(withBucket.rankedPreview?.rows.map((row) => `${row.label} ${row.share}`)).toEqual([
      'Alpha 50.00%',
      'Others 50.00%',
    ]);
  });

  it('never fabricates an Others share when nothing remains or the total is zero (CH-01)', () => {
    const zero = {
      title: 'Used',
      series: [{ key: 'a', label: 'A', color: '#a' }],
      data: [{ date: '2024-01-03', values: { a: '0', others: '0', total: '0', drawnTotal: '0' } }],
      bands: [{ key: 'a', label: 'A', members: ['a'] }],
    };
    const model = chartModel(CHART_BY_SLUG.get('most-utilized-scripts')!, {
      title: 'Used',
      usedShare: zero,
      capacityShare: zero,
    });
    expect(model.rankedPreview?.rows).toEqual([{ label: 'A', title: 'A', share: '—', width: 0 }]);
    const single = chartModel(CHART_BY_SLUG.get('miner-address-distribution')!, {
      ...miners,
      totalBlocks: 50,
      restBlocks: 0,
    });
    expect(single.rankedPreview?.rows.map((row) => row.label)).toEqual(['Pool']);
  });

  it('adds the exact miner remainder and keeps miner identities distinguishable (CH-01, CH-04)', () => {
    const prefix = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq';
    const rows = ['93hhsj', 'p909qk', 'aaaaaa', 'bbbbbb', 'cccccc', 'dddddd'].map((tail, i) => ({
      minerLockHash: `0x${String(i).repeat(64)}`,
      address: `${prefix}${'x'.repeat(30)}${tail}`,
      minerName: null,
      blocksMined: [41743, 20000, 2000, 1000, 611, 1123][i],
      percentage: '0',
    }));
    const model = chartModel(CHART_BY_SLUG.get('miner-address-distribution')!, {
      ...miners,
      totalBlocks: 66477,
      // the sixth miner's 1,123 blocks, the route's
      restBlocks: 1123,
      data: rows,
    });
    const preview = model.rankedPreview!;
    // R9 (re-pinned): `ckb1…` and the last 8 characters, two of the fixture's x and its tail
    expect(preview.rows.map((row) => row.label)).toEqual([
      'ckb1…xx93hhsj',
      'ckb1…xxp909qk',
      'ckb1…xxaaaaaa',
      'ckb1…xxbbbbbb',
      'ckb1…xxcccccc',
      'Others',
    ]);
    // 1,123 of 66,477 blocks at two decimals, the one share form (re-pinned for CH-03 / CH-14:
    // was floored at the API's four decimals, 1.6893%), cut (re-pinned for the D1 hand-back,
    // item 1: was half up, 1.69%).
    expect(preview.rows.at(-1)?.share).toBe('1.68%');
    expect(preview.rows[0].title).toBe(rows[0].address);
    // The detail names the miner in the same short form as a fact and as its ranked row; the full
    // address is the row's link title (re-pinned for CH-03: the row printed all 97 characters).
    expect(model.facts?.[0]).toEqual({
      label: 'Leading miner',
      name: 'ckb1…xx93hhsj',
      // the review's item I (re-pinned): the share is the miner's blocks in the window's, 41,743 of
      // 66,477 (the fixture's API percentage, '0', was a placeholder the model read before)
      // the review's item C (re-pinned): the count through formatCount, a no-break space
      value: '41,743\u00a0blocks · 62.79%',
    });
    expect(model.rankedRows?.[0]).toMatchObject({
      label: 'ckb1…xx93hhsj',
      title: rows[0].address,
    });
  });

  it('summarises nominal APC at its single current sample and rejects a missing one (CH-03)', () => {
    const definition = CHART_BY_SLUG.get('nominal-apc')!;
    const timeline = (markers: string[]) => ({
      title: 'Nominal APC',
      yAxisLabel: 'Nominal APC (%)',
      y2AxisLabel: 'nominal-apc-timeline',
      data: markers.map((marker, i) => ({
        date: String(i * 3.25),
        value: ['4.2', '3.1', '2.3297', '1.9', '1.5881'][i],
        value2: marker,
      })),
    });
    const model = chartModel(definition, timeline(['past', 'past', 'current', 'future', 'future']));
    // re-pinned for the D1 hand-back, item 1: a rate is cut (was half up, 2.33)
    expect(model.latest).toEqual({ label: 'year 6.5', raw: '2.3297', display: '2.32' });
    // Re-pinned for CH-18: the head reads today; no "Today" fact repeats it.
    expect(model.facts).toBeUndefined();
    expect(model.projectedAfter).toBe(2);
    expect(model.series).toHaveLength(1);
    expect(() =>
      chartModel(definition, timeline(['past', 'past', 'past', 'past', 'past']))
    ).toThrow('no current sample');
    expect(() =>
      chartModel(definition, timeline(['past', 'current', 'current', 'future', 'future']))
    ).toThrow('second current sample');
    expect(() =>
      chartModel(definition, timeline(['past', 'future', 'current', 'future', 'future']))
    ).toThrow('out of timeline order');
  });

  it("takes nominal APC's main reading from the API's today point, never the last sample (CH-03)", () => {
    const definition = CHART_BY_SLUG.get('nominal-apc')!;
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
    expect(
      chartModel(definition, {
        ...payload,
        today: { blockNumber: 20876568, date: '2026-10-30', year: '6.83', value: '2.3601' },
      }).today
    ).toEqual({ label: 'year 6.83', values: { primary: '2.3601' } });
    // Lane R8 has not shipped `today` yet: the gap stays visible (no reading), not the year-20 sample.
    expect(chartModel(definition, payload).today).toBeNull();
    // Other charts carry no model-timeline reading at all.
    expect(
      chartModel(CHART_BY_SLUG.get('total-deposit')!, {
        title: 'Total Deposit',
        yAxisLabel: 'CKB',
        data: [{ date: '2024-01-01', value: '1' }],
      }).today
    ).toBeUndefined();
  });

  it('prints exact chart values with every digit and thousands groups', () => {
    expect(formatChartExact('65053585849.85425816', 'CKB')).toBe('65,053,585,849.85425816');
    expect(formatChartExact('93.685105781336746', 'PH/s')).toBe('93.685105781336746');
    expect(formatChartExact('+208801027.17121727', 'CKB')).toBe('+208,801,027.17121727');
  });

  it('keeps physical units separate and does not fill sparse dates', () => {
    const definition = CHART_BY_SLUG.get('total-deposit')!;
    const model = chartModel(definition, simple);
    expect(model.secondary?.unit).toBe('depositors');
    expect(model.secondary?.points[1].values.secondary).toBe('4');
    expect(model.points.map((point) => point.label)).toEqual(['2024-01-01', '2024-01-03']);
  });

  it('reports incomplete dual-unit points instead of silently using zero', () => {
    const definition = CHART_BY_SLUG.get('total-deposit')!;
    expect(() =>
      chartModel(definition, {
        ...simple,
        data: [simple.data[0], { date: '2024-01-03', value: '3' }],
      })
    ).toThrow(/secondary series is missing/);
  });

  it('filters date ranges from the latest returned day without interpolating gaps', () => {
    const points = [
      { label: '2024-01-01', values: { primary: '1' } },
      { label: '2024-01-03', values: { primary: '3' } },
      { label: '2024-02-10', values: { primary: '10' } },
    ];
    expect(rangePoints(points, '30d').map((point) => point.label)).toEqual(['2024-02-10']);
    expect(rangePoints(points, 'all')).toHaveLength(3);
  });

  it('maps epoch hardfork markers numerically, including four-digit boundaries', () => {
    const chart = CHART_BY_SLUG.get('epoch-time-length')!;
    const points = [
      { label: '999', values: { primary: '4' } },
      { label: '1000', values: { primary: '4.1' } },
    ];
    const event = {
      id: 'fork',
      name: 'Fork',
      shortName: 'F',
      editionYear: 2024,
      activationEpoch: 1000,
      activationDate: '2024-01-01',
      activationBlock: 100,
      status: 'activated' as const,
      summary: '',
      resources: [],
    };
    expect(chartHardforkMarkers(chart, points, [event])).toEqual([
      // Named in the plot like the board, not a bare ◆ (CH-23).
      { index: 1, label: 'F', plotLabel: 'F', href: '/hardforks' },
    ]);
  });

  // Re-pinned for the D1 hand-back, item 2: a stack's whole is the route's, exactly, never the
  // page's sum of its bands (this test summed 1.123456789 and 2.000000001 to 3.12345679); and
  // re-pinned for the four bands (lane D1, second follow-up, item 1): Supply heads with the
  // route's circulating, its whole C
  it('takes a stack’s whole from its route at every digit, never the sum of its bands', () => {
    const model = chartModel(CHART_BY_SLUG.get('total-supply')!, {
      title: 'Supply',
      series: [
        { key: 'circulatingNotDeposited', label: 'Circulating, not deposited', color: '#a' },
        { key: 'daoDeposited', label: 'Deposited in the Nervos DAO', color: '#b' },
      ],
      data: [
        {
          date: '2024-01-01',
          values: {
            circulatingNotDeposited: '1.123456789',
            daoDeposited: '2.000000001',
            totalIssuance: '10.123456790000000001',
            circulating: '3.12345679',
          },
        },
      ],
    });
    expect(model.latest?.raw).toBe('3.12345679');
    expect(model.points[0].whole).toBe('10.123456790000000001');
  });

  it('summarizes live cells, the largest HODL band, and bucket distributions semantically', () => {
    const cells = chartModel(CHART_BY_SLUG.get('cell-count')!, stacked);
    expect(cells.latest?.raw).toBe('3');
    // Re-pinned for CH-18: no "Live cells" fact repeats the head figure.
    expect(cells.facts).toBeUndefined();
    const hodl = chartModel(
      CHART_BY_SLUG.get('hodl-wave')!,
      banded(
        {
          title: 'HODL',
          series: HODL_KEYS.map((key) => ({ key, label: key, color: '#a' })),
          data: [
            {
              date: '2024-01-01',
              values: {
                ...Object.fromEntries(HODL_KEYS.map((key) => [key, '0'])),
                gt3y: '35',
                '24h': '65',
                holderCount: '9007199254740993',
              },
            },
          ],
        },
        { '1m6m': '0', lt1m: '65' }
      )
    );
    // Re-pinned for lane D1 (the chart-hero board, rule 3): the figure is the answer the chart is
    // asked, the CKB unmoved for 3 years or more (was the largest band, 65 under 1 day).
    expect(hodl.latest?.raw).toBe('35');
    expect(hodl.secondary?.points[0].values.holders).toBe('9007199254740993');
    expect(() =>
      chartModel(
        CHART_BY_SLUG.get('hodl-wave')!,
        banded(keyed(HODL_KEYS), { '1m6m': '9', lt1m: '21' }) as ChartPayload
      )
    ).toThrow(/missing holderCount/);
    const buckets = chartModel(CHART_BY_SLUG.get('cell-size-distribution')!, {
      title: 'Buckets',
      yAxisLabel: 'Cells',
      data: [
        { date: '<100 CKB', value: '90', value2: '1' },
        { date: '>=1m CKB', value: '1', value2: '10' },
      ],
      liveCells: '91',
      occupiedCapacity: '11',
    });
    // In the axis' words (re-pinned for CH-12: was the API's '<100 CKB').
    expect(buckets.latest?.label).toBe('< 100 CKB');
  });

  it('retains both exact script histories and the separate consensus target', () => {
    const script = chartModel(CHART_BY_SLUG.get('most-utilized-scripts')!, utilized);
    expect(script.historicalShares).toHaveLength(2);
    expect(script.historicalShares?.[0].points[0].values.top0).toBe('9007199254.740993');
    expect(script.historicalShares?.[1].points[0].values.top0).toBe('0.00000001');
    expect(script.latest?.display).not.toContain('Alpha');
    // Re-pinned for CH-18: the leader is the named fact (was the label "Largest script: Alpha").
    expect(script.facts?.[0]).toMatchObject({ label: 'Largest script', name: 'Alpha' });
    const miner = chartModel(CHART_BY_SLUG.get('miner-address-distribution')!, miners);
    expect(miner.latest?.display).toBe('50');
    // Re-pinned for CH-03: the leader is a named fact at the one share form.
    expect(miner.facts?.[0].value).toContain('50.00%');
    // One value per day, as the API sends it (re-pinned for CH-21: an unnamed companion fails).
    const uncle = chartModel(CHART_BY_SLUG.get('uncle-rate')!, {
      ...simple,
      data: simple.data.map(({ date, value }) => ({ date, value })),
      orphanRateTarget: { numerator: 1, denominator: 40 },
      // each day's operands, the route's (lane exactness, part A, A2)
      counts: simple.data.map(({ date }) => ({ date, uncles: 1, blocks: 10 })),
    });
    expect(uncle.facts).toEqual([{ label: 'Consensus target', value: '1 / 40 blocks' }]);
  });

  it('links to the actual source endpoint for client-derived activity charts', () => {
    expect(chartApiPath(CHART_BY_SLUG.get('activity-volume')!)).toBe(
      '/stats/daily-activities?days=0'
    );
    expect(chartApiPath(CHART_BY_SLUG.get('total-deposit')!)).toBe('/dao/charts/total-deposit');
    expect(chartApiPath(CHART_BY_SLUG.get('hash-rate')!)).toBe('/charts/hash-rate');
  });

  it('consumes exact Rust supply and secondary-issuance producer JSON', () => {
    const supply = chartModel(CHART_BY_SLUG.get('total-supply')!, supplyProducer);
    const secondary = chartModel(CHART_BY_SLUG.get('secondary-issuance')!, secondaryProducer);
    expect(supply.points).toHaveLength(13);
    expect(supply.points.map((point) => point.label).at(0)).toBe('2020-05-12');
    expect(supply.points.map((point) => point.label).at(-1)).toBe('2020-05-24');
    // re-pinned for the four bands (lane D1, second follow-up, item 1): the DAO's deposits keyed
    // daoDeposited (were nervosdao), and the head the route's circulating, 1,000 CKB (was C,
    // 8,400,001,000)
    expect(supply.points[0].values.daoDeposited).toBe('123.00000001');
    expect(supply.points[1].values).toEqual(supply.points[0].values);
    expect(supply.points[0].whole).toBe('8400001000');
    expect(supply.latest?.raw).toBe('1000');
    expect(secondary.points).toHaveLength(13);
    expect(secondary.points[1].values).toEqual(secondary.points[0].values);
    expect(secondary.points[0].values.mining).toBe('1.50000001');
  });

  // Re-pinned for lane D1's third round (S1): the protocol total is the one whole, and its destinations make it
  // (this test sent a protocol total of 4.00000001 CKB beside destinations of 5, the "overlap" the
  // route's partition now refuses); the frozen phase-1 compensation is a part of the unclaimed
  // compensation, a developer fact, never a difference between two wholes (was "Frozen phase-1
  // overlap").
  it('heads secondary issuance with the protocol total its destinations make', () => {
    const model = chartModel(CHART_BY_SLUG.get('secondary-issuance')!, {
      title: 'Secondary issuance',
      series: [
        { key: 'compensation', label: 'Compensation', color: 'var(--c-jade)' },
        { key: 'burnt', label: 'Treasury', color: 'var(--c-aqua)' },
      ],
      data: [
        {
          date: '2024-01-01',
          values: { compensation: '2', burnt: '3.00000001' },
          protocolTotalShannons: '500000001',
          phase1CompensationShannons: '100000000',
        },
      ],
    });
    expect(model.latest?.raw).toBe('5.00000001');
    // Re-pinned for CH-18: a developer fact, in the evidence tier.
    expect(model.evidence).toContainEqual({
      label: 'Frozen phase-1 compensation',
      value: '1 CKB',
    });
    expect(model.points[0].values).toEqual({ compensation: '2', burnt: '3.00000001' });
  });

  it('preserves signed exact knowledge flow above the safe integer range', () => {
    const model = chartModel(CHART_BY_SLUG.get('knowledge-size')!, {
      title: 'Common Knowledge Size',
      yAxisLabel: 'CKB',
      y2AxisLabel: 'Utilization (%)',
      data: [
        { date: '2026-02-18', value: '9007199254740993.00000003', value2: '10' },
        { date: '2026-02-19', value: '9007199254740993.00000001', value2: '10' },
      ],
    });
    expect(model.dailyFlow?.points).toEqual([
      { label: '2026-02-19', values: { flow: '-0.00000002' } },
    ]);
    expect(model.secondary?.title).toBe('Utilization');
  });

  // The review's item I: a miner's share is its blocks in the window's blocks through the one
  // percentage (it was the API's floored four decimals rounded to two: 1 block of 1,000,000 read
  // 0.00%, 999,999 read 100.00%)
  it('reads each miner’s share from its blocks through the one percentage', () => {
    const at = (blocksMined: number[]) =>
      chartModel(CHART_BY_SLUG.get('miner-address-distribution')!, {
        title: 'Miners',
        totalBlocks: 1_000_000,
        restBlocks: 0,
        windowDays: 7,
        fromDate: '2026-07-23',
        toDate: '2026-07-29',
        data: blocksMined.map((n, i) => ({
          minerLockHash: `0x${String(i).repeat(64)}`,
          address: null,
          minerName: `Pool ${i}`,
          blocksMined: n,
          percentage: ((n / 1_000_000) * 100).toFixed(4),
        })),
      });
    const model = at([999_999, 1]);
    // re-pinned for the D1 hand-back, item 1: cut, 99.9999% reads 99.99% and is true (half up it
    // read 100.00%, bounded as '>99.99%'); a share that cuts to nothing stays under a hundredth
    expect(model.rankedRows?.map((row) => row.share)).toEqual(['99.99%', '<0.01%']);
    // Re-pinned for lane D1 (rule 9, CH-07): the card heads with the page's own figure, the
    // leading miner's blocks (was its share in a card headline of its own); the share stays here.
    expect(chartHead(CHART_BY_SLUG.get('miner-address-distribution')!, model).figure).toMatchObject(
      { display: '999,999', unit: 'blocks' }
    );
    // the leading miner's fact ends on the same share (its count's form is the review's item C)
    expect(model.facts?.[0].value).toMatch(/ · 99\.99%$/);
    expect(at([600_000, 400_000]).rankedRows?.map((row) => row.share)).toEqual([
      '60.00%',
      '40.00%',
    ]);
  });

  // The review's item C: the leading miner's blocks go through the one plural rule; a leader of
  // one block read "1 blocks"
  it('names a leading miner of one block in the singular', () => {
    const model = chartModel(CHART_BY_SLUG.get('miner-address-distribution')!, {
      title: 'Miners',
      totalBlocks: 2,
      restBlocks: 0,
      windowDays: 7,
      fromDate: '2026-07-23',
      toDate: '2026-07-29',
      data: [0, 1].map((i) => ({
        minerLockHash: `0x${String(i).repeat(64)}`,
        address: null,
        minerName: `Pool ${i}`,
        blocksMined: 1,
        percentage: '50.0000',
      })),
    });
    expect(model.facts?.[0]).toMatchObject({
      label: 'Leading miner',
      value: '1\u00a0block · 50.00%',
    });
  });

  it('retains miner address and unresolved lock-hash identities in ranking links', () => {
    const model = chartModel(CHART_BY_SLUG.get('miner-address-distribution')!, {
      title: 'Miners',
      totalBlocks: 10,
      restBlocks: 0,
      windowDays: 7,
      fromDate: '2026-07-23',
      toDate: '2026-07-29',
      data: [
        {
          minerLockHash: `0x${'a'.repeat(64)}`,
          address: 'ckb1resolved',
          minerName: 'Resolved Pool',
          blocksMined: 6,
          percentage: '60.0000',
        },
        {
          minerLockHash: `0x${'b'.repeat(64)}`,
          address: null,
          minerName: null,
          blocksMined: 4,
          percentage: '40.0000',
        },
      ],
    });
    // Re-pinned for CH-03: no bracketed kind word; a lock-hash miner is marked for the one note.
    expect(model.rankedRows?.[0]).toMatchObject({
      href: '/address/ckb1resolved',
      title: 'ckb1resolved',
    });
    expect(model.rankedRows?.[0].lockHashOnly).toBeUndefined();
    expect(model.rankedRows?.[1]).toMatchObject({
      href: `/address/${'b'.repeat(64)}`,
      label: `0x${'b'.repeat(8)}…${'b'.repeat(6)}`,
      lockHashOnly: true,
    });
    expect(model.facts).toContainEqual({
      label: 'Window',
      value: '2026-07-23 → 2026-07-29 · 10\u00a0blocks',
    });
  });
});
