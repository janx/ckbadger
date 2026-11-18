'use client';

import { useMemo, useState } from 'react';
import { BlocksOfDay } from '@/components/charts/blocks-of-day';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { Prose } from '@/components/ui/prose';
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
  bucketName,
  bucketUnit,
  categoricalAxis,
  chartModel,
  chartSpan,
  formatChartExact,
  formatChartHead,
  formatChartHover,
  formatEpochLabel,
  type ChartModel,
  type SubChart,
} from '@/components/charts/chart-model';
import { getChartDescription } from '@/components/charts/chart-calculation-descriptions';
import { chartAxisFormat, unitAfter, withUnit } from '@/lib/format-unit';
import { api } from '@/lib/api';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { exactSignedDecimalRatio } from '@/lib/exact-integer';
import { apiBaseFor } from '@/lib/active-network';
import { unitText } from '@/components/ui/units';

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

/**
 * A ranking, one row per entry (CH-03): the rank, the entry's name or shortened address as its
 * link, and its value at the row's right end — one line where the width holds it, else the value
 * under the name, still at the right. A miner known only by its lock hash is said once, under it.
 */
function RankedDetail({ model, definition }: { model: ChartModel; definition: ChartDefinition }) {
  const rows = model.rankedRows ?? [];
  return (
    <div className="max-w-3xl">
      <ol className="space-y-3" aria-label={`${definition.title} ranking`}>
        {rows.map((row, index) => {
          // the bar's width is the model's, from the exact parts (never the printed share)
          const width = row.width;
          if (!Number.isFinite(width) || width < 0 || width > 100)
            throw new Error(`${definition.slug}: invalid share width ${width} for ${row.label}`);
          return (
            <li key={`${row.label}:${index}`} className="min-w-0">
              <div className="text-small flex min-w-0 flex-wrap items-baseline gap-x-4">
                <span className="text-text-bright min-w-0 break-words">
                  {!row.remainder && `${index + 1}. `}
                  {row.assetClass && <ClassMark kind={row.assetClass} size={12} className="mr-1" />}
                  {row.href ? (
                    <Link href={row.href} title={row.title} className="text-jade hover:underline">
                      {unitText(row.label)}
                    </Link>
                  ) : (
                    unitText(row.label)
                  )}
                </span>
                <span className="ml-auto shrink-0 whitespace-nowrap tabular-nums">
                  {withUnit(formatChartExact(row.value, model.unit), model.unit)} · {row.share}
                </span>
              </div>
              <div className="bg-bar-track mt-1 h-1">
                <div className="bg-jade scan-fill h-1" style={{ width: `${width}%` }} />
              </div>
            </li>
          );
        })}
      </ol>
      {rows.some((row) => row.lockHashOnly) && (
        <p className="text-text-dim text-small mt-4">
          A row beginning 0x is a miner known only by its lock hash.
        </p>
      )}
    </div>
  );
}

/**
 * One big reading of the chart head: a small label over its figure in the head's form
 * (`formatChartHead`: a CKB amount cut to the summary form, a rate at two decimals), exact on hover.
 */
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
    <div className="flex flex-col items-end" title={withUnit(formatChartExact(raw, unit), unit)}>
      <span className="text-text-dim text-meta">{unitText(label)}</span>
      <span className="flex items-baseline">
        <span
          className={`text-hero font-medium tabular-nums ${quiet ? 'text-text' : 'text-text-bright'}`}
        >
          {formatChartHead(raw, unit)}
        </span>
        {/* One form per quantity (CH-14): 2.55%, 10.43 s — the unit never wraps away. */}
        <span className="text-text-dim text-meta whitespace-pre">{unitAfter(unit, '\u00a0')}</span>
      </span>
    </div>
  );
}

/**
 * The head of a chart under the main one: one dim line saying what it counts (CH-06) and its
 * figure under the label the page's head figure carries (CH-05).
 */
function SubChartHead({ sub }: { sub: SubChart }) {
  const key = sub.series[0].key;
  return (
    <div className="mb-3 flex min-w-0 flex-wrap items-end justify-between gap-x-6 gap-y-2">
      <p className="text-text-dim text-small max-w-measure min-w-0">{sub.note}</p>
      {sub.reading && (
        // keeps the right edge when a narrow width puts it under the note
        <div role="group" aria-label={`${sub.title} reading`} className="ml-auto">
          <Reading label={sub.reading.label} raw={sub.reading.point.values[key]} unit={sub.unit} />
        </div>
      )}
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
  // Every chart here pins a tapped point in a readout line under it (R6). The row's own "tap or
  // hover" is the section's one hint (C10), so its idle words say what it will read, not how.
  const readout = (title: string) => ({
    label: `${title} readout`,
    idle: `A ${style === 'date' ? 'day' : 'point'} on the chart reads its value here.`,
  });
  // A point read on the page: an epoch numbered, a bucket with its unit (CH-12, CH-35).
  const xUnit = bucketUnit(definition);
  const pointLabel = (label: string) =>
    style === 'epoch'
      ? `#${formatEpochLabel(label, definition.slug)}`
      : style === 'bucket'
        ? bucketName(label, xUnit)
        : label;
  // Method and evidence are the quieter developer tier, after related charts (CH-20).
  const method = (
    <DetailSection title="How it is computed / Evidence">
      <div className="text-small grid min-w-0 gap-4 md:grid-cols-2">
        <div className="border-base-border min-w-0 border-t pt-3">
          <div className="text-text-dim mb-2">Calculation scope</div>
          <Prose>
            <p>{description.overview}</p>
          </Prose>
          <ul className="text-text-dim max-w-measure mt-3 space-y-2">
            {description.legendItems.map((item) => (
              <li key={item.label}>
                <span className="text-text-bright">{item.label}:</span> {item.description}
              </li>
            ))}
          </ul>
        </div>
        <div className="border-base-border min-w-0 border-t pt-3">
          <div className="text-text-dim mb-2">Evidence path</div>
          <Prose>
            <p>
              Values come from the local index of canonical chain {definition.source}. The chart
              returns observations at its own recorded dates or buckets; gaps are not interpolated.
            </p>
          </Prose>
          <div className="mt-3 flex flex-col gap-1">
            {/* R-10 (CH-27): a dated block chart names the blocks of its latest day exactly. */}
            {definition.source === 'blocks' &&
              model.latest &&
              /^\d{4}-\d{2}-\d{2}$/.test(model.latest.label) && (
                <BlocksOfDay date={model.latest.label} />
              )}
            {/* Lane L: every chart links the topic page its numbers come from. */}
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
          {model.evidence?.map((fact) => (
            <div key={fact.label} className="text-text-dim mt-2">
              {fact.label}: <span className="text-text">{fact.value}</span>
            </div>
          ))}
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
          style === 'bucket'
            ? `most common, ${pointLabel(model.latest.label)}`
            : pointLabel(model.latest.label)
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
    // A named leader is the bright part of its value, never broken (CH-03, CH-18).
    ...(model.facts ?? []).map((fact) => ({
      label: fact.label,
      value: fact.name ? (
        <>
          <span className="whitespace-nowrap">{fact.name}</span> · {fact.value}
        </>
      ) : (
        fact.value
      ),
    })),
    ...(model.delta
      ? [
          {
            label: 'Last change',
            value: withUnit(formatChartHover(model.delta, model.unit), model.unit),
          },
        ]
      : []),
  ];
  return (
    <>
      <nav aria-label="Breadcrumb" className="text-small mb-3">
        <Link
          href={`/charts#${chartSectionId(definition.section)}`}
          className="text-text-dim hover:text-jade"
        >
          Charts / {definition.section}
        </Link>
      </nav>
      <DetailLayout
        title={definition.title}
        documentTitle={[definition.title, 'chart']}
        identity={definition.subtitle}
        actions={readings}
        facts={facts.length ? facts : undefined}
        tier2={method}
      >
        {/* The command runs on what it draws (rubbing × Song print, phase 5): `full history 1,204
            days`, `distribution 24 buckets`; a ranked chart counts nothing here. */}
        <DetailSection
          // A model curve is a model of its years, not a history (CH-19).
          title={distribution ? 'Distribution' : style === 'model-year' ? 'Model' : 'Full history'}
          args={model.kind !== 'ranked' ? chartSpan(points, style) : undefined}
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
                    className={`text-small border px-3 py-1 ${range === option ? 'border-jade text-text-bright bg-jade/10' : 'border-base-border text-text-dim hover:text-text-bright'}`}
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
                  className={`text-small border px-3 py-1 ${logScale ? 'border-jade text-text-bright bg-jade/10' : 'border-base-border text-text-dim hover:text-text-bright'}`}
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
                  className="border-base-border text-text-dim hover:text-text-bright text-small border px-2 py-1"
                >
                  ?
                </button>
              )}
            </div>
          </div>
          {helpOpen && (
            <p className="text-text-dim text-small max-w-measure mb-4 leading-5">
              {/* C10: the readout row under the chart already says "tap or hover". */}
              The line under the chart reads one {style === 'date' ? 'day' : 'point'}: select it to
              keep it there, select it again to let it go. Or focus the chart and use ← and → to
              step through; Esc returns to the latest reading.
              {hasDatedHistory && ' 30 d, 90 d and 1 y zoom to the most recent days.'}
              {supportsLog && ' Log scale spreads a series that grew by orders of magnitude.'}
            </p>
          )}
          {model.latest === null ? (
            <p className="text-text-dim text-small py-8">No indexed observations for this chart.</p>
          ) : model.kind === 'ranked' ? (
            <RankedDetail model={model} definition={definition} />
          ) : (
            <Chart
              title={definition.title}
              subtitle={`${definition.subtitle} · ${chartSpan(points, style)}`}
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
              // A histogram reads its most common bucket, cohorts the last complete one, a model
              // its today (CH-30, CH-19, CH-03).
              reading={model.reading ?? model.today}
              formatValue={(value) => formatChartHover(value, model.unit)}
              // F2 charts: the board's detail grammar (CH-12, CH-15, CH-22, CH-23, CH-24, CH-35).
              xTicks={axisTicks(points, style, 8, xUnit, model.bucketRun)}
              formatAxis={chartAxisFormat(model.unit)}
              pointDots
              slotBars={style === 'bucket' || style === 'month'}
              categoricalX={categoricalAxis(definition)}
              hideMarkerLegend
              yDomain={model.yDomain}
              referenceLines={model.referenceLines}
              formatLabel={(label) =>
                style === 'epoch' ? `epoch ${pointLabel(label)}` : pointLabel(label)
              }
              pinnedReadout={readout(definition.title)}
              // The caption is hidden, so several bands (the HODL wave's eight) get a swatch key
              // under the plot: name and value at the reading, latest by default (C#4).
              seriesLegend={model.series.length > 1}
            />
          )}
        </DetailSection>
        {model.historicalShares?.map((history) => {
          const shown = rangePoints(history.points, range);
          const span = chartSpan(shown, 'date');
          // Named once, by the command line; the note says what is counted; the chart's caption
          // is its own quantity and span, for assistive technology (CH-04, CH-06).
          return (
            <DetailSection key={history.title} title={history.title} args={span}>
              <SubChartHead sub={history} />
              {history.points.length ? (
                <Chart
                  title={history.title}
                  subtitle={`${history.caption} · ${span}`}
                  data={shown}
                  series={history.series}
                  unit={history.unit}
                  kind="stacked"
                  height={260}
                  hideCaption
                  formatValue={(value) => formatChartHover(value, history.unit)}
                  xTicks={axisTicks(shown, 'date', 8)}
                  formatAxis={chartAxisFormat(history.unit)}
                  pointDots
                  // Up to 21 bands: a swatch key under the plot, not headline readouts (C#5).
                  seriesLegend
                  pinnedReadout={readout(history.title)}
                />
              ) : (
                <p className="text-text-dim text-small py-8">
                  No indexed observations for this scope.
                </p>
              )}
            </DetailSection>
          );
        })}
        {/* A chart under the main one is named once, by its command line; one dim line says
            what it counts; its own title and caption are for assistive technology (CH-06). */}
        {model.secondary && model.secondary.points.length > 0 && (
          <DetailSection title={model.secondary.title} args={chartSpan(secondaryPoints, style)}>
            <SubChartHead sub={model.secondary} />
            <Chart
              title={model.secondary.title}
              reading={model.secondary.reading?.point}
              subtitle={`${model.secondary.caption} · ${chartSpan(secondaryPoints, style)}`}
              hideCaption
              data={secondaryPoints}
              series={model.secondary.series}
              unit={model.secondary.unit}
              kind={definition.visual === 'bars' ? 'bars' : 'line'}
              height={220}
              formatValue={(value) => formatChartHover(value, model.secondary!.unit)}
              xTicks={axisTicks(secondaryPoints, style, 8, xUnit)}
              formatAxis={chartAxisFormat(model.secondary.unit)}
              formatLabel={pointLabel}
              pointDots
              slotBars={style === 'bucket' || style === 'month'}
              categoricalX={categoricalAxis(definition)}
              pinnedReadout={readout(model.secondary.title)}
            />
          </DetailSection>
        )}
        {model.dailyFlow && model.dailyFlow.points.length > 0 && (
          <DetailSection
            title={model.dailyFlow.title}
            args={chartSpan(rangePoints(model.dailyFlow.points, range), 'date')}
          >
            <SubChartHead sub={model.dailyFlow} />
            <Chart
              title={model.dailyFlow.title}
              reading={model.dailyFlow.reading?.point}
              subtitle={`${model.dailyFlow.caption} · ${chartSpan(rangePoints(model.dailyFlow.points, range), 'date')}`}
              hideCaption
              data={rangePoints(model.dailyFlow.points, range)}
              series={model.dailyFlow.series}
              unit={model.dailyFlow.unit}
              kind="line"
              height={220}
              formatValue={(value) => formatChartHover(value, model.dailyFlow!.unit)}
              xTicks={axisTicks(rangePoints(model.dailyFlow.points, range), 'date', 8)}
              formatAxis={chartAxisFormat(model.dailyFlow.unit)}
              pointDots
              pinnedReadout={readout(model.dailyFlow.title)}
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
    <div className="min-h-screen">
      <main className="container mx-auto min-w-0 py-8">
        {sync.data?.syncStatus?.chartDataMayBeIncomplete && (
          <p
            role="status"
            className="text-warning border-warning/30 bg-warning/10 text-small mb-5 border px-3 py-2"
          >
            Chart data may be incomplete while the indexer synchronizes history.
          </p>
        )}
        {isAwaited(data) && (
          <div
            className="bg-base-surface h-96 animate-pulse"
            aria-label={`${definition.title} loading`}
          />
        )}
        {data.error && !model && !modelError && (
          <DetailLayout title={definition.title} documentTitle={[definition.title, 'chart']}>
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
          <DetailLayout title={definition.title} documentTitle={[definition.title, 'chart']}>
            <p role="alert" className="text-negative max-w-measure break-words">
              Invalid {definition.title} chart data: {modelError}
            </p>
          </DetailLayout>
        )}
        {data.error && model && (
          <p role="alert" className="text-negative text-small max-w-measure mb-5 break-words">
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
          <p role="alert" className="text-negative text-small max-w-measure mt-4">
            Hardfork markers unavailable: {hardforks.error.message}
          </p>
        )}
      </main>
    </div>
  );
}
