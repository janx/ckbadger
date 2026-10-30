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

export interface RankedPreviewRow {
  /** Short display label; `title` keeps the full identity. */
  label: string;
  title: string;
  share: string;
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
  rankedRows?: { label: string; value: string; share: string; href?: string; identity?: string }[];
  /** Card preview of a ranking: the top five plus an exact "Others" for every remaining rank. */
  rankedPreview?: { rows: RankedPreviewRow[]; summaryLabel: string | undefined };
  /** Exact signed day-over-day change in occupied CKB, when the source is knowledge size. */
  dailyFlow?: { points: ChartPoint[]; series: ChartSeries[]; unit: string; title: string };
  /** Exact daily histories from both distinct utilization scopes. */
  historicalShares?: { title: string; points: ChartPoint[]; series: ChartSeries[]; unit: string }[];
  summaryLabel?: string;
  contextFacts?: { label: string; value: string }[];
  coverage?: string;
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
  const bucketDistribution = [
    'cell-size-distribution',
    'block-time-distribution',
    'epoch-time-distribution',
    'address-cohort-retention',
  ].includes(definition.slug);
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
  return {
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
      bucketDistribution || todayPoint
        ? null
        : last && previous
          ? exactDelta(previous.values.primary, last.values.primary, definition.slug)
          : null,
    summaryLabel: leadingBucket ? 'Largest bucket' : todayPoint ? 'Today' : undefined,
  };
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

function stackedChart(
  definition: ChartDefinition,
  payload: {
    title: string;
    data: {
      date: string;
      values: Record<string, string>;
      protocolTotalShannons?: string;
      phase1CompensationShannons?: string;
    }[];
    series: { key: string; label: string; color: string }[];
  }
): ChartModel {
  const selectedSeries =
    definition.slug === 'cell-count'
      ? payload.series.filter((series) => series.key !== 'allCells')
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
  const series = selectedSeries.map((entry, index) => ({
    key: entry.key,
    label: entry.label,
    // Gold belongs to DAO or a proposed state. Other charts share the cool hue ramp.
    color:
      definition.source === 'dao' || definition.source === 'supply'
        ? entry.color
        : ['#2edba3', '#68ccf0', '#8f9cdb', '#52b8a7', '#547fae', '#6fcfc2', '#879abb', '#a0a8be'][
            index % 8
          ],
  }));
  const last = points.at(-1);
  const previous = points.at(-2);
  const total = (point: ChartPoint): string => {
    const ratios = selectedSeries.map((item) =>
      exactDecimalRatio(point.values[item.key], `${definition.slug} ${item.key}`)
    );
    const denominator = ratios.reduce(
      (largest, [, divisor]) => (divisor > largest ? divisor : largest),
      BigInt(1)
    );
    const sum = ratios.reduce((acc, [numerator, divisor]) => {
      if (denominator % divisor !== BigInt(0))
        throw new Error(`${definition.slug}: incompatible decimal divisors`);
      return acc + numerator * (denominator / divisor);
    }, BigInt(0));
    const whole = sum / denominator;
    const fraction = (sum % denominator)
      .toString()
      .padStart(denominator.toString().length - 1, '0')
      .replace(/0+$/, '');
    return fraction ? `${whole}.${fraction}` : whole.toString();
  };
  const summarySeries =
    definition.slug === 'cell-count'
      ? selectedSeries.find((entry) => entry.key === 'liveCells')
      : undefined;
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
    ? summarySeries
      ? last.values[summarySeries.key]
      : isSecondaryIssuance
        ? protocolTotal(payload.data.at(-1)!)
        : leadingAge
          ? last.values[leadingAge.key]
          : total(last)
    : null;
  const previousTotal = previous
    ? summarySeries
      ? previous.values[summarySeries.key]
      : isSecondaryIssuance
        ? protocolTotal(payload.data.at(-2)!)
        : total(previous)
    : null;
  const holderCount = isComposition && payload.data.length > 0;
  if (holderCount && payload.data.some((point) => point.values.holderCount === undefined))
    throw new Error(`${definition.slug}: missing holderCount at returned point`);
  return {
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
    summaryLabel: summarySeries
      ? 'Live cells'
      : isSecondaryIssuance
        ? 'Protocol secondary issuance'
        : leadingAge
          ? `Largest age band: ${leadingAge.label}`
          : undefined,
    contextFacts:
      isSecondaryIssuance && payload.data.length
        ? [
            {
              label: 'Frozen phase-1 overlap',
              value: `${chartUnitValue(payload.data.at(-1)!.phase1CompensationShannons!, 'shannons', `${definition.slug} latest phase-1`)} CKB`,
            },
          ]
        : undefined,
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
  // The API's own bucket holds the ranks beyond its named list; say so, the card's "Others" differs.
  const rankedRows = sorted.map((entry) => ({
    label: entry.label === othersKey ? `Others beyond the top ${named.length}` : entry.label,
    value: entry.value,
    share: share(entry.shannons),
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
      ),
      summaryLabel,
    },
    historicalShares: histories,
    coverage: `${payload.capacityShare.data.length} owned-capacity dates`,
    summaryLabel,
  };
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
          display: rankedRows[0].value,
        }
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
    summaryLabel: rankedRows[0]
      ? `Leading miner: ${rankedRows[0].label} (${rankedRows[0].share})`
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
  }
  return model;
}
