'use client';

import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { CHART_PRIMARY_COLOR } from '@/lib/chart-colors';

function formatCkb(value?: string): string {
  if (!value) return '\u2014';
  const num = parseFloat(value);
  if (num >= 1e9) return `${(num / 1e9).toFixed(2)}B`;
  if (num >= 1e6) return `${(num / 1e6).toFixed(2)}M`;
  return num.toLocaleString();
}

export function DaoOverview() {
  const { data: daoStats, isLoading } = useQuery({
    queryKey: ['dao-statistics'],
    queryFn: () => api.getDaoStatistics(),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  const { data: depositChart } = useQuery({
    queryKey: ['dao-total-deposit-chart'],
    queryFn: () => api.getDaoTotalDepositChart(),
    staleTime: 300_000,
    refetchInterval: 300_000,
  });

  const deltaData = useMemo(() => {
    const points = depositChart?.data?.slice(-31) ?? [];
    if (points.length < 2) return [];
    const deltas: { date: string; delta: number }[] = [];
    for (let i = 1; i < points.length; i++) {
      deltas.push({
        date: points[i].date,
        delta: parseFloat(points[i].value) - parseFloat(points[i - 1].value),
      });
    }
    return deltas;
  }, [depositChart]);

  const maxDelta = Math.max(...deltaData.map((d) => Math.abs(d.delta)), 1);

  return (
    <div className="border-base-border bg-base-surface rounded-lg border px-4 py-3">
      {/* Header row: title + stats inline */}
      <div className="mb-1.5 flex items-baseline justify-between">
        <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
          Nervos DAO
        </div>
        {!isLoading && (
          <div className="flex items-baseline gap-3">
            <span className="text-emphasis font-mono text-xs font-bold tabular-nums">
              {formatCkb(daoStats?.totalDepositedCkb)} CKB
            </span>
            <span className="text-jade font-mono text-[10px] font-bold tabular-nums">
              {daoStats?.estimatedApc ? `APC ${daoStats.estimatedApc}%` : ''}
            </span>
            <span className="text-text-dim font-mono text-[10px] tabular-nums">
              {daoStats?.totalDepositors != null
                ? `${daoStats.totalDepositors.toLocaleString()} depositors`
                : ''}
            </span>
          </div>
        )}
      </div>

      {/* Daily delta bar chart */}
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
              title={`${d.date}: ${d.delta >= 0 ? '+' : ''}${d.delta.toFixed(2)} CKB`}
            />
          ))}
        </div>
      )}
    </div>
  );
}
