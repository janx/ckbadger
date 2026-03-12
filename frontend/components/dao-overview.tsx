'use client';

import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import { SparkChart } from '@/components/ui/spark-chart';

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

  const sparkData = depositChart?.data?.slice(-30).map((d) => parseFloat(d.value)) ?? [];

  return (
    <div className="border-base-border bg-base-surface rounded-lg border p-4">
      {/* Total Deposited */}
      <div className="mb-4">
        <div className="text-text-dim text-xs uppercase tracking-wider">Total Deposited</div>
        {isLoading ? (
          <div className="bg-base-elevated mt-1 inline-block h-6 w-32 animate-pulse rounded" />
        ) : (
          <div className="text-emphasis mt-1 font-mono text-lg font-bold tabular-nums">
            {formatCkb(daoStats?.totalDepositedCkb)}{' '}
            <span className="text-text-dim text-sm font-normal">CKB</span>
          </div>
        )}
      </div>

      {/* APC and Depositors row */}
      <div className="mb-4 grid grid-cols-2 gap-4">
        <div>
          <div className="text-text-dim text-xs uppercase tracking-wider">APC</div>
          {isLoading ? (
            <div className="bg-base-elevated mt-1 inline-block h-5 w-16 animate-pulse rounded" />
          ) : (
            <div className="text-jade mt-1 font-mono text-base font-bold tabular-nums">
              {daoStats?.estimatedApc ? `${daoStats.estimatedApc}%` : '\u2014'}
            </div>
          )}
        </div>
        <div>
          <div className="text-text-dim text-xs uppercase tracking-wider">Depositors</div>
          {isLoading ? (
            <div className="bg-base-elevated mt-1 inline-block h-5 w-16 animate-pulse rounded" />
          ) : (
            <div className="text-text-bright mt-1 font-mono text-base font-bold tabular-nums">
              {daoStats?.totalDepositors != null
                ? daoStats.totalDepositors.toLocaleString()
                : '\u2014'}
            </div>
          )}
        </div>
      </div>

      {/* 30-day trend sparkline */}
      <div>
        <div className="text-text-dim mb-1 text-[10px] uppercase tracking-wider">30-Day Trend</div>
        {sparkData.length > 0 ? (
          <SparkChart data={sparkData} height={40} />
        ) : (
          <div className="bg-base-elevated h-10 w-full animate-pulse rounded" />
        )}
      </div>
    </div>
  );
}
