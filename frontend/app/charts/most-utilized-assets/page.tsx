'use client';

import Link from 'next/link';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { api } from '@/lib/api';

function SeriesLegend({
  series,
}: {
  series: Array<{ key: string; label: string; color: string }>;
}) {
  return (
    <div className="mt-4 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
      {series.map((item) => (
        <div key={item.key} className="flex items-center gap-2 text-xs">
          <span className="h-2.5 w-2.5 shrink-0 rounded" style={{ backgroundColor: item.color }} />
          <span className="truncate font-mono text-slate-400" title={item.label}>
            {item.label}
          </span>
        </div>
      ))}
    </div>
  );
}

export default function MostUtilizedAssetsPage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['chart-most-utilized-assets'],
    queryFn: api.getMostUtilizedAssetsChart,
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
            ← Back to Charts
          </Link>
        </div>

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Most Utilized Assets</TerminalPanelHeader>
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
                <section>
                  <h3 className="mb-3 font-mono text-sm uppercase tracking-wider text-slate-300">
                    Occupied Share (%) - Top 20 + Others
                  </h3>
                  <StackedAreaChart
                    data={data.occupiedShare.data}
                    series={data.occupiedShare.series}
                    height={360}
                    isPercentage
                    valueUnit="shannon"
                  />
                  <SeriesLegend series={data.occupiedShare.series} />
                </section>

                <section className="border-t border-slate-800 pt-6">
                  <h3 className="mb-3 font-mono text-sm uppercase tracking-wider text-slate-300">
                    Total Cells Capacity Share (%) - Top 20 + Others
                  </h3>
                  <StackedAreaChart
                    data={data.capacityShare.data}
                    series={data.capacityShare.series}
                    height={360}
                    isPercentage
                    valueUnit="shannon"
                  />
                  <SeriesLegend series={data.capacityShare.series} />
                </section>

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
