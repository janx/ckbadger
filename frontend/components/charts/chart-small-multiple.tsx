'use client';

import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { Chart } from '@/components/ui/chart';
import { ClassMark } from '@/components/ui/class-mark';
import type { ChartDefinition } from '@/components/charts/chart-catalogue';
import { chartAxisFormat, unitAfter, withUnit } from '@/lib/format-unit';
import {
  axisStyle,
  axisTicks,
  bucketUnit,
  categoricalAxis,
  chartModel,
  formatChartExact,
  formatChartReadout,
  type ChartHeadline,
  type ChartModel,
} from '@/components/charts/chart-model';

/** A preview bar's width: the model's, from the exact parts (never the printed share). */
function shareWidth(row: { width: number; label: string }, title: string): number {
  if (!Number.isFinite(row.width) || row.width < 0 || row.width > 100)
    throw new Error(`${title}: invalid share width ${row.width} for ${row.label}`);
  return row.width;
}

/** Top five ranks plus the exact remainder as "Others", both computed by the chart model. */
function RankedPreview({ model }: { model: ChartModel }) {
  if (!model.rankedPreview) throw new Error(`${model.title}: ranked chart has no preview rows`);
  return (
    <ol className="space-y-2" aria-label={`${model.title} ranked preview`}>
      {model.rankedPreview.rows.map((row) => (
        <li key={row.title} className="text-meta min-w-0">
          <div className="flex justify-between gap-2">
            <span className="flex min-w-0 items-center gap-1.5" title={row.title}>
              {/* The Assets ranking shows each entry's class by its mark (CH-06). */}
              {row.assetClass && <ClassMark kind={row.assetClass} size={12} />}
              {row.href ? (
                <Link
                  href={row.href}
                  title={row.title}
                  className="hover:text-jade truncate font-mono"
                >
                  {row.label}
                </Link>
              ) : (
                <span className="truncate font-mono">{row.label}</span>
              )}
            </span>
            <span className="shrink-0 tabular-nums">{row.share}</span>
          </div>
          <div className="bg-bar-track mt-1 h-1">
            <div
              className="bg-jade scan-fill h-1"
              style={{ width: `${shareWidth(row, model.title)}%` }}
            />
          </div>
        </li>
      ))}
    </ol>
  );
}

/** The card's headline: the model's own, else the latest reading under its date. */
function cardHeadline(model: ChartModel): ChartHeadline | null {
  if (model.headline !== undefined) return model.headline;
  return model.latest
    ? { pre: model.latest.label, display: model.latest.display, unit: model.unit }
    : null;
}

export function ChartSmallMultiple({ definition }: { definition: ChartDefinition }) {
  const read = useQuery({
    queryKey: [definition.queryKey],
    queryFn: definition.fetch,
  });
  const { data, error, refetch } = read;
  const style = axisStyle(definition);
  let model: ChartModel | null = null;
  let ticks: { index: number; label: string }[] = [];
  let modelError: string | null = null;
  if (data) {
    try {
      model = chartModel(definition, data);
      // The bucket unit once, on the axis' last label (CH-12); a histogram's round values over the
      // run its model filled, at the tile's own step.
      ticks = axisTicks(
        model.card?.points ?? model.points,
        style,
        style === 'bucket' ? 6 : 3,
        bucketUnit(definition),
        model.bucketRun
      );
    } catch (cause) {
      model = null;
      modelError = cause instanceof Error ? cause.message : String(cause);
    }
  }
  const headline = model ? cardHeadline(model) : null;
  const points = model?.card?.points ?? model?.points ?? [];
  return (
    <article className="min-w-0" aria-label={`${definition.title} chart preview`}>
      {/* The readout keeps the right edge; in a card too narrow for title and readout side by
          side it drops under the title whole instead of being cut (responsive review 2026-11-01). */}
      <div className="mb-2 flex min-w-0 flex-wrap items-end justify-between gap-x-3 gap-y-1">
        <div className="min-w-0">
          <Link
            href={`/charts/${definition.slug}`}
            className="text-text-bright hover:text-jade text-body"
          >
            {definition.title}
          </Link>
          <div className="text-text-dim text-meta">{definition.cardSubtitle}</div>
        </div>
        {model?.latest && headline && (
          <div
            className="ml-auto min-w-0 text-right"
            title={`${model.latest.label}: ${withUnit(formatChartExact(model.latest.raw, model.unit), model.unit)}`}
          >
            <div className="text-text-dim text-meta whitespace-nowrap">{headline.pre}</div>
            <div
              role="group"
              aria-label={`${definition.title} current readout`}
              className="min-w-0 overflow-x-auto"
            >
              <div className="flex w-max min-w-full items-baseline justify-end">
                <span className="text-text-bright text-hero whitespace-nowrap font-medium tabular-nums">
                  {headline.display}
                </span>
                {/* Every card figure carries its unit, in the one form (CH-14): 2.55%, 10.43 s. */}
                <span className="text-text-dim text-meta whitespace-pre">
                  {unitAfter(headline.unit, '\u00a0')}
                </span>
              </div>
            </div>
          </div>
        )}
      </div>
      {isAwaited(read) && (
        <div
          className="bg-base-surface h-[120px] animate-pulse"
          aria-label={`${definition.title} loading`}
        />
      )}
      {error && (
        <div role="alert" className="text-negative text-small break-words">
          {definition.title}: {error.message}
          <button type="button" onClick={() => void refetch()} className="text-jade ml-2 underline">
            Retry
          </button>
        </div>
      )}
      {modelError && (
        <p role="alert" className="text-negative text-small max-w-measure break-words">
          {modelError}
        </p>
      )}
      {model && !model.latest && (
        <p className="text-text-dim text-small py-8">No indexed observations for this chart.</p>
      )}
      {model?.latest &&
        (model.kind === 'ranked' ? (
          <RankedPreview model={model} />
        ) : (
          <Chart
            title={definition.title}
            subtitle={definition.subtitle}
            data={points}
            series={model.card?.series ?? model.series}
            unit={model.unit}
            kind={model.kind}
            height={120}
            hideCaption
            projectedAfter={model.projectedAfter}
            // F2 charts: the board's card grammar — x labels, end dot, targets, share axis.
            xTicks={ticks}
            pointDots={model.kind === 'line'}
            slotBars={style === 'bucket' || style === 'month'}
            categoricalX={categoricalAxis(definition)}
            yDomain={model.yDomain}
            referenceLines={model.referenceLines}
            formatValue={(value) => formatChartReadout(value, model.unit)}
            formatAxis={chartAxisFormat(model.unit)}
          />
        ))}
      {model?.card && model.card.legend.length > 0 && (
        <ul
          aria-label={`${definition.title} legend`}
          className="text-text-dim text-meta mt-1 flex flex-wrap gap-x-3 gap-y-0.5"
        >
          {model.card.legend.map((entry) => (
            <li key={entry.key} className="flex items-center gap-1.5">
              <span
                aria-hidden="true"
                className="h-[7px] w-[7px]"
                style={{ backgroundColor: entry.color }}
              />
              <span>{entry.label}</span>
              <span className="text-text tabular-nums">{entry.value}</span>
            </li>
          ))}
        </ul>
      )}
    </article>
  );
}
