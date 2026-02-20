'use client';

import Link from 'next/link';
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
import { LineChart, LineChartType } from '@/components/ui/line-chart';
import { api, ChartResponse } from '@/lib/api';

function ChartDataWarning({ show }: { show: boolean }) {
  if (!show) return null;
  return (
    <div className="mb-6 rounded border border-yellow-500/30 bg-yellow-500/10 px-4 py-3">
      <div className="flex items-center gap-2">
        <span className="text-yellow-500">⚠</span>
        <span className="font-mono text-sm text-yellow-500">
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
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href={backLink}
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            ← {backLabel}
          </Link>
        </div>

        <ChartDataWarning show={networkStats?.syncStatus?.chartDataMayBeIncomplete ?? false} />

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">{title}</TerminalPanelHeader>
          <TerminalPanelContent className="p-6">
            {isLoading && (
              <div className="h-96 animate-pulse rounded border border-slate-800 bg-slate-900/50" />
            )}
            {error && (
              <div className="flex h-96 items-center justify-center text-slate-500">
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
                />
                <div className="mt-6 flex items-center justify-center gap-6 text-sm">
                  <div className="flex items-center gap-2">
                    <span
                      className={
                        chartType === 'bar'
                          ? 'h-3 w-3 rounded bg-purple-500'
                          : 'h-0.5 w-4 bg-purple-500'
                      }
                    />
                    <span className="text-slate-400">{data.yAxisLabel}</span>
                  </div>
                  {chartType === 'line' && data.y2AxisLabel && (
                    <div className="flex items-center gap-2">
                      <span className="bg-terminal-green h-0.5 w-4" />
                      <span className="text-slate-400">{data.y2AxisLabel}</span>
                    </div>
                  )}
                </div>
                <div className="mt-4 text-center font-mono text-xs text-slate-600">
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
