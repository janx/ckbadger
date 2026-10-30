'use client';

import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { Chart, type ChartMarker, type ChartPoint } from '@/components/ui/chart';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { ClassMark } from '@/components/ui/class-mark';
import {
  CHART_BY_SLUG,
  chartApiPath,
  chartSectionId,
  type ChartDefinition,
} from '@/components/charts/chart-catalogue';
import {
  axisStyle,
  axisTicks,
  chartModel,
  formatChartExact,
  formatChartHover,
  formatEpochLabel,
  type ChartModel,
} from '@/components/charts/chart-model';
import { getChartDescription } from '@/components/charts/chart-calculation-descriptions';
import { api } from '@/lib/api';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { exactSignedDecimalRatio } from '@/lib/exact-integer';
import { apiBaseFor } from '@/lib/active-network';

export type ChartRange = '30d' | '90d' | '1y' | 'all';

export function rangePoints(points: readonly ChartPoint[], range: ChartRange): ChartPoint[] {
  if (range === 'all' || points.length === 0) return [...points];
  const last = points.at(-1)!.label;
  if (!/^\d{4}-\d{2}-\d{2}$/.test(last)) return [...points];
  const end = Date.parse(`${last}T00:00:00.000Z`);
  if (!Number.isFinite(end)) throw new Error(`invalid indexed chart date ${last}`);
  const days = range === '30d' ? 30 : range === '90d' ? 90 : 365;
  const first = new Date(end - (days - 1) * 86_400_000).toISOString().slice(0, 10);
  return points.filter((point) => point.label >= first && point.label <= last);
}

type HardforkEvent = Awaited<ReturnType<typeof api.getHardforks>>['events'][number];
export function chartHardforkMarkers(
  definition: ChartDefinition,
  points: readonly ChartPoint[],
  events: readonly HardforkEvent[]
): ChartMarker[] {
  if (!points.length || definition.visual === 'bars' || definition.visual === 'ranked') return [];
  const epoch = definition.slug === 'epoch-time-length';
  if (
    epoch &&
    points.some((point) => !/^\d+$/.test(point.label) || !Number.isSafeInteger(Number(point.label)))
  )
    throw new Error(`${definition.slug}: invalid indexed epoch label`);
  if (!epoch && !/^\d{4}-\d{2}-\d{2}$/.test(points[0].label)) return [];
  return events.flatMap((event) => {
    if (event.status !== 'activated') return [];
    const eventKey = epoch ? String(event.activationEpoch) : event.activationDate.slice(0, 10);
    const first = points.findIndex((point) =>
      epoch ? Number(point.label) >= event.activationEpoch : point.label >= eventKey
    );
    const outside = epoch
      ? event.activationEpoch < Number(points[0].label) ||
        event.activationEpoch > Number(points.at(-1)!.label)
      : eventKey < points[0].label || eventKey > points.at(-1)!.label;
    if (first < 0 || outside) return [];
    // Named in the plot, like the board; the evidence links the hardforks page (CH-23).
    return [
      { index: first, label: event.shortName, plotLabel: event.shortName, href: '/hardforks' },
    ];
  });
}

function RankedDetail({ model, definition }: { model: ChartModel; definition: ChartDefinition }) {
  return (
    <ol className="max-w-3xl space-y-3" aria-label={`${definition.title} ranking`}>
      {model.rankedRows?.map((row, index) => {
        const width = row.share === '—' ? 0 : Number(row.share.slice(0, -1));
        if (!Number.isFinite(width) || width < 0 || width > 100)
          throw new Error(`${definition.slug}: invalid share ${row.share}`);
        return (
          <li key={`${row.label}:${index}`} className="min-w-0">
            <div className="flex min-w-0 flex-wrap items-baseline justify-between gap-x-4 text-xs">
              <span className="text-text-bright min-w-0 break-all">
                {index + 1}.{' '}
                {row.assetClass && <ClassMark kind={row.assetClass} size={12} className="mr-1" />}
                {row.href ? (
                  <Link href={row.href} className="text-jade hover:underline">
                    {row.label}
                  </Link>
                ) : (
                  row.label
                )}
                {row.identity && <span className="text-text-dim ml-2">({row.identity})</span>}
              </span>
              <span className="shrink-0 tabular-nums">
                {formatChartExact(row.value, model.unit)} {model.unit} · {row.share}
              </span>
            </div>
            <div className="bg-base-elevated mt-1 h-1 rounded">
              <div className="bg-jade h-1 rounded" style={{ width: `${width}%` }} />
            </div>
          </li>
        );
      })}
    </ol>
  );
}

/** How the chart's points are counted under its title (CH-36). */
const POINT_NOUN = {
  date: 'days',
  month: 'cohorts',
  epoch: 'epochs',
  'model-year': 'model samples',
  bucket: 'buckets',
} as const;

/** One big reading of the chart head: a small label over a rounded value, exact on hover. */
function Reading({
  label,
  raw,
  unit,
  quiet = false,
}: {
  label: string;
  raw: string;
  unit: string;
  quiet?: boolean;
}) {
  return (
    <div className="flex flex-col items-end" title={`${formatChartExact(raw, unit)} ${unit}`}>
      <span className="text-text-dim text-[11px]">{label}</span>
      <span className="flex items-baseline gap-1">
        <span
          className={`text-[22px] font-medium tabular-nums ${quiet ? 'text-text' : 'text-text-bright'}`}
        >
          {formatChartHover(raw, unit)}
        </span>
        <span className="text-text-dim text-[11px]">{unit}</span>
      </span>
    </div>
  );
}

function DetailBody({
  definition,
  model,
  events,
}: {
  definition: ChartDefinition;
  model: ChartModel;
  events: readonly HardforkEvent[];
}) {
  const network = useActiveNetwork();
  const [range, setRange] = useState<ChartRange>('all');
  const [logScale, setLogScale] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const points = useMemo(() => rangePoints(model.points, range), [model.points, range]);
  const secondaryPoints = useMemo(
    () => (model.secondary ? rangePoints(model.secondary.points, range) : []),
    [model.secondary, range]
  );
  const markers = chartHardforkMarkers(definition, points, events);
  // Every activated hardfork the full history crosses, linked from the evidence (CH-23).
  const crossed = chartHardforkMarkers(definition, model.points, events).flatMap((marker) =>
    events.filter((event) => event.shortName === marker.label)
  );
  const style = axisStyle(definition);
  const distribution = definition.visual === 'ranked' || style === 'bucket';
  const hasDatedHistory =
    (model.points.length > 0 &&
      model.points.every((point) => /^\d{4}-\d{2}-\d{2}$/.test(point.label))) ||
    !!model.historicalShares?.some(
      (history) =>
        history.points.length > 0 &&
        history.points.every((point) => /^\d{4}-\d{2}-\d{2}$/.test(point.label))
    );
  const supportsLog =
    model.kind === 'line' &&
    model.points.length > 0 &&
    model.points.every((point) =>
      model.series.every(
        (series) =>
          exactSignedDecimalRatio(point.values[series.key], `${definition.slug} log scale`)[0] >
          BigInt(0)
      )
    );
  const description = getChartDescription(definition.queryKey, {
    yAxisLabel: model.series[0]?.label,
    seriesLabels: model.series.map((series) => series.label),
  });
  if (!description) throw new Error(`${definition.slug}: no calculation description`);
  const pointLabel = (label: string) =>
    style === 'epoch' ? `#${formatEpochLabel(label, definition.slug)}` : label;
  // Method and evidence are the quieter developer tier, after related charts (CH-20).
  const method = (
    <DetailSection title="How it is computed / Evidence">
      <div className="grid min-w-0 gap-4 text-xs md:grid-cols-2">
        <div className="border-base-border min-w-0 border-t pt-3">
          <div className="text-text-dim mb-2">Calculation scope</div>
          <p className="text-text-bright max-w-[62ch] leading-6">{description.overview}</p>
          <ul className="text-text-dim mt-3 space-y-2">
            {description.legendItems.map((item) => (
              <li key={item.label}>
                <span className="text-text-bright">{item.label}:</span> {item.description}
              </li>
            ))}
          </ul>
        </div>
        <div className="border-base-border min-w-0 border-t pt-3">
          <div className="text-text-dim mb-2">Evidence path</div>
          <p className="leading-6">
            Values come from the local index of canonical chain {definition.source}. The chart
            returns observations at its own recorded dates or buckets; gaps are not interpolated.
          </p>
          <div className="mt-3 flex flex-col gap-1">
            <Link href={definition.evidenceHref} className="text-jade hover:underline">
              Explore {definition.evidenceLabel} →
            </Link>
            {crossed.length > 0 && (
              <Link href="/hardforks" className="text-jade hover:underline">
                Hardforks:{' '}
                {crossed
                  .map(
                    (event) =>
                      `${event.shortName}, epoch ${formatEpochLabel(String(event.activationEpoch), definition.slug)}`
                  )
                  .join(' · ')}{' '}
                →
              </Link>
            )}
          </div>
          <div className="text-text-dim mt-2">
            Raw series:{' '}
            <a
              href={`${apiBaseFor(network)}${chartApiPath(definition)}`}
              className="text-jade break-all hover:underline"
            >
              <code>{chartApiPath(definition)}</code>
            </a>
          </div>
        </div>
      </div>
    </DetailSection>
  );
  // The head reads the latest value and the API's peak, rates at the board's two decimals (CH-02,
  // CH-21); a semantic summary (leader, protocol total) stays a fact under it.
  const readings = model.latest && (
    <div
      role="group"
      aria-label={`${definition.title} readings`}
      className="flex flex-wrap items-baseline justify-end gap-7"
    >
      <Reading
        label={
          style === 'bucket' ? `most common, ${model.latest.label}` : pointLabel(model.latest.label)
        }
        raw={model.latest.raw}
        unit={model.unit}
      />
      {model.peak && (
        <Reading
          label={`peak, ${pointLabel(model.peak.label)}`}
          raw={model.peak.raw}
          unit={model.unit}
          quiet
        />
      )}
    </div>
  );
  const facts = [
    ...(model.latest && model.summaryLabel
      ? [
          {
            label: model.summaryLabel,
            value: `${pointLabel(model.latest.label)} · ${formatChartHover(model.latest.raw, model.unit)} ${model.unit}`,
          },
        ]
      : []),
    ...(model.delta
      ? [
          {
            label: 'Last change',
            value: `${formatChartHover(model.delta, model.unit)} ${model.unit}`,
          },
        ]
      : []),
    ...(model.coverage ? [{ label: 'Coverage', value: model.coverage }] : []),
    ...(model.contextFacts ?? []),
  ];
  return (
    <>
      <nav aria-label="Breadcrumb" className="mb-3 text-[12px]">
        <Link
          href={`/charts#${chartSectionId(definition.section)}`}
          className="text-text-dim hover:text-jade"
        >
          Charts / {definition.section}
        </Link>
      </nav>
      <DetailLayout
        title={definition.title}
        identity={definition.subtitle}
        actions={readings}
        facts={facts.length ? facts : undefined}
        tier2={method}
      >
        <DetailSection
          title={distribution ? 'Distribution' : 'Full history'}
          actions={
            model.kind !== 'ranked' && (
              <span className="text-text-dim text-[12px]">
                {points.length.toLocaleString()} {POINT_NOUN[style]}
              </span>
            )
          }
        >
          <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
            <div role="group" aria-label="Chart range" className="flex flex-wrap gap-1">
              {hasDatedHistory &&
                (
                  [
                    ['30d', '30 d'],
                    ['90d', '90 d'],
                    ['1y', '1 y'],
                    ['all', 'All'],
                  ] as const
                ).map(([option, label]) => (
                  <button
                    key={option}
                    type="button"
                    aria-pressed={range === option}
                    onClick={() => setRange(option)}
                    className={`rounded border px-3 py-1 text-xs ${range === option ? 'border-jade text-text-bright bg-jade/10' : 'border-base-border text-text-dim hover:text-text-bright'}`}
                  >
                    {label}
                  </button>
                ))}
            </div>
            <div className="flex gap-1">
              {supportsLog && (
                <button
                  type="button"
                  aria-pressed={logScale}
                  onClick={() => setLogScale((current) => !current)}
                  className={`rounded border px-3 py-1 text-xs ${logScale ? 'border-jade text-text-bright bg-jade/10' : 'border-base-border text-text-dim hover:text-text-bright'}`}
                >
                  Log scale
                </button>
              )}
              {model.kind !== 'ranked' && (
                <button
                  type="button"
                  aria-label="How to read and zoom this chart"
                  aria-expanded={helpOpen}
                  onClick={() => setHelpOpen((open) => !open)}
                  className="border-base-border text-text-dim hover:text-text-bright rounded border px-2 py-1 text-xs"
                >
                  ?
                </button>
              )}
            </div>
          </div>
          {helpOpen && (
            <p className="text-text-dim mb-4 max-w-[62ch] text-xs leading-5">
              Hover the chart for one {style === 'date' ? 'day' : 'point'}&apos;s value, or focus it
              and use ← and → to step through; Esc returns to the latest reading.
              {hasDatedHistory && ' 30 d, 90 d and 1 y zoom to the most recent days.'}
              {supportsLog && ' Log scale spreads a series that grew by orders of magnitude.'}
            </p>
          )}
          {model.latest === null ? (
            <p className="text-text-dim py-8 text-xs">No indexed observations for this chart.</p>
          ) : model.kind === 'ranked' ? (
            <RankedDetail model={model} definition={definition} />
          ) : (
            <Chart
              title={definition.title}
              subtitle={`${definition.subtitle} · ${points.length} ${POINT_NOUN[style]}`}
              data={points}
              series={model.series}
              unit={model.unit}
              kind={model.kind}
              yScale={supportsLog && logScale ? 'log' : 'linear'}
              height={360}
              // The page head already names the chart and reads it (visual pass).
              hideCaption
              markers={markers}
              projectedAfter={range === 'all' ? model.projectedAfter : undefined}
              // A histogram reads its most common bucket, a model its today (CH-30, CH-03).
              reading={
                style === 'bucket'
                  ? { label: model.latest.label, values: { primary: model.latest.raw } }
                  : model.today
              }
              formatValue={(value) => formatChartHover(value, model.unit)}
              // F2 charts: the board's detail grammar (CH-12, CH-15, CH-22, CH-23, CH-24, CH-35).
              xTicks={axisTicks(points, style, 8)}
              pointDots
              slotBars={style === 'bucket' || style === 'month'}
              hideMarkerLegend
              yDomain={model.yDomain}
              referenceLines={model.referenceLines}
              formatLabel={(label) => (style === 'epoch' ? `epoch ${pointLabel(label)}` : label)}
            />
          )}
        </DetailSection>
        {model.historicalShares?.map((history) => (
          <DetailSection key={history.title} title={history.title}>
            <p className="text-text-dim mb-3 text-xs">
              {history.title === 'Occupied common knowledge'
                ? 'Exact occupied CKB in live cells attributed to each script or asset.'
                : 'Full live cell capacity attributed to each script or asset. This is a different scope from occupied common knowledge.'}
            </p>
            {history.points.length ? (
              <Chart
                title={history.title}
                subtitle={`${definition.subtitle} · ${history.points.length} returned dates`}
                data={rangePoints(history.points, range)}
                series={history.series}
                unit={history.unit}
                kind="stacked"
                height={260}
                formatValue={(value) => formatChartHover(value, history.unit)}
                xTicks={axisTicks(rangePoints(history.points, range), 'date', 8)}
                pointDots
              />
            ) : (
              <p className="text-text-dim py-8 text-xs">No indexed observations for this scope.</p>
            )}
          </DetailSection>
        ))}
        {model.secondary && model.secondary.points.length > 0 && (
          <DetailSection title={model.secondary.title}>
            <p className="text-text-dim mb-3 text-xs">
              Separate scale: {model.secondary.unit}. These values are never combined with{' '}
              {model.unit}.
            </p>
            <Chart
              title={model.secondary.title}
              subtitle={`${model.secondary.title} · ${secondaryPoints.length.toLocaleString()} ${POINT_NOUN[style]}`}
              data={secondaryPoints}
              series={model.secondary.series}
              unit={model.secondary.unit}
              kind={definition.visual === 'bars' ? 'bars' : 'line'}
              height={220}
              formatValue={(value) => formatChartHover(value, model.secondary!.unit)}
              xTicks={axisTicks(secondaryPoints, style, 8)}
              pointDots
              slotBars={style === 'bucket' || style === 'month'}
            />
          </DetailSection>
        )}
        {model.dailyFlow && model.dailyFlow.points.length > 0 && (
          <DetailSection title={model.dailyFlow.title}>
            <p className="text-text-dim mb-3 text-xs">
              Exact signed change in occupied CKB from the preceding returned day.
            </p>
            <Chart
              title={model.dailyFlow.title}
              subtitle="Daily change in common knowledge"
              data={rangePoints(model.dailyFlow.points, range)}
              series={model.dailyFlow.series}
              unit={model.dailyFlow.unit}
              kind="line"
              height={220}
              formatValue={(value) => formatChartHover(value, model.dailyFlow!.unit)}
              xTicks={axisTicks(rangePoints(model.dailyFlow.points, range), 'date', 8)}
              pointDots
            />
          </DetailSection>
        )}
        <DetailSection title="Related charts">
          <div className="grid min-w-0 gap-x-10 gap-y-8 sm:grid-cols-2 xl:grid-cols-4">
            {definition.related.map((slug) => {
              const related = CHART_BY_SLUG.get(slug);
              if (!related) throw new Error(`${definition.slug}: unknown related chart ${slug}`);
              return <ChartSmallMultiple key={slug} definition={related} />;
            })}
          </div>
        </DetailSection>
      </DetailLayout>
    </>
  );
}

export function ChartDetail({ slug }: { slug: string }) {
  const definition = CHART_BY_SLUG.get(slug);
  if (!definition) throw new Error(`unknown chart route ${slug}`);
  const network = useActiveNetwork();
  const data = useQuery({ queryKey: [definition.queryKey], queryFn: definition.fetch });
  const hardforks = useQuery({
    queryKey: ['hardforks-for-charts', network],
    queryFn: () => api.getHardforks({ network }),
    staleTime: 60_000,
  });
  const sync = useQuery({ queryKey: ['network-stats'], queryFn: api.getNetworkStats });
  let model: ChartModel | null = null;
  let modelError: string | null = null;
  if (data.data) {
    try {
      model = chartModel(definition, data.data);
    } catch (cause) {
      modelError = cause instanceof Error ? cause.message : String(cause);
    }
  }
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8">
        {sync.data?.syncStatus?.chartDataMayBeIncomplete && (
          <p
            role="status"
            className="text-warning border-warning/30 bg-warning/10 mb-5 rounded border px-3 py-2 text-xs"
          >
            Chart data may be incomplete while the indexer synchronizes history.
          </p>
        )}
        {data.isLoading && (
          <div
            className="bg-base-surface h-96 animate-pulse rounded"
            aria-label={`${definition.title} loading`}
          />
        )}
        {data.error && !model && !modelError && (
          <DetailLayout title={definition.title}>
            <div role="alert" className="text-negative break-words">
              Failed to load {definition.title}: {data.error.message}
              <button
                type="button"
                onClick={() => void data.refetch()}
                className="text-jade ml-2 underline"
              >
                Retry
              </button>
            </div>
          </DetailLayout>
        )}
        {modelError && (
          <DetailLayout title={definition.title}>
            <p role="alert" className="text-negative break-words">
              Invalid {definition.title} chart data: {modelError}
            </p>
          </DetailLayout>
        )}
        {data.error && model && (
          <p role="alert" className="text-negative mb-5 break-words text-xs">
            Could not refresh {definition.title}: {data.error.message}. Showing previously loaded
            data.
            <button
              type="button"
              onClick={() => void data.refetch()}
              className="text-jade ml-2 underline"
            >
              Retry
            </button>
          </p>
        )}
        {model && (
          <DetailBody definition={definition} model={model} events={hardforks.data?.events ?? []} />
        )}
        {hardforks.error && (
          <p role="alert" className="text-negative mt-4 text-xs">
            Hardfork markers unavailable: {hardforks.error.message}
          </p>
        )}
      </main>
    </div>
  );
}
