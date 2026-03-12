'use client';

import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { api, type NetworkStats } from '@/lib/api';
import { ChartCard } from '@/components/ui/chart-card';
import { SparkChart } from '@/components/ui/spark-chart';
import { CHART_PRIMARY_COLOR, CHART_SECONDARY_COLOR } from '@/lib/chart-colors';

const BAR_COLORS = ['#8ce00a', '#00d7eb', '#ff66aa', '#bb88ff', '#ff8800'];

// ---------------------------------------------------------------------------
// KnowledgeSizeTrend
// ---------------------------------------------------------------------------

export function KnowledgeSizeTrend() {
  const { data: chart, isLoading } = useQuery({
    queryKey: ['knowledge-size-chart'],
    queryFn: () => api.getKnowledgeSizeChart(),
    staleTime: 300_000,
    refetchInterval: 300_000,
  });

  const deltaData = useMemo(() => {
    const points = chart?.data?.slice(-31) ?? [];
    if (points.length < 2) return [];
    const deltas: { date: string; delta: number }[] = [];
    for (let i = 1; i < points.length; i++) {
      deltas.push({
        date: points[i].date,
        delta: parseFloat(points[i].value) - parseFloat(points[i - 1].value),
      });
    }
    return deltas;
  }, [chart]);

  const maxDelta = Math.max(...deltaData.map((d) => Math.abs(d.delta)), 1);

  return (
    <div className="border-base-border bg-base-surface rounded-lg border px-4 py-3">
      <div className="text-text-dim mb-1.5 font-mono text-[10px] uppercase tracking-wider">
        Knowledge Size — Daily Change
      </div>
      {isLoading || deltaData.length === 0 ? (
        <div className="bg-base-elevated h-8 w-full animate-pulse rounded" />
      ) : (
        <div className="flex h-8 items-end gap-[1px]">
          {deltaData.map((d) => (
            <div
              key={d.date}
              className="flex-1 rounded-t-sm"
              style={{
                height: `${Math.max((Math.abs(d.delta) / maxDelta) * 100, 4)}%`,
                backgroundColor: d.delta >= 0 ? CHART_PRIMARY_COLOR : '#e8555a',
                opacity: 0.8,
              }}
              title={`${d.date}: ${d.delta >= 0 ? '+' : ''}${d.delta.toFixed(2)}`}
            />
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// NetworkHealth
// ---------------------------------------------------------------------------

interface NetworkHealthProps {
  stats: NetworkStats | null;
}

export function NetworkHealth({ stats }: NetworkHealthProps) {
  const { data: blockTimeChart, isLoading: btLoading } = useQuery({
    queryKey: ['network-health-block-time'],
    queryFn: () => api.getAverageBlockTimeChart(),
    staleTime: 300_000,
    refetchInterval: 300_000,
  });

  const { data: hashRateChart, isLoading: hrLoading } = useQuery({
    queryKey: ['network-health-hash-rate'],
    queryFn: () => api.getHashRateChart(),
    staleTime: 300_000,
    refetchInterval: 300_000,
  });

  const btSparkData = useMemo(
    () => blockTimeChart?.data?.slice(-14).map((d) => parseFloat(d.value)) ?? [],
    [blockTimeChart]
  );

  const hrSparkData = useMemo(
    () => hashRateChart?.data?.slice(-14).map((d) => parseFloat(d.value)) ?? [],
    [hashRateChart]
  );

  const isLoading = btLoading || hrLoading;

  return (
    <ChartCard
      title="Network Health"
      href="/charts/average-block-time"
      isLoading={isLoading}
      height={100}
    >
      <div className="space-y-3">
        <div className="flex items-center gap-3">
          <div className="min-w-0 flex-shrink-0">
            <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
              Block Time
            </div>
            <div className="text-text-bright font-mono text-sm font-bold tabular-nums">
              {stats?.avgBlockTime ?? '-'}
            </div>
          </div>
          <div className="min-w-0 flex-1">
            <SparkChart data={btSparkData} height={28} color={CHART_PRIMARY_COLOR} />
          </div>
        </div>

        <div className="flex items-center gap-3">
          <div className="min-w-0 flex-shrink-0">
            <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
              Hash Rate
            </div>
            <div className="text-text-bright font-mono text-sm font-bold tabular-nums">
              {stats?.hashRate ?? '-'}
            </div>
          </div>
          <div className="min-w-0 flex-1">
            <SparkChart data={hrSparkData} height={28} color={CHART_SECONDARY_COLOR} />
          </div>
        </div>
      </div>
    </ChartCard>
  );
}

// ---------------------------------------------------------------------------
// ScriptUtilization
// ---------------------------------------------------------------------------

export function ScriptUtilization() {
  const { data, isLoading } = useQuery({
    queryKey: ['scripts-top5'],
    queryFn: () => api.getScripts({ limit: 5, sortKey: 'used', sortDirection: 'desc' }),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });

  const scripts = data?.data ?? [];
  const maxCap = Math.max(...scripts.map((s) => parseFloat(s.liveUsedCapacitySum ?? '0')), 1);

  return (
    <ChartCard
      title="Script Utilization"
      href="/charts/most-utilized-scripts"
      isLoading={isLoading}
      height={100}
    >
      <div className="space-y-2">
        {scripts.map((s, i) => {
          const cap = parseFloat(s.liveUsedCapacitySum ?? '0');
          const pct = (cap / maxCap) * 100;
          return (
            <div key={s.codeHash} className="flex items-center gap-2">
              <span className="text-text-dim w-20 truncate font-mono text-[10px]">{s.name}</span>
              <div className="bg-base-elevated h-2 flex-1 overflow-hidden rounded-full">
                <div
                  className="h-full rounded-full"
                  style={{ width: `${pct}%`, backgroundColor: BAR_COLORS[i % BAR_COLORS.length] }}
                />
              </div>
            </div>
          );
        })}
      </div>
    </ChartCard>
  );
}
