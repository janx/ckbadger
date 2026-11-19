'use client';

import { RetryAction } from '@/components/ui/retry-action';
import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import Link from '@/components/ui/link';
import { Chart } from '@/components/ui/chart';
import { ClassMark } from '@/components/ui/class-mark';
import type { ChartDefinition } from '@/components/charts/chart-catalogue';
import { chartAxisFormat } from '@/lib/format-unit';
import { decimalAmountText } from '@/lib/amount-form';
import {
  axisStyle,
  axisTicks,
  bucketUnit,
  categoricalAxis,
  chartModel,
  formatChartReadout,
  type ChartModel,
} from '@/components/charts/chart-model';
import { chartDrawing, chartHead, type ChartHead } from '@/components/charts/chart-head';

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
                <Link href={row.href} title={row.title} look="key" className="truncate font-mono">
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

/** Count nouns: on a card the title is the noun, so the figure carries no unit word. */
const COUNT_NOUNS = new Set([
  'transactions',
  'addresses',
  'activities',
  'kind counts',
  'cells',
  'blocks',
  'epochs',
  'holders',
]);

/**
 * The card's figure (rule 9, CH-07): the page's own head figure, the same point and quantity; a CKB
 * amount in the compact form a 278 px card holds (cut, never rounded, its exact value the title),
 * a share or a rate with its unit's base (`%`, `PH/s`), a count bare under its noun, the title.
 */
export function cardFigure(head: ChartHead): { text: string; unit: string } | null {
  const figure = head.figure;
  if (!figure) return null;
  if (figure.unit === 'CKB') return { text: decimalAmountText(figure.raw, 'compact'), unit: 'CKB' };
  const base = figure.unit.startsWith('%') ? '%' : figure.unit;
  if (COUNT_NOUNS.has(base)) return { text: figure.display, unit: '' };
  return { text: figure.display, unit: base };
}

export function ChartSmallMultiple({ definition }: { definition: ChartDefinition }) {
  const read = useQuery({
    queryKey: [definition.queryKey],
    queryFn: definition.fetch,
  });
  const { data, error, refetch } = read;
  const style = axisStyle(definition);
  let model: ChartModel | null = null;
  let head: ChartHead | null = null;
  let ticks: { index: number; label: string }[] = [];
  let drawing: ReturnType<typeof chartDrawing> | null = null;
  let modelError: string | null = null;
  if (data) {
    try {
      model = chartModel(definition, data);
      head = chartHead(definition, model);
      // The bucket unit once, on the axis' last label (CH-12); a histogram's round values over the
      // run its model filled, at the tile's own step.
      ticks = axisTicks(
        model.card?.points ?? model.points,
        style,
        style === 'bucket' ? 6 : 3,
        bucketUnit(definition),
        model.bucketRun
      );
      // the page's default scale and cap, drawn the page's way (the D1 hand-back, item 3)
      if (model.kind !== 'ranked')
        drawing = chartDrawing(
          definition,
          model,
          model.card?.points ?? model.points,
          model.card?.series ?? model.series
        );
    } catch (cause) {
      model = null;
      modelError = cause instanceof Error ? cause.message : String(cause);
    }
  }
  const figure = head ? cardFigure(head) : null;
  const points = model?.card?.points ?? model?.points ?? [];
  return (
    <article className="min-w-0" aria-label={`${definition.title} chart preview`}>
      {/* Rule 9 (CH-16): a fixed head of two lines, so every plot of a row of cards starts on one
          line: the name and the figure with its unit; the caption and the date. Each line is
          written to fit the narrowest card and is never cut (chart-card-head.test.tsx). */}
      <div data-role="card-head" className="mb-2.5 min-w-0">
        <div className="flex min-w-0 items-baseline gap-x-2.5">
          <Link
            href={`/charts/${definition.slug}`}
            look="key"
            className="text-body whitespace-nowrap"
          >
            {definition.title}
          </Link>
          {figure && head?.figure && (
            <span
              role="group"
              aria-label={`${definition.title} current readout`}
              title={head.figure.title}
              className="text-text-bright text-section ml-auto whitespace-nowrap tabular-nums"
            >
              {figure.text}
              {figure.unit && (
                <span className="text-text-dim text-meta">
                  {figure.unit === '%' ? '%' : `\u00a0${figure.unit}`}
                </span>
              )}
            </span>
          )}
        </div>
        <div className="text-text-dim text-meta mt-0.5 flex min-w-0 gap-x-2.5">
          <span data-role="card-caption" className="whitespace-nowrap">
            {definition.cardSubtitle}
          </span>
          {head?.at && (
            <span data-role="card-date" className="ml-auto whitespace-nowrap">
              {head.at}
            </span>
          )}
        </div>
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
          <RetryAction words="Retry" onRetry={() => void refetch()} className="ml-2" />
        </div>
      )}
      {modelError && (
        <p role="alert" className="text-negative text-small max-w-measure break-words">
          {modelError}
        </p>
      )}
      {model && !model.latest && model.kind !== 'ranked' && (
        <span className="text-text-dim text-small block py-8">
          No indexed observations for this chart.
        </span>
      )}
      {model &&
        (model.kind === 'ranked' ? (
          <RankedPreview model={model} />
        ) : (
          model.latest && (
            <Chart
              title={definition.title}
              subtitle={definition.subtitle}
              data={points}
              series={model.card?.series ?? model.series}
              unit={model.unit}
              kind={model.kind}
              height={130}
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
              // The page's default scale and cap (rule 6), the axis saying which: a log scale is
              // named with the unit, a cut axis names what is above it.
              yScale={drawing?.yScale}
              axisCap={drawing?.axisCap}
              // Rule 4: every plot's left edge on one line; its ticks inside it, bare, its unit
              // named once; rule 5: a stack in the page's solid bands.
              plotName={drawing?.yScale === 'log' ? `${model.unit} · log scale` : model.unit}
              insideYTicks
              bareTicks
              solidBands={model.kind === 'stacked'}
            />
          )
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
              <span className="text-text">{entry.label}</span>
              <span className="text-text tabular-nums">{entry.value}</span>
            </li>
          ))}
        </ul>
      )}
    </article>
  );
}
