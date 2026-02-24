export interface LegendDescriptionItem {
  label: string;
  description: string;
}

export interface ChartDescription {
  overview: string;
  legendItems: LegendDescriptionItem[];
}

interface ChartDescriptionContext {
  yAxisLabel?: string;
  y2AxisLabel?: string;
  seriesLabels?: string[];
}

type ChartDescriptionBuilder = (ctx: ChartDescriptionContext) => ChartDescription;

function bySeriesLabels(
  seriesLabels: string[] | undefined,
  builder: (label: string) => string
): LegendDescriptionItem[] {
  if (!seriesLabels || seriesLabels.length === 0) return [];
  return seriesLabels.map((label) => ({
    label,
    description: builder(label),
  }));
}

const CHART_DESCRIPTION_BUILDERS: Record<string, ChartDescriptionBuilder> = {
  'dao-chart-total-deposit': ({ yAxisLabel }) => ({
    overview:
      'Shows the running total of CKB locked in DAO deposits at each daily snapshot on canonical chain data.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Total Deposit',
        description:
          'For each day: sum occupied capacity of all live DAO deposit cells as of end-of-day state.',
      },
    ],
  }),
  'dao-chart-daily-deposit': ({ yAxisLabel }) => ({
    overview: 'Shows how much new CKB was deposited into DAO each day.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Daily Deposit',
        description:
          'For each day: sum capacities of outputs that create new DAO deposits in confirmed transactions.',
      },
    ],
  }),
  'dao-chart-circulation-ratio': ({ yAxisLabel }) => ({
    overview: 'Shows the share of circulating supply currently locked in DAO deposits.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Deposit to Circulation Ratio',
        description: 'For each day: (total DAO deposit / circulating supply) × 100%.',
      },
    ],
  }),
  'chart-transaction-count': ({ yAxisLabel }) => ({
    overview: 'Shows transaction throughput trend by day.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Transaction Count',
        description: 'For each day: count all confirmed transactions in canonical blocks.',
      },
    ],
  }),
  'chart-cell-count': ({ seriesLabels }) => ({
    overview: 'Shows daily cell counts by lifecycle state.',
    legendItems: bySeriesLabels(seriesLabels, (label) => {
      const normalized = label.toLowerCase();
      if (normalized.includes('all')) {
        return 'For each day: cumulative count of all created cells (live + consumed).';
      }
      if (normalized.includes('live')) {
        return 'For each day: count of currently unspent cells at end-of-day snapshot.';
      }
      if (normalized.includes('dead') || normalized.includes('consumed')) {
        return 'For each day: cumulative count of consumed cells.';
      }
      return `For each day: value of the "${label}" cell-count series from indexed canonical data.`;
    }),
  }),
  'chart-knowledge-size': ({ yAxisLabel, y2AxisLabel }) => ({
    overview:
      'Shows protocol common knowledge size and its utilization trend, plus day-over-day net occupied capacity flow.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Common Knowledge Size',
        description:
          'For each day: total occupied capacity from indexed chain snapshots (common knowledge size).',
      },
      {
        label: y2AxisLabel ?? 'Utilization (%)',
        description: 'For each day: (occupied capacity / total capacity base) × 100%.',
      },
      {
        label: 'Net Flow (CKB/day)',
        description:
          'For each day: common knowledge size(today) - common knowledge size(yesterday).',
      },
    ],
  }),
  'chart-common-knowledge-composition': ({ seriesLabels }) => ({
    overview:
      'Shows how total common knowledge size is split across composition categories over time.',
    legendItems: bySeriesLabels(
      seriesLabels,
      (label) =>
        `For each day: sum occupied capacity of live cells classified into "${label}" category.`
    ),
  }),
  'chart-cell-age-vs-occupied-capacity': ({ seriesLabels }) => ({
    overview: 'Shows how occupied capacity is distributed across different cell age buckets.',
    legendItems: bySeriesLabels(
      seriesLabels,
      (label) =>
        `For each day: sum occupied capacity of live cells whose age falls in "${label}" bucket.`
    ),
  }),
  'chart-capacity-turnover-ratio': ({ yAxisLabel }) => ({
    overview: 'Shows how actively live capacity is being consumed and replaced.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Capacity Turnover Ratio',
        description:
          'For each day: daily consumed capacity / daily average live occupied capacity.',
      },
    ],
  }),
  'chart-cell-size-distribution': ({ yAxisLabel }) => ({
    overview: 'Shows how live cells are distributed across size buckets.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Cell Count',
        description:
          'For each bucket: count live cells whose serialized size falls within that size range.',
      },
    ],
  }),
  'chart-address-cohort-retention': ({ yAxisLabel }) => ({
    overview:
      'Groups addresses by first-seen month, then shows how much of each cohort’s balance is currently occupied (locked) versus total balance.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Retention Rate',
        description:
          'For each cohort month: (sum of occupied_capacity for addresses first seen in that month / sum of balance for the same addresses) × 100%.',
      },
    ],
  }),
  'chart-most-utilized-scripts': () => ({
    overview:
      'Ranks scripts by utilization in live state: occupied capacity and total cells capacity.',
    legendItems: [
      {
        label: 'Occupied CKB',
        description:
          'For each script: sum live occupied capacity across deployments; ranked descending (top 20).',
      },
      {
        label: 'Total Cells Capacity',
        description:
          'For each script: sum live total cell capacity across deployments; ranked descending (top 20).',
      },
    ],
  }),
  'chart-most-utilized-assets': () => ({
    overview: 'Ranks token and NFT collection assets by utilization in live state.',
    legendItems: [
      {
        label: 'Occupied CKB',
        description:
          'For each asset: cumulative live occupied capacity derived from exact daily deltas; ranked descending (top 20).',
      },
      {
        label: 'Total Cells Capacity',
        description:
          'For each asset: cumulative live total cell capacity derived from exact daily deltas; ranked descending (top 20).',
      },
    ],
  }),
  'chart-block-time-distribution': ({ yAxisLabel }) => ({
    overview: 'Shows frequency distribution of observed block intervals.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Frequency',
        description:
          'For each interval bucket: count canonical blocks whose parent-to-child timestamp gap falls in that bucket.',
      },
    ],
  }),
  'chart-epoch-time-distribution': ({ yAxisLabel }) => ({
    overview: 'Shows frequency distribution of observed epoch durations.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Frequency',
        description:
          'For each duration bucket: count epochs whose elapsed duration falls in that bucket.',
      },
    ],
  }),
  'chart-epoch-time-length': ({ yAxisLabel }) => ({
    overview: 'Shows elapsed wall-clock duration for each epoch.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Epoch Time Length',
        description:
          'For each epoch: timestamp(last block in epoch) - timestamp(first block in epoch).',
      },
    ],
  }),
  'chart-average-block-time': ({ yAxisLabel }) => ({
    overview: 'Shows average block interval trend over reporting periods.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Average Block Time',
        description:
          'For each period: average of canonical parent-to-child block timestamp intervals.',
      },
    ],
  }),
  'chart-hash-rate': ({ yAxisLabel }) => ({
    overview: 'Shows estimated network mining power trend.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Hash Rate',
        description:
          'For each period: hash-rate estimate derived from chain difficulty and observed block production interval.',
      },
    ],
  }),
  'chart-difficulty': ({ yAxisLabel }) => ({
    overview: 'Shows canonical network difficulty trend.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Difficulty',
        description:
          'For each period: canonical difficulty value from block headers (or period aggregate of header values).',
      },
    ],
  }),
  'chart-uncle-rate': ({ yAxisLabel }) => ({
    overview: 'Shows uncle-block share trend.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Uncle Rate',
        description: 'For each period: (uncle blocks / canonical blocks) × 100%.',
      },
    ],
  }),
  'chart-miner-address-distribution': ({ seriesLabels }) => ({
    overview: 'Shows block production share by miner payout address.',
    legendItems: bySeriesLabels(
      seriesLabels,
      (label) =>
        `For "${label}": (blocks mined by this address group / total blocks in scope) × 100%.`
    ),
  }),
  'chart-total-supply': ({ seriesLabels }) => ({
    overview: 'Shows total supply composition over time.',
    legendItems: bySeriesLabels(
      seriesLabels,
      (label) =>
        `For each day: CKB amount attributed to "${label}" component from indexed supply accounting.`
    ),
  }),
  'chart-nominal-apc': ({ yAxisLabel }) => ({
    overview: 'Shows nominal annual compensation rate trend for DAO economics.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Nominal APC',
        description:
          'For each period: (annualized secondary issuance allocated to compensation / circulating supply) × 100%.',
      },
    ],
  }),
  'chart-secondary-issuance': ({ seriesLabels }) => ({
    overview: 'Shows secondary issuance allocation split by destination category.',
    legendItems: bySeriesLabels(
      seriesLabels,
      (label) =>
        `For each period: ("${label}" secondary issuance / total secondary issuance) × 100%.`
    ),
  }),
  'chart-inflation-rate': ({ yAxisLabel }) => ({
    overview: 'Shows annualized supply growth rate trend.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Inflation Rate',
        description:
          'For each period: (new issuance over period / supply base) annualized to percentage.',
      },
    ],
  }),
  'chart-hodl-wave': ({ seriesLabels }) => ({
    overview: 'Shows supply age structure and holder-count trend.',
    legendItems: [
      ...bySeriesLabels(
        seriesLabels,
        (label) => `For each day: ("${label}" age-band supply / total tracked supply) × 100%.`
      ),
      {
        label: 'Holder Count',
        description:
          'For each day: count of unique addresses holding positive balance in indexed data.',
      },
    ],
  }),
};

export function getChartDescription(
  queryKey: string,
  context: ChartDescriptionContext = {}
): ChartDescription | undefined {
  return CHART_DESCRIPTION_BUILDERS[queryKey]?.(context);
}
