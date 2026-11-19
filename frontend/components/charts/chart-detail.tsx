'use client';

import { useMemo, useState, type ReactNode } from 'react';
import { BlocksOfDay } from '@/components/charts/blocks-of-day';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { ExternalLink } from '@/components/ui/external-link';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { Prose } from '@/components/ui/prose';
import { Chart, type ChartMarker, type ChartPoint } from '@/components/ui/chart';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { ClassMark } from '@/components/ui/class-mark';
import { RangeSelect, type RangeOption } from '@/components/ui/range-select';
import { FilterChips, type FilterValue } from '@/components/ui/filter-chips';
import { HelpPopover } from '@/components/ui/help-popover';
import {
  CHART_BY_SLUG,
  chartApiPath,
  chartSectionId,
  type ChartDefinition,
} from '@/components/charts/chart-catalogue';
import {
  axisOutliers,
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
  type AxisStyle,
  type ChartModel,
  type SubChart,
} from '@/components/charts/chart-model';
import { capNote, chartHead, type ChartHead } from '@/components/charts/chart-head';
import { getChartDescription } from '@/components/charts/chart-calculation-descriptions';
import { chartAxisFormat, unitAfter, withUnit } from '@/lib/format-unit';
import { api } from '@/lib/api';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { useFromSm } from '@/hooks/useFromMd';
import { exactSignedDecimalRatio } from '@/lib/exact-integer';
import { apiBaseFor } from '@/lib/active-network';
import { unitText } from '@/components/ui/units';
import { cn } from '@/lib/utils';

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

/** The one spelling of a span (the controls board): the range of a dated chart. */
const RANGES: readonly RangeOption<ChartRange>[] = [
  { key: '30d', label: '30 d' },
  { key: '90d', label: '90 d' },
  { key: '1y', label: '1 y' },
  { key: 'all', label: 'all' },
];

type Scale = 'linear' | 'log';
/** The scale of a series that can be drawn on a log axis: one of two, so a filter of two chips. */
const SCALES: readonly FilterValue<Scale>[] = [
  { key: 'linear', label: 'linear' },
  { key: 'log', label: 'log' },
];

/**
 * What a plot's name says after its unit (rule 4): the span its points cover, in the page's words —
 * `every complete day since 2019-11-16`, `every completed epoch since #0`.
 */
function spanWords(points: readonly ChartPoint[], style: AxisStyle): string {
  const first = points[0]?.label;
  if (first === undefined) return 'no points';
  switch (style) {
    case 'date':
      return `every complete day since ${first}`;
    case 'epoch':
      return `every completed epoch since #${formatEpochLabel(first, 'chart span')}`;
    case 'month':
      return `every first-seen month since ${first}`;
    case 'model-year':
      return `year ${first} to year ${points.at(-1)!.label}`;
    case 'bucket':
      return chartSpan(points, 'bucket');
  }
}

/**
 * A ranking, one row per entry (CH-03): the rank, the entry's name or shortened address as its
 * link, and its value at the row's right end — one line where the width holds it, else the value
 * under the name, still at the right. A miner known only by its lock hash is said once, under it.
 * The ranking is the table of every entry the page's stacks merge (rule 5).
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
                    <Link href={row.href} title={row.title} look="key">
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
        <span className="text-text-dim text-small mt-4 block">
          A row beginning 0x is a miner known only by its lock hash.
        </span>
      )}
    </div>
  );
}

/**
 * A sub-chart's reading (CH-05): a small label over its figure in the head's form, exact on hover.
 */
function Reading({ label, raw, unit }: { label: string; raw: string; unit: string }) {
  return (
    <div className="flex flex-col items-end" title={withUnit(formatChartExact(raw, unit), unit)}>
      <span className="text-text-dim text-meta">{unitText(label)}</span>
      <span className="flex items-baseline">
        <span className="text-section text-text-bright font-medium tabular-nums">
          {formatChartHead(raw, unit)}
        </span>
        <span className="text-text-dim text-meta whitespace-pre">{unitAfter(unit, ' ')}</span>
      </span>
    </div>
  );
}

/**
 * The head of a chart under the main one: one line saying what it counts (CH-06) and its figure
 * under the label the page's head figure carries (CH-05).
 */
function SubChartHead({ sub }: { sub: SubChart }) {
  const key = sub.series[0].key;
  return (
    <div className="mb-3 flex min-w-0 flex-wrap items-end justify-between gap-x-6 gap-y-2">
      <span className="text-text text-small max-w-measure min-w-0">{sub.note}</span>
      {sub.reading && (
        // keeps the right edge when a narrow width puts it under the note
        <div role="group" aria-label={`${sub.title} reading`} className="ml-auto">
          <Reading label={sub.reading.label} raw={sub.reading.point.values[key]} unit={sub.unit} />
        </div>
      )}
    </div>
  );
}

/**
 * The figure (the chart-hero board, rules 1 and 3): an eyebrow that names it in words and says where
 * it is read, the figure at 40 px from 640 (26 below), its unit a step quieter; the DAO's in gold,
 * as on /dao (rule 7). Its exact value is its title.
 */
function HeroFigure({ head, gold }: { head: ChartHead; gold: boolean }) {
  const figure = head.figure;
  return (
    <section data-role="chart-figure" aria-label="figure" className="min-w-0">
      <div data-testid="figure-eyebrow" className="eyebrow text-text-dim text-meta mb-1.5">
        {head.eyebrow}
        {head.at && ` · ${head.at}`}
      </div>
      {figure && (
        <div
          data-testid="figure-value"
          title={figure.title}
          className={cn(
            'text-index sm:text-display whitespace-nowrap tabular-nums',
            gold ? 'text-gold' : 'text-text-bright'
          )}
        >
          {figure.display}
          <span className="text-hero text-text-dim font-normal">
            {figure.unit.startsWith('%') ? figure.unit : ` ${figure.unit}`}
          </span>
        </div>
      )}
    </section>
  );
}

function DetailBody({
  definition,
  model,
  head,
  events,
}: {
  definition: ChartDefinition;
  model: ChartModel;
  head: ChartHead;
  events: readonly HardforkEvent[];
}) {
  const network = useActiveNetwork();
  const fromSm = useFromSm();
  const [range, setRange] = useState<ChartRange>('all');
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
  // The default scale is the one in which the series has shape (CH-08): the catalogue's log where
  // a series spans orders of magnitude, else linear with its outliers named (rule 6).
  const [scale, setScale] = useState<Scale>(definition.logScale && supportsLog ? 'log' : 'linear');
  const points = useMemo(() => rangePoints(model.points, range), [model.points, range]);
  const markers = chartHardforkMarkers(definition, points, events);
  // Every activated hardfork the full history crosses, linked from the evidence (CH-23).
  const crossed = chartHardforkMarkers(definition, model.points, events).flatMap((marker) =>
    events.filter((event) => event.shortName === marker.label)
  );
  const style = axisStyle(definition);
  const hasDatedHistory =
    (model.points.length > 0 &&
      model.points.every((point) => /^\d{4}-\d{2}-\d{2}$/.test(point.label))) ||
    !!model.historicalShares?.some(
      (history) =>
        history.points.length > 0 &&
        history.points.every((point) => /^\d{4}-\d{2}-\d{2}$/.test(point.label))
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
  const yScale = supportsLog && scale === 'log' ? 'log' : 'linear';
  // Rule 6: an outlier does not set a linear axis; its value is named over the plot.
  const outliers =
    yScale === 'linear' && !model.yDomain && (style === 'date' || style === 'epoch')
      ? axisOutliers(points, model.series, model.kind)
      : null;
  const axisCap = outliers
    ? {
        value: outliers.cap,
        note: capNote(
          model,
          points,
          outliers.indexes,
          pointLabel,
          style === 'epoch' ? 'epoch' : 'day'
        ),
      }
    : undefined;
  const ranked = model.kind === 'ranked';
  // Rule 8: one controls row of the controls library. The range and the scale stand at the title
  // row's right end from 640 and with the chart below it; a ranking's range belongs to its
  // histories. The help mark keeps the title row at every width.
  const rangeControl = hasDatedHistory && (
    <RangeSelect label="range" options={RANGES} value={range} onChange={setRange} />
  );
  const scaleControl = supportsLog && (
    <FilterChips label="scale" values={SCALES} value={scale} onChange={setScale} />
  );
  const controls = (rangeControl || scaleControl) && (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
      {rangeControl}
      {scaleControl}
    </div>
  );
  const help = (
    <HelpPopover label="How to read and zoom this chart" align="end">
      {/* C10: the readout row under the chart already says "tap or hover". */}
      The line under the chart reads one {style === 'date' ? 'day' : 'point'}: select it to keep it
      there, select it again to let it go. Or focus the chart and use ← and → to step through; Esc
      returns to the latest reading.
      {hasDatedHistory && ' 30 d, 90 d and 1 y zoom to the most recent days.'}
      {supportsLog && ' Log spreads a series that grew by orders of magnitude.'}
    </HelpPopover>
  );
  const titleRow = (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
      {fromSm && !ranked && controls}
      {help}
    </div>
  );
  const plotName = (unit: string, shown: readonly ChartPoint[], shownStyle: AxisStyle) =>
    `${unit} · ${spanWords(shown, shownStyle)}`;
  // Method and evidence are the quieter developer tier, after related charts (CH-20).
  const method = (
    <div id="how-it-is-computed" className="scroll-mt-4">
      <DetailSection title="How it is computed / Evidence">
        <div className="text-small grid min-w-0 gap-4 md:grid-cols-2">
          <div className="border-base-border min-w-0 border-t pt-3">
            <div className="text-text-dim mb-2">Calculation scope</div>
            <Prose>
              <p>{description.overview}</p>
            </Prose>
            <ul className="text-text max-w-measure mt-3 space-y-2">
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
                returns observations at its own recorded dates or buckets; gaps are not
                interpolated.
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
              <Link href={definition.evidenceHref} look="onward">
                Explore {definition.evidenceLabel}
              </Link>
              {crossed.length > 0 && (
                <Link href="/hardforks" look="onward">
                  Hardforks:{' '}
                  {crossed
                    .map(
                      (event) =>
                        `${event.shortName}, epoch ${formatEpochLabel(String(event.activationEpoch), definition.slug)}`
                    )
                    .join(' · ')}
                </Link>
              )}
            </div>
            {/* The facts the head's sentence does not carry, and the developer facts (CH-18). */}
            {[...(model.facts ?? []), ...(model.evidence ?? [])].map((fact) => (
              <div key={fact.label} className="text-text-dim mt-2">
                {fact.label}:{' '}
                <span className="text-text">
                  {fact.name ? `${fact.name} · ${fact.value}` : fact.value}
                </span>
              </div>
            ))}
            <div className="mt-2">
              <span className="text-text-dim">Raw series:</span>{' '}
              <ExternalLink
                look="inline"
                href={
                  new URL(
                    `${apiBaseFor(network)}${chartApiPath(definition)}`,
                    window.location.origin
                  ).href
                }
                className="break-all"
              >
                <code>{chartApiPath(definition)}</code>
              </ExternalLink>
            </div>
          </div>
        </div>
      </DetailSection>
    </div>
  );
  const sub = (
    history: SubChart,
    shown: readonly ChartPoint[],
    shownStyle: AxisStyle,
    kind: 'line' | 'bars' | 'stacked',
    height: number,
    extra: { actions?: ReactNode } = {}
  ) => (
    <DetailSection
      key={history.title}
      title={history.title}
      args={chartSpan(shown, shownStyle)}
      actions={extra.actions}
    >
      <SubChartHead sub={history} />
      {history.points.length ? (
        <Chart
          title={history.title}
          subtitle={`${history.caption} · ${chartSpan(shown, shownStyle)}`}
          reading={history.reading?.point}
          hideCaption
          data={shown}
          series={history.series}
          unit={history.unit}
          kind={kind}
          height={height}
          formatValue={(value) => formatChartHover(value, history.unit)}
          xTicks={axisTicks(shown, shownStyle, 8, shownStyle === 'bucket' ? xUnit : undefined)}
          formatAxis={chartAxisFormat(history.unit)}
          formatLabel={shownStyle === 'date' ? undefined : pointLabel}
          pointDots
          slotBars={shownStyle === 'bucket' || shownStyle === 'month'}
          categoricalX={shownStyle === 'bucket' && categoricalAxis(definition)}
          pinnedReadout={readout(history.title)}
          // Rule 4: ticks inside the plot, the unit once in its name; rule 7: a quiet line.
          plotName={plotName(history.caption, shown, shownStyle)}
          insideYTicks
          bareTicks
          quiet
          // Rule 5: a ranked history's five and the rest, named at their right ends.
          bandEnds={kind === 'stacked' ? history.bandEnds : undefined}
          solidBands={kind === 'stacked'}
        />
      ) : (
        <span className="text-text-dim text-small block py-8">
          No indexed observations for this scope.
        </span>
      )}
    </DetailSection>
  );
  return (
    <>
      <nav aria-label="Breadcrumb" className="text-small mb-3">
        <Link href="/charts" look="inline">
          charts
        </Link>{' '}
        <span className="text-text-dim">/</span>{' '}
        <Link href={`/charts#${chartSectionId(definition.section)}`} look="inline">
          {definition.section.toLowerCase()}
        </Link>
      </nav>
      <DetailLayout
        kind="topic"
        title={definition.title}
        documentTitle={[definition.title, 'chart']}
        actions={titleRow}
        // Rule 2: one sentence says what the series did, in place of the line of figures.
        lede={
          <p data-role="chart-sentence" className="text-text text-body">
            <span data-role="sentence-words">{head.sentence}</span>{' '}
            <Link href="#how-it-is-computed" look="onward">
              How it is computed
            </Link>
          </p>
        }
        tier2={method}
      >
        <div className="min-w-0">
          <HeroFigure head={head} gold={definition.source === 'dao'} />
          {!fromSm && !ranked && controls && <div className="mt-4">{controls}</div>}
          <div className="mt-3.5 min-w-0">
            {model.latest === null && model.points.length === 0 && !ranked ? (
              <span className="text-text-dim text-small block py-8">
                No indexed observations for this chart.
              </span>
            ) : ranked ? (
              <RankedDetail model={model} definition={definition} />
            ) : (
              <Chart
                title={definition.title}
                subtitle={`${definition.subtitle} · ${chartSpan(points, style)}`}
                data={points}
                series={model.series}
                unit={model.unit}
                kind={model.kind}
                yScale={yScale}
                height={360}
                // The page head names the chart and reads it (visual pass).
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
                // Rule 4: ticks inside the plot, bare; the unit said once, in the plot's name.
                plotName={plotName(definition.plotUnit, points, style)}
                insideYTicks
                bareTicks
                // Rule 6: an outlier is named over the plot instead of setting the axis.
                axisCap={axisCap}
                // Rule 5: a stack's bands named at their right ends, in solid fills; two lines of
                // one unit keep their key under the plot.
                bandEnds={model.kind === 'stacked' ? model.bandEnds : undefined}
                solidBands={model.kind === 'stacked'}
                seriesLegend={model.kind !== 'stacked' && model.series.length > 1}
              />
            )}
          </div>
          {/* Rule 5: every band a stack merges, at the last point, in a table under the chart. */}
          {model.table && (
            <table aria-label={`${definition.title}, every band`} className="text-small mt-5">
              <thead>
                <tr className="text-text-dim">
                  <th className="pr-8 text-left font-normal">band</th>
                  <th className="text-right font-normal">{`share, ${model.latest?.label ?? ''}`}</th>
                </tr>
              </thead>
              <tbody>
                {model.table.map((row) => (
                  <tr key={row.name}>
                    <td className="text-text pr-8">{unitText(row.name)}</td>
                    <td className="text-text-bright text-right tabular-nums">{row.value}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        {model.historicalShares?.map((history, index) =>
          sub(history, rangePoints(history.points, range), 'date', 'stacked', 260, {
            // a ranking's range is its histories' (the ranking itself is one day)
            actions: index === 0 ? controls : undefined,
          })
        )}
        {model.secondary &&
          model.secondary.points.length > 0 &&
          sub(
            model.secondary,
            rangePoints(model.secondary.points, range),
            style,
            definition.visual === 'bars' ? 'bars' : 'line',
            220
          )}
        {model.dailyFlow &&
          model.dailyFlow.points.length > 0 &&
          sub(model.dailyFlow, rangePoints(model.dailyFlow.points, range), 'date', 'line', 220)}
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
  let head: ChartHead | null = null;
  let modelError: string | null = null;
  if (data.data) {
    try {
      model = chartModel(definition, data.data);
      // the head reads the model's points: a payload it cannot read is invalid data too
      head = chartHead(definition, model);
    } catch (cause) {
      model = null;
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
        {model && head && (
          <DetailBody
            definition={definition}
            model={model}
            head={head}
            events={hardforks.data?.events ?? []}
          />
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
