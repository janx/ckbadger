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
      'Shows the capacity of live phase-0 Nervos DAO deposit cells at each complete daily snapshot, alongside the current depositor count.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Total Deposit',
        description:
          'For each day: sum full capacity of live phase-0 DAO deposit cells as of end-of-day state.',
      },
      {
        label: 'Current depositors',
        description:
          'Number of addresses with a live phase-0 DAO deposit at that daily snapshot; not cumulative-ever depositors.',
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
      'Shows protocol common knowledge size and its share of total capacity, plus day-over-day net common knowledge flow.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Common Knowledge Size',
        description: 'For each day: total common knowledge size from indexed chain snapshots.',
      },
      {
        label: y2AxisLabel ?? 'Common Knowledge Share (%)',
        description: 'For each day: (common knowledge size / total capacity base) × 100%.',
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
        `For each day: sum common knowledge size of live cells classified into "${label}" category.`
    ),
  }),
  'chart-capacity-turnover-ratio': ({ yAxisLabel }) => ({
    overview: 'Shows how actively live capacity is being consumed and replaced.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Capacity Turnover Ratio',
        description:
          'For each day: daily consumed capacity / daily average live common knowledge size.',
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
      'Groups addresses by first-seen month, then shows how much of each cohort’s balance is currently recorded as common knowledge versus total balance.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Retention Rate',
        description:
          'For each cohort month: (sum of common_knowledge_size for addresses first seen in that month / sum of balance for the same addresses) × 100%.',
      },
    ],
  }),
  'chart-most-utilized-scripts': () => ({
    overview: 'Ranks scripts by live common knowledge size against total cells capacity.',
    legendItems: [
      {
        label: 'Common Knowledge CKB',
        description:
          'For each script: sum live common knowledge size across deployments; ranked descending (top 20).',
      },
      {
        label: 'Total Cells Capacity',
        description:
          'For each script: sum live total cell capacity across deployments; ranked descending (top 20).',
      },
    ],
  }),
  'chart-most-utilized-assets': () => ({
    overview: 'Ranks token and Object collection assets by live common knowledge size.',
    legendItems: [
      {
        label: 'Common Knowledge CKB',
        description:
          'For each asset: cumulative live common knowledge size derived from exact daily deltas; ranked descending (top 20).',
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
    overview:
      "For each complete UTC+8 day: the sum of the difficulty of that day's blocks, divided by the sum of their intervals in seconds. The day in progress is left out until it is complete.",
    legendItems: [
      {
        label: yAxisLabel ?? 'Hash Rate',
        description:
          'Hashes per second, drawn in PH/s (10^15). Every day is its own estimate; nothing is averaged across days.',
      },
    ],
  }),
  'chart-difficulty': ({ yAxisLabel }) => ({
    overview:
      "For each complete UTC+8 day: the mean difficulty of that day's blocks, the sum of their header difficulties divided by the number of blocks.",
    legendItems: [
      {
        label: yAxisLabel ?? 'Difficulty',
        description: 'Drawn in P (10^15). The day in progress is left out until it is complete.',
      },
    ],
  }),
  'chart-uncle-rate': ({ yAxisLabel }) => ({
    overview:
      "For each complete UTC+8 day: the uncles included by that day's blocks divided by the number of blocks, as a percentage. The dashed line is the consensus orphan-rate target the difficulty adjustment aims for.",
    legendItems: [
      {
        label: yAxisLabel ?? 'Uncle Rate',
        description: 'Exact ratio of two daily counts; the day in progress is left out.',
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
    overview:
      'Shows a protocol-rate model by years since genesis, rather than a sequence of observed daily chain snapshots.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Nominal APC',
        description:
          'For each modeled quarter-year: fixed annual secondary issuance divided by modeled total issued supply after the primary issuance halvings, multiplied by 100%. The genesis issuance base comes from this network.',
      },
    ],
  }),
  'chart-secondary-issuance': ({ seriesLabels }) => ({
    overview:
      'Shows cumulative secondary issuance components in CKB at complete daily snapshots. The headline protocol total comes independently from the canonical DAO fields; frozen phase-1 compensation can overlap the legacy component partition.',
    legendItems: bySeriesLabels(
      seriesLabels,
      (label) =>
        `For each complete day: exact cumulative CKB in the "${label}" secondary issuance component.`
    ),
  }),
  'chart-inflation-rate': ({ yAxisLabel }) => ({
    overview:
      'Shows realized issuance growth over each trailing 365 complete UTC+8 days from on-chain DAO headers.',
    legendItems: [
      {
        label: yAxisLabel ?? 'Inflation Rate',
        description:
          'Nominal uses total DAO C-field growth; real removes the exact secondary-issuance growth, leaving primary-issuance dilution. Both divide by the C field 365 days earlier.',
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
  'chart-activity-volume': () => ({
    overview:
      'Counts indexed activity events in each canonical chain day, across transfers, DAO changes, tokens, objects, identities and script calls.',
    legendItems: [
      {
        label: 'Activities',
        description:
          'Sum the daily counts of each indexed activity category; one transaction can yield more than one activity.',
      },
    ],
  }),
  'chart-activity-type-breakdown': ({ seriesLabels }) => ({
    overview:
      'Splits each indexed chain day’s activity count by the event category recorded by the activity index.',
    legendItems: bySeriesLabels(
      seriesLabels,
      (label) => `Daily indexed ${label.toLowerCase()} activity count.`
    ),
  }),
  'chart-active-addresses': () => ({
    overview:
      'Counts distinct addresses participating in indexed activities for each canonical chain day.',
    legendItems: [
      {
        label: 'Addresses',
        description:
          'Each address contributes once to a day, regardless of its number of activities.',
      },
    ],
  }),
  'chart-ckb-volume': () => ({
    overview:
      'Sums the exact CKB capacity moved by indexed transfer activities for each canonical chain day.',
    legendItems: [
      {
        label: 'CKB moved',
        description:
          'The activity index records moved capacity in shannons; the chart converts it exactly to CKB.',
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
