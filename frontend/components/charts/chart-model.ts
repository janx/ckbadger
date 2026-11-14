import type { ChartPoint, ChartSeries } from '@/components/ui/chart';
import type {
  ChartDefinition,
  ChartPayload,
  ChartScale,
} from '@/components/charts/chart-catalogue';
import {
  exactDecimalRatio,
  exactSignedDecimalRatio,
  exactUnsignedInteger,
  formatCompactRatio,
  formatExactDecimal,
  formatScaledInteger,
} from '@/lib/exact-integer';
import { MINUS, niceStep, stepDecimals } from '@/lib/chart-ticks';
import {
  cellSizeBucketLabel,
  formatDateWindow,
  percentFigure,
  percentOf,
  withUnit,
} from '@/lib/format-unit';
import { formatCount } from '@/lib/format-count';
import {
  ACTIVITY_KIND_COLORS,
  CLASS_COLORS,
  DATA_COLOR,
  jadeRamp,
  type AssetClass,
} from '@/lib/design-tokens';
import { shortenAddress, shortenHash } from '@/lib/shorten';
import { formatPercent } from '@/lib/ckb-decimal';

export interface RankedPreviewRow {
  /** Short display label; `title` keeps the full identity. */
  label: string;
  title: string;
  share: string;
  /** The bar's width, % of the whole from the exact parts (never read back from `share`). */
  width: number;
  /** The asset class the API's label names (Assets ranking), drawn as its mark (CH-06). */
  assetClass?: AssetClass;
  /** Lane L: the entity's page (the same link as the full ranking's row); none for "Others". */
  href?: string;
}

// --- F2 charts group ---
/** A card headline: a small qualifier over one rounded number (CH-07, CH-09, CH-31, CH-33). */
export interface ChartHeadline {
  pre: string;
  display: string;
  unit: string;
}
export interface ChartLegendEntry {
  key: string;
  label: string;
  color: string;
  /** The entry's reading on the last returned day, with its unit. */
  value: string;
}

export interface ChartFact {
  label: string;
  /** A named leader (a miner, a script), written first and never broken (CH-03, CH-18). */
  name?: string;
  value: string;
}

/**
 * A chart drawn under the main one (CH-04, CH-06): named once by its `title` (the section's
 * command line), `note` says in plain words what is counted, and `caption` is its own quantity and
 * unit for the chart's caption — never the page's subtitle.
 */
export interface SubChart {
  title: string;
  caption: string;
  note: string;
  points: ChartPoint[];
  series: ChartSeries[];
  unit: string;
  /**
   * Its figure (CH-05): the point it reads, labelled the way the head figure is — the date, the
   * epoch, the cohort, or a distribution's largest bucket by name. A history keyed by many bands
   * has none (its legend reads them).
   */
  reading?: { label: string; point: ChartPoint };
}

/** A dated sub-chart read at its last day, the head's date (CH-05). */
function withLastReading(sub: SubChart): SubChart {
  const last = sub.points.at(-1);
  return last ? { ...sub, reading: { label: last.label, point: last } } : sub;
}

/**
 * The HODL wave's age bands as ranges (CH-14, CH-21): an en dash with spaces, the youngest band
 * `< 1 d` (the API's "24h" read as a duration among ranges).
 */
const SERIES_NAMES: Readonly<Record<string, Readonly<Record<string, string>>>> = {
  'hodl-wave': {
    gt3y: '> 3 y',
    '1y3y': '1 y – 3 y',
    '6m1y': '6 m – 1 y',
    '3m6m': '3 m – 6 m',
    '1m3m': '1 m – 3 m',
    '1w1m': '1 w – 1 m',
    '1d1w': '1 d – 1 w',
    '24h': '< 1 d',
  },
};

/** A stacked series' name: the page's own where it has one (an unknown band fails), else the API's. */
function seriesName(slug: string, entry: { key: string; label: string }): string {
  const names = SERIES_NAMES[slug];
  if (!names) return entry.label;
  const name = names[entry.key];
  if (name === undefined) throw new Error(`${slug}: unknown band ${entry.key}`);
  return name;
}

/** The point whose `key` value is largest; the first of equals. */
function largestPoint(points: readonly ChartPoint[], key: string, context: string): ChartPoint {
  if (!points.length) throw new Error(`${context}: no points to read`);
  return points.reduce((best, point) => {
    const [n, d] = exactSignedDecimalRatio(point.values[key], `${context} ${point.label}`);
    const [bn, bd] = exactSignedDecimalRatio(best.values[key], `${context} ${best.label}`);
    return n * bd > bn * d ? point : best;
  });
}

/** A sub-chart's figure at the point labelled `label`, written as the head writes it (CH-05). */
function readingAt(
  sub: SubChart,
  label: string,
  style: AxisStyle,
  slug: string
): NonNullable<SubChart['reading']> {
  const point = sub.points.find((entry) => entry.label === label);
  if (!point) throw new Error(`${slug}: ${sub.title} has no point at ${label}`);
  return { label: style === 'epoch' ? `#${formatEpochLabel(label, slug)}` : label, point };
}

export interface ChartModel {
  title: string;
  points: ChartPoint[];
  series: ChartSeries[];
  unit: string;
  kind: 'line' | 'bars' | 'stacked' | 'ranked';
  /** Present only when the API actually provides a second physical unit. */
  secondary?: SubChart;
  latest: { label: string; raw: string; display: string } | null;
  /** Exact, signed change between the final two returned points. */
  delta: string | null;
  rankedRows?: {
    label: string;
    value: string;
    share: string;
    /** The bar's width, % of the whole from the exact parts (never read back from `share`). */
    width: number;
    href?: string;
    /** The full identity a shortened label stands for, as the link's hover title (CH-03). */
    title?: string;
    /** A miner known only by its lock hash: the ranking says so once, under the list (CH-03). */
    lockHashOnly?: boolean;
    /** The API's remainder past its named ranks: listed last, without a rank number (CH-21). */
    remainder?: boolean;
    assetClass?: AssetClass;
  }[];
  /** Card preview of a ranking: the top five plus an exact "Others" for every remaining rank. */
  rankedPreview?: { rows: RankedPreviewRow[] };
  /** Exact signed day-over-day change in occupied CKB, when the source is knowledge size. */
  dailyFlow?: SubChart;
  /** Exact daily histories from both distinct utilization scopes. */
  historicalShares?: SubChart[];
  /** A model timeline's "today" index: drawn solid up to it and dashed ahead (CH-03). */
  projectedAfter?: number;
  /**
   * A model timeline's main reading: the API's `today` point (lane R8), or `null` while the API
   * does not send one, so the detail chart shows no reading rather than the last (future) sample.
   * Absent on every other chart.
   */
  today?: ChartPoint | null;
  /** Developer facts, in the page's evidence tier, never under the lede (CH-04, CH-18). */
  evidence?: ChartFact[];
  /**
   * Facts under the lede that the head figure does not say (CH-18): a leader's `name` is the bright
   * part of the value, kept on one line (CH-03).
   */
  facts?: ChartFact[];
  // --- F2 charts group ---
  /** The card's headline; `null` when the source has no reading to head it (a model's today). */
  headline?: ChartHeadline | null;
  /** What the index card draws when it summarises the detail: fewer bands, a legend (CH-07). */
  card?: { points: ChartPoint[]; series: ChartSeries[]; legend: ChartLegendEntry[] };
  /** A fixed value domain: a share chart runs 0–100 (CH-15). */
  yDomain?: readonly [number, number];
  /** Dashed reference values drawn across the plot, e.g. the consensus target (CH-12). */
  referenceLines?: { value: number; label: string }[];
  /**
   * The point the head reads when it is not the last: a histogram's most common bucket, the last
   * complete cohort (CH-19); the chart reads it while idle.
   */
  reading?: ChartPoint;
  /** The API's peak of the drawn series, in display units (CH-21); absent where none applies. */
  peak?: { label: string; raw: string } | null;
}

function decimalByPower(value: string, power: number, context: string): string {
  const n = exactUnsignedInteger(value, context);
  const divisor = BigInt(10) ** BigInt(power);
  const whole = n / divisor;
  const fraction = (n % divisor).toString().padStart(power, '0').replace(/0+$/, '');
  return fraction ? `${whole}.${fraction}` : whole.toString();
}

function multiplyDecimal(value: string, factor: bigint, context: string): string {
  const [numerator, denominator] = exactDecimalRatio(value, context);
  const result = numerator * factor;
  const whole = result / denominator;
  const fractionLength = denominator.toString().length - 1;
  const fraction = (result % denominator)
    .toString()
    .padStart(fractionLength, '0')
    .replace(/0+$/, '');
  return fraction ? `${whole}.${fraction}` : whole.toString();
}

export function chartUnitValue(value: string, scale: ChartScale, context: string): string {
  switch (scale) {
    case 'raw':
      exactSignedDecimalRatio(value, context);
      return value;
    case 'shannons':
      return decimalByPower(value, 8, context);
    case 'hash-rate':
    case 'difficulty':
      return decimalByPower(value, 15, context);
    case 'fraction-percent':
      return multiplyDecimal(value, BigInt(100), context);
  }
}

function exactDelta(a: string, b: string, context: string): string {
  const [an, ad] = exactSignedDecimalRatio(a, `${context} previous`);
  const [bn, bd] = exactSignedDecimalRatio(b, `${context} latest`);
  const n = bn * ad - an * bd;
  const d = bd * ad;
  if (n === BigInt(0)) return '0';
  const negative = n < BigInt(0);
  const abs = negative ? -n : n;
  // All chart value divisors are powers of ten, so their product is too.
  const digits = d.toString().length - 1;
  const whole = abs / d;
  const fraction = (abs % d).toString().padStart(digits, '0').replace(/0+$/, '');
  return `${negative ? '-' : '+'}${whole}${fraction ? `.${fraction}` : ''}`;
}

/** Units whose readouts are rates or durations: the design shows them at two decimals. */
const RATE_UNITS = new Set(['%', 'PH/s', 'P', 'h', 's']);
/** Units that count discrete things; an exact whole count is shown without decimals. */
const COUNT_UNITS = new Set([
  'activities',
  'addresses',
  'blocks',
  'cells',
  'depositors',
  'epochs',
  'holders',
  'transactions',
]);

/**
 * The rounded chart readout of a card headline, card "Last change" and the hover readout (CH-02):
 * rates at two decimals, exact whole counts as integers, everything else compact at two decimals.
 * A leading `+` (an exact delta) is kept. The exact value is `formatChartExact`.
 */
export function formatChartReadout(value: string, unit: string): string {
  const plus = value.startsWith('+');
  const [numerator, denominator] = exactSignedDecimalRatio(
    plus ? value.slice(1) : value,
    `chart ${unit} readout`
  );
  const negative = numerator < BigInt(0);
  const abs = negative ? -numerator : numerator;
  const shown = RATE_UNITS.has(unit)
    ? formatExactDecimal(formatScaledInteger(abs, denominator, 2), `chart ${unit} readout`)
    : // A whole count under 100,000 reads exactly, grouped, as the Charts board shows it ("16,826").
      COUNT_UNITS.has(unit) && abs % denominator === BigInt(0) && abs < denominator * BigInt(100000)
      ? formatExactDecimal((abs / denominator).toString(), `chart ${unit} readout`)
      : formatCompactRatio(abs, denominator);
  // A value that rounds to zero has no sign ("0.00", never "-0.00"); a negative one takes the
  // minus sign (CH-21).
  const sign = /^[0.,]+[A-Z]?$/.test(shown) ? '' : negative ? MINUS : plus ? '+' : '';
  return `${sign}${shown}`;
}

/** The exact chart value, every returned digit, thousands grouped, a negative's minus sign (CH-21). */
export function formatChartExact(value: string, unit: string): string {
  return formatExactDecimal(value, `chart ${unit} exact value`).replace(/^-/, MINUS);
}

/** Detail-page hover readout: rates at the design's two decimals, every other value exact. */
export function formatChartHover(value: string, unit: string): string {
  return RATE_UNITS.has(unit) ? formatChartReadout(value, unit) : formatChartExact(value, unit);
}

function summary(value: string, unit: string): string {
  return formatChartReadout(value, unit);
}

function singleChart(
  definition: ChartDefinition,
  payload: {
    title: string;
    yAxisLabel: string;
    y2AxisLabel?: string;
    data: { date: string; value: string; value2?: string | null }[];
    today?: { blockNumber: number; date: string; year: string; value: string };
    peak?: { date: string; value: string } | null;
    peak2?: { date: string; value: string } | null;
    target?: { seconds: number; hours: string };
  }
): ChartModel {
  const primaryField = definition.primaryField ?? 'value';
  const companionField = primaryField === 'value' ? 'value2' : 'value';
  const timelineMarkers = payload.y2AxisLabel === 'nominal-apc-timeline';
  const hasSecond =
    !timelineMarkers &&
    payload.data.some((point) => point.value2 !== undefined && point.value2 !== null);
  if (
    hasSecond &&
    payload.data.some((point) => point.value2 === undefined || point.value2 === null)
  ) {
    throw new Error(`${definition.slug}: secondary series is missing at a returned point`);
  }
  // Cell size names its buckets in the axis' words (CH-12); every other label is the API's own.
  const pointLabel = (date: string) =>
    definition.slug === 'cell-size-distribution' ? cellSizeBucketLabel(date) : date;
  const points: ChartPoint[] = payload.data.map((point) => {
    const primary = point[primaryField];
    if (primary === undefined || primary === null)
      throw new Error(`${definition.slug}: missing ${primaryField} at ${point.date}`);
    const values: Record<string, string> = {
      primary: chartUnitValue(primary, definition.scale, `${definition.slug} at ${point.date}`),
    };
    if (hasSecond && !definition.secondary) {
      const companion = point[companionField];
      if (companion === undefined || companion === null)
        throw new Error(`${definition.slug}: missing ${companionField} at ${point.date}`);
      values.companion = chartUnitValue(
        companion,
        definition.scale,
        `${definition.slug} companion at ${point.date}`
      );
    }
    return { label: pointLabel(point.date), values };
  });
  const currentIndex = timelineMarkers ? timelineCurrentIndex(definition.slug, payload.data) : null;
  // Two series of one unit are named in the page's words (CH-21); a companion with no names fails.
  const companionNames = hasSecond && !definition.secondary ? definition.seriesNames : undefined;
  if (hasSecond && !definition.secondary && !companionNames)
    throw new Error(`${definition.slug}: a companion series with no names`);
  const series: ChartSeries[] = [
    {
      key: 'primary',
      label: companionNames?.primary ?? payload.yAxisLabel,
      color: 'var(--c-jade)',
    },
  ];
  if (hasSecond && !definition.secondary)
    series.push({
      key: 'companion',
      label: companionNames!.companion,
      color: 'var(--c-aqua)',
      // A companion reading of the same unit (real inflation, daily turnover) is dashed (CH-12).
      dashed: true,
    });
  const secondary: SubChart | undefined =
    hasSecond && definition.secondary
      ? {
          points: payload.data.map((point) => ({
            label: pointLabel(point.date),
            values: {
              secondary: chartUnitValue(
                point.value2!,
                'raw',
                `${definition.slug} secondary at ${point.date}`
              ),
            },
          })),
          series: [{ key: 'secondary', label: definition.secondary.label, color: 'var(--c-aqua)' }],
          unit: definition.secondary.unit,
          title: definition.secondary.label,
          caption: definition.secondary.caption,
          note: definition.secondary.note,
        }
      : undefined;
  const last = points.at(-1);
  const previous = points.at(-2);
  const bucketDistribution = BUCKET_DISTRIBUTIONS.includes(definition.slug);
  // Cohorts are ordered by first-seen month: the card reads the newest one, not the largest (CH-09).
  const cohorts = definition.slug === 'address-cohort-retention';
  const leadingBucket =
    bucketDistribution && points.length
      ? largestPoint(points, 'primary', `${definition.slug} bucket`)
      : null;
  // A model timeline is summarised at its "today" sample; any other series at its last point.
  const todayPoint = currentIndex === null ? null : points[currentIndex];
  // The newest cohort is the month the snapshot falls in, still filling; any older cohort is
  // complete, since a newer one exists. The head reads the last complete cohort (CH-19).
  const completeCohort = cohorts ? (points.at(-2) ?? null) : null;
  const summaryPoint = leadingBucket ?? todayPoint ?? completeCohort ?? last;
  const today =
    timelineMarkers && payload.today
      ? {
          label: `year ${modelYear(payload.today.year, definition.slug)}`,
          values: {
            primary: chartUnitValue(
              payload.today.value,
              definition.scale,
              `${definition.slug} today at ${payload.today.date}`
            ),
          },
        }
      : null;
  const apiPeak = primaryField === 'value2' ? payload.peak2 : payload.peak;
  const model: ChartModel = {
    title: payload.title,
    points,
    series,
    unit: definition.unit,
    kind: definition.visual === 'bars' ? 'bars' : 'line',
    secondary,
    dailyFlow:
      definition.slug === 'knowledge-size'
        ? {
            title: 'Net knowledge flow',
            caption: 'Change in occupied CKB, per day',
            note: 'Occupied CKB gained since the previous day; below zero, CKB freed.',
            unit: 'CKB/day',
            series: [{ key: 'flow', label: 'Daily net flow', color: 'var(--c-aqua)' }],
            points: points.slice(1).map((point, index) => ({
              label: point.label,
              values: {
                flow: exactDelta(
                  points[index].values.primary,
                  point.values.primary,
                  `knowledge size at ${point.label}`
                ).replace(/^\+/, ''),
              },
            })),
          }
        : undefined,
    latest: summaryPoint
      ? {
          label: todayPoint
            ? `year ${summaryPoint.label}`
            : cohorts && !completeCohort
              ? `${summaryPoint.label}, so far`
              : summaryPoint.label,
          raw: summaryPoint.values.primary,
          display: summary(summaryPoint.values.primary, definition.unit),
        }
      : null,
    // Neighbouring samples of a model curve or of a bucket histogram are not a change over time.
    delta:
      bucketDistribution || cohorts || todayPoint
        ? null
        : last && previous
          ? exactDelta(previous.values.primary, last.values.primary, definition.slug)
          : null,
    reading: leadingBucket ?? completeCohort ?? undefined,
    projectedAfter: currentIndex ?? undefined,
    today: timelineMarkers ? today : undefined,
    // A bucket histogram's peak is its most common bucket, already the headline; a model has none.
    peak:
      bucketDistribution || cohorts || timelineMarkers || apiPeak === undefined
        ? undefined
        : apiPeak === null
          ? null
          : {
              label: apiPeak.date,
              raw: chartUnitValue(apiPeak.value, definition.scale, `${definition.slug} peak`),
            },
  };
  // A figure under the main chart reads where the head does, labelled alike; a distribution's
  // at its own largest bucket, by name, never its last (CH-05).
  const style = axisStyle(definition);
  if (secondary?.points.length && summaryPoint)
    secondary.reading = bucketDistribution
      ? (() => {
          const point = largestPoint(secondary.points, 'secondary', `${definition.slug} secondary`);
          return { label: `largest, ${point.label}`, point };
        })()
      : readingAt(secondary, summaryPoint.label, style, definition.slug);
  const flow = model.dailyFlow;
  if (flow?.points.length)
    flow.reading = readingAt(flow, flow.points.at(-1)!.label, 'date', definition.slug);
  // A model timeline's headline is the API's today, at its exact age (CH-03, CH-33).
  if (timelineMarkers && today) {
    model.latest = {
      label: today.label,
      raw: today.values.primary,
      display: summary(today.values.primary, definition.unit),
    };
    model.headline = { pre: today.label, display: model.latest.display, unit: definition.unit };
  } else if (timelineMarkers) model.headline = null;
  else if (leadingBucket) model.headline = bucketHeadline(definition, points, leadingBucket);
  else if (cohorts && model.latest)
    model.headline = {
      pre: model.latest.label,
      display: model.latest.display,
      unit: definition.unit,
    };
  else if (definition.slug === 'epoch-time-length' && last)
    model.headline = {
      pre: `#${formatEpochLabel(last.label, definition.slug)}`,
      display: summary(last.values.primary, definition.unit),
      unit: definition.unit,
    };
  // The consensus epoch-duration target, from the API (CH-12, R2b-b).
  if (payload.target) {
    const hours = Number(payload.target.hours);
    if (!Number.isFinite(hours) || hours <= 0)
      throw new Error(`${definition.slug}: invalid epoch duration target ${payload.target.hours}`);
    model.referenceLines = [{ value: hours, label: `target ${payload.target.hours} h` }];
  }
  // The weekly share alone on the card; the daily share stays on the detail (CH-12).
  if (definition.slug === 'capacity-turnover-ratio')
    model.card = { points, series: series.filter((entry) => entry.key === 'primary'), legend: [] };
  return model;
}

/**
 * The nominal-APC timeline marks every sample past, current or future relative to the tip year.
 * Exactly one `current` sample, after every past one and before every future one, is the contract;
 * anything else fails here rather than summarising some other year (CH-03).
 */
function timelineCurrentIndex(
  slug: string,
  data: readonly { date: string; value2?: string | null }[]
): number | null {
  if (data.length === 0) return null;
  let current: number | null = null;
  for (const [index, point] of data.entries()) {
    const marker = point.value2;
    if (marker !== 'past' && marker !== 'current' && marker !== 'future')
      throw new Error(
        `${slug}: invalid timeline marker ${JSON.stringify(marker)} at ${point.date}`
      );
    if (marker === 'current') {
      if (current !== null) throw new Error(`${slug}: second current sample at ${point.date}`);
      current = index;
    } else if ((marker === 'past') !== (current === null))
      throw new Error(`${slug}: ${marker} sample at ${point.date} is out of timeline order`);
  }
  if (current === null) throw new Error(`${slug}: timeline has no current sample`);
  return current;
}

type StackedPayload = {
  title: string;
  data: {
    date: string;
    values: Record<string, string>;
    protocolTotalShannons?: string;
    phase1CompensationShannons?: string;
  }[];
  series: { key: string; label: string; color: string }[];
  peaks?: Record<string, { date: string; value: string }>;
};

function stackedChart(definition: ChartDefinition, payload: StackedPayload): ChartModel {
  if (definition.slug === 'cell-count') return liveCellsChart(definition, payload);
  const classes = CLASSED_SERIES[definition.slug];
  const selectedSeries = classes
    ? classedSeries(definition.slug, classes, payload)
    : payload.series;
  if (!selectedSeries.length) throw new Error(`${definition.slug}: no chart series`);
  const points = payload.data.map((point) => ({
    label: point.date,
    values: Object.fromEntries(
      selectedSeries.map((series) => {
        const raw = point.values[series.key];
        if (raw === undefined)
          throw new Error(`${definition.slug}: missing ${series.key} at ${point.date}`);
        return [
          series.key,
          chartUnitValue(
            raw,
            definition.scale,
            `${definition.slug} ${series.key} at ${point.date}`
          ),
        ];
      })
    ),
  }));
  // Asset classes keep their colour wherever a chart splits by them, gold only for DAO; every
  // other split shares the jade ramp (CH-06, CH-14).
  const ramp = jadeRamp(selectedSeries.length);
  const series: ChartSeries[] = selectedSeries.map((entry, index) =>
    classes
      ? { key: entry.key, label: classes[entry.key].label, color: classes[entry.key].color }
      : { key: entry.key, label: seriesName(definition.slug, entry), color: ramp[index] }
  );
  const last = points.at(-1);
  const previous = points.at(-2);
  const total = (point: ChartPoint): string =>
    exactSum(
      selectedSeries.map((item) => point.values[item.key]),
      definition.slug
    );
  const isComposition = definition.slug === 'hodl-wave';
  const isSecondaryIssuance = definition.slug === 'secondary-issuance';
  const protocolTotal = (point: (typeof payload.data)[number]): string => {
    if (point.protocolTotalShannons === undefined || point.phase1CompensationShannons === undefined)
      throw new Error(
        `${definition.slug}: missing protocol total or frozen phase-1 evidence at ${point.date}`
      );
    chartUnitValue(
      point.phase1CompensationShannons,
      'shannons',
      `${definition.slug} phase-1 at ${point.date}`
    );
    return chartUnitValue(
      point.protocolTotalShannons,
      'shannons',
      `${definition.slug} protocol total at ${point.date}`
    );
  };
  if (isSecondaryIssuance) payload.data.forEach(protocolTotal);
  const leadingAge =
    isComposition && last
      ? selectedSeries.reduce(
          (best, entry) => {
            const [currentNumerator, currentDenominator] = exactDecimalRatio(
              last.values[entry.key],
              `${definition.slug} ${entry.key}`
            );
            if (!best) return entry;
            const [bestNumerator, bestDenominator] = exactDecimalRatio(
              last.values[best.key],
              `${definition.slug} ${best.key}`
            );
            return currentNumerator * bestDenominator > bestNumerator * currentDenominator
              ? entry
              : best;
          },
          null as (typeof selectedSeries)[number] | null
        )
      : null;
  const lastTotal = last
    ? isSecondaryIssuance
      ? protocolTotal(payload.data.at(-1)!)
      : leadingAge
        ? last.values[leadingAge.key]
        : total(last)
    : null;
  const previousTotal = previous
    ? isSecondaryIssuance
      ? protocolTotal(payload.data.at(-2)!)
      : total(previous)
    : null;
  const holderCount = isComposition && payload.data.length > 0;
  if (holderCount && payload.data.some((point) => point.values.holderCount === undefined))
    throw new Error(`${definition.slug}: missing holderCount at returned point`);
  const model: ChartModel = {
    title: payload.title,
    points,
    series,
    unit: definition.unit,
    kind: 'stacked',
    secondary: holderCount
      ? withLastReading({
          title: 'Holder count',
          caption: 'Holders, per day',
          note: 'Addresses holding a positive balance at the end of each day.',
          unit: 'holders',
          series: [{ key: 'holders', label: 'Holders', color: 'var(--c-aqua)' }],
          points: payload.data.map((point) => ({
            label: point.date,
            values: {
              holders: chartUnitValue(
                point.values.holderCount,
                'raw',
                `${definition.slug} holders at ${point.date}`
              ),
            },
          })),
        })
      : undefined,
    latest:
      last && lastTotal !== null
        ? { label: last.label, raw: lastTotal, display: summary(lastTotal, definition.unit) }
        : null,
    delta: isComposition
      ? null
      : last && previous && previousTotal !== null && lastTotal !== null
        ? exactDelta(previousTotal, lastTotal, definition.slug)
        : null,
    // The head reads the largest band's share; the fact names it (CH-18).
    facts:
      leadingAge && last
        ? [
            {
              label: 'Largest age band',
              name: series.find((entry) => entry.key === leadingAge.key)!.label,
              value: `${summary(last.values[leadingAge.key], '%')}%`,
            },
          ]
        : undefined,
    // The frozen phase-1 overlap explains the protocol total for developers (CH-18).
    evidence:
      isSecondaryIssuance && payload.data.length
        ? [
            {
              label: 'Frozen phase-1 overlap',
              // Grouped like the head figure (CH-34).
              value: `${formatChartExact(chartUnitValue(payload.data.at(-1)!.phase1CompensationShannons!, 'shannons', `${definition.slug} latest phase-1`), 'CKB')} CKB`,
            },
          ]
        : undefined,
  };
  // F2 charts group: what the index card shows of a stacked chart (CH-07, CH-15).
  if (definition.slug === 'hodl-wave') model.yDomain = [0, 100];
  if (!last) return model;
  const groups = CARD_GROUPS[definition.slug];
  if (groups) {
    const cardRamp = jadeRamp(groups.length);
    // A classed chart's card bands are its types, one each, in the page's colours (CH-02).
    const cardColor = (group: (typeof groups)[number], index: number): string => {
      if (!classes) return cardRamp[index];
      const [member, ...more] = group.members;
      if (more.length || !classes[member])
        throw new Error(`${definition.slug}: card band ${group.key} is not one classed type`);
      return classes[member].color;
    };
    const cardSeries = groups.map((group, index) => ({
      key: group.key,
      label: group.label,
      color: cardColor(group, index),
    }));
    const cardPoints = points.map((point) => ({
      label: point.label,
      values: Object.fromEntries(
        groups.map((group) => [
          group.key,
          exactSum(
            group.members.map((member) => {
              const value = point.values[member];
              if (value === undefined)
                throw new Error(`${definition.slug}: card group ${group.key} has no ${member}`);
              return value;
            }),
            `${definition.slug} ${group.key} at ${point.label}`
          ),
        ])
      ),
    }));
    const lastGroups = cardPoints.at(-1)!;
    // A percentage band already is its share; a count is read against the day's total.
    const share = (key: string) =>
      definition.unit === '%'
        ? summary(lastGroups.values[key], '%')
        : shareOf(lastGroups.values[key], total(last), `${definition.slug} ${key}`);
    model.card = {
      points: cardPoints,
      series: cardSeries,
      legend: cardSeries.map((entry) => ({
        key: entry.key,
        label: entry.label,
        color: entry.color,
        value: withUnit(share(entry.key), '%'),
      })),
    };
    model.headline = {
      pre: groups[0].label.toLowerCase(),
      display: share(groups[0].key),
      unit: '%',
    };
  } else {
    const legendValue = (key: string) =>
      definition.slug === 'common-knowledge-composition'
        ? withUnit(shareOf(last.values[key], total(last), `${definition.slug} ${key}`), '%')
        : withUnit(summary(last.values[key], definition.unit), definition.unit);
    model.card = {
      points,
      series,
      legend: series.map((entry) => ({
        key: entry.key,
        label: entry.label,
        color: entry.color!,
        value: legendValue(entry.key),
      })),
    };
    if (definition.slug === 'common-knowledge-composition')
      model.headline = {
        pre: series[0].label.toLowerCase(),
        display: shareOf(last.values[series[0].key], total(last), definition.slug),
        unit: '%',
      };
  }
  return model;
}

/**
 * Live cells as the one live count it is named for; the cumulative consumed count is a separate
 * chart on the detail, never stacked on top of it (CH-08). `allCells` is their sum and is not drawn.
 */
function liveCellsChart(definition: ChartDefinition, payload: StackedPayload): ChartModel {
  for (const key of ['liveCells', 'deadCells'])
    if (!payload.series.some((series) => series.key === key))
      throw new Error(`${definition.slug}: no ${key} series`);
  const read = (point: StackedPayload['data'][number], key: string) => {
    const raw = point.values[key];
    if (raw === undefined) throw new Error(`${definition.slug}: missing ${key} at ${point.date}`);
    return chartUnitValue(raw, definition.scale, `${definition.slug} ${key} at ${point.date}`);
  };
  const points = payload.data.map((point) => ({
    label: point.date,
    values: { liveCells: read(point, 'liveCells') },
  }));
  const last = points.at(-1);
  const previous = points.at(-2);
  const peak = payload.peaks?.liveCells;
  return {
    title: payload.title,
    points,
    series: [{ key: 'liveCells', label: 'Live cells', color: DATA_COLOR }],
    unit: definition.unit,
    kind: 'line',
    secondary: withLastReading({
      title: 'Consumed cells, cumulative',
      caption: 'Consumed cells, cumulative',
      note: 'Cells consumed since genesis, counted at the end of each day.',
      unit: 'cells',
      series: [{ key: 'secondary', label: 'Consumed cells', color: 'var(--c-aqua)' }],
      points: payload.data.map((point) => ({
        label: point.date,
        values: { secondary: read(point, 'deadCells') },
      })),
    }),
    latest: last
      ? {
          label: last.label,
          raw: last.values.liveCells,
          display: summary(last.values.liveCells, definition.unit),
        }
      : null,
    delta:
      last && previous
        ? exactDelta(previous.values.liveCells, last.values.liveCells, definition.slug)
        : null,
    peak:
      peak === undefined
        ? undefined
        : {
            label: peak.date,
            raw: chartUnitValue(peak.value, definition.scale, `${definition.slug} peak`),
          },
  };
}

const PREVIEW_RANKS = 5;

/** A share bar's width: % of the whole from the exact parts at two decimals; no whole, no bar. */
function barWidth(part: bigint, total: bigint): number {
  return total === BigInt(0) ? 0 : Number(formatPercent(part, total, 2));
}

/**
 * Top five ranked entries plus one "Others" row for the exact remainder of the total, so the card
 * never drops ranks six and beyond (CH-01). The remainder is computed from exact amounts, never
 * from rounded shares; a zero remainder (or zero total) has no "Others" row.
 */
function rankedPreviewRows(
  ranked: readonly {
    label: string;
    shortLabel: string;
    amount: bigint;
    share: string;
    href?: string;
    /** The full id a shortened label stands for; it joins the hover title (lane L). */
    fullId?: string;
  }[],
  total: bigint,
  remainderShare: (remainder: bigint) => string,
  context: string
): RankedPreviewRow[] {
  const top = ranked.slice(0, PREVIEW_RANKS);
  const shown = top.reduce((sum, entry) => sum + entry.amount, BigInt(0));
  const remainder = total - shown;
  if (remainder < BigInt(0))
    throw new Error(`${context}: top ${top.length} ranks exceed the returned total`);
  return [
    ...top.map((entry) => ({
      label: entry.shortLabel,
      title: entry.fullId ? `${entry.label} · ${entry.fullId}` : entry.label,
      share: entry.share,
      width: barWidth(entry.amount, total),
      ...(entry.href ? { href: entry.href } : {}),
    })),
    ...(remainder > BigInt(0)
      ? [
          {
            label: 'Others',
            title: 'Others',
            share: remainderShare(remainder),
            width: barWidth(remainder, total),
          },
        ]
      : []),
  ];
}

function shareRows(
  definition: ChartDefinition,
  payload: {
    usedShare: {
      data: { date: string; values: Record<string, string> }[];
      series: { key: string; label: string; color: string; id?: string }[];
    };
    capacityShare: {
      data: { date: string; values: Record<string, string> }[];
      series: { key: string; label: string; color: string; id?: string }[];
    };
  }
): ChartModel {
  const used = payload.usedShare;
  const last = used.data.at(-1);
  const entity = definition.source === 'scripts' ? 'script' : 'asset';
  // Each scope captioned by its own quantity, never the page's subtitle (CH-04).
  const histories: SubChart[] = [
    {
      key: 'usedShare',
      title: 'Occupied common knowledge',
      caption: `CKB occupied, by ${entity}`,
      note: `Occupied CKB in the live cells attributed to each ${entity}.`,
      payload: payload.usedShare,
    },
    {
      key: 'capacityShare',
      title: 'Owned live capacity',
      caption: `CKB owned, by ${entity}`,
      note: `The full capacity of the live cells attributed to each ${entity}, a different scope from occupied common knowledge.`,
      payload: payload.capacityShare,
    },
  ].map(({ key, title, caption, note, payload: history }) => ({
    title,
    caption,
    note,
    unit: 'CKB',
    series: history.series.map((series, index) => ({
      key: series.key,
      label: series.label,
      color: [
        'var(--c-jade)',
        'var(--c-aqua)',
        'var(--c-lavender)',
        'var(--c-jade-dim)',
        'var(--c-cell-lock)',
        'var(--c-cell-data)',
        'var(--c-flow-ckb)',
        'var(--c-text)',
      ][index % 8],
    })),
    points: history.data.map((point) => ({
      label: point.date,
      values: Object.fromEntries(
        history.series.map((series) => {
          const raw = point.values[series.key];
          if (raw === undefined)
            throw new Error(`${definition.slug} ${key}: missing ${series.key} at ${point.date}`);
          return [
            series.key,
            chartUnitValue(
              raw,
              'shannons',
              `${definition.slug} ${key} ${series.key} at ${point.date}`
            ),
          ];
        })
      ),
    })),
  }));
  if (!last)
    return {
      title: definition.title,
      points: [],
      series: [],
      unit: 'CKB',
      kind: 'ranked',
      latest: null,
      delta: null,
      rankedRows: [],
      historicalShares: histories,
    };
  const entries = used.series.map((series) => {
    const raw = last.values[series.key];
    if (raw === undefined)
      throw new Error(`${definition.slug}: missing ${series.key} at ${last.date}`);
    return {
      id: series.id,
      label: series.label,
      shannons: exactUnsignedInteger(raw, `${definition.slug} ${series.key}`),
      value: chartUnitValue(raw, 'shannons', `${definition.slug} ${series.key}`),
    };
  });
  if (!entries.length) throw new Error(`${definition.slug}: dated ranking has no series`);
  const total = entries.reduce((sum, entry) => sum + entry.shannons, BigInt(0));
  const share = (shannons: bigint) => (total === BigInt(0) ? '—' : percentOf(shannons, total));
  const sorted = [...entries].sort((a, b) =>
    a.shannons === b.shannons ? a.label.localeCompare(b.label) : a.shannons > b.shannons ? -1 : 1
  );
  const othersKey = used.series.find((series) => series.key === 'others')?.label;
  const named = sorted.filter((entry) => entry.label !== othersKey);
  const others = sorted.filter((entry) => entry.label === othersKey);
  // The API's own bucket holds the ranks beyond its named list; say so, the card's "Others" differs.
  // It is a remainder, not a rank: it closes the list instead of leading it (CH-29).
  // Two entities can share a name (two "MEMES" tokens): the id tells them apart (CH-29).
  const shownName = (entry: (typeof entries)[number]) =>
    definition.source === 'assets' ? assetLabel(entry.label).label : entry.label;
  const nameCount = new Map<string, number>();
  for (const entry of named)
    nameCount.set(shownName(entry), (nameCount.get(shownName(entry)) ?? 0) + 1);
  const distinctLabel = (entry: (typeof entries)[number]) =>
    (nameCount.get(shownName(entry)) ?? 0) > 1 && entry.id
      ? `${shownName(entry)} · ${shortenHash(fullSeriesId(entry.id), 'tight')}`
      : shownName(entry);
  const rankedRows = [...named, ...others].map((entry) =>
    entry.label === othersKey
      ? {
          label: `Others beyond the top ${named.length}`,
          // a remainder, not a rank: it is not numbered (CH-21)
          remainder: true,
          value: entry.value,
          share: share(entry.shannons),
          width: barWidth(entry.shannons, total),
        }
      : {
          label: distinctLabel(entry),
          value: entry.value,
          share: share(entry.shannons),
          width: barWidth(entry.shannons, total),
          ...(entry.id ? { href: rankedSeriesHref(entry.id, entry.label) } : {}),
          ...(definition.source === 'assets'
            ? { assetClass: assetLabel(entry.label).assetClass }
            : {}),
        }
  );
  const leader = named[0] ?? sorted[0];
  return {
    title: definition.title,
    points: [],
    series: [],
    unit: 'CKB',
    kind: 'ranked',
    latest: { label: last.date, raw: leader.value, display: summary(leader.value, 'CKB') },
    delta: null,
    rankedRows,
    rankedPreview: {
      rows: rankedPreviewRows(
        named.map((entry) => ({
          label: entry.label,
          shortLabel: distinctLabel(entry),
          amount: entry.shannons,
          share: share(entry.shannons),
          ...(entry.id
            ? {
                href: rankedSeriesHref(entry.id, entry.label),
                // a hash id joins the hover title; a family id is its name, already the label
                ...(/^0x/.test(fullSeriesId(entry.id)) ? { fullId: fullSeriesId(entry.id) } : {}),
              }
            : {}),
        })),
        total,
        share,
        definition.slug
      ).map((row) =>
        definition.source === 'assets' && row.title !== 'Others'
          ? { ...row, assetClass: assetLabel(row.title).assetClass }
          : row
      ),
    },
    // The leader by name, bright, with its amount and share (CH-18).
    facts: [
      {
        label: `Largest ${entity}`,
        name: distinctLabel(leader),
        value: `${formatChartExact(leader.value, 'CKB')} CKB · ${share(leader.shannons)}`,
      },
    ],
    historicalShares: histories,
    evidence: [
      {
        label: 'Owned-capacity history',
        value: formatCount(payload.capacityShare.data.length, 'day'),
      },
    ],
    headline:
      total === BigInt(0)
        ? null
        : {
            pre: definition.source === 'assets' ? assetLabel(leader.label).label : leader.label,
            display: share(leader.shannons).replace(/%$/, ''),
            unit: '%',
          },
  };
}

/** The route of a ranked entity, from its R2b-b series id (CH-29); an unknown kind fails. */
function rankedSeriesHref(id: string, label: string): string {
  const [kind, ...rest] = id.split(':');
  const ref = rest.join(':');
  if (kind === 'family') return `/scripts/${encodeURIComponent(label)}`;
  if (kind === 'ref') return `/script/${encodeURIComponent(ref.split(':')[0])}`;
  if (kind === 'token') return `/tokens/${encodeURIComponent(ref)}`;
  if (kind === 'dob') return `/clusters/${encodeURIComponent(ref)}`;
  if (kind === 'object') return `/classes/${encodeURIComponent(ref)}`;
  if (kind === 'identity') return `/identities/${encodeURIComponent(ref)}`;
  throw new Error(`unknown ranked series id ${id}`);
}

/**
 * The hash a series id names (`token:0x…` → `0x…`); a ranking label writes it at the tight size, a
 * chart label (U-09): "0x5cb3…b4fa".
 */
function fullSeriesId(id: string): string {
  return id.split(':')[1] ?? id;
}

/** A bucket label without the unit word its subtitle already names: "100-1k CKB" → "100-1k". */
export function shortBucketLabel(label: string, unit = 'CKB'): string {
  return label.endsWith(` ${unit}`) ? label.slice(0, -unit.length - 1) : label;
}

/**
 * The Assets ranking labels each entry "<name> (<class>)"; the class becomes its mark (CH-06). A
 * suffix outside the class vocabulary stays in the label, unmarked.
 */
function assetLabel(label: string): { label: string; assetClass?: AssetClass } {
  const match = /^(.*) \((token|object|identity)\)$/.exec(label);
  if (!match) return { label };
  const assetClass = ({ token: 'tokens', object: 'objects', identity: 'identities' } as const)[
    match[2] as 'token' | 'object' | 'identity'
  ];
  return { label: match[1], assetClass };
}

function minerRows(
  definition: ChartDefinition,
  payload: {
    data: {
      minerLockHash: string;
      address: string | null;
      minerName: string | null;
      blocksMined: number;
      percentage: string;
    }[];
    totalBlocks: number;
    fromDate: string;
    toDate: string;
    title: string;
  }
): ChartModel {
  if (!Number.isSafeInteger(payload.totalBlocks) || payload.totalBlocks < 0)
    throw new Error(`${definition.slug}: invalid totalBlocks`);
  if (payload.data.length > 0 && payload.totalBlocks === 0)
    throw new Error(`${definition.slug}: ${payload.data.length} miners in a window of no blocks`);
  const miners = payload.data.map((miner) => {
    if (!Number.isSafeInteger(miner.blocksMined) || miner.blocksMined < 0)
      throw new Error(`${definition.slug}: invalid blocksMined for ${miner.minerLockHash}`);
    exactDecimalRatio(miner.percentage, `${definition.slug} percentage`);
    const identity = miner.address ?? miner.minerLockHash;
    // A miner is its pool's name, else its address at the row size, the ranking being rows (CH-03,
    // CH-04, U-09); a miner with no recorded address is its lock hash, said once under the ranking.
    const label =
      miner.minerName ??
      (miner.address
        ? shortenAddress(miner.address, 'row')
        : shortenHash(miner.minerLockHash, 'row'));
    return {
      row: {
        label,
        value: String(miner.blocksMined),
        // The one percentage of its blocks in the window's (CH-14, the review's item I): the
        // API's percentage is floored at four decimals, so 1 block of 1,000,000 read 0.00%.
        share: percentOf(BigInt(miner.blocksMined), BigInt(payload.totalBlocks)),
        width: barWidth(BigInt(miner.blocksMined), BigInt(payload.totalBlocks)),
        href: miner.address
          ? `/address/${miner.address}`
          : `/address/${miner.minerLockHash.replace(/^0x/, '')}`,
        title: identity,
        ...(miner.address ? {} : { lockHashOnly: true }),
      },
      blocks: BigInt(miner.blocksMined),
    };
  });
  const rankedRows = miners.map((miner) => miner.row);
  // The remainder in the same form, from the exact block counts.
  const percent2 = (blocks: bigint) => percentOf(blocks, BigInt(payload.totalBlocks));
  const window = formatDateWindow(payload.fromDate, payload.toDate);
  const leader = miners[0]?.row;
  return {
    title: payload.title,
    points: [],
    series: [],
    unit: 'blocks',
    kind: 'ranked',
    latest: leader
      ? {
          label: window,
          raw: leader.value,
          // Grouped like the detail's fact, never a bare 42705 (CH-31).
          display: formatChartExact(leader.value, 'blocks'),
        }
      : null,
    headline: rankedRows[0]
      ? {
          pre: 'top',
          display: percentFigure(miners[0].blocks, BigInt(payload.totalBlocks)),
          unit: '%',
        }
      : null,
    delta: null,
    rankedRows,
    rankedPreview: {
      rows: rankedPreviewRows(
        miners.map((miner) => ({
          label: miner.row.title,
          shortLabel: miner.row.label,
          amount: miner.blocks,
          share: miner.row.share,
          href: miner.row.href,
        })),
        BigInt(payload.totalBlocks),
        percent2,
        definition.slug
      ),
    },
    // The leader by name, in the card's head…tail form, never broken; the window's own total
    // (CH-03, CH-18).
    facts: [
      ...(leader
        ? [
            {
              label: 'Leading miner',
              name: leader.label,
              value: `${formatCount(Number(leader.value), 'block')} · ${leader.share}`,
            },
          ]
        : []),
      { label: 'Window', value: `${window} · ${formatCount(payload.totalBlocks, 'block')}` },
    ],
  };
}

export function chartModel(definition: ChartDefinition, payload: ChartPayload): ChartModel {
  if ('usedShare' in payload) return shareRows(definition, payload);
  if ('totalBlocks' in payload) return minerRows(definition, payload);
  if ('series' in payload) return stackedChart(definition, payload);
  const model = singleChart(definition, payload);
  if (definition.cardDays !== undefined) {
    if (model.card) throw new Error(`${definition.slug}: a card window over a card model`);
    model.card = {
      points: model.points.slice(-definition.cardDays),
      series: model.series,
      legend: [],
    };
  }
  if ('orphanRateTarget' in payload) {
    const { numerator, denominator } = payload.orphanRateTarget;
    if (
      !Number.isSafeInteger(numerator) ||
      !Number.isSafeInteger(denominator) ||
      numerator < 0 ||
      denominator <= 0
    )
      throw new Error(`${definition.slug}: invalid consensus orphan-rate target`);
    model.facts = [{ label: 'Consensus target', value: `${numerator} / ${denominator} blocks` }];
    // The target as a percentage of blocks: exact, at the rate units' two decimals (CH-12).
    const percent = formatScaledInteger(BigInt(numerator) * BigInt(100), BigInt(denominator), 2);
    model.referenceLines = [{ value: Number(percent), label: withUnit(`target ${percent}`, '%') }];
  }
  return model;
}

// --- F2 charts group: board encodings computed once here, rendered by card and detail ---

/** Bucket histograms (no time axis): headlined by their most common bucket. */
const BUCKET_DISTRIBUTIONS: readonly string[] = [
  'cell-size-distribution',
  'block-time-distribution',
  'epoch-time-distribution',
];

/**
 * The API series of the charts that split CKB by asset class, in the board's stacking order (bottom
 * first), with the class vocabulary's names and colours (CH-06, CH-14). Burnt and plain CKB are the
 * neutral band; gold is DAO alone; jade is the data hue (circulating, miners).
 */
const CLASSED_SERIES: Readonly<
  Record<string, Readonly<Record<string, { label: string; color: string }>>>
> = {
  'common-knowledge-composition': {
    transfer: { label: 'Plain CKB', color: CLASS_COLORS.ckb },
    dao: { label: 'Nervos DAO', color: CLASS_COLORS.dao },
    udt: { label: 'Tokens', color: CLASS_COLORS.tokens },
    nftSpore: { label: 'Objects', color: CLASS_COLORS.objects },
    otherContracts: { label: 'Other scripts', color: CLASS_COLORS.other },
  },
  'total-supply': {
    circulating: { label: 'Circulating', color: DATA_COLOR },
    nervosdao: { label: 'Nervos DAO', color: CLASS_COLORS.dao },
    burnt: { label: 'Burnt', color: CLASS_COLORS.ckb },
  },
  'secondary-issuance': {
    burnt: { label: 'Burnt', color: CLASS_COLORS.ckb },
    compensation: { label: 'DAO compensation', color: CLASS_COLORS.dao },
    mining: { label: 'Miners', color: DATA_COLOR },
  },
  // The activity kinds in the colours and words the rest of the site gives them (CH-02).
  'activity-type-breakdown': {
    transfer: { label: 'CKB transfers', color: ACTIVITY_KIND_COLORS.transfer },
    dao: { label: 'Nervos DAO', color: ACTIVITY_KIND_COLORS.dao },
    token: { label: 'Tokens', color: ACTIVITY_KIND_COLORS.token },
    object: { label: 'Objects', color: ACTIVITY_KIND_COLORS.object },
    identity: { label: 'Identities', color: ACTIVITY_KIND_COLORS.identity },
    scriptCall: { label: 'Script calls', color: ACTIVITY_KIND_COLORS.scriptCall },
  },
};

/** The returned series of a classed chart, every key known, in board order (CH-06). */
function classedSeries(
  slug: string,
  classes: Readonly<Record<string, { label: string; color: string }>>,
  payload: StackedPayload
): StackedPayload['series'] {
  for (const entry of payload.series)
    if (!classes[entry.key]) throw new Error(`${slug}: unknown series ${entry.key}`);
  const order = Object.keys(classes).filter((key) =>
    payload.series.some((entry) => entry.key === key)
  );
  const byKey = new Map(payload.series.map((entry) => [entry.key, entry]));
  const ordered = order.map((key) => byKey.get(key)!);
  // Knowledge by kind stacks the largest kind today at the bottom, like the board.
  const last = payload.data.at(-1);
  if (slug !== 'common-knowledge-composition' || !last) return ordered;
  const amount = (key: string) => {
    const raw = last.values[key];
    if (raw === undefined) throw new Error(`${slug}: missing ${key} at ${last.date}`);
    return exactDecimalRatio(raw, `${slug} ${key}`);
  };
  return [...ordered].sort((a, b) => {
    const [an, ad] = amount(a.key);
    const [bn, bd] = amount(b.key);
    const left = bn * ad;
    const right = an * bd;
    return left === right ? 0 : left > right ? 1 : -1;
  });
}

/**
 * The card's bands: the HODL wave's four coarser age groups, as on the board (CH-07); activity by
 * type its six kinds, largest first, each in the page's class colour (CH-02).
 */
const CARD_GROUPS: Readonly<
  Record<string, readonly { key: string; label: string; members: readonly string[] }[]>
> = {
  'hodl-wave': [
    { key: 'over1y', label: 'over 1 y', members: ['gt3y', '1y3y'] },
    { key: 'm3y1', label: '3 m – 1 y', members: ['6m1y', '3m6m'] },
    { key: 'w1m3', label: '1 w – 3 m', members: ['1m3m', '1w1m'] },
    { key: 'under1w', label: 'under 1 w', members: ['1d1w', '24h'] },
  ],
  'activity-type-breakdown': ['scriptCall', 'transfer', 'token', 'object', 'identity', 'dao'].map(
    (key) => ({
      key,
      label: CLASSED_SERIES['activity-type-breakdown'][key].label,
      members: [key],
    })
  ),
};

/** The exact sum of non-negative decimals, trailing zeros trimmed. */
function exactSum(values: readonly string[], context: string): string {
  const ratios = values.map((value) => exactDecimalRatio(value, context));
  const denominator = ratios.reduce(
    (largest, [, divisor]) => (divisor > largest ? divisor : largest),
    BigInt(1)
  );
  const sum = ratios.reduce((acc, [numerator, divisor]) => {
    if (denominator % divisor !== BigInt(0))
      throw new Error(`${context}: incompatible decimal divisors`);
    return acc + numerator * (denominator / divisor);
  }, BigInt(0));
  const whole = sum / denominator;
  const fraction = (sum % denominator)
    .toString()
    .padStart(denominator.toString().length - 1, '0')
    .replace(/0+$/, '');
  return fraction ? `${whole}.${fraction}` : whole.toString();
}

/** `part` as a share of `whole`, in percent at two decimals (exact, rounded half up). */
function shareOf(part: string, whole: string, context: string): string {
  const [pn, pd] = exactDecimalRatio(part, `${context} part`);
  const [wn, wd] = exactDecimalRatio(whole, `${context} whole`);
  if (wn === BigInt(0)) throw new Error(`${context}: share of a zero total`);
  return percentFigure(pn * wd, pd * wn);
}

/** A histogram's headline: its most common bucket, by what that bucket means (CH-09). */
function bucketHeadline(
  definition: ChartDefinition,
  points: readonly ChartPoint[],
  leading: ChartPoint
): ChartHeadline {
  if (definition.slug === 'cell-size-distribution')
    return {
      pre: leading.label,
      display: shareOf(
        leading.values.primary,
        exactSum(
          points.map((point) => point.values.primary),
          definition.slug
        ),
        definition.slug
      ),
      unit: '%',
    };
  const unit = bucketUnit(definition);
  if (unit === undefined) throw new Error(`${definition.slug}: a bucket histogram with no unit`);
  return { pre: 'most common', display: leading.label, unit };
}

/** A model's exact age in years, at two decimals: "6.96" (CH-33). */
function modelYear(year: string, slug: string): string {
  const [numerator, denominator] = exactDecimalRatio(year, `${slug} today year`);
  return formatScaledInteger(numerator, denominator, 2);
}

/** An epoch number with thousands groups: "15,202" (CH-35). */
export function formatEpochLabel(label: string, slug: string): string {
  if (!/^\d+$/.test(label)) throw new Error(`${slug}: invalid epoch label ${label}`);
  return formatExactDecimal(label, `${slug} epoch`);
}

export type AxisStyle = 'date' | 'month' | 'epoch' | 'model-year' | 'bucket';

/** What a chart's points are, counted (CH-04, CH-36): `2,555 days`, `85 cohorts`, `6 buckets`. */
const POINT_NOUN: Record<Exclude<AxisStyle, 'model-year'>, string> = {
  date: 'day',
  month: 'cohort',
  epoch: 'epoch',
  bucket: 'bucket',
};

/**
 * The span a chart draws, in the page's words, grouped: `2,555 days` (CH-04); a model curve the
 * years it runs over, `20 years`, not its samples (CH-19).
 */
export function chartSpan(points: readonly ChartPoint[], style: AxisStyle): string {
  if (style !== 'model-year' || points.length < 2)
    return style === 'model-year'
      ? formatCount(0, 'year')
      : formatCount(points.length, POINT_NOUN[style]);
  const [fn, fd] = exactDecimalRatio(points[0].label, 'model span start');
  const [ln, ld] = exactDecimalRatio(points.at(-1)!.label, 'model span end');
  const years = ln * fd - fn * ld;
  const per = ld * fd;
  if (years < BigInt(0)) throw new Error('model span runs backwards');
  return years % per === BigInt(0)
    ? formatCount(years / per, 'year')
    : `${formatScaledInteger(years, per, 2)} years`;
}

/** How a chart's x labels read (CH-24, CH-35). */
export function axisStyle(definition: ChartDefinition): AxisStyle {
  if (definition.slug === 'epoch-time-length') return 'epoch';
  if (definition.slug === 'nominal-apc') return 'model-year';
  if (definition.slug === 'address-cohort-retention') return 'month';
  if (BUCKET_DISTRIBUTIONS.includes(definition.slug)) return 'bucket';
  return 'date';
}

/**
 * Bucket charts whose buckets are named ranges, one bar each: every bar keeps its label, stacked
 * when a row cannot hold them (responsive review C#6). The second-resolution histograms have
 * hundreds of numeric bins and keep a thinned numeric axis.
 */
const CATEGORY_BUCKETS: readonly string[] = ['cell-size-distribution'];

export function categoricalAxis(definition: ChartDefinition): boolean {
  return CATEGORY_BUCKETS.includes(definition.slug);
}

/**
 * Every k-th item from the first, for the smallest k that keeps at most `max`: a regular sequence
 * stays regular (CH-12: thinning 9 model years evenly to 8 dropped year 10).
 */
function everyKth<T>(items: readonly T[], max: number): T[] {
  if (!Number.isSafeInteger(max) || max < 1) throw new Error(`axis ticks: invalid maximum ${max}`);
  const k = Math.ceil(items.length / max);
  return items.filter((_, i) => i % k === 0);
}

/** The smallest 1 / 2 / 2.5 / 5 × 10^k step putting at most `max` round values in [from, to]. */
function tickStep(from: number, to: number, max: number): number {
  let step = niceStep((to - from) / max);
  const count = (s: number) => Math.floor(to / s + 1e-9) - Math.ceil(from / s - 1e-9) + 1;
  while (count(step) > max) step = niceStep(step * (1 + 1e-6));
  return step;
}

/** The round values from `values[lo]` to `values[hi]` by `step`, each at the first point reaching it. */
function roundTicks(
  values: readonly number[],
  lo: number,
  hi: number,
  step: number,
  label: (value: number, index: number) => string
): { index: number; label: string }[] {
  const ticks: { index: number; label: string }[] = [];
  const first = Math.ceil(values[lo] / step - 1e-9);
  const last = Math.floor(values[hi] / step + 1e-9);
  for (let n = first; n <= last; n++) {
    const tick = n * step;
    let index = lo;
    while (index < hi && values[index] < tick - step * 1e-9) index++;
    if (!ticks.some((t) => t.index === index)) ticks.push({ index, label: label(tick, index) });
  }
  return ticks;
}

/** A bucket axis' x unit: what its bucket labels measure (CH-12). */
const BUCKET_UNITS: Readonly<Record<string, string>> = {
  'cell-size-distribution': 'CKB',
  'block-time-distribution': 's',
  'epoch-time-distribution': 'h',
};

/** The x unit of a bucket histogram, which its axis writes once (CH-12). */
export function bucketUnit(definition: ChartDefinition): string | undefined {
  return BUCKET_UNITS[definition.slug];
}

const NUMERIC_BUCKET = /^\d+(\.\d+)?$/;

/** A bucket as read on the page, with its unit: `0.8 s`, `4.02 h`, `< 100 CKB` (CH-12, CH-14). */
export function bucketName(label: string, unit: string | undefined): string {
  if (unit === undefined) return label;
  if (NUMERIC_BUCKET.test(label)) return `${label} ${unit}`;
  if (!label.endsWith(` ${unit}`)) throw new Error(`bucket ${label} is not in ${unit}`);
  return label;
}

/**
 * A numeric histogram's ticks (CH-12): round values over its regular run of buckets — the longest
 * stretch with no gap wider than one tick step — and, beyond the run at either end, the outlying
 * buckets marked open-ended on the run's side (`≤ 2.05`, `≥ 50.50`). `max` bounds the round values.
 */
function numericBucketTicks(
  points: readonly ChartPoint[],
  max: number
): { index: number; label: string }[] {
  const values = points.map((point) => Number(point.label));
  values.forEach((value, i) => {
    if (i > 0 && !(value > values[i - 1]))
      throw new Error(`bucket axis: ${points[i].label} does not follow ${points[i - 1].label}`);
  });
  let lo = 0;
  let hi = values.length - 1;
  let step = values[hi] > values[lo] ? tickStep(values[lo], values[hi], max) : 1;
  while (values[hi] > values[lo]) {
    // Split the run where neighbours lie further apart than a step; keep its longest part.
    let best = { lo, hi: lo };
    let from = lo;
    for (let i = lo + 1; i <= hi + 1; i++)
      if (i > hi || values[i] - values[i - 1] > step * (1 + 1e-9)) {
        if (i - 1 - from > best.hi - best.lo) best = { lo: from, hi: i - 1 };
        from = i;
      }
    if (best.lo === lo && best.hi === hi) break;
    lo = best.lo;
    hi = best.hi;
    if (values[hi] > values[lo]) step = tickStep(values[lo], values[hi], max);
  }
  const decimals = stepDecimals(step);
  const ticks =
    values[hi] > values[lo]
      ? roundTicks(values, lo, hi, step, (value) => value.toFixed(decimals))
      : [{ index: lo, label: points[lo].label }];
  return [
    ...(lo > 0 ? [{ index: lo - 1, label: `≤ ${points[lo - 1].label}` }] : []),
    ...ticks,
    ...(hi < points.length - 1 ? [{ index: hi + 1, label: `≥ ${points[hi + 1].label}` }] : []),
  ];
}

/**
 * X-axis ticks (CH-24, CH-09, CH-12): where a calendar unit starts (years, else months), at round
 * epoch numbers or model years, or at a histogram's round values — a regular sequence, every k-th
 * kept so that at most `max` remain and none in between is skipped. A bucket axis writes its
 * `unit` once, on its last label; named buckets (Cell size) are all kept, each a category.
 */
export function axisTicks(
  points: readonly ChartPoint[],
  style: AxisStyle,
  max: number,
  unit?: string
): { index: number; label: string }[] {
  if (!points.length) return [];
  if (style === 'bucket') {
    const numeric = points.every((point) => NUMERIC_BUCKET.test(point.label));
    const ticks = numeric
      ? numericBucketTicks(points, max)
      : points.map((point, index) => ({
          index,
          label:
            unit === undefined
              ? point.label
              : shortBucketLabel(bucketName(point.label, unit), unit),
        }));
    if (unit === undefined) return ticks;
    return ticks.map((tick, i) =>
      i === ticks.length - 1 ? { ...tick, label: `${tick.label} ${unit}` } : tick
    );
  }
  if (style === 'epoch' || style === 'model-year') {
    const values = points.map((point) => {
      const value = Number(point.label);
      if (!Number.isFinite(value)) throw new Error(`invalid ${style} axis label ${point.label}`);
      return value;
    });
    const first = values[0];
    const last = values[values.length - 1];
    if (last <= first) return [{ index: 0, label: tickLabel(style, first) }];
    return roundTicks(values, 0, values.length - 1, tickStep(first, last, max), (_, index) =>
      tickLabel(style, values[index])
    );
  }
  const dated = (label: string) => {
    if (style === 'month' ? !/^\d{4}-\d{2}$/.test(label) : !/^\d{4}-\d{2}-\d{2}$/.test(label))
      throw new Error(`invalid ${style} axis label ${label}`);
    return label;
  };
  const starts = (cut: number) =>
    points.flatMap((point, index) => {
      const key = dated(point.label).slice(0, cut);
      return index > 0 && key !== points[index - 1].label.slice(0, cut)
        ? [{ index, label: key }]
        : [];
    });
  const years = starts(4);
  if (years.length >= 2 || style === 'month') return everyKth(years, max);
  const months = starts(7);
  if (months.length >= 2) return everyKth(months, max);
  // Inside one or two months: the days themselves, month-day (CH-58).
  return everyKth(
    points.map((point, index) => ({ index, label: point.label.slice(5) })),
    max
  );
}

function tickLabel(style: 'epoch' | 'model-year', value: number): string {
  return style === 'epoch'
    ? `#${formatExactDecimal(String(value), 'epoch tick')}`
    : `year ${value}`;
}
