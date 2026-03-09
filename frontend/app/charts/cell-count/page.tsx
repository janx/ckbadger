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
import { MultiSeriesLineChart } from '@/components/ui/multi-series-line-chart';
import { api } from '@/lib/api';

export default function CellCountPage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['chart-cell-count'],
    queryFn: api.getCellCountChart,
  });

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/charts"
            className="hover:text-emphasis text-text-muted text-sm transition-colors"
          >
            &larr; Back to Charts
          </Link>
        </div>

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Cell Count</TerminalPanelHeader>
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
                <MultiSeriesLineChart
                  data={data.data}
                  series={data.series}
                  height={400}
                  defaultVisibleSeries={['liveCells']}
                />
                <ChartCalculationNote
                  description={
                    getChartDescription('chart-cell-count', {
                      seriesLabels: data.series.map((s) => s.label),
                    }) ?? {
                      overview: 'Shows daily cell totals by state.',
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
