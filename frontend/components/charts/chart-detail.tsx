'use client';

import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { Chart, type ChartMarker, type ChartPoint } from '@/components/ui/chart';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import {
  CHART_BY_SLUG,
  chartApiPath,
  type ChartDefinition,
} from '@/components/charts/chart-catalogue';
import {
  chartModel,
  formatChartExact,
  formatChartHover,
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
    return [{ index: first, label: event.shortName, plotLabel: '◆', href: '/hardforks' }];
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
  const points = useMemo(() => rangePoints(model.points, range), [model.points, range]);
  const secondaryPoints = useMemo(
    () => (model.secondary ? rangePoints(model.secondary.points, range) : []),
    [model.secondary, range]
  );
  const markers = chartHardforkMarkers(definition, points, events);
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
  // Method and evidence are the quieter developer tier, after related charts (CH-20).
  const method = (
    <DetailSection title="How it is computed / Evidence">
      <div className="grid min-w-0 gap-4 text-xs md:grid-cols-2">
        <div className="border-base-border min-w-0 border-t pt-3">
          <div className="text-text-dim mb-2">Calculation scope</div>
          <p className="text-text-bright leading-6">{description.overview}</p>
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
          <Link
            href={definition.evidenceHref}
            className="text-jade mt-3 inline-block hover:underline"
          >
            Explore {definition.evidenceLabel} →
          </Link>
          <div className="text-text-dim mt-2">
            Source:{' '}
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
  return (
    <DetailLayout
      title={definition.title}
      identity={`${definition.section} · ${definition.subtitle}`}
      facts={
        model.latest
          ? [
              {
                label: model.summaryLabel ?? 'Latest returned',
                value: `${model.latest.label} · ${formatChartExact(model.latest.raw, model.unit)} ${model.unit}`,
              },
              ...(model.delta
                ? [
                    {
                      label: 'Last change',
                      value: `${formatChartExact(model.delta, model.unit)} ${model.unit}`,
                    },
                  ]
                : []),
              ...(model.coverage ? [{ label: 'Coverage', value: model.coverage }] : []),
              ...(model.contextFacts ?? []),
            ]
          : undefined
      }
      tier2={method}
    >
      <DetailSection
        title={definition.visual === 'ranked' ? 'Distribution' : 'Full history'}
        actions={
          <Link href="/charts" className="text-text-dim hover:text-jade text-xs">
            ← All charts
          </Link>
        }
      >
        {(hasDatedHistory || supportsLog) && (
          <div role="group" aria-label="Chart range" className="mb-4 flex flex-wrap gap-1">
            {hasDatedHistory &&
              (['30d', '90d', '1y', 'all'] as const).map((option) => (
                <button
                  key={option}
                  type="button"
                  aria-pressed={range === option}
                  onClick={() => setRange(option)}
                  className={`rounded border px-3 py-1 text-xs ${range === option ? 'border-jade text-text-bright bg-jade/10' : 'border-base-border text-text-dim hover:text-text-bright'}`}
                >
                  {option.toUpperCase()}
                </button>
              ))}
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
          </div>
        )}
        {model.latest === null ? (
          <p className="text-text-dim py-8 text-xs">No indexed observations for this chart.</p>
        ) : model.kind === 'ranked' ? (
          <RankedDetail model={model} definition={definition} />
        ) : (
          <Chart
            title={definition.title}
            subtitle={`${definition.subtitle} · ${points.length} returned observations`}
            data={points}
            series={model.series}
            unit={model.unit}
            kind={model.kind}
            yScale={supportsLog && logScale ? 'log' : 'linear'}
            height={360}
            markers={markers}
            projectedAfter={range === 'all' ? model.projectedAfter : undefined}
            formatValue={(value) => formatChartHover(value, model.unit)}
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
            subtitle={definition.subtitle}
            data={secondaryPoints}
            series={model.secondary.series}
            unit={model.secondary.unit}
            kind={definition.visual === 'bars' ? 'bars' : 'line'}
            height={220}
            formatValue={(value) => formatChartHover(value, model.secondary!.unit)}
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
