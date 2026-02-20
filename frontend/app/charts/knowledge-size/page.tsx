'use client';

import Link from 'next/link';
import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
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
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/charts"
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            ← Back to Charts
          </Link>
        </div>

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Common Knowledge Size</TerminalPanelHeader>
          <TerminalPanelContent className="space-y-8 p-6">
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
                <div>
                  <LineChart
                    data={data.data}
                    yAxisLabel={data.yAxisLabel}
                    y2AxisLabel={data.y2AxisLabel}
                    height={360}
                  />
                  <div className="mt-4 flex flex-wrap items-center justify-center gap-6 text-sm">
                    <div className="flex items-center gap-2">
                      <span className="h-0.5 w-4 bg-purple-500" />
                      <span className="text-slate-400">{data.yAxisLabel}</span>
                    </div>
                    {data.y2AxisLabel && (
                      <div className="flex items-center gap-2">
                        <span className="bg-terminal-green h-0.5 w-4" />
                        <span className="text-slate-400">{data.y2AxisLabel}</span>
                      </div>
                    )}
                  </div>
                </div>

                <div className="border-t border-slate-800 pt-6">
                  <h3 className="mb-4 font-mono text-sm uppercase tracking-wider text-slate-300">
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
                    <span className="h-0.5 w-4 bg-amber-500" />
                    <span className="text-slate-400">Net Flow (CKB/day)</span>
                  </div>
                </div>

                <div className="text-center font-mono text-xs text-slate-600">
                  Drag to select range • Scroll to zoom • Middle-click drag to pan • Click Reset to
                  restore
                </div>
              </>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
