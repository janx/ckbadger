'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Chart } from '@/components/ui/chart';
import type { ChartDefinition } from '@/components/charts/chart-catalogue';
import { chartModel, type ChartModel } from '@/components/charts/chart-model';

function RankedPreview({ model }: { model: ChartModel }) {
  const allRows = model.rankedRows ?? [];
  const rows = [
    ...allRows.filter((row) => row.label !== 'Others').slice(0, 5),
    ...allRows.filter((row) => row.label === 'Others'),
  ];
  return (
    <ol className="space-y-2" aria-label={`${model.title} ranked preview`}>
      {rows.map((row) => {
        const width = Number(row.share.slice(0, -1));
        if (row.share !== '—' && (!Number.isFinite(width) || width < 0 || width > 100))
          throw new Error(`${model.title}: invalid share ${row.share}`);
        return (
          <li key={row.label} className="min-w-0 text-[11px]">
            <div className="flex justify-between gap-2">
              <span className="truncate" title={row.label}>
                {row.label}
              </span>
              <span className="shrink-0 tabular-nums">{row.share}</span>
            </div>
            <div className="bg-base-elevated mt-1 h-1 rounded">
              <div
                className="bg-jade h-1 rounded"
                style={{ width: row.share === '—' ? '0%' : `${width}%` }}
              />
            </div>
          </li>
        );
      })}
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
          <div className="text-text-dim text-[11px]">{definition.subtitle}</div>
        </div>
        {model?.latest && (
          <div
            className="min-w-0 max-w-[60%] text-right"
            title={`${model.latest.label}: ${model.latest.raw} ${model.unit}`}
          >
            <div className="text-text-dim text-[10px]">{model.latest.label}</div>
            {model.summaryLabel && (
              <div className="text-text-dim break-words text-[10px]" title={model.summaryLabel}>
                {model.summaryLabel}
              </div>
            )}
            <div className="text-text-bright break-all text-[17px] tabular-nums">
              {model.latest.display}
              <span className="text-text-dim ml-1 text-[10px]">{model.unit}</span>
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
          />
        ))}
      {model?.delta && (
        <div
          className="text-text-dim mt-1 break-all text-[10px]"
          title={`Exact change: ${model.delta} ${model.unit}`}
        >
          Last change {model.delta} {model.unit}
        </div>
      )}
    </article>
  );
}
