'use client';

import Link from '@/components/ui/link';
import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
import { ChartCalculationNote } from '@/components/charts/chart-calculation-note';
import { getChartDescription } from '@/components/charts/chart-calculation-descriptions';
import { LineChart } from '@/components/ui/line-chart';
import { ChartDataPoint, api } from '@/lib/api';

function buildNetFlowData(data: ChartDataPoint[]): ChartDataPoint[] {
  let previous: number | null = null;

  return data.map((point) => {
    const current = Number.parseFloat(point.value);
    const safeCurrent = Number.isFinite(current) ? current : 0;
    const delta = previous == null ? 0 : safeCurrent - previous;
    previous = safeCurrent;

    return {
      date: point.date,
      value: delta.toFixed(8),
    };
  });
}

export default function KnowledgeSizePage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['chart-knowledge-size'],
    queryFn: api.getKnowledgeSizeChart,
  });

  const netFlowData = useMemo(() => buildNetFlowData(data?.data ?? []), [data?.data]);

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/charts"
            className="hover:text-emphasis text-text-muted text-sm transition-colors"
          >
            ← Back to Charts
          </Link>
        </div>

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Common Knowledge Size</TerminalPanelHeader>
          <TerminalPanelContent className="space-y-8 p-6">
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
                <div>
                  <LineChart
                    data={data.data}
                    yAxisLabel={data.yAxisLabel}
                    y2AxisLabel={data.y2AxisLabel}
                    height={360}
                  />
                  <div className="mt-4 flex flex-wrap items-center justify-center gap-6 text-sm">
                    <div className="flex items-center gap-2">
                      <span className="bg-emphasis h-0.5 w-4" />
                      <span className="text-text-muted">{data.yAxisLabel}</span>
                    </div>
                    {data.y2AxisLabel && (
                      <div className="flex items-center gap-2">
                        <span className="bg-warning h-0.5 w-4" />
                        <span className="text-text-muted">{data.y2AxisLabel}</span>
                      </div>
                    )}
                  </div>
                </div>

                <div className="border-base-border border-t pt-6">
                  <h3 className="text-text-secondary mb-4 font-mono text-sm uppercase tracking-wider">
                    Net Occupied Capacity Flow
                  </h3>
                  <LineChart
                    data={netFlowData}
                    yAxisLabel="Net Flow (CKB/day)"
                    height={260}
                    defaultLogScale={false}
                    primaryColor="#f59e0b"
                  />
                  <div className="mt-4 flex items-center justify-center gap-2 text-sm">
                    <span className="bg-warning h-0.5 w-4" />
                    <span className="text-text-muted">Net Flow (CKB/day)</span>
                  </div>
                </div>

                <div className="text-text-muted text-center font-mono text-xs">
                  Drag to select range • Scroll to zoom • Middle-click drag to pan • Click Reset to
                  restore
                </div>
                <ChartCalculationNote
                  description={
                    getChartDescription('chart-knowledge-size', {
                      yAxisLabel: data.yAxisLabel,
                      y2AxisLabel: data.y2AxisLabel,
                    }) ?? {
                      overview: 'Shows common knowledge size and day-over-day net flow.',
                      legendItems: [],
                    }
                  }
                />
              </>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
