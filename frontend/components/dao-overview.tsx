'use client';

import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { CHART_PRIMARY_COLOR } from '@/lib/chart-colors';
import Link from '@/components/ui/link';

function formatCkb(value?: string): string {
  if (!value) return '\u2014';
  const num = parseFloat(value);
  if (num >= 1e9) return `${(num / 1e9).toFixed(2)}B`;
  if (num >= 1e6) return `${(num / 1e6).toFixed(2)}M`;
  if (num >= 1e3) return `${(num / 1e3).toFixed(1)}k`;
  return num.toLocaleString();
}

export function DaoOverview() {
  const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

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
  const hovered = hoveredIdx !== null ? deltaData[hoveredIdx] : null;

  return (
    <Link href="/dao" className="block h-full">
      <div className="border-base-border bg-base-surface hover:border-jade/30 flex h-full flex-col rounded-lg border px-4 py-3 transition-colors">
        {/* Header row */}
        <div className="mb-1.5 flex items-baseline justify-between">
          <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">
            Nervos DAO - Daily Change
          </div>
          {hovered ? (
            <div className="font-mono text-[10px] tabular-nums">
              <span className="text-text-dim">{hovered.date}</span>{' '}
              <span className={hovered.delta >= 0 ? 'text-jade' : 'text-[#e8555a]'}>
                {hovered.delta >= 0 ? '+' : ''}
                {hovered.delta.toFixed(2)} CKB
              </span>
            </div>
          ) : (
            !isLoading && (
              <span className="text-emphasis font-mono text-[10px] font-bold tabular-nums">
                {formatCkb(daoStats?.totalDepositedCkb)} CKB
              </span>
            )
          )}
        </div>

        {/* Daily delta bar chart */}
        {isLoading || deltaData.length === 0 ? (
          <div className="bg-base-elevated h-10 w-full animate-pulse rounded lg:h-14" />
        ) : (
          <div
            className="flex h-10 items-end gap-[1px] lg:h-14"
            onMouseLeave={() => setHoveredIdx(null)}
          >
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
                title={`${d.date}: ${d.delta >= 0 ? '+' : ''}${d.delta.toFixed(2)} CKB`}
              />
            ))}
          </div>
        )}

        {/* Stats grid */}
        {!isLoading && daoStats && (
          <div className="border-base-border/40 divide-base-border/40 mt-2 grid grid-cols-2 divide-x divide-y border-t lg:flex-1 lg:grid-rows-2">
            <DaoStat label="Estimated APC" value={`${daoStats.estimatedApc}%`} highlight />
            <DaoStat label="Depositors" value={daoStats.totalDepositors.toLocaleString()} />
            <DaoStat label="Active Deposits" value={daoStats.activeDeposits.toLocaleString()} />
            <DaoStat
              label="Avg Deposit"
              value={`${parseFloat(daoStats.averageDepositDays).toFixed(0)}d`}
            />
          </div>
        )}
      </div>
    </Link>
  );
}

function DaoStat({
  label,
  value,
  highlight,
}: {
  label: string;
  value: string;
  highlight?: boolean;
}) {
  return (
    <div className="px-3 py-2 text-center lg:flex lg:min-h-0 lg:flex-col lg:justify-center">
      <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">{label}</div>
      <div
        className={`mt-0.5 font-mono text-sm font-bold tabular-nums ${highlight ? 'text-jade' : 'text-text-bright'}`}
      >
        {value}
      </div>
    </div>
  );
}
