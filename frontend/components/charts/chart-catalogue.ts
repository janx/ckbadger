import { api } from '@/lib/api';

export type ChartPayload =
  | Awaited<ReturnType<typeof api.getKnowledgeSizeChart>>
  | Awaited<ReturnType<typeof api.getCellCountChart>>
  | Awaited<ReturnType<typeof api.getMostUtilizedScriptsChart>>
  | Awaited<ReturnType<typeof api.getMinerAddressDistributionChart>>
  | Awaited<ReturnType<typeof api.getUncleRateChart>>
  | Awaited<ReturnType<typeof api.getCellSizeDistributionChart>>
  | Awaited<ReturnType<typeof api.getBlockTimeDistributionChart>>;

/**
 * The one stack that draws more bands than the board's rule of five (rule 5; the lead's ruling,
 * lane D1's third round, T's MUST-1): Activity by type's activity kinds are the site's own closed
 * set, each with its class colour and its filter chip, and none of them is "others"; five and
 * "others" would hide a named kind behind a word. It draws every kind (seven since F-6 gave the
 * protocols their own, so the kinds partition the day's activities). An exception to a rule the
 * owner passed: the owner's to confirm at the gate. Every other stack, classed or not, draws at most
 * five.
 */
export const ACTIVITY_KINDS_SLUG = 'activity-type-breakdown';

export type ChartVisual = 'line' | 'bars' | 'stacked' | 'ranked';
export type ChartScale =
  | 'raw'
  | 'shannons'
  | 'hash-rate'
  | 'difficulty'
  | 'fraction-percent'
  // a duration in whole ms, shown in seconds or hours (lane D1's third round, M1)
  | 'ms-seconds'
  | 'ms-hours';
export type ChartSource =
  | 'blocks'
  | 'cells'
  | 'activities'
  | 'addresses'
  | 'dao'
  | 'supply'
  | 'scripts'
  | 'assets';

export interface ChartDefinition {
  slug: string;
  title: string;
  subtitle: string;
  /**
   * The index card's caption (lane D1, the chart-hero board, rule 9): what the card's figure is,
   * written to stand with the head's date on one 12 px line of the narrowest card, never cut
   * (`__tests__/components/charts/chart-card-head.test.tsx` holds every caption to it).
   */
  cardSubtitle: string;
  /**
   * The plot's name before its span (lane D1, rule 4): the unit said once, in words where it is a
   * count (`PH/s`, `transactions a day`); its ticks are bare numbers.
   */
  plotUnit: string;
  /**
   * Drawn on a log scale until the reader picks linear (CH-08, rule 6): a series that spans orders of
   * magnitude, where a linear axis flattens every day under one spike.
   */
  logScale?: true;
  /** The index card draws only the newest N points, exactly as served (CH-13); the detail all. */
  cardDays?: number;
  section: string;
  visual: ChartVisual;
  unit: string;
  scale: ChartScale;
  source: ChartSource;
  evidenceHref: string;
  evidenceLabel: string;
  queryKey: string;
  fetch: () => Promise<ChartPayload>;
  /**
   * A second physical unit the API returns beside the main series: its own chart under the main
   * one, never stacked or combined with it (CH-06). `label` names it once, as the section's command
   * line; `note` says in plain words what it counts; `caption` is its quantity, for its caption.
   */
  secondary?: { unit: string; label: string; caption: string; note: string };
  primaryField?: 'value' | 'value2';
  /**
   * The names of a chart's two series of one unit (the API's value and its companion), in the
   * page's words, never the API's axis labels with their "(%)" (CH-21).
   */
  seriesNames?: { primary: string; companion: string };
  /** Four related charts, as on the ChartDetail board (CH-28). */
  related: readonly string[];
}

// The user's order (F2 charts, CH-05): what every reader follows first, protocol charts later.
const section = {
  activity: 'Activity',
  dao: 'Nervos DAO & economics',
  knowledge: 'Common knowledge',
  work: 'Proof of work',
  blocks: 'Blocks',
} as const;

export const CHARTS: readonly ChartDefinition[] = [
  {
    slug: 'transaction-count',
    title: 'Transactions',
    subtitle: 'Confirmed transactions per day',
    cardSubtitle: 'confirmed a day',
    plotUnit: 'transactions a day',
    logScale: true,
    section: section.activity,
    visual: 'line',
    unit: 'transactions',
    scale: 'raw',
    source: 'blocks',
    evidenceHref: '/transactions',
    evidenceLabel: 'Transactions',
    queryKey: 'chart-transaction-count',
    fetch: api.getTransactionCountChart,
    related: ['active-addresses', 'activity-volume', 'ckb-volume', 'average-block-time'],
  },
  {
    slug: 'active-addresses',
    title: 'Active addresses',
    subtitle: 'Addresses taking part, per day',
    cardSubtitle: 'active a day',
    plotUnit: 'addresses a day',
    section: section.activity,
    visual: 'line',
    unit: 'addresses',
    scale: 'raw',
    source: 'addresses',
    evidenceHref: '/activities',
    evidenceLabel: 'Address activity',
    queryKey: 'chart-active-addresses',
    fetch: api.getActiveAddressesChart,
    related: ['transaction-count', 'activity-volume', 'address-cohort-retention', 'hodl-wave'],
  },
  {
    slug: 'ckb-volume',
    title: 'CKB moved',
    subtitle: 'CKB moved per day',
    cardSubtitle: 'CKB moved a day',
    plotUnit: 'CKB moved a day',
    section: section.activity,
    visual: 'bars',
    unit: 'CKB',
    scale: 'shannons',
    source: 'activities',
    evidenceHref: '/activities',
    evidenceLabel: 'Activities',
    queryKey: 'chart-ckb-volume',
    fetch: api.getCkbVolumeChart,
    related: ['transaction-count', 'activity-volume', 'capacity-turnover-ratio', 'hodl-wave'],
  },
  {
    slug: 'activity-volume',
    title: 'Activities',
    subtitle: 'Activities per day',
    cardSubtitle: 'activities a day',
    plotUnit: 'activities a day',
    logScale: true,
    section: section.activity,
    visual: 'line',
    unit: 'activities',
    scale: 'raw',
    source: 'activities',
    evidenceHref: '/activities',
    evidenceLabel: 'Activities',
    queryKey: 'chart-activity-volume',
    fetch: api.getActivityVolumeChart,
    related: ['activity-type-breakdown', 'transaction-count', 'active-addresses', 'ckb-volume'],
  },
  {
    slug: 'activity-type-breakdown',
    title: 'Activity by type',
    subtitle: 'Activities per day, by kind',
    cardSubtitle: 'largest kind a day',
    plotUnit: 'activities a day, by kind',
    // a band is the activities of its kind; the kinds partition the day's activities
    section: section.activity,
    visual: 'stacked',
    unit: 'activities',
    scale: 'raw',
    source: 'activities',
    evidenceHref: '/activities',
    evidenceLabel: 'Activities',
    queryKey: 'chart-activity-type-breakdown',
    fetch: api.getActivityTypeBreakdownChart,
    related: ['activity-volume', 'transaction-count', 'active-addresses', 'daily-deposit'],
  },
  {
    slug: 'cell-count',
    title: 'Live cells',
    subtitle: 'Live cells, count',
    cardSubtitle: 'live cells',
    plotUnit: 'live cells',
    section: section.activity,
    visual: 'line',
    unit: 'cells',
    scale: 'raw',
    source: 'cells',
    evidenceHref: '/inventory',
    evidenceLabel: 'Inventory',
    queryKey: 'chart-cell-count',
    fetch: api.getCellCountChart,
    related: [
      'knowledge-size',
      'cell-size-distribution',
      'transaction-count',
      'common-knowledge-composition',
    ],
  },
  {
    slug: 'hodl-wave',
    title: 'HODL wave',
    subtitle: 'CKB by time since it last moved',
    cardSubtitle: 'unmoved 3 y or more',
    plotUnit: '% of all CKB, by time since it last moved',
    section: section.activity,
    visual: 'stacked',
    unit: '%',
    scale: 'raw',
    source: 'addresses',
    evidenceHref: '/activities',
    evidenceLabel: 'Address activity',
    queryKey: 'chart-hodl-wave',
    fetch: api.getHodlWaveChart,
    related: [
      'active-addresses',
      'ckb-volume',
      'capacity-turnover-ratio',
      'address-cohort-retention',
    ],
  },
  {
    slug: 'total-supply',
    title: 'Supply',
    subtitle: 'CKB issued, by where it is',
    cardSubtitle: 'circulating',
    plotUnit: 'CKB issued, by where it is',
    section: section.dao,
    visual: 'stacked',
    unit: 'CKB',
    scale: 'raw',
    source: 'supply',
    evidenceHref: '/dao',
    evidenceLabel: 'Supply and DAO',
    queryKey: 'chart-total-supply',
    fetch: api.getTotalSupplyChart,
    related: ['secondary-issuance', 'inflation-rate', 'total-deposit', 'nominal-apc'],
  },
  {
    slug: 'total-deposit',
    title: 'DAO deposits',
    subtitle: 'CKB locked in Nervos DAO',
    cardSubtitle: 'deposited in the DAO',
    plotUnit: 'CKB deposited in the Nervos DAO',
    section: section.dao,
    visual: 'line',
    unit: 'CKB',
    scale: 'raw',
    source: 'dao',
    evidenceHref: '/dao',
    evidenceLabel: 'Nervos DAO',
    queryKey: 'dao-chart-total-deposit',
    fetch: api.getDaoTotalDepositChart,
    secondary: {
      unit: 'depositors',
      label: 'Current depositors',
      caption: 'Depositors, per day',
      note: 'Addresses holding a live Nervos DAO deposit at the end of each day.',
    },
    related: ['daily-deposit', 'circulation-ratio', 'nominal-apc', 'total-supply'],
  },
  {
    slug: 'daily-deposit',
    title: 'Deposited per day',
    subtitle: 'CKB deposited into Nervos DAO per day',
    // CH-13 (Charts.dc.html): the card shows the last 60 complete days; the detail all of them.
    cardSubtitle: 'deposited a day, last 60 d',
    plotUnit: 'CKB deposited a day',
    cardDays: 60,
    section: section.dao,
    visual: 'bars',
    unit: 'CKB',
    scale: 'raw',
    source: 'dao',
    evidenceHref: '/dao',
    evidenceLabel: 'Nervos DAO',
    queryKey: 'dao-chart-daily-deposit',
    fetch: api.getDaoDailyDepositChart,
    secondary: {
      unit: 'deposits',
      label: 'Deposit count',
      caption: 'Deposits, per day',
      note: 'New Nervos DAO deposits made each day.',
    },
    related: ['total-deposit', 'circulation-ratio', 'nominal-apc', 'activity-type-breakdown'],
  },
  {
    slug: 'nominal-apc',
    title: 'Nominal APC',
    subtitle: '% a year, by year since genesis',
    cardSubtitle: 'a year, the model today',
    plotUnit: '% a year',
    section: section.dao,
    visual: 'line',
    unit: '%',
    scale: 'raw',
    source: 'dao',
    evidenceHref: '/dao',
    evidenceLabel: 'Nervos DAO',
    queryKey: 'chart-nominal-apc',
    fetch: api.getNominalApcChart,
    related: ['total-deposit', 'secondary-issuance', 'inflation-rate', 'total-supply'],
  },
  {
    slug: 'inflation-rate',
    title: 'Inflation',
    subtitle: 'Trailing 365 days, %, real dashed',
    cardSubtitle: 'nominal, trailing 365 d',
    plotUnit: '% over the trailing 365 days',
    section: section.dao,
    visual: 'line',
    unit: '%',
    scale: 'raw',
    source: 'supply',
    evidenceHref: '/dao',
    evidenceLabel: 'Supply and DAO',
    queryKey: 'chart-inflation-rate',
    fetch: api.getInflationRateChart,
    seriesNames: { primary: 'Nominal inflation', companion: 'Real inflation' },
    related: ['total-supply', 'secondary-issuance', 'nominal-apc', 'total-deposit'],
  },
  {
    slug: 'secondary-issuance',
    title: 'Secondary issuance',
    subtitle: 'CKB since genesis, by destination',
    cardSubtitle: 'issued since genesis',
    plotUnit: 'CKB issued since genesis, by destination',
    section: section.dao,
    visual: 'stacked',
    unit: 'CKB',
    scale: 'raw',
    source: 'supply',
    evidenceHref: '/dao',
    evidenceLabel: 'Supply and DAO',
    queryKey: 'chart-secondary-issuance',
    fetch: api.getSecondaryIssuanceChart,
    related: ['total-supply', 'nominal-apc', 'inflation-rate', 'total-deposit'],
  },
  {
    slug: 'circulation-ratio',
    title: 'DAO / circulation',
    subtitle: 'Current DAO deposits as share of circulating CKB',
    cardSubtitle: 'of circulating CKB',
    plotUnit: '% of circulating CKB in the Nervos DAO',
    section: section.dao,
    visual: 'line',
    unit: '%',
    scale: 'raw',
    source: 'dao',
    evidenceHref: '/dao',
    evidenceLabel: 'Nervos DAO',
    queryKey: 'dao-chart-circulation-ratio',
    fetch: api.getDaoCirculationRatioChart,
    related: ['total-deposit', 'daily-deposit', 'total-supply', 'nominal-apc'],
  },
  {
    slug: 'knowledge-size',
    title: 'Knowledge size',
    subtitle: 'CKB occupied by live cells',
    cardSubtitle: 'occupied by live cells',
    plotUnit: 'CKB occupied by live cells',
    section: section.knowledge,
    visual: 'line',
    unit: 'CKB',
    scale: 'raw',
    source: 'cells',
    evidenceHref: '/inventory',
    evidenceLabel: 'Inventory',
    queryKey: 'chart-knowledge-size',
    fetch: api.getKnowledgeSizeChart,
    secondary: {
      unit: '%',
      label: 'Utilization',
      caption: 'Occupied share of circulating CKB',
      note: 'CKB occupied by live cells as a share of circulating CKB, each day.',
    },
    related: [
      'common-knowledge-composition',
      'cell-count',
      'cell-size-distribution',
      'capacity-turnover-ratio',
    ],
  },
  {
    slug: 'common-knowledge-composition',
    title: 'Knowledge by kind',
    subtitle: 'CKB occupied, by what the cell holds',
    cardSubtitle: 'largest kind',
    plotUnit: 'CKB occupied, by what the cell holds',
    section: section.knowledge,
    visual: 'stacked',
    unit: 'CKB',
    scale: 'raw',
    source: 'cells',
    evidenceHref: '/inventory',
    evidenceLabel: 'Inventory',
    queryKey: 'chart-common-knowledge-composition',
    fetch: api.getCommonKnowledgeCompositionChart,
    related: [
      'knowledge-size',
      'most-utilized-scripts',
      'most-utilized-assets',
      'cell-size-distribution',
    ],
  },
  {
    slug: 'most-utilized-scripts',
    title: 'Scripts',
    subtitle: 'Share of common knowledge, top scripts',
    cardSubtitle: 'largest script',
    plotUnit: 'CKB',
    section: section.knowledge,
    visual: 'ranked',
    unit: 'CKB',
    scale: 'shannons',
    source: 'scripts',
    evidenceHref: '/scripts',
    evidenceLabel: 'Scripts',
    queryKey: 'chart-most-utilized-scripts',
    fetch: api.getMostUtilizedScriptsChart,
    related: [
      'most-utilized-assets',
      'common-knowledge-composition',
      'knowledge-size',
      'cell-size-distribution',
    ],
  },
  {
    slug: 'most-utilized-assets',
    title: 'Assets',
    subtitle: 'Share of common knowledge, top assets',
    cardSubtitle: 'largest asset',
    plotUnit: 'CKB',
    section: section.knowledge,
    visual: 'ranked',
    unit: 'CKB',
    scale: 'shannons',
    source: 'assets',
    evidenceHref: '/inventory',
    evidenceLabel: 'Inventory',
    queryKey: 'chart-most-utilized-assets',
    fetch: api.getMostUtilizedAssetsChart,
    related: [
      'most-utilized-scripts',
      'common-knowledge-composition',
      'knowledge-size',
      'hodl-wave',
    ],
  },
  {
    slug: 'cell-size-distribution',
    title: 'Cell size',
    subtitle: 'Live cells by occupied size, CKB',
    cardSubtitle: 'live cells, most common size',
    plotUnit: 'live cells, by occupied size',
    section: section.knowledge,
    visual: 'bars',
    unit: 'cells',
    scale: 'raw',
    source: 'cells',
    evidenceHref: '/inventory',
    evidenceLabel: 'Inventory',
    queryKey: 'chart-cell-size-distribution',
    fetch: api.getCellSizeDistributionChart,
    secondary: {
      unit: 'CKB',
      label: 'Common knowledge',
      caption: 'CKB occupied, by cell size',
      note: 'CKB occupied by the live cells in each size bucket.',
    },
    related: [
      'knowledge-size',
      'cell-count',
      'common-knowledge-composition',
      'most-utilized-scripts',
    ],
  },
  {
    slug: 'capacity-turnover-ratio',
    title: 'Capacity turnover',
    subtitle: 'Weekly share of live capacity moved',
    cardSubtitle: 'weekly, of live capacity',
    plotUnit: '% of live capacity moved',
    logScale: true,
    section: section.knowledge,
    visual: 'line',
    unit: '%',
    scale: 'raw',
    source: 'cells',
    evidenceHref: '/inventory',
    evidenceLabel: 'Inventory',
    queryKey: 'chart-capacity-turnover-ratio',
    fetch: api.getCapacityTurnoverRatioChart,
    primaryField: 'value2',
    seriesNames: { primary: 'Weekly turnover', companion: 'Daily turnover' },
    related: ['knowledge-size', 'ckb-volume', 'hodl-wave', 'transaction-count'],
  },
  {
    slug: 'address-cohort-retention',
    title: 'Cohort retention',
    subtitle: 'Common knowledge / balance, % by first-seen month',
    cardSubtitle: 'occupied share, last cohort',
    plotUnit: '% of its balance occupied',
    section: section.knowledge,
    visual: 'bars',
    unit: '%',
    scale: 'raw',
    source: 'addresses',
    evidenceHref: '/activities',
    evidenceLabel: 'Address activity',
    queryKey: 'chart-address-cohort-retention',
    fetch: api.getAddressCohortRetentionChart,
    secondary: {
      unit: 'CKB',
      label: 'Common knowledge',
      caption: 'CKB occupied, by first-seen month',
      note: 'CKB occupied today by the addresses first seen in each month.',
    },
    related: ['active-addresses', 'hodl-wave', 'knowledge-size', 'common-knowledge-composition'],
  },
  {
    slug: 'hash-rate',
    title: 'Hash rate',
    subtitle: 'Daily network estimate, PH/s',
    cardSubtitle: 'daily estimate',
    plotUnit: 'PH/s',
    section: section.work,
    visual: 'line',
    unit: 'PH/s',
    scale: 'hash-rate',
    source: 'blocks',
    evidenceHref: '/blocks',
    evidenceLabel: 'Blocks',
    queryKey: 'chart-hash-rate',
    fetch: api.getHashRateChart,
    related: ['difficulty', 'uncle-rate', 'average-block-time', 'epoch-time-length'],
  },
  {
    slug: 'difficulty',
    title: 'Difficulty',
    subtitle: 'Daily average difficulty, P',
    cardSubtitle: 'daily average',
    plotUnit: 'P',
    section: section.work,
    visual: 'line',
    unit: 'P',
    scale: 'difficulty',
    source: 'blocks',
    evidenceHref: '/blocks',
    evidenceLabel: 'Blocks',
    queryKey: 'chart-difficulty',
    fetch: api.getDifficultyChart,
    related: ['hash-rate', 'uncle-rate', 'average-block-time', 'epoch-time-length'],
  },
  {
    slug: 'uncle-rate',
    title: 'Uncle rate',
    subtitle: 'Daily, %, against the consensus target',
    cardSubtitle: 'daily, against its target',
    plotUnit: '% of blocks',
    section: section.work,
    visual: 'line',
    unit: '%',
    scale: 'fraction-percent',
    source: 'blocks',
    evidenceHref: '/blocks',
    evidenceLabel: 'Blocks',
    queryKey: 'chart-uncle-rate',
    fetch: api.getUncleRateChart,
    related: ['hash-rate', 'difficulty', 'block-time-distribution', 'miner-address-distribution'],
  },
  {
    slug: 'miner-address-distribution',
    title: 'Miners',
    subtitle: 'Share of blocks mined over the last 7 days',
    cardSubtitle: 'largest',
    plotUnit: 'blocks',
    section: section.work,
    visual: 'ranked',
    unit: 'blocks',
    scale: 'raw',
    source: 'blocks',
    evidenceHref: '/blocks',
    evidenceLabel: 'Blocks',
    queryKey: 'chart-miner-address-distribution',
    fetch: api.getMinerAddressDistributionChart,
    related: ['hash-rate', 'difficulty', 'uncle-rate', 'average-block-time'],
  },
  {
    slug: 'average-block-time',
    title: 'Block time',
    subtitle: 'Daily average, seconds',
    cardSubtitle: 'daily average',
    plotUnit: "s, the day's average",
    section: section.blocks,
    visual: 'line',
    unit: 's',
    scale: 'ms-seconds',
    source: 'blocks',
    evidenceHref: '/blocks',
    evidenceLabel: 'Blocks',
    queryKey: 'chart-average-block-time',
    fetch: api.getAverageBlockTimeChart,
    related: ['block-time-distribution', 'epoch-time-length', 'hash-rate', 'difficulty'],
  },
  {
    slug: 'epoch-time-length',
    title: 'Epoch length',
    subtitle: 'Hours each completed epoch took',
    cardSubtitle: 'hours an epoch took',
    plotUnit: 'h each epoch took',
    section: section.blocks,
    visual: 'line',
    unit: 'h',
    scale: 'ms-hours',
    source: 'blocks',
    evidenceHref: '/blocks',
    evidenceLabel: 'Blocks',
    queryKey: 'chart-epoch-time-length',
    fetch: api.getEpochTimeLengthChart,
    secondary: {
      unit: 'blocks',
      label: 'Epoch blocks',
      caption: 'Blocks, per epoch',
      note: 'Blocks in each completed epoch.',
    },
    related: ['epoch-time-distribution', 'average-block-time', 'difficulty', 'hash-rate'],
  },
  {
    slug: 'block-time-distribution',
    title: 'Block intervals',
    // the route clamps the window to the complete epochs there are; the head names how many (S4)
    subtitle: 'Share of blocks by seconds since their parent, up to the last 42 complete epochs',
    cardSubtitle: 'most common interval',
    plotUnit: '% of blocks, by seconds since their parent',
    section: section.blocks,
    visual: 'bars',
    unit: '%',
    scale: 'raw',
    source: 'blocks',
    evidenceHref: '/blocks',
    evidenceLabel: 'Blocks',
    queryKey: 'chart-block-time-distribution',
    fetch: api.getBlockTimeDistributionChart,
    related: ['average-block-time', 'epoch-time-distribution', 'uncle-rate', 'hash-rate'],
  },
  {
    slug: 'epoch-time-distribution',
    title: 'Epoch durations',
    subtitle: 'Completed epochs by hours taken',
    cardSubtitle: 'most common duration',
    plotUnit: 'epochs, by hours taken',
    section: section.blocks,
    visual: 'bars',
    unit: 'epochs',
    scale: 'raw',
    source: 'blocks',
    evidenceHref: '/blocks',
    evidenceLabel: 'Blocks',
    queryKey: 'chart-epoch-time-distribution',
    fetch: api.getEpochTimeDistributionChart,
    related: ['epoch-time-length', 'block-time-distribution', 'average-block-time', 'difficulty'],
  },
] as const;

export const CHART_SECTIONS = Object.values(section);
/** The in-page anchor of a chart section (CH-10). */
export function chartSectionId(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '');
}
export const CHART_BY_SLUG = new Map(CHARTS.map((chart) => [chart.slug, chart]));
/** The index lists every chart page, each in its section (CH-22: none is left out). */
export const INDEX_CHARTS = CHARTS;

export function chartApiPath(definition: ChartDefinition): string {
  if (
    ['activity-volume', 'activity-type-breakdown', 'active-addresses', 'ckb-volume'].includes(
      definition.slug
    )
  ) {
    return '/stats/daily-activities?days=0';
  }
  if (definition.queryKey.startsWith('dao-')) return `/dao/charts/${definition.slug}`;
  return `/charts/${definition.slug}`;
}
