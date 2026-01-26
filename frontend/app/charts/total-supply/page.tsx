'use client';

import Link from 'next/link';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { api } from '@/lib/api';

export default function TotalSupplyPage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['chart-total-supply'],
    queryFn: api.getTotalSupplyChart,
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
          <TerminalPanelHeader indicator="active">Total Supply</TerminalPanelHeader>
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
                <StackedAreaChart data={data.data} series={data.series} height={400} />
                <div className="mt-6 flex flex-wrap items-center justify-center gap-6 text-sm">
                  {data.series.map((s) => (
                    <div key={s.key} className="flex items-center gap-2">
                      <span className="h-3 w-3 rounded" style={{ backgroundColor: s.color }} />
                      <span className="text-slate-400">{s.label}</span>
                    </div>
                  ))}
                </div>
                <div className="mt-4 text-center font-mono text-xs text-slate-600">
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
