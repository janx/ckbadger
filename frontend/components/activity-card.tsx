'use client';

import { useQuery } from '@tanstack/react-query';
import { api, type ActivitySummary24h } from '@/lib/api';
import { PieChart } from '@/components/ui/pie-chart';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { formatCkbCompact } from '@/lib/utils';
import { CHART_PRIMARY_COLOR } from '@/lib/chart-colors';
import Link from '@/components/ui/link';

function formatCompact(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}k`;
  return n.toString();
}

const ACTIVITY_COLORS: Record<string, string> = {
  Transfer: '#00ffaa',
  'DAO Deposit': '#44ee77',
  'DAO Withdraw': '#2daa55',
  Token: '#ff66aa',
  Object: '#bb88ff',
  Identity: '#44bbff',
  'Script Call': '#ff8800',
};

function buildPieData(stats: ActivitySummary24h) {
  return [
    { label: 'Transfer', value: stats.transferCount, color: ACTIVITY_COLORS.Transfer },
    { label: 'DAO Deposit', value: stats.daoDepositCount, color: ACTIVITY_COLORS['DAO Deposit'] },
    {
      label: 'DAO Withdraw',
      value: stats.daoWithdrawRequestCount + stats.daoWithdrawCompleteCount,
      color: ACTIVITY_COLORS['DAO Withdraw'],
    },
    { label: 'Token', value: stats.tokenCount, color: ACTIVITY_COLORS.Token },
    { label: 'Object', value: stats.objectCount, color: ACTIVITY_COLORS.Object },
    { label: 'Identity', value: stats.identityCount, color: ACTIVITY_COLORS.Identity },
    { label: 'Script Call', value: stats.scriptCallCount, color: ACTIVITY_COLORS['Script Call'] },
  ].filter((s) => s.value > 0);
}

interface ActivityCardProps {
  isRealtime?: boolean;
}

export function ActivityCard({ isRealtime = false }: ActivityCardProps) {
  const { data: dailyStats, isLoading: isDailyLoading } = useQuery({
    queryKey: ['daily-activity-stats', 14],
    queryFn: () => api.getDailyActivityStats(14),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });

  const { data: summary, isLoading: isSummaryLoading } = useQuery({
    queryKey: ['activity-summary-24h'],
    queryFn: () => api.getActivitySummary24h(),
    refetchInterval: 30000,
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

  const pieData = summary ? buildPieData(summary) : [];

  const headerActions = (
    <Link
      href="/charts"
      className="text-text-dim hover:text-jade font-mono text-xs transition-colors"
    >
      VIEW ALL &rarr;
    </Link>
  );

  return (
    <TerminalPanel variant="default" glow={isRealtime}>
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'} actions={headerActions}>
        <Link href="/charts" className="hover:text-jade transition-colors">
          Activity
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

        {/* Type breakdown */}
        {!isLoading && (
          <div className="mb-4">
            <div className="text-text-dim flex flex-wrap gap-x-3 gap-y-0.5 font-mono text-[10px]">
              {breakdownItems.map((item) => (
                <span key={item.label}>
                  {item.label}: <span className="text-text-bright">{item.value}</span>
                </span>
              ))}
            </div>
          </div>
        )}

        {/* Pie chart */}
        {!isLoading && pieData.length > 0 && (
          <div className="mb-4 flex justify-center">
            <PieChart data={pieData} size={160} formatValue={(v) => v.toLocaleString()} />
          </div>
        )}

        {/* 24h stats */}
        <div className="grid grid-cols-3 gap-x-4 gap-y-2">
          <StatItem
            label="Activities"
            value={
              summary
                ? formatCompact(
                    summary.transferCount +
                      summary.daoDepositCount +
                      summary.daoWithdrawRequestCount +
                      summary.daoWithdrawCompleteCount +
                      summary.tokenCount +
                      summary.objectCount +
                      summary.identityCount +
                      summary.scriptCallCount
                  )
                : '\u2014'
            }
            isLoading={isLoading}
          />
          <StatItem
            label="Addresses"
            value={summary ? summary.uniqueAddressCount.toLocaleString() : '\u2014'}
            isLoading={isLoading}
          />
          <StatItem
            label="Volume"
            value={summary ? formatCkbCompact(summary.totalCkbMoved).value + ' CKB' : '\u2014'}
            isLoading={isLoading}
          />
        </div>
      </TerminalPanelContent>
    </TerminalPanel>
  );
}

function StatItem({
  label,
  value,
  isLoading,
}: {
  label: string;
  value: string;
  isLoading: boolean;
}) {
  return (
    <div className="text-center">
      <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">{label}</div>
      {isLoading ? (
        <div className="bg-base-elevated mx-auto mt-1 h-4 w-12 animate-pulse rounded" />
      ) : (
        <div className="text-emphasis mt-1 font-mono text-sm">{value}</div>
      )}
    </div>
  );
}
