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
import { niceStep } from '@/lib/chart-ticks';
import { CLASS_COLORS, DATA_COLOR, HUE_RAMP, type AssetClass } from '@/lib/design-tokens';

export interface RankedPreviewRow {
  /** Short display label; `title` keeps the full identity. */
  label: string;
  title: string;
  share: string;
  /** The asset class the API's label names (Assets ranking), drawn as its mark (CH-06). */
  assetClass?: AssetClass;
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

export interface ChartModel {
  title: string;
  points: ChartPoint[];
  series: ChartSeries[];
  unit: string;
  kind: 'line' | 'bars' | 'stacked' | 'ranked';
  /** Present only when the API actually provides a second physical unit. */
  secondary?: { points: ChartPoint[]; series: ChartSeries[]; unit: string; title: string };
  latest: { label: string; raw: string; display: string } | null;
  /** Exact, signed change between the final two returned points. */
  delta: string | null;
  rankedRows?: {
    label: string;
    value: string;
    share: string;
    href?: string;
    identity?: string;
    assetClass?: AssetClass;
  }[];
  /** Card preview of a ranking: the top five plus an exact "Others" for every remaining rank. */
  rankedPreview?: { rows: RankedPreviewRow[]; summaryLabel: string | undefined };
  /** Exact signed day-over-day change in occupied CKB, when the source is knowledge size. */
  dailyFlow?: { points: ChartPoint[]; series: ChartSeries[]; unit: string; title: string };
  /** Exact daily histories from both distinct utilization scopes. */
  historicalShares?: { title: string; points: ChartPoint[]; series: ChartSeries[]; unit: string }[];
  summaryLabel?: string;
  /** A model timeline's "today" index: drawn solid up to it and dashed ahead (CH-03). */
  projectedAfter?: number;
  /**
   * A model timeline's main reading: the API's `today` point (lane R8), or `null` while the API
   * does not send one, so the detail chart shows no reading rather than the last (future) sample.
   * Absent on every other chart.
   */
  today?: ChartPoint | null;
  contextFacts?: { label: string; value: string }[];
  coverage?: string;
  // --- F2 charts group ---
  /** The card's headline; `null` when the source has no reading to head it (a model's today). */
  headline?: ChartHeadline | null;
  /** What the index card draws when it summarises the detail: fewer bands, a legend (CH-07). */
  card?: { points: ChartPoint[]; series: ChartSeries[]; legend: ChartLegendEntry[] };
  /** A fixed value domain: a share chart runs 0–100 (CH-15). */
  yDomain?: readonly [number, number];
  /** Dashed reference values drawn across the plot, e.g. the consensus target (CH-12). */
  referenceLines?: { value: number; label: string }[];
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
const RATE_UNITS = new Set(['%', 'PH/s', 'P', 'hours', 'seconds']);
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
  const sign = negative ? '-' : plus ? '+' : '';
  if (RATE_UNITS.has(unit))
    return `${sign}${formatExactDecimal(formatScaledInteger(abs, denominator, 2), `chart ${unit} readout`)}`;
  if (COUNT_UNITS.has(unit) && abs % denominator === BigInt(0) && abs < denominator * BigInt(1000))
    return `${sign}${abs / denominator}`;
  return `${sign}${formatCompactRatio(abs, denominator)}`;
}

/** The exact chart value, every returned digit, thousands grouped (detail facts and rows). */
export function formatChartExact(value: string, unit: string): string {
  return formatExactDecimal(value, `chart ${unit} exact value`);
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
  const points: ChartPoint[] = payload.data.map((point) => {
    const primary = point[primaryField];
    if (primary === undefined || primary === null)
      throw new Error(`${definition.slug}: missing ${primaryField} at ${point.date}`);
    const values: Record<string, string> = {
      primary: chartUnitValue(primary, definition.scale, `${definition.slug} at ${point.date}`),
    };
    if (hasSecond && !definition.secondaryUnit) {
      const companion = point[companionField];
      if (companion === undefined || companion === null)
        throw new Error(`${definition.slug}: missing ${companionField} at ${point.date}`);
      values.companion = chartUnitValue(
        companion,
        definition.scale,
        `${definition.slug} companion at ${point.date}`
      );
    }
    return { label: point.date, values };
  });
  const currentIndex = timelineMarkers ? timelineCurrentIndex(definition.slug, payload.data) : null;
  const series: ChartSeries[] = [
    {
      key: 'primary',
      label:
        primaryField === 'value2' ? (payload.y2AxisLabel ?? definition.title) : payload.yAxisLabel,
      color: '#2edba3',
    },
  ];
  if (hasSecond && !definition.secondaryUnit)
    series.push({
      key: 'companion',
      label: primaryField === 'value2' ? payload.yAxisLabel : (payload.y2AxisLabel ?? 'Companion'),
      color: '#68ccf0',
      // A companion reading of the same unit (real inflation, daily turnover) is dashed (CH-12).
      dashed: true,
    });
  const secondary =
    hasSecond && definition.secondaryUnit
      ? {
          points: payload.data.map((point) => ({
            label: point.date,
            values: {
              secondary: chartUnitValue(
                point.value2!,
                'raw',
                `${definition.slug} secondary at ${point.date}`
              ),
            },
          })),
          series: [
            {
              key: 'secondary',
              label: definition.secondaryLabel ?? payload.y2AxisLabel ?? 'Secondary',
              color: '#68ccf0',
            },
          ],
          unit: definition.secondaryUnit,
          title: definition.secondaryLabel ?? payload.y2AxisLabel ?? 'Secondary series',
        }
      : undefined;
  const last = points.at(-1);
  const previous = points.at(-2);
  const bucketDistribution = BUCKET_DISTRIBUTIONS.includes(definition.slug);
  // Cohorts are ordered by first-seen month: the card reads the newest one, not the largest (CH-09).
  const cohorts = definition.slug === 'address-cohort-retention';
  const leadingBucket = bucketDistribution
    ? points.reduce<ChartPoint | null>((best, point) => {
        if (!best) return point;
        const [currentNumerator, currentDenominator] = exactSignedDecimalRatio(
          point.values.primary,
          `${definition.slug} bucket ${point.label}`
        );
        const [bestNumerator, bestDenominator] = exactSignedDecimalRatio(
          best.values.primary,
          `${definition.slug} bucket ${best.label}`
        );
        return currentNumerator * bestDenominator > bestNumerator * currentDenominator
          ? point
          : best;
      }, null)
    : null;
  // A model timeline is summarised at its "today" sample; any other series at its last point.
  const todayPoint = currentIndex === null ? null : points[currentIndex];
  const summaryPoint = leadingBucket ?? todayPoint ?? last;
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
            unit: 'CKB/day',
            series: [{ key: 'flow', label: 'Daily net flow', color: '#68ccf0' }],
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
          label: todayPoint ? `year ${summaryPoint.label}` : summaryPoint.label,
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
    summaryLabel: leadingBucket
      ? 'Most common'
      : todayPoint
        ? 'Today'
        : cohorts
          ? 'Newest cohort'
          : undefined,
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
  else if (cohorts && last)
    model.headline = {
      pre: last.label,
      display: summary(last.values.primary, definition.unit),
      unit: definition.unit,
    };
  else if (definition.slug === 'epoch-time-length' && last)
    model.headline = {
      pre: `#${formatEpochLabel(last.label, definition.slug)}`,
      display: summary(last.values.primary, definition.unit),
      unit: definition.unit,
    };
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
  const series: ChartSeries[] = selectedSeries.map((entry, index) =>
    classes
      ? { key: entry.key, label: classes[entry.key].label, color: classes[entry.key].color }
      : { key: entry.key, label: entry.label, color: HUE_RAMP[index % HUE_RAMP.length] }
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
      ? {
          title: 'Holder count',
          unit: 'holders',
          series: [{ key: 'holders', label: 'Holders', color: '#68ccf0' }],
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
        }
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
    summaryLabel: isSecondaryIssuance
      ? 'Protocol secondary issuance'
      : leadingAge
        ? `Largest age band: ${leadingAge.label}`
        : undefined,
    contextFacts:
      isSecondaryIssuance && payload.data.length
        ? [
            {
              label: 'Frozen phase-1 overlap',
              // Grouped like the line above it (CH-34).
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
    const cardSeries = groups.map((group, index) => ({
      key: group.key,
      label: group.label,
      color: HUE_RAMP[index % HUE_RAMP.length],
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
        value: `${share(entry.key)} %`,
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
        ? `${shareOf(last.values[key], total(last), `${definition.slug} ${key}`)} %`
        : `${summary(last.values[key], definition.unit)} ${definition.unit}`;
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
    secondary: {
      title: 'Consumed cells, cumulative',
      unit: 'cells',
      series: [{ key: 'secondary', label: 'Consumed cells', color: '#68ccf0' }],
      points: payload.data.map((point) => ({
        label: point.date,
        values: { secondary: read(point, 'deadCells') },
      })),
    },
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
    summaryLabel: 'Live cells',
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

/**
 * Top five ranked entries plus one "Others" row for the exact remainder of the total, so the card
 * never drops ranks six and beyond (CH-01). The remainder is computed from exact amounts, never
 * from rounded shares; a zero remainder (or zero total) has no "Others" row.
 */
function rankedPreviewRows(
  ranked: readonly { label: string; shortLabel: string; amount: bigint; share: string }[],
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
    ...top.map((entry) => ({ label: entry.shortLabel, title: entry.label, share: entry.share })),
    ...(remainder > BigInt(0)
      ? [{ label: 'Others', title: 'Others', share: remainderShare(remainder) }]
      : []),
  ];
}

/** Long identities share long prefixes; keep the design's head…tail form (CH-04). */
export function shortRankLabel(label: string): string {
  return label.length > 20 ? `${label.slice(0, 10)}…${label.slice(-6)}` : label;
}

function shareRows(
  definition: ChartDefinition,
  payload: {
    usedShare: {
      data: { date: string; values: Record<string, string> }[];
      series: { key: string; label: string; color: string }[];
    };
    capacityShare: {
      data: { date: string; values: Record<string, string> }[];
      series: { key: string; label: string; color: string }[];
    };
  }
): ChartModel {
  const used = payload.usedShare;
  const last = used.data.at(-1);
  const histories = [
    { key: 'usedShare', title: 'Occupied common knowledge', payload: payload.usedShare },
    { key: 'capacityShare', title: 'Owned live capacity', payload: payload.capacityShare },
  ].map(({ key, title, payload: history }) => ({
    title,
    unit: 'CKB',
    series: history.series.map((series, index) => ({
      key: series.key,
      label: series.label,
      color: [
        '#2edba3',
        '#68ccf0',
        '#8f9cdb',
        '#52b8a7',
        '#547fae',
        '#6fcfc2',
        '#879abb',
        '#a0a8be',
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
      label: series.label,
      shannons: exactUnsignedInteger(raw, `${definition.slug} ${series.key}`),
      value: chartUnitValue(raw, 'shannons', `${definition.slug} ${series.key}`),
    };
  });
  if (!entries.length) throw new Error(`${definition.slug}: dated ranking has no series`);
  const total = entries.reduce((sum, entry) => sum + entry.shannons, BigInt(0));
  const share = (shannons: bigint) =>
    total === BigInt(0) ? '—' : `${formatScaledInteger(shannons * BigInt(100), total, 2)}%`;
  const sorted = [...entries].sort((a, b) =>
    a.shannons === b.shannons ? a.label.localeCompare(b.label) : a.shannons > b.shannons ? -1 : 1
  );
  const othersKey = used.series.find((series) => series.key === 'others')?.label;
  const named = sorted.filter((entry) => entry.label !== othersKey);
  const others = sorted.filter((entry) => entry.label === othersKey);
  // The API's own bucket holds the ranks beyond its named list; say so, the card's "Others" differs.
  // It is a remainder, not a rank: it closes the list instead of leading it (CH-29).
  const rankedRows = [...named, ...others].map((entry) => ({
    label: entry.label === othersKey ? `Others beyond the top ${named.length}` : entry.label,
    value: entry.value,
    share: share(entry.shannons),
    ...(definition.source === 'assets' && entry.label !== othersKey ? assetLabel(entry.label) : {}),
  }));
  const leader = named[0] ?? sorted[0];
  const summaryLabel = `Largest ${definition.source === 'scripts' ? 'script' : 'asset'}: ${leader.label}`;
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
          shortLabel: entry.label,
          amount: entry.shannons,
          share: share(entry.shannons),
        })),
        total,
        share,
        definition.slug
      ).map((row) =>
        definition.source === 'assets' && row.label !== 'Others'
          ? { ...row, ...assetLabel(row.label) }
          : row
      ),
      summaryLabel,
    },
    historicalShares: histories,
    coverage: `${payload.capacityShare.data.length} owned-capacity dates`,
    summaryLabel,
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
  const miners = payload.data.map((miner) => {
    if (!Number.isSafeInteger(miner.blocksMined) || miner.blocksMined < 0)
      throw new Error(`${definition.slug}: invalid blocksMined for ${miner.minerLockHash}`);
    exactDecimalRatio(miner.percentage, `${definition.slug} percentage`);
    const label = miner.minerName ?? miner.address ?? miner.minerLockHash;
    return {
      row: {
        label,
        value: String(miner.blocksMined),
        share: `${miner.percentage}%`,
        href: miner.address
          ? `/address/${miner.address}`
          : `/address/${miner.minerLockHash.replace(/^0x/, '')}`,
        identity: miner.address ? 'Address' : 'Lock hash',
      },
      shortLabel: miner.minerName ?? shortRankLabel(label),
      blocks: BigInt(miner.blocksMined),
    };
  });
  const rankedRows = miners.map((miner) => miner.row);
  // The API floors each percentage at four decimals; the remainder follows the same rule.
  const floorPercent4 = (blocks: bigint) => {
    const scaled = (blocks * BigInt(1_000_000)) / BigInt(payload.totalBlocks);
    return `${scaled / BigInt(10_000)}.${(scaled % BigInt(10_000)).toString().padStart(4, '0')}%`;
  };
  return {
    title: payload.title,
    points: [],
    series: [],
    unit: 'blocks',
    kind: 'ranked',
    latest: rankedRows[0]
      ? {
          label: `${payload.fromDate}–${payload.toDate}`,
          raw: rankedRows[0].value,
          // Grouped like the detail's fact, never a bare 42705 (CH-31).
          display: formatChartExact(rankedRows[0].value, 'blocks'),
        }
      : null,
    headline: rankedRows[0]
      ? { pre: 'top', display: summary(payload.data[0].percentage, '%'), unit: '%' }
      : null,
    delta: null,
    rankedRows,
    rankedPreview: {
      rows: rankedPreviewRows(
        miners.map((miner) => ({
          label: miner.row.label,
          shortLabel: miner.shortLabel,
          amount: miner.blocks,
          share: miner.row.share,
        })),
        BigInt(payload.totalBlocks),
        floorPercent4,
        definition.slug
      ),
      summaryLabel: miners[0]
        ? `Leading miner: ${miners[0].shortLabel} (${miners[0].row.share})`
        : undefined,
    },
    coverage: `${payload.fromDate}–${payload.toDate} · ${payload.totalBlocks.toLocaleString()} blocks`,
    // The fact names the miner in the same head…tail form as the card; the ranking links the
    // full address (CH-04).
    summaryLabel: miners[0]
      ? `Leading miner: ${miners[0].shortLabel} (${miners[0].row.share})`
      : undefined,
  };
}

export function chartModel(definition: ChartDefinition, payload: ChartPayload): ChartModel {
  if ('usedShare' in payload) return shareRows(definition, payload);
  if ('totalBlocks' in payload) return minerRows(definition, payload);
  if ('series' in payload) return stackedChart(definition, payload);
  const model = singleChart(definition, payload);
  if ('orphanRateTarget' in payload) {
    const { numerator, denominator } = payload.orphanRateTarget;
    if (
      !Number.isSafeInteger(numerator) ||
      !Number.isSafeInteger(denominator) ||
      numerator < 0 ||
      denominator <= 0
    )
      throw new Error(`${definition.slug}: invalid consensus orphan-rate target`);
    model.contextFacts = [
      { label: 'Consensus target', value: `${numerator} / ${denominator} blocks` },
    ];
    // The target as a percentage of blocks: exact, at the rate units' two decimals (CH-12).
    const percent = formatScaledInteger(BigInt(numerator) * BigInt(100), BigInt(denominator), 2);
    model.referenceLines = [{ value: Number(percent), label: `target ${percent} %` }];
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

/** The card's coarser bands of the HODL wave and activity by type, as on the board (CH-07). */
const CARD_GROUPS: Readonly<
  Record<string, readonly { key: string; label: string; members: readonly string[] }[]>
> = {
  'hodl-wave': [
    { key: 'over1y', label: 'over 1 y', members: ['gt3y', '1y3y'] },
    { key: 'm3y1', label: '3 m – 1 y', members: ['6m1y', '3m6m'] },
    { key: 'w1m3', label: '1 w – 3 m', members: ['1m3m', '1w1m'] },
    { key: 'under1w', label: 'under 1 w', members: ['1d1w', '24h'] },
  ],
  'activity-type-breakdown': [
    { key: 'scriptCall', label: 'Script calls', members: ['scriptCall'] },
    { key: 'transfer', label: 'Transfers', members: ['transfer'] },
    { key: 'assets', label: 'Assets', members: ['token', 'object', 'identity'] },
    { key: 'dao', label: 'DAO', members: ['dao'] },
  ],
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
  return formatScaledInteger(pn * wd * BigInt(100), pd * wn, 2);
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
  return {
    pre: 'most common',
    display: leading.label,
    unit: definition.slug === 'block-time-distribution' ? 's' : 'hours',
  };
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

/** How a chart's x labels read (CH-24, CH-35). */
export function axisStyle(definition: ChartDefinition): AxisStyle {
  if (definition.slug === 'epoch-time-length') return 'epoch';
  if (definition.slug === 'nominal-apc') return 'model-year';
  if (definition.slug === 'address-cohort-retention') return 'month';
  if (BUCKET_DISTRIBUTIONS.includes(definition.slug)) return 'bucket';
  return 'date';
}

function thin<T>(items: readonly T[], max: number): T[] {
  if (items.length <= max) return [...items];
  if (max === 1) return [items[items.length - 1]];
  // Evenly spaced, always keeping the first and the last.
  return Array.from(
    { length: max },
    (_, i) => items[Math.round((i * (items.length - 1)) / (max - 1))]
  );
}

/**
 * X-axis ticks at the points where a calendar unit starts (years, else months), at round epoch
 * numbers, at round model years, or at buckets; at most `max`, evenly thinned (CH-24, CH-09).
 */
export function axisTicks(
  points: readonly ChartPoint[],
  style: AxisStyle,
  max: number
): { index: number; label: string }[] {
  if (!points.length) return [];
  if (style === 'bucket')
    return thin(
      points.map((point, index) => ({ index, label: point.label })),
      max
    );
  if (style === 'epoch' || style === 'model-year') {
    const values = points.map((point) => {
      const value = Number(point.label);
      if (!Number.isFinite(value)) throw new Error(`invalid ${style} axis label ${point.label}`);
      return value;
    });
    const first = values[0];
    const last = values[values.length - 1];
    if (last <= first) return [{ index: 0, label: tickLabel(style, first) }];
    const step = niceStep((last - first) / max);
    const ticks: { index: number; label: string }[] = [];
    for (let tick = Math.ceil(first / step) * step; tick <= last; tick += step) {
      const index = values.findIndex((value) => value >= tick);
      if (index >= 0 && !ticks.some((t) => t.index === index))
        ticks.push({ index, label: tickLabel(style, values[index]) });
    }
    return thin(ticks, max);
  }
  const unit = (label: string) => {
    if (style === 'month' ? !/^\d{4}-\d{2}$/.test(label) : !/^\d{4}-\d{2}-\d{2}$/.test(label))
      throw new Error(`invalid ${style} axis label ${label}`);
    return label;
  };
  const starts = (cut: number) =>
    points.flatMap((point, index) => {
      const key = unit(point.label).slice(0, cut);
      return index > 0 && key !== points[index - 1].label.slice(0, cut)
        ? [{ index, label: key }]
        : [];
    });
  const years = starts(4);
  if (years.length >= 2 || style === 'month') return thin(years, max);
  const months = starts(7);
  if (months.length >= 2) return thin(months, max);
  // Inside one or two months: the days themselves, month-day (CH-58).
  return thin(
    points.map((point, index) => ({ index, label: point.label.slice(5) })),
    max
  );
}

function tickLabel(style: 'epoch' | 'model-year', value: number): string {
  return style === 'epoch'
    ? `#${formatExactDecimal(String(value), 'epoch tick')}`
    : `year ${value}`;
}
