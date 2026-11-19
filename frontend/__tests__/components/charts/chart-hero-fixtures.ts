/**
 * One payload per chart page (lane D1, the chart-hero board): small, exact and shaped as the API
 * sends it, keyed by the `api` method the catalogue fetches it with. Dated series end on
 * 2026-11-18; a few carry the shapes the board rules on (a peak, a year before, a target, the
 * genesis epochs, a spike).
 */

const DAY = 86_400_000;
const END = Date.parse('2026-11-18T00:00:00Z');
/** `n` consecutive days ending 2026-11-18, oldest first. */
export function days(n: number): string[] {
  return Array.from({ length: n }, (_, i) =>
    new Date(END - (n - 1 - i) * DAY).toISOString().slice(0, 10)
  );
}

/** The first point holding the greatest value, as the API's `peak` (R8). */
function peakOf(data: readonly { date: string; value: string }[]) {
  let best = data[0];
  for (const point of data) if (Number(point.value) > Number(best.value)) best = point;
  return { date: best.date, value: best.value };
}

function line(title: string, values: readonly string[], extra: Record<string, unknown> = {}) {
  const dates = days(values.length);
  const data = values.map((value, i) => ({ date: dates[i], value, value2: null }));
  return {
    title,
    yAxisLabel: title,
    y2AxisLabel: null,
    data,
    peak: peakOf(data),
    peak2: null,
    ...extra,
  };
}

/** 400 days of a daily count: 20,000 a day, one spike of 1,111,902, then the last four days. */
function dailyCount(last: readonly number[]): string[] {
  const values = Array.from({ length: 400 - last.length }, (_, i) =>
    String(i === 100 ? 1_111_902 : 20_000 + (i % 7) * 100)
  );
  return [...values, ...last.map(String)];
}

/** A day of activity kinds, the newest 5,425 script calls, day 50 a spike; with the API's whole. */
const activityDay = (i: number, n: number) => {
  const scriptCall = i === n - 1 ? 5425 : i === 50 ? 1_000_000 : 5000;
  const kinds = { transfer: 600 + (i % 30), dao: 5, token: 3, object: 0, identity: 70, scriptCall };
  return {
    ...Object.fromEntries(Object.entries(kinds).map(([key, count]) => [key, String(count)])),
    // the day's kind occurrences, the API's sum of the kinds (lane D1, the hand-back's item 2)
    kindOccurrences: String(Object.values(kinds).reduce((sum, count) => sum + count, 0)),
  };
};

function stacked(
  title: string,
  n: number,
  values: (i: number) => Record<string, string>,
  series: readonly [string, string][]
) {
  const dates = days(n);
  return {
    title,
    data: dates.map((date, i) => ({ date, values: values(i) })),
    series: series.map(([key, label]) => ({ key, label, color: '#000000' })),
  };
}

/** A ranked share history: seven named ranks and the API's remainder, the five and the rest drawn. */
function shareHistory(names: readonly string[], ids: readonly string[]) {
  const ckb = (n: number) => `${n}00000000`;
  const last = [600, 500, 400, 300, 200, 100, 50];
  const dates = days(3);
  const point = (date: string, k: number) => {
    const values: Record<string, string> = {};
    last.forEach((v, r) => (values[`top${r}`] = ckb(v - k)));
    values.others = ckb(50);
    values.rest = ckb(100 - k + 50 - k + 50);
    // the day's whole, the API's (the ranks and its remainder)
    values.total = ckb(last.reduce((sum, v) => sum + v - k, 0) + 50);
    return { date, values };
  };
  return {
    title: 'share',
    data: dates.map((date, i) => point(date, 2 - i)),
    series: [
      ...names.map((label, r) => ({ key: `top${r}`, label, color: '#000000', id: ids[r] })),
      { key: 'others', label: 'Others', color: '#000000' },
    ],
    bands: [
      ...names.slice(0, 5).map((label, r) => ({ key: `top${r}`, label, members: [`top${r}`] })),
      { key: 'rest', label: 'Others', members: ['top5', 'top6', 'others'] },
    ],
  };
}

const SCRIPT_NAMES = [
  'Default Lock',
  'Nervos DAO',
  'Omnilock',
  'xUDT',
  'Spore',
  'JoyID',
  'Cluster',
];
const SCRIPT_IDS = SCRIPT_NAMES.map((_, r) => `family:${r + 1}`);
const ASSET_NAMES = [
  'RUSD (token)',
  'Unicorn (object)',
  'USDI (token)',
  'Seal (token)',
  'Nervape (object)',
  '.bit (identity)',
  'CKBI (token)',
];
const ASSET_IDS = ASSET_NAMES.map((_, r) => `token:0x${String(r + 1).padStart(64, '0')}`);

const hodlDates = ['2025-11-18', '2026-11-18'];
const hodlPoint = (gt3y: string, lt1m: string) => ({
  gt3y,
  '1y3y': '31.05',
  '6m1y': '12.40',
  '3m6m': '8.00',
  '1m3m': '8.31',
  '1w1m': '6.00',
  '1d1w': '4.00',
  '24h': '2.87',
  '1m6m': '16.31',
  lt1m,
  holderCount: '42000',
});

export const CHART_FIXTURES: Record<string, unknown> = {
  getTransactionCountChart: line('Transaction Count', dailyCount([15_000, 20_000, 9_000, 12_000])),
  getActiveAddressesChart: line('Active addresses', dailyCount([2_500, 2_600, 2_575])),
  getCkbVolumeChart: line(
    'CKB moved',
    dailyCount([300_000_000, 302_594_731]).map((v) => `${v}12345678`)
  ),
  getActivityVolumeChart: line('Activities', dailyCount([7_000, 6_177])),
  getActivityTypeBreakdownChart: stacked('Activity by type', 120, (i) => activityDay(i, 120), [
    ['transfer', 'Transfer'],
    ['dao', 'DAO'],
    ['token', 'Token'],
    ['object', 'Object'],
    ['identity', 'Identity'],
    ['scriptCall', 'Script Call'],
  ]),
  getCellCountChart: {
    ...stacked(
      'Cell Count',
      3,
      (i) => ({
        liveCells: String(1_000_000 + i * 10),
        deadCells: String(500_000 + i),
        allCells: String(1_500_000 + i * 11),
      }),
      [
        ['allCells', 'All Cells'],
        ['liveCells', 'Live Cells'],
        ['deadCells', 'Dead Cells'],
      ]
    ),
    peaks: { liveCells: { date: '2026-11-18', value: '1000020' } },
  },
  getHodlWaveChart: {
    title: 'CKB HODL Wave',
    data: [
      { date: hodlDates[0], values: hodlPoint('21.74', '14.00') },
      { date: hodlDates[1], values: hodlPoint('27.37', '12.87') },
    ],
    series: [
      ['gt3y', '> 3y'],
      ['1y3y', '1y-3y'],
      ['6m1y', '6m-1y'],
      ['3m6m', '3m-6m'],
      ['1m3m', '1m-3m'],
      ['1w1m', '1w-1m'],
      ['1d1w', '1d-1w'],
      ['24h', '24h'],
    ].map(([key, label]) => ({ key, label, color: '#000000' })),
    bands: [
      { key: 'gt3y', label: '3 years or more', members: ['gt3y'] },
      { key: '1y3y', label: '1 to 3 years', members: ['1y3y'] },
      { key: '6m1y', label: '6 to 12 months', members: ['6m1y'] },
      { key: '1m6m', label: '1 to 6 months', members: ['3m6m', '1m3m'] },
      { key: 'lt1m', label: 'under 1 month', members: ['1w1m', '1d1w', '24h'] },
    ],
  },
  getTotalSupplyChart: stacked(
    'Total Supply',
    2,
    (i) => ({
      // four bands that partition C, the day's total issuance (the route checks it per row)
      outsideDao: i ? '5000' : '4999',
      daoDeposited: '3000',
      unclaimedCompensation: '500',
      burnt: '3500',
      totalIssuance: i ? '12000' : '11999',
      // the protocol circulation, C − genesis burnt − S: outside the DAO and in it
      circulating: i ? '8000' : '7999',
    }),
    [
      ['outsideDao', 'CKB circulating outside the DAO'],
      ['daoDeposited', 'Nervos DAO deposits'],
      ['unclaimedCompensation', 'DAO compensation not yet claimed'],
      ['burnt', 'Burnt'],
    ]
  ),
  getDaoTotalDepositChart: {
    ...line('Total Deposit', [
      '8000000000.5',
      '11190757666.00891121',
      '8190000000',
      '8191619544.72754077',
    ]),
    data: line('x', [
      '8000000000.5',
      '11190757666.00891121',
      '8190000000',
      '8191619544.72754077',
    ]).data.map((point, i) => ({ ...point, value2: String(16_000 + i) })),
  },
  getDaoDailyDepositChart: {
    ...line(
      'Daily Deposit',
      dailyCount([5_000_000, 12_232_783]).map((v) => `${v}.45045686`)
    ),
    data: line(
      'x',
      dailyCount([5_000_000, 12_232_783]).map((v) => `${v}.45045686`)
    ).data.map((point) => ({ ...point, value2: '5' })),
  },
  getNominalApcChart: {
    title: 'Nominal DAO Compensation Rate',
    yAxisLabel: 'APC',
    y2AxisLabel: 'nominal-apc-timeline',
    data: [
      { date: '0', value: '5.3333', value2: 'past' },
      { date: '7', value: '2.3642', value2: 'current' },
      { date: '20', value: '1.5881', value2: 'future' },
    ],
    peak: { date: '0', value: '5.3333' },
    peak2: null,
    today: { blockNumber: 21_000_000, date: '2026-11-18', year: '7.01', value: '2.3601' },
  },
  getInflationRateChart: {
    ...line(
      'Inflation',
      Array.from({ length: 366 }, (_, i) => (i === 0 ? '5.9712' : '5.4852'))
    ),
    data: line(
      'x',
      Array.from({ length: 366 }, (_, i) => (i === 0 ? '5.9712' : '5.4852'))
    ).data.map((point) => ({ ...point, value2: '3.3446' })),
  },
  getSecondaryIssuanceChart: {
    title: 'Secondary Issuance',
    data: days(2).map((date) => ({
      date,
      values: { burnt: '6817.5', compensation: '1700', mining: '900' },
      protocolTotalShannons: '939761189025563741',
      phase1CompensationShannons: '0',
      // the three destinations' total, the API's: 6,817.5 + 1,700 + 900 CKB
      componentsTotalShannons: '941750000000',
    })),
    series: [
      { key: 'compensation', label: 'Deposit Compensation', color: '#000000' },
      { key: 'mining', label: 'Mining', color: '#000000' },
      { key: 'burnt', label: 'Burnt', color: '#000000' },
    ],
  },
  getDaoCirculationRatioChart: line('Ratio', ['35.8909', '16.3844', '16.3744']),
  getKnowledgeSizeChart: {
    ...line('Common Knowledge Size', ['180360561', '149000000', '149076490']),
    data: line('x', ['180360561', '149000000', '149076490']).data.map((point) => ({
      ...point,
      value2: '0.3563',
    })),
    peak2: { date: '2026-11-16', value: '0.3563' },
  },
  getCommonKnowledgeCompositionChart: stacked(
    'Composition',
    2,
    () => ({
      transfer: '40',
      nftSpore: '5',
      dao: '2',
      udt: '9',
      otherContracts: '44',
      knowledgeSize: '100',
    }),
    [
      ['transfer', 'CKB Transfer'],
      ['nftSpore', 'NFT'],
      ['dao', 'DAO'],
      ['udt', 'UDT'],
      ['otherContracts', 'Other'],
    ]
  ),
  getMostUtilizedScriptsChart: {
    title: 'Scripts Used & Total CKBytes',
    usedShare: shareHistory(SCRIPT_NAMES, SCRIPT_IDS),
    capacityShare: shareHistory(SCRIPT_NAMES, SCRIPT_IDS),
  },
  getMostUtilizedAssetsChart: {
    title: 'Assets Used & Total CKBytes',
    usedShare: shareHistory(ASSET_NAMES, ASSET_IDS),
    capacityShare: shareHistory(ASSET_NAMES, ASSET_IDS),
  },
  getCellSizeDistributionChart: {
    title: 'Cell Size Distribution',
    yAxisLabel: 'Live Cells',
    y2AxisLabel: 'Common Knowledge Size (CKB)',
    data: [
      { date: '<100 CKB', value: '911281', value2: '55925957' },
      { date: '100-1k CKB', value: '90000', value2: '20000000' },
      { date: '1k-10k CKB', value: '9000', value2: '20000000' },
      { date: '10k-100k CKB', value: '900', value2: '20000000' },
      { date: '100k-1m CKB', value: '90', value2: '60000000' },
      { date: '>=1m CKB', value: '9', value2: '1048617' },
    ],
    peak: { date: '<100 CKB', value: '911281' },
    peak2: { date: '100k-1m CKB', value: '60000000' },
    // the two wholes, the API's
    liveCells: '1011280',
    occupiedCapacity: '176974574',
  },
  getCapacityTurnoverRatioChart: {
    ...line('Turnover', ['29.516483', '3.085013', '2.5', '2.171181']),
    data: line('x', ['29.516483', '3.085013', '2.5', '1.451822']).data.map((point, i) => ({
      ...point,
      value2: ['29.516483', '15.770769', '2.6', '2.171181'][i],
    })),
    peak2: { date: days(4)[0], value: '29.516483' },
  },
  getAddressCohortRetentionChart: {
    title: 'Address Cohort Retention',
    yAxisLabel: 'Common Knowledge / Balance (%)',
    y2AxisLabel: 'Common Knowledge Size (CKB)',
    data: [
      { date: '2024-06', value: '7.895999', value2: '3440098' },
      { date: '2026-10', value: '0.013256', value2: '1000' },
      { date: '2026-11', value: '0.02', value2: '10' },
    ],
    peak: { date: '2024-06', value: '7.895999' },
    peak2: { date: '2024-06', value: '3440098' },
  },
  getHashRateChart: {
    title: 'Hash Rate',
    yAxisLabel: 'Hash Rate',
    y2AxisLabel: null,
    data: [
      { date: '2024-12-27', value: '465310000000000000', value2: null },
      { date: '2026-11-17', value: '79890000000000000', value2: null },
      { date: '2026-11-18', value: '81050000000000000', value2: null },
    ],
    peak: { date: '2024-12-27', value: '465310000000000000' },
    peak2: null,
  },
  getDifficultyChart: line('Difficulty', [
    '5229343207306844236',
    '788000000000000000',
    '788781501657058581',
  ]),
  getUncleRateChart: {
    ...line(
      'Uncle Rate',
      Array.from({ length: 40 }, (_, i) => (i >= 10 && i < 22 ? '0.030000' : '0.020000'))
    ),
    orphanRateTarget: { numerator: 1, denominator: 40 },
  },
  getMinerAddressDistributionChart: {
    title: 'Miner Distribution',
    windowDays: 7,
    fromDate: '2026-11-12',
    toDate: '2026-11-18',
    totalBlocks: 60_583,
    data: [
      ['F2Pool', 41_816],
      [null, 9_000],
      ['ViaBTC', 5_000],
      [null, 3_000],
      [null, 1_000],
    ].map(([name, blocks], r) => ({
      minerLockHash: `0x${String(r + 1)
        .repeat(64)
        .slice(0, 64)}`,
      address: `ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq${r}abcdefghij`,
      minerName: name,
      blocksMined: blocks,
      percentage: '0',
    })),
  },
  getAverageBlockTimeChart: line(
    'Average Block Time',
    Array.from({ length: 200 }, (_, i) =>
      i === 0 ? '113.42' : i === 1 ? '116.32' : i === 199 ? '9.95' : i === 150 ? '10.5' : '9.5'
    )
  ),
  getEpochTimeLengthChart: {
    title: 'Epoch Time Length',
    yAxisLabel: 'Hours',
    y2AxisLabel: 'Blocks',
    data: Array.from({ length: 300 }, (_, i) => ({
      date: String(i),
      value: i === 0 ? '50.48' : i === 1 ? '17.21' : i % 2 ? '4.05' : '3.95',
      value2: '1700',
    })),
    peak: { date: '0', value: '50.48' },
    peak2: { date: '1', value: '1800' },
    target: { seconds: 14_400, hours: '4.00' },
  },
  getBlockTimeDistributionChart: {
    title: 'Block Time Distribution (Last 42 Epochs)',
    yAxisLabel: 'Block Ratio (%)',
    y2AxisLabel: null,
    data: Array.from({ length: 60 }, (_, i) => ({
      date: (i / 10).toFixed(1),
      value: i === 13 ? '1.03' : i < 50 ? '0.9' : '0.1',
      value2: null,
    })),
    peak: { date: '1.3', value: '1.03' },
    peak2: null,
  },
  getEpochTimeDistributionChart: {
    title: 'Epoch Time Distribution',
    yAxisLabel: 'Epochs',
    y2AxisLabel: null,
    data: [
      { date: '2.05', value: '1', value2: null },
      // minutes 234, 237, 240, 241, 243 and 246 in hours, cut as the API writes them (241 read 4.02
      // half up)
      ...['3.90', '3.95', '4.00', '4.01', '4.05', '4.10'].map((date) => ({
        date,
        value: date === '4.01' ? '628' : '300',
        value2: null,
      })),
      { date: '50.50', value: '1', value2: null },
    ],
    peak: { date: '4.01', value: '628' },
    peak2: null,
  },
};
