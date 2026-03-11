'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { CHART_PRIMARY_COLOR } from '@/lib/chart-colors';

function formatCompact(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}k`;
  return n.toString();
}

function formatCkbCompact(shannons: string): string {
  const ckb = Number(BigInt(shannons)) / 1e8;
  if (ckb >= 1e9) return `${(ckb / 1e9).toFixed(1)}B`;
  if (ckb >= 1e6) return `${(ckb / 1e6).toFixed(1)}M`;
  return ckb.toLocaleString();
}

export function ActivityTrend() {
  const { data: dailyStats, isLoading: isDailyLoading } = useQuery({
    queryKey: ['daily-activity-stats', 14],
    queryFn: () => api.getDailyActivityStats(14),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });

  const { data: summary, isLoading: isSummaryLoading } = useQuery({
    queryKey: ['activity-summary-24h'],
    queryFn: () => api.getActivitySummary24h(),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  const isLoading = isDailyLoading || isSummaryLoading;

  const barData =
    dailyStats?.map((d) => ({
      date: d.date,
      total:
        d.transferCount +
        d.daoDepositCount +
        d.daoWithdrawRequestCount +
        d.daoWithdrawCompleteCount +
        d.tokenCount +
        d.objectCount +
        d.identityCount +
        d.scriptCallCount,
    })) ?? [];

  const maxVal = Math.max(...barData.map((d) => d.total), 1);

  const daoTotal = summary
    ? summary.daoDepositCount + summary.daoWithdrawRequestCount + summary.daoWithdrawCompleteCount
    : 0;

  const breakdownItems = summary
    ? [
        { label: 'Transfers', value: formatCompact(summary.transferCount) },
        { label: 'DAO', value: formatCompact(daoTotal) },
        { label: 'Tokens', value: formatCompact(summary.tokenCount) },
        { label: 'Objects', value: formatCompact(summary.objectCount) },
      ]
    : [];

  const headerActions = (
    <Link
      href="/charts"
      className="text-text-dim hover:text-jade font-mono text-xs transition-colors"
    >
      VIEW ALL &rarr;
    </Link>
  );

  return (
    <TerminalPanel>
      <TerminalPanelHeader actions={headerActions}>
        <Link href="/charts" className="hover:text-jade transition-colors">
          Activity Trend
        </Link>
      </TerminalPanelHeader>
      <TerminalPanelContent padding="md">
        {/* 14-day bar chart */}
        <div className="mb-4">
          {isLoading || barData.length === 0 ? (
            <div className="bg-base-elevated h-16 w-full animate-pulse rounded" />
          ) : (
            <div className="flex h-16 items-end gap-[2px]">
              {barData.map((d) => (
                <div
                  key={d.date}
                  className="flex-1 rounded-t-sm"
                  style={{
                    height: `${Math.max((d.total / maxVal) * 100, 2)}%`,
                    backgroundColor: CHART_PRIMARY_COLOR,
                    opacity: 0.8,
                  }}
                  title={`${d.date}: ${d.total.toLocaleString()} activities`}
                />
              ))}
            </div>
          )}
        </div>

        {/* Type breakdown text */}
        <div className="mb-4">
          {isLoading ? (
            <div className="bg-base-elevated h-4 w-48 animate-pulse rounded" />
          ) : (
            <div className="text-text-dim flex flex-wrap gap-x-3 gap-y-0.5 font-mono text-[10px]">
              {breakdownItems.map((item) => (
                <span key={item.label}>
                  {item.label}: <span className="text-text-bright">{item.value}</span>
                </span>
              ))}
            </div>
          )}
        </div>

        {/* 24h stats */}
        <div className="grid grid-cols-2 gap-4">
          <div>
            <div className="text-text-dim text-[10px] uppercase tracking-wider">
              Unique Addr (24h)
            </div>
            {isLoading ? (
              <div className="bg-base-elevated mt-1 inline-block h-5 w-16 animate-pulse rounded" />
            ) : (
              <div className="text-text-bright mt-1 font-mono text-sm font-bold tabular-nums">
                {summary ? summary.uniqueAddressCount.toLocaleString() : '\u2014'}
              </div>
            )}
          </div>
          <div>
            <div className="text-text-dim text-[10px] uppercase tracking-wider">
              CKB Moved (24h)
            </div>
            {isLoading ? (
              <div className="bg-base-elevated mt-1 inline-block h-5 w-16 animate-pulse rounded" />
            ) : (
              <div className="text-text-bright mt-1 font-mono text-sm font-bold tabular-nums">
                {summary ? formatCkbCompact(summary.totalCkbMoved) : '\u2014'}
              </div>
            )}
          </div>
        </div>
      </TerminalPanelContent>
    </TerminalPanel>
  );
}
