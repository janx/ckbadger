'use client';

import { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, NetworkHistory, StackedAreaDataPoint, StackedAreaSeries } from '@/lib/api';
import { MultiSeriesLineChart } from '@/components/ui/multi-series-line-chart';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { getChartPaletteColor } from '@/lib/chart-colors';

const MAX_SHARE_SERIES = 8;

// Seconds-precision "now" so the API drops the current (incomplete) daily bucket.
function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

function formatDay(ts: number): string {
  return new Date(ts * 1000).toISOString().slice(0, 10);
}

// Merge the two independent scalar histories (total + reachable node counts) into a single
// same-axis dataset keyed by day, so both lines share one honest node-count scale.
function mergeNodeCounts(
  total: NetworkHistory | undefined,
  reachable: NetworkHistory | undefined
): StackedAreaDataPoint[] {
  const byTs = new Map<number, { total?: number; reachable?: number }>();
  for (const p of total?.points ?? []) {
    byTs.set(p.ts, { ...(byTs.get(p.ts) ?? {}), total: p.scalar });
  }
  for (const p of reachable?.points ?? []) {
    byTs.set(p.ts, { ...(byTs.get(p.ts) ?? {}), reachable: p.scalar });
  }
  return [...byTs.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([ts, v]) => ({
      date: formatDay(ts),
      values: {
        totalNodes: String(v.total ?? 0),
        reachableNodes: String(v.reachable ?? 0),
      },
    }));
}

// Turn per-day label buckets into stacked-area series. Series are the top-N labels by total
// count across the whole window, so the stack is stable across days.
function bucketsToStacked(history: NetworkHistory | undefined): {
  data: StackedAreaDataPoint[];
  series: StackedAreaSeries[];
} {
  const points = history?.points ?? [];
  const totals = new Map<string, number>();
  for (const p of points) {
    for (const b of p.buckets) {
      totals.set(b.label, (totals.get(b.label) ?? 0) + b.count);
    }
  }
  const topLabels = [...totals.entries()]
    .sort((a, b) => b[1] - a[1])
    .slice(0, MAX_SHARE_SERIES)
    .map(([label]) => label);
  const series: StackedAreaSeries[] = topLabels.map((label, i) => ({
    key: label,
    label,
    color: getChartPaletteColor(i),
  }));
  const data: StackedAreaDataPoint[] = points.map((p) => {
    const values: Record<string, string> = {};
    for (const label of topLabels) {
      const found = p.buckets.find((b) => b.label === label);
      values[label] = String(found?.count ?? 0);
    }
    return { date: formatDay(p.ts), values };
  });
  return { data, series };
}

function ChartLegend({ series }: { series: StackedAreaSeries[] }) {
  return (
    <div className="mt-4 flex flex-wrap items-center justify-center gap-4">
      {series.map((s) => (
        <div key={s.key} className="flex items-center gap-2">
          <span className="h-3 w-3 rounded" style={{ backgroundColor: s.color }} />
          <span className="text-text-dim font-mono text-xs">{s.label}</span>
        </div>
      ))}
    </div>
  );
}

function TrendPanel({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="border-base-border bg-base-surface rounded border p-4">
      <h3 className="text-text-bright mb-3 font-mono text-sm font-bold">{title}</h3>
      {children}
    </div>
  );
}

function ShareTrend({ title, history }: { title: string; history: NetworkHistory | undefined }) {
  const { data, series } = bucketsToStacked(history);
  return (
    <TrendPanel title={title}>
      {series.length === 0 ? (
        <p className="text-text-dim font-mono text-xs">No data yet</p>
      ) : (
        <>
          <StackedAreaChart data={data} series={series} isPercentage height={260} />
          <ChartLegend series={series} />
        </>
      )}
    </TrendPanel>
  );
}

export function NetworkTrends() {
  const totalNodes = useQuery({
    queryKey: ['network', 'history', 'totalNodes', 'day'],
    queryFn: () => api.getNetworkHistory('totalNodes', 'day', undefined, nowSeconds()),
    refetchInterval: 60000,
  });
  const reachableNodes = useQuery({
    queryKey: ['network', 'history', 'reachableNodes', 'day'],
    queryFn: () => api.getNetworkHistory('reachableNodes', 'day', undefined, nowSeconds()),
    refetchInterval: 60000,
  });
  const versionShare = useQuery({
    queryKey: ['network', 'history', 'versionShare', 'day'],
    queryFn: () => api.getNetworkHistory('versionShare', 'day', undefined, nowSeconds()),
    refetchInterval: 60000,
  });
  const countryShare = useQuery({
    queryKey: ['network', 'history', 'countryShare', 'day'],
    queryFn: () => api.getNetworkHistory('countryShare', 'day', undefined, nowSeconds()),
    refetchInterval: 60000,
  });

  const nodeSeries: StackedAreaSeries[] = [
    { key: 'totalNodes', label: 'Total Nodes', color: getChartPaletteColor(0) },
    { key: 'reachableNodes', label: 'Reachable Nodes', color: getChartPaletteColor(2) },
  ];
  const nodeData = mergeNodeCounts(totalNodes.data, reachableNodes.data);

  return (
    <section className="space-y-4">
      <h2 className="text-text-bright font-mono text-lg font-bold">Trends</h2>
      <p className="text-text-dim font-mono text-xs">
        Daily history of discovered nodes. The current (incomplete) day is excluded.
      </p>

      <TrendPanel title="Discovered Nodes (daily)">
        {nodeData.length === 0 ? (
          <p className="text-text-dim font-mono text-xs">No data yet</p>
        ) : (
          <MultiSeriesLineChart data={nodeData} series={nodeSeries} height={260} />
        )}
      </TrendPanel>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <ShareTrend title="Client Version Share" history={versionShare.data} />
        <ShareTrend title="Country Share" history={countryShare.data} />
      </div>
    </section>
  );
}
