'use client';

import Link from 'next/link';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { MultiSeriesLineChart } from '@/components/ui/multi-series-line-chart';
import { api } from '@/lib/api';

export default function CellCountPage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['chart-cell-count'],
    queryFn: api.getCellCountChart,
  });

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/charts"
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            &larr; Back to Charts
          </Link>
        </div>

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Cell Count</TerminalPanelHeader>
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
              <MultiSeriesLineChart
                data={data.data}
                series={data.series}
                height={400}
                defaultVisibleSeries={['liveCells']}
              />
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
