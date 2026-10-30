'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Chart } from '@/components/ui/chart';
import { ClassMark } from '@/components/ui/class-mark';
import type { ChartDefinition } from '@/components/charts/chart-catalogue';
import {
  axisStyle,
  axisTicks,
  chartModel,
  formatChartExact,
  formatChartReadout,
  shortBucketLabel,
  type ChartHeadline,
  type ChartModel,
} from '@/components/charts/chart-model';

function shareWidth(share: string, title: string): number {
  if (share === '—') return 0;
  if (!share.endsWith('%')) throw new Error(`${title}: share missing %: ${share}`);
  const width = Number(share.slice(0, -1));
  if (!Number.isFinite(width) || width < 0 || width > 100)
    throw new Error(`${title}: invalid share ${share}`);
  return width;
}

/** Top five ranks plus the exact remainder as "Others", both computed by the chart model. */
function RankedPreview({ model }: { model: ChartModel }) {
  if (!model.rankedPreview) throw new Error(`${model.title}: ranked chart has no preview rows`);
  return (
    <ol className="space-y-2" aria-label={`${model.title} ranked preview`}>
      {model.rankedPreview.rows.map((row) => (
        <li key={row.title} className="min-w-0 text-[11px]">
          <div className="flex justify-between gap-2">
            <span className="flex min-w-0 items-center gap-1.5" title={row.title}>
              {/* The Assets ranking shows each entry's class by its mark (CH-06). */}
              {row.assetClass && <ClassMark kind={row.assetClass} size={12} />}
              <span className="truncate font-mono">{row.label}</span>
            </span>
            <span className="shrink-0 tabular-nums">{row.share}</span>
          </div>
          <div className="bg-base-elevated mt-1 h-1 rounded">
            <div
              className="bg-jade h-1 rounded"
              style={{ width: `${shareWidth(row.share, model.title)}%` }}
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
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: [definition.queryKey],
    queryFn: definition.fetch,
  });
  const style = axisStyle(definition);
  let model: ChartModel | null = null;
  let ticks: { index: number; label: string }[] = [];
  let modelError: string | null = null;
  if (data) {
    try {
      model = chartModel(definition, data);
      ticks = axisTicks(model.card?.points ?? model.points, style, style === 'bucket' ? 6 : 3).map(
        (tick) => (style === 'bucket' ? { ...tick, label: shortBucketLabel(tick.label) } : tick)
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
      <div className="mb-2 flex min-w-0 items-end justify-between gap-3">
        <div className="min-w-0">
          <Link
            href={`/charts/${definition.slug}`}
            className="text-text-bright hover:text-jade text-[13px]"
          >
            {definition.title}
          </Link>
          <div className="text-text-dim text-[11px]">{definition.cardSubtitle}</div>
        </div>
        {model?.latest && headline && (
          <div
            className="min-w-0 shrink-0 text-right"
            title={`${model.latest.label}: ${formatChartExact(model.latest.raw, model.unit)} ${model.unit}`}
          >
            <div className="text-text-dim text-[11px]">{headline.pre}</div>
            <div
              role="group"
              aria-label={`${definition.title} current readout`}
              className="min-w-0 overflow-x-auto"
            >
              <div className="flex w-max min-w-full items-baseline justify-end gap-1">
                <span className="text-text-bright whitespace-nowrap text-[18px] font-medium tabular-nums">
                  {headline.display}
                </span>
                {/* The unit is in the subtitle, as on the board; only a percentage keeps its sign. */}
                {headline.unit === '%' && (
                  <span className="text-text-dim whitespace-nowrap text-[11px]">%</span>
                )}
              </div>
            </div>
          </div>
        )}
      </div>
      {isLoading && (
        <div
          className="bg-base-surface h-[120px] animate-pulse rounded"
          aria-label={`${definition.title} loading`}
        />
      )}
      {error && (
        <div role="alert" className="text-negative break-words text-xs">
          {definition.title}: {error.message}
          <button type="button" onClick={() => void refetch()} className="text-jade ml-2 underline">
            Retry
          </button>
        </div>
      )}
      {modelError && (
        <p role="alert" className="text-negative break-words text-xs">
          {modelError}
        </p>
      )}
      {model && !model.latest && (
        <p className="text-text-dim py-8 text-xs">No indexed observations for this chart.</p>
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
            yDomain={model.yDomain}
            referenceLines={model.referenceLines}
            formatValue={(value) => formatChartReadout(value, model.unit)}
          />
        ))}
      {model?.card && model.card.legend.length > 0 && (
        <ul
          aria-label={`${definition.title} legend`}
          className="text-text-dim mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-[11px]"
        >
          {model.card.legend.map((entry) => (
            <li key={entry.key} className="flex items-center gap-1.5">
              <span
                aria-hidden="true"
                className="h-[7px] w-[7px] rounded-[2px]"
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
