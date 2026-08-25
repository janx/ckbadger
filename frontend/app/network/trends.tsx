'use client';

import { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, NetworkHistory, StackedAreaDataPoint, StackedAreaSeries } from '@/lib/api';
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

// Merge the two independent verified-peer histories into a same-axis dataset keyed by day.
export function mergePeerCounts(
  verified: NetworkHistory | undefined,
  reachable: NetworkHistory | undefined
): { data: StackedAreaDataPoint[]; error: string | null } {
  if (!verified || !reachable) return { data: [], error: null };

  const reachableByTs = new Map<number, number>();
  for (const point of reachable.points) {
    if (reachableByTs.has(point.ts)) {
      return {
        data: [],
        error: `Invalid peer history: duplicate reachablePeers point at ${point.ts}`,
      };
    }
    reachableByTs.set(point.ts, point.scalar);
  }

  const verifiedTimestamps = new Set<number>();
  const data: StackedAreaDataPoint[] = [];
  for (const point of [...verified.points].sort((left, right) => left.ts - right.ts)) {
    if (verifiedTimestamps.has(point.ts)) {
      return {
        data: [],
        error: `Invalid peer history: duplicate verifiedPeers point at ${point.ts}`,
      };
    }
    verifiedTimestamps.add(point.ts);
    const reachableCount = reachableByTs.get(point.ts);
    if (reachableCount == null) {
      return {
        data: [],
        error: `Invalid peer history: missing reachablePeers point at ${point.ts}`,
      };
    }
    if (reachableCount > point.scalar) {
      return {
        data: [],
        error: `Invalid peer history at ${point.ts}: reachablePeers ${reachableCount} exceeds verifiedPeers ${point.scalar}`,
      };
    }
    reachableByTs.delete(point.ts);
    data.push({
      date: formatDay(point.ts),
      values: {
        sameNetworkReachable: String(reachableCount),
        verifiedUnavailable: String(point.scalar - reachableCount),
      },
    });
  }
  const unmatchedReachableTimestamp = reachableByTs.keys().next().value;
  if (unmatchedReachableTimestamp != null) {
    return {
      data: [],
      error: `Invalid peer history: missing verifiedPeers point at ${unmatchedReachableTimestamp}`,
    };
  }
  return { data, error: null };
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
  const verifiedPeers = useQuery({
    queryKey: ['network', 'history', 'verifiedPeers', 'day'],
    queryFn: () => api.getNetworkHistory('verifiedPeers', 'day', undefined, nowSeconds()),
    refetchInterval: 60000,
  });
  const reachablePeers = useQuery({
    queryKey: ['network', 'history', 'reachablePeers', 'day'],
    queryFn: () => api.getNetworkHistory('reachablePeers', 'day', undefined, nowSeconds()),
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

  const peerSeries: StackedAreaSeries[] = [
    {
      key: 'sameNetworkReachable',
      label: 'Same-network reachable',
      color: getChartPaletteColor(2),
    },
    {
      key: 'verifiedUnavailable',
      label: 'Verified unavailable',
      color: getChartPaletteColor(0),
    },
  ];
  const peerCounts = mergePeerCounts(verifiedPeers.data, reachablePeers.data);
  const peerHistoryError =
    verifiedPeers.isError || reachablePeers.isError
      ? 'Failed to load verified peer history.'
      : peerCounts.error;

  return (
    <section className="space-y-4">
      <h2 className="text-text-bright font-mono text-lg font-bold">Trends</h2>
      <p className="text-text-dim font-mono text-xs">
        Daily history of retained verified peers. The current (incomplete) day is excluded.
      </p>

      <TrendPanel title="Retained verification state (daily)">
        {peerHistoryError ? (
          <p className="text-negative font-mono text-xs">{peerHistoryError}</p>
        ) : peerCounts.data.length === 0 ? (
          <p className="text-text-dim font-mono text-xs">No data yet</p>
        ) : (
          <>
            <StackedAreaChart data={peerCounts.data} series={peerSeries} height={260} />
            <ChartLegend series={peerSeries} />
          </>
        )}
      </TrendPanel>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <ShareTrend title="Client Version Share" history={versionShare.data} />
        <ShareTrend title="Country Share" history={countryShare.data} />
      </div>
    </section>
  );
}
