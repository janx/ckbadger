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
      { date: '2024-01-03', values: { top0: '900719925474099300', others: '900719925474099300' } },
    ],
  },
  capacityShare: {
    title: 'Capacity',
    series: [{ key: 'top0', label: 'Alpha', color: '#a' }],
    data: [{ date: '2024-01-03', values: { top0: '1' } }],
  },
};
const miners = {
  title: 'Miners',
  windowDays: 7,
  totalBlocks: 100,
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

function payloadFor(definition: ChartDefinition): ChartPayload {
  if (definition.slug === 'secondary-issuance') return secondaryProducer;
  if (definition.slug === 'hodl-wave')
    return {
      ...stacked,
      data: stacked.data.map((point) => ({
        ...point,
        values: { ...point.values, holderCount: '123' },
      })),
    };
  if (definition.slug === 'most-utilized-scripts' || definition.slug === 'most-utilized-assets')
    return utilized;
  if (definition.slug === 'miner-address-distribution') return miners;
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
  if (definition.slug === 'uncle-rate')
    return { ...simple, orphanRateTarget: { numerator: 1, denominator: 40 } };
  return simple;
}

describe('28 chart variants', () => {
  it('keeps every existing route and exactly 28 approved previews', () => {
    expect(CHARTS).toHaveLength(29);
    expect(INDEX_CHARTS).toHaveLength(28);
    expect(CHART_BY_SLUG.get('circulation-ratio')?.supplemental).toBe(true);
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
    expect(formatChartReadout('1234', 'transactions')).toBe('1.23K');
    expect(formatChartReadout('9007199254.740993', 'CKB')).toBe('9.01B');
    expect(formatChartReadout('-1234.5', 'CKB')).toBe('-1.23K');
    // Rates and durations at two decimals, as charts.json shows them (91.34 PH/s, 1.34%).
    expect(formatChartReadout('12.5', '%')).toBe('12.50');
    expect(formatChartReadout('93.685105781336746', 'PH/s')).toBe('93.69');
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
          values: Object.fromEntries(keys.map((key, i) => [key, `${amounts[i]}000`])),
        },
      ],
    };
    const model = chartModel(CHART_BY_SLUG.get('most-utilized-scripts')!, {
      title: 'Used',
      usedShare: share,
      capacityShare: share,
    });
    expect(model.rankedPreview?.rows.map((row) => `${row.label} ${row.share}`)).toEqual([
      's0 30.00%',
      's1 10.01%',
      's2 10.01%',
      's3 10.01%',
      's4 10.01%',
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
      data: [{ date: '2024-01-03', values: { a: '0' } }],
    };
    const model = chartModel(CHART_BY_SLUG.get('most-utilized-scripts')!, {
      title: 'Used',
      usedShare: zero,
      capacityShare: zero,
    });
    expect(model.rankedPreview?.rows).toEqual([{ label: 'A', title: 'A', share: '—' }]);
    const single = chartModel(CHART_BY_SLUG.get('miner-address-distribution')!, {
      ...miners,
      totalBlocks: 50,
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
      data: rows,
    });
    const preview = model.rankedPreview!;
    expect(preview.rows.map((row) => row.label)).toEqual([
      'ckb1qzda0c…93hhsj',
      'ckb1qzda0c…p909qk',
      'ckb1qzda0c…aaaaaa',
      'ckb1qzda0c…bbbbbb',
      'ckb1qzda0c…cccccc',
      'Others',
    ]);
    // 1,123 of 66,477 blocks, floored at four decimals like the API's own percentages.
    expect(preview.rows.at(-1)?.share).toBe('1.6893%');
    expect(preview.rows[0].title).toBe(rows[0].address);
    expect(preview.summaryLabel).toBe('Leading miner: ckb1qzda0c…93hhsj (0%)');
    expect(model.summaryLabel).toBe(`Leading miner: ${rows[0].address} (0%)`);
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
    expect(model.latest).toEqual({ label: 'year 6.5', raw: '2.3297', display: '2.33' });
    expect(model.summaryLabel).toBe('Today');
    expect(model.projectedAfter).toBe(2);
    expect(model.delta).toBeNull();
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
    expect(model.delta).toBe('+2');
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
      { index: 1, label: 'F', plotLabel: '◆', href: '/hardforks' },
    ]);
  });

  it('adds arbitrary decimal precision in stacked totals without truncating', () => {
    const model = chartModel(CHART_BY_SLUG.get('total-supply')!, {
      title: 'Supply',
      series: stacked.series,
      data: [
        { date: '2024-01-01', values: { liveCells: '1.123456789', deadCells: '2.000000001' } },
      ],
    });
    expect(model.latest?.raw).toBe('3.12345679');
  });

  it('summarizes live cells, the largest HODL band, and bucket distributions semantically', () => {
    const cells = chartModel(CHART_BY_SLUG.get('cell-count')!, stacked);
    expect(cells.latest?.raw).toBe('3');
    expect(cells.summaryLabel).toBe('Live cells');
    const hodl = chartModel(CHART_BY_SLUG.get('hodl-wave')!, {
      ...stacked,
      data: [
        {
          date: '2024-01-01',
          values: { liveCells: '35', deadCells: '65', holderCount: '9007199254740993' },
        },
      ],
    });
    expect(hodl.latest?.raw).toBe('65');
    expect(hodl.delta).toBeNull();
    expect(hodl.secondary?.points[0].values.holders).toBe('9007199254740993');
    expect(() => chartModel(CHART_BY_SLUG.get('hodl-wave')!, stacked)).toThrow(
      /missing holderCount/
    );
    const buckets = chartModel(CHART_BY_SLUG.get('cell-size-distribution')!, {
      title: 'Buckets',
      yAxisLabel: 'Cells',
      data: [
        { date: '<100 CKB', value: '90', value2: '1' },
        { date: '>=1m CKB', value: '1', value2: '10' },
      ],
    });
    expect(buckets.latest?.label).toBe('<100 CKB');
    expect(buckets.delta).toBeNull();
  });

  it('retains both exact script histories and the separate consensus target', () => {
    const script = chartModel(CHART_BY_SLUG.get('most-utilized-scripts')!, utilized);
    expect(script.historicalShares).toHaveLength(2);
    expect(script.historicalShares?.[0].points[0].values.top0).toBe('9007199254.740993');
    expect(script.historicalShares?.[1].points[0].values.top0).toBe('0.00000001');
    expect(script.latest?.display).not.toContain('Alpha');
    expect(script.summaryLabel).toContain('Alpha');
    const miner = chartModel(CHART_BY_SLUG.get('miner-address-distribution')!, miners);
    expect(miner.latest?.display).toBe('50');
    expect(miner.summaryLabel).toContain('50%');
    const uncle = chartModel(CHART_BY_SLUG.get('uncle-rate')!, {
      ...simple,
      orphanRateTarget: { numerator: 1, denominator: 40 },
    });
    expect(uncle.contextFacts).toEqual([{ label: 'Consensus target', value: '1 / 40 blocks' }]);
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
    expect(supply.points[0].values.nervosdao).toBe('123.00000001');
    expect(supply.points[1].values).toEqual(supply.points[0].values);
    expect(supply.latest?.raw).toBe('8400001000');
    expect(secondary.points).toHaveLength(13);
    expect(secondary.points[1].values).toEqual(secondary.points[0].values);
    expect(secondary.points[0].values.mining).toBe('1.50000001');
  });

  it('uses independent protocol issuance total when stacked legacy components overlap', () => {
    const model = chartModel(CHART_BY_SLUG.get('secondary-issuance')!, {
      title: 'Secondary issuance',
      series: [
        { key: 'compensation', label: 'Compensation', color: '#2edba3' },
        { key: 'burnt', label: 'Treasury', color: '#68ccf0' },
      ],
      data: [
        {
          date: '2024-01-01',
          values: { compensation: '2', burnt: '3' },
          protocolTotalShannons: '400000001',
          phase1CompensationShannons: '100000000',
        },
      ],
    });
    expect(model.latest?.raw).toBe('4.00000001');
    expect(model.contextFacts).toContainEqual({ label: 'Frozen phase-1 overlap', value: '1 CKB' });
    expect(model.points[0].values).toEqual({ compensation: '2', burnt: '3' });
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

  it('retains miner address and unresolved lock-hash identities in ranking links', () => {
    const model = chartModel(CHART_BY_SLUG.get('miner-address-distribution')!, {
      title: 'Miners',
      totalBlocks: 10,
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
    expect(model.rankedRows?.[0]).toMatchObject({
      href: '/address/ckb1resolved',
      identity: 'Address',
    });
    expect(model.rankedRows?.[1]).toMatchObject({
      href: `/address/${'b'.repeat(64)}`,
      identity: 'Lock hash',
    });
    expect(model.coverage).toContain('2026-07-23–2026-07-29');
  });
});
