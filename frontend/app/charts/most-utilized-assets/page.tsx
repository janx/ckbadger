'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
} from '@/components/ui/terminal-panel';
import { ChartCalculationNote } from '@/components/charts/chart-calculation-note';
import { getChartDescription } from '@/components/charts/chart-calculation-descriptions';
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
          <span className="text-text-dim truncate font-mono" title={item.label}>
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
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/charts"
            className="hover:text-emphasis text-text-dim text-sm transition-colors"
          >
            ← Back to Charts
          </Link>
        </div>

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Assets Used & Total CKBytes</TerminalPanelHeader>
          <TerminalPanelContent className="space-y-8 p-6">
            {isLoading && (
              <div className="border-base-border bg-base-surface/50 h-96 animate-pulse rounded border" />
            )}
            {error && (
              <div className="text-text-dim flex h-96 items-center justify-center">
                Failed to load chart data
              </div>
            )}
            {data && (
              <>
                <section>
                  <h3 className="text-text mb-3 font-mono text-sm uppercase tracking-wider">
                    Used CKBytes Share (%) - Top 20 + Others
                  </h3>
                  <StackedAreaChart
                    data={data.usedShare.data}
                    series={data.usedShare.series}
                    height={360}
                    isPercentage
                    valueUnit="shannon"
                  />
                  <SeriesLegend series={data.usedShare.series} />
                </section>

                <section className="border-base-border border-t pt-6">
                  <h3 className="text-text mb-3 font-mono text-sm uppercase tracking-wider">
                    Total CKBytes Share (%) - Top 20 + Others
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

                <div className="text-text-dim text-center font-mono text-xs">
                  Drag to select range • Scroll to zoom • Middle-click drag to pan • Click Reset to
                  restore
                </div>
                <ChartCalculationNote
                  description={
                    getChartDescription('chart-most-utilized-assets') ?? {
                      overview:
                        'Ranks assets by common knowledge size and total live capacity share over time.',
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
