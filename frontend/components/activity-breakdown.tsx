'use client';

import { useQuery } from '@tanstack/react-query';
import { api, type DailyActivityStats } from '@/lib/api';
import { PieChart } from '@/components/ui/pie-chart';
import {
  TerminalPanel,
  TerminalPanelHeader,
  TerminalPanelContent,
} from '@/components/ui/terminal-panel';
import { formatCkbCompact } from '@/lib/utils';

interface ActivityBreakdownProps {
  isRealtime?: boolean;
}

const ACTIVITY_COLORS: Record<string, string> = {
  Transfer: '#8ce00a',
  'DAO Deposit': '#00d7eb',
  'DAO Withdraw': '#ffb900',
  Token: '#a78bfa',
  NFT: '#f472b6',
  Coinbase: '#64748b',
};

function buildChartData(stats: DailyActivityStats) {
  return [
    { label: 'Transfer', value: stats.transferCount, color: ACTIVITY_COLORS.Transfer },
    { label: 'DAO Deposit', value: stats.daoDepositCount, color: ACTIVITY_COLORS['DAO Deposit'] },
    {
      label: 'DAO Withdraw',
      value: stats.daoWithdrawRequestCount + stats.daoWithdrawCompleteCount,
      color: ACTIVITY_COLORS['DAO Withdraw'],
    },
    { label: 'Token', value: stats.tokenCount, color: ACTIVITY_COLORS.Token },
    { label: 'NFT', value: stats.nftCount, color: ACTIVITY_COLORS.NFT },
    { label: 'Coinbase', value: stats.coinbaseCount, color: ACTIVITY_COLORS.Coinbase },
  ].filter((s) => s.value > 0);
}

export function ActivityBreakdown({ isRealtime = false }: ActivityBreakdownProps) {
  const { data: stats, isLoading } = useQuery({
    queryKey: ['daily-activity-stats-today'],
    queryFn: () => api.getDailyActivityStats(1),
    refetchInterval: 30000,
  });

  const today = stats?.[0];
  const chartData = today ? buildChartData(today) : [];
  const totalActivities = today
    ? today.transferCount +
      today.daoDepositCount +
      today.daoWithdrawRequestCount +
      today.daoWithdrawCompleteCount +
      today.tokenCount +
      today.nftCount +
      today.coinbaseCount
    : 0;

  return (
    <TerminalPanel variant="default" glow={isRealtime}>
      <TerminalPanelHeader indicator={isRealtime ? 'active' : 'inactive'}>
        Activity Breakdown
      </TerminalPanelHeader>
      <TerminalPanelContent>
        {isLoading || !today ? (
          <div className="flex h-full items-center justify-center py-8">
            <div className="bg-base-elevated h-32 w-32 animate-pulse rounded-full" />
          </div>
        ) : (
          <div className="flex flex-col items-center gap-4">
            <PieChart data={chartData} size={200} formatValue={(v) => v.toLocaleString()} />
            <div className="grid w-full grid-cols-3 gap-x-4 gap-y-2">
              <StatItem label="Activities" value={totalActivities.toLocaleString()} />
              <StatItem label="Addresses" value={today.uniqueAddressCount.toLocaleString()} />
              <StatItem
                label="Volume"
                value={formatCkbCompact(today.totalCkbMoved).value + ' CKB'}
              />
            </div>
          </div>
        )}
      </TerminalPanelContent>
    </TerminalPanel>
  );
}

function StatItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="text-center">
      <div className="text-text-muted font-mono text-[10px] uppercase tracking-wider">{label}</div>
      <div className="text-emphasis font-mono text-sm">{value}</div>
    </div>
  );
}
