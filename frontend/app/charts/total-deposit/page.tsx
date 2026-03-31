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
import { LineChart } from '@/components/ui/line-chart';
import { api } from '@/lib/api';

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

export default function TotalDepositPage() {
  const { data: networkStats } = useQuery({
    queryKey: ['network-stats'],
    queryFn: () => api.getNetworkStats(),
  });

  const {
    data: depositData,
    isLoading: depositLoading,
    error: depositError,
  } = useQuery({
    queryKey: ['dao-chart-total-deposit'],
    queryFn: api.getDaoTotalDepositChart,
  });

  const {
    data: ratioData,
    isLoading: ratioLoading,
    error: ratioError,
  } = useQuery({
    queryKey: ['dao-chart-circulation-ratio'],
    queryFn: api.getDaoCirculationRatioChart,
  });

  const depositDescription = depositData
    ? getChartDescription('dao-chart-total-deposit', {
        yAxisLabel: depositData.yAxisLabel,
        y2AxisLabel: depositData.y2AxisLabel,
      })
    : undefined;

  const ratioDescription = ratioData
    ? getChartDescription('dao-chart-circulation-ratio', {
        yAxisLabel: ratioData.yAxisLabel,
        y2AxisLabel: ratioData.y2AxisLabel,
      })
    : undefined;

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link href="/charts" className="hover:text-jade text-text-dim text-sm transition-colors">
            ← Back to Charts
          </Link>
        </div>

        <ChartDataWarning show={networkStats?.syncStatus?.chartDataMayBeIncomplete ?? false} />

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Total Deposit</TerminalPanelHeader>
          <TerminalPanelContent className="p-6">
            {depositLoading && (
              <div className="border-base-border bg-base-surface/50 h-96 animate-pulse rounded border" />
            )}
            {depositError && (
              <div className="text-text-dim flex h-96 items-center justify-center">
                Failed to load chart data
              </div>
            )}
            {depositData && (
              <>
                <LineChart
                  data={depositData.data}
                  yAxisLabel={depositData.yAxisLabel}
                  y2AxisLabel={depositData.y2AxisLabel}
                  height={400}
                />
                <div className="mt-6 flex items-center justify-center gap-6 text-sm">
                  <div className="flex items-center gap-2">
                    <span className="bg-jade h-0.5 w-4" />
                    <span className="text-text-dim">{depositData.yAxisLabel}</span>
                  </div>
                  {depositData.y2AxisLabel && (
                    <div className="flex items-center gap-2">
                      <span className="bg-rouge h-0.5 w-4" />
                      <span className="text-text-dim">{depositData.y2AxisLabel}</span>
                    </div>
                  )}
                </div>
                <div className="text-text-dim mt-4 text-center font-mono text-xs">
                  Drag to select range • Scroll to zoom • Middle-click drag to pan • Click Reset to
                  restore
                </div>
                {depositDescription && <ChartCalculationNote description={depositDescription} />}
              </>
            )}
          </TerminalPanelContent>
        </TerminalPanel>

        <TerminalPanel className="mt-6">
          <TerminalPanelHeader indicator="active">Deposit to Circulation Ratio</TerminalPanelHeader>
          <TerminalPanelContent className="p-6">
            {ratioLoading && (
              <div className="border-base-border bg-base-surface/50 h-96 animate-pulse rounded border" />
            )}
            {ratioError && (
              <div className="text-text-dim flex h-96 items-center justify-center">
                Failed to load chart data
              </div>
            )}
            {ratioData && (
              <>
                <LineChart
                  data={ratioData.data}
                  yAxisLabel={ratioData.yAxisLabel}
                  y2AxisLabel={ratioData.y2AxisLabel}
                  height={400}
                />
                <div className="mt-6 flex items-center justify-center gap-6 text-sm">
                  <div className="flex items-center gap-2">
                    <span className="bg-jade h-0.5 w-4" />
                    <span className="text-text-dim">{ratioData.yAxisLabel}</span>
                  </div>
                  {ratioData.y2AxisLabel && (
                    <div className="flex items-center gap-2">
                      <span className="bg-rouge h-0.5 w-4" />
                      <span className="text-text-dim">{ratioData.y2AxisLabel}</span>
                    </div>
                  )}
                </div>
                <div className="text-text-dim mt-4 text-center font-mono text-xs">
                  Drag to select range • Scroll to zoom • Middle-click drag to pan • Click Reset to
                  restore
                </div>
                {ratioDescription && <ChartCalculationNote description={ratioDescription} />}
              </>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
