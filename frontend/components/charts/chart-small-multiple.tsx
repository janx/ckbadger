'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Chart } from '@/components/ui/chart';
import type { ChartDefinition } from '@/components/charts/chart-catalogue';
import {
  chartModel,
  formatChartExact,
  formatChartReadout,
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
            <span className="truncate font-mono" title={row.title}>
              {row.label}
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

export function ChartSmallMultiple({ definition }: { definition: ChartDefinition }) {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: [definition.queryKey],
    queryFn: definition.fetch,
  });
  let model: ChartModel | null = null;
  let modelError: string | null = null;
  if (data) {
    try {
      model = chartModel(definition, data);
    } catch (cause) {
      modelError = cause instanceof Error ? cause.message : String(cause);
    }
  }
  // A ranked card names its leader in the preview's short form; the detail keeps the full one.
  const cardSummary = model?.rankedPreview ? model.rankedPreview.summaryLabel : model?.summaryLabel;
  return (
    <article className="min-w-0" aria-label={`${definition.title} chart preview`}>
      <div className="mb-2 min-w-0">
        <div className="min-w-0">
          <Link
            href={`/charts/${definition.slug}`}
            className="text-text-bright hover:text-jade text-[13px]"
          >
            {definition.title}
          </Link>
          <div className="text-text-dim text-[11px]">{definition.subtitle}</div>
        </div>
        {model?.latest && (
          <div
            className="mt-2 min-w-0 text-right"
            title={`${model.latest.label}: ${formatChartExact(model.latest.raw, model.unit)} ${model.unit}`}
          >
            <div className="text-text-dim text-[10px]">{model.latest.label}</div>
            {cardSummary && (
              <div className="text-text-dim break-words text-[10px]" title={model.summaryLabel}>
                {cardSummary}
              </div>
            )}
            <div
              role="group"
              aria-label={`${definition.title} current readout`}
              className="min-w-0 overflow-x-auto"
            >
              <div className="flex w-max min-w-full items-baseline justify-end gap-1">
                <span className="text-text-bright whitespace-nowrap text-[17px] tabular-nums">
                  {model.latest.display}
                </span>
                <span className="text-text-dim whitespace-nowrap text-[10px]">{model.unit}</span>
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
            data={model.points}
            series={model.series}
            unit={model.unit}
            kind={model.kind}
            height={120}
            hideCaption
            hideXAxis
            formatValue={(value) => formatChartReadout(value, model.unit)}
          />
        ))}
      {model?.delta && (
        <div
          className="text-text-dim mt-1 break-all text-[10px]"
          title={`Exact change: ${formatChartExact(model.delta, model.unit)} ${model.unit}`}
        >
          Last change {formatChartReadout(model.delta, model.unit)} {model.unit}
        </div>
      )}
    </article>
  );
}
