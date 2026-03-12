'use client';

import { useQuery } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { api, type NetworkStats } from '@/lib/api';
import { ChartCard } from '@/components/ui/chart-card';
import { SparkChart } from '@/components/ui/spark-chart';
import { CHART_PRIMARY_COLOR, CHART_SECONDARY_COLOR } from '@/lib/chart-colors';

// ---------------------------------------------------------------------------
// KnowledgeSizeTrend
// ---------------------------------------------------------------------------

export function KnowledgeSizeTrend() {
  const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

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
  const hovered = hoveredIdx !== null ? deltaData[hoveredIdx] : null;

  return (
    <div className="border-base-border bg-base-surface rounded-lg border px-4 py-3">
      <div className="mb-1.5 flex items-baseline justify-between">
        <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
          Knowledge Bytes — Daily Change
        </div>
        {hovered && (
          <div className="font-mono text-[10px] tabular-nums">
            <span className="text-text-dim">{hovered.date}</span>{' '}
            <span className={hovered.delta >= 0 ? 'text-jade' : 'text-[#e8555a]'}>
              {hovered.delta >= 0 ? '+' : ''}
              {hovered.delta.toFixed(2)} CKB
            </span>
          </div>
        )}
      </div>
      {isLoading || deltaData.length === 0 ? (
        <div className="bg-base-elevated h-14 w-full animate-pulse rounded" />
      ) : (
        <div className="flex h-14 items-end gap-[1px]" onMouseLeave={() => setHoveredIdx(null)}>
          {deltaData.map((d, i) => (
            <div
              key={d.date}
              className="flex-1 cursor-crosshair rounded-t-sm transition-opacity duration-100"
              style={{
                height: `${Math.max((Math.abs(d.delta) / maxDelta) * 100, 4)}%`,
                backgroundColor: d.delta >= 0 ? CHART_PRIMARY_COLOR : '#e8555a',
                opacity: hoveredIdx !== null && hoveredIdx !== i ? 0.3 : 0.8,
              }}
              onMouseEnter={() => setHoveredIdx(i)}
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
