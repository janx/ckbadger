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
import { ACTIVITY_TYPE_COLORS, SCRIPT_CHART_COLORS } from '@/lib/chart-colors';

interface ActivityBreakdownProps {
  isRealtime?: boolean;
}

function buildChartData(stats: ActivitySummary24h) {
  return [
    { label: 'Transfer', value: stats.transferCount, color: ACTIVITY_TYPE_COLORS.Transfer },
    {
      label: 'DAO Deposit',
      value: stats.daoDepositCount,
      color: ACTIVITY_TYPE_COLORS['DAO Deposit'],
    },
    {
      label: 'DAO Withdraw',
      value: stats.daoWithdrawRequestCount + stats.daoWithdrawCompleteCount,
      color: ACTIVITY_TYPE_COLORS['DAO Withdraw'],
    },
    { label: 'Token', value: stats.tokenCount, color: ACTIVITY_TYPE_COLORS.Token },
    { label: 'Object', value: stats.objectCount, color: ACTIVITY_TYPE_COLORS.Object },
    { label: 'Identity', value: stats.identityCount, color: ACTIVITY_TYPE_COLORS.Identity },
    {
      label: 'Script Call',
      value: stats.scriptCallCount,
      color: ACTIVITY_TYPE_COLORS['Script Call'],
    },
  ].filter((s) => s.value > 0);
}

function buildScriptChartData(stats: ActivitySummary24h) {
  return stats.scriptCounts
    .filter((s) => s.count > 0)
    .sort((a, b) => b.count - a.count)
    .map((s, i) => ({
      label: s.name || `${s.codeHash.slice(0, 10)}...`,
      value: s.count,
      color: SCRIPT_CHART_COLORS[i % SCRIPT_CHART_COLORS.length],
    }));
}

export function ActivityBreakdown({ isRealtime = false }: ActivityBreakdownProps) {
  const { data: summary, isLoading } = useQuery({
    queryKey: ['activity-summary-24h'],
    queryFn: () => api.getActivitySummary24h(),
    refetchInterval: 30000,
  });

  const chartData = summary ? buildChartData(summary) : [];
  const scriptChartData = summary ? buildScriptChartData(summary) : [];
  const totalActivities = summary
    ? summary.transferCount +
      summary.daoDepositCount +
      summary.daoWithdrawRequestCount +
      summary.daoWithdrawCompleteCount +
      summary.tokenCount +
      summary.objectCount +
      summary.identityCount +
      summary.scriptCallCount
    : 0;

  return (
    <TerminalPanel variant="default" glow={isRealtime}>
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'}>
        Activity Breakdown (24h)
      </TerminalPanelHeader>
      <TerminalPanelContent>
        {isLoading ? (
          <div className="flex h-full items-center justify-center py-8">
            <div className="bg-base-elevated h-32 w-32 animate-pulse rounded-full" />
          </div>
        ) : !summary ? (
          <div className="flex h-full items-center justify-center py-8">
            <span className="text-text-dim font-mono text-xs">No activity data yet</span>
          </div>
        ) : (
          <div className="flex flex-col items-center gap-4">
            <PieChart data={chartData} size={200} formatValue={(v) => v.toLocaleString()} />
            <div className="grid w-full grid-cols-3 gap-x-4 gap-y-2">
              <StatItem label="Activities" value={totalActivities.toLocaleString()} />
              <StatItem label="Addresses" value={summary.uniqueAddressCount.toLocaleString()} />
              <StatItem
                label="Volume"
                value={formatCkbCompact(summary.totalCkbMoved).value + ' CKB'}
              />
            </div>
            {scriptChartData.length > 0 && (
              <>
                <div className="text-text-dim mt-2 font-mono text-[10px] uppercase tracking-wider">
                  Script Usage
                </div>
                <PieChart
                  data={scriptChartData}
                  size={200}
                  formatValue={(v) => v.toLocaleString()}
                />
              </>
            )}
          </div>
        )}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}

function StatItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="text-center">
      <div className="text-text-dim font-mono text-[10px] uppercase tracking-wider">{label}</div>
      <div className="text-emphasis font-mono text-sm">{value}</div>
    </div>
  );
}
