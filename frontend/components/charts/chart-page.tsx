'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { ChartCalculationNote } from '@/components/charts/chart-calculation-note';
import { getChartDescription } from '@/components/charts/chart-calculation-descriptions';
import type { ChartDescription } from '@/components/charts/chart-calculation-descriptions';
import { LineChart, LineChartMarker, LineChartType } from '@/components/ui/line-chart';
import { api, ChartResponse } from '@/lib/api';

function ChartDataWarning({ show }: { show: boolean }) {
  if (!show) return null;
  return (
    <div className="border-warning/30 bg-warning/10 mb-6 rounded border px-4 py-3">
      <div className="flex items-center gap-2">
        <span className="text-warning">⚠</span>
        <span className="text-warning font-mono text-sm">
          Chart data may be incomplete. The indexer is still syncing historical statistics.
        </span>
      </div>
    </div>
  );
}

interface ChartPageProps {
  title: string;
  queryKey: string;
  queryFn: () => Promise<ChartResponse>;
  backLink?: string;
  backLabel?: string;
  defaultLogScale?: boolean;
  chartType?: LineChartType;
  description?: ChartDescription;
  markers?: LineChartMarker[];
}

export function ChartPage({
  title,
  queryKey,
  queryFn,
  backLink = '/charts',
  backLabel = 'Back to Charts',
  defaultLogScale = false,
  chartType = 'line',
  description,
  markers,
}: ChartPageProps) {
  const { data: networkStats } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
  });

  const { data, isLoading, error } = useQuery({
    queryKey: [queryKey],
    queryFn,
  });
  const resolvedDescription = data
    ? (description ??
      getChartDescription(queryKey, {
        yAxisLabel: data.yAxisLabel,
        y2AxisLabel: data.y2AxisLabel,
      }))
    : undefined;

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href={backLink}
            className="hover:text-emphasis text-text-muted text-sm transition-colors"
          >
            ← {backLabel}
          </Link>
        </div>

        <ChartDataWarning show={networkStats?.syncStatus?.chartDataMayBeIncomplete ?? false} />

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">{title}</TerminalPanelHeader>
          <TerminalPanelContent className="p-6">
            {isLoading && (
              <div className="border-base-border bg-base-surface/50 h-96 animate-pulse rounded border" />
            )}
            {error && (
              <div className="text-text-muted flex h-96 items-center justify-center">
                Failed to load chart data
              </div>
            )}
            {data && (
              <>
                <LineChart
                  data={data.data}
                  yAxisLabel={data.yAxisLabel}
                  y2AxisLabel={data.y2AxisLabel}
                  height={400}
                  defaultLogScale={defaultLogScale}
                  chartType={chartType}
                  markers={markers}
                />
                <div className="mt-6 flex items-center justify-center gap-6 text-sm">
                  <div className="flex items-center gap-2">
                    <span
                      className={
                        chartType === 'bar'
                          ? 'bg-emphasis h-3 w-3 rounded'
                          : 'bg-emphasis h-0.5 w-4'
                      }
                    />
                    <span className="text-text-muted">{data.yAxisLabel}</span>
                  </div>
                  {chartType === 'line' && data.y2AxisLabel && (
                    <div className="flex items-center gap-2">
                      <span className="bg-warning h-0.5 w-4" />
                      <span className="text-text-muted">{data.y2AxisLabel}</span>
                    </div>
                  )}
                </div>
                <div className="text-text-muted mt-4 text-center font-mono text-xs">
                  Drag to select range • Scroll to zoom • Middle-click drag to pan • Click Reset to
                  restore
                </div>
                {resolvedDescription && <ChartCalculationNote description={resolvedDescription} />}
              </>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
