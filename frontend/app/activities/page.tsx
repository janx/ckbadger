'use client';
import { useQuery } from '@tanstack/react-query';
import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import { Header } from '@/components/layout/header';
import { DetailLayout } from '@/components/ui/detail-layout';
import Link from '@/components/ui/link';
import { api } from '@/lib/api';
import {
  completedYesterdayActivities,
  completedYesterdayTransactions,
  nonCellbaseCount,
  previousChainDay,
} from '@/lib/activity-stats';

export default function ActivitiesPage() {
  const { data, error } = useQuery({
    queryKey: ['daily-activity-stats', 2],
    queryFn: () => api.getDailyActivityStats(2),
    refetchInterval: 60000,
  });
  const { data: txStats, error: txStatsError } = useQuery({
    queryKey: ['tx-stats'],
    queryFn: () => api.getTxStats(),
    refetchInterval: 60000,
  });
  const yesterday = previousChainDay();
  const summary = data ? completedYesterdayActivities(data) : undefined;
  const transactions = txStats ? completedYesterdayTransactions(txStats) : undefined;
  const count =
    summary && transactions !== undefined
      ? nonCellbaseCount(transactions, summary.coinbaseCount, yesterday)
      : undefined;
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <DetailLayout
          title="Activities"
          kind="index"
          actions={
            <Link href="/transactions" className="text-text-dim hover:text-jade text-xs">
              Transactions →
            </Link>
          }
        >
          {summary && count !== undefined ? (
            <p className="text-text text-[15px]">
              <span className="text-text-bright">{count.toLocaleString('en-US')}</span> yesterday
              (UTC+8) ·{' '}
              <span className="text-text-bright">
                {summary.uniqueAddressCount.toLocaleString('en-US')}
              </span>{' '}
              addresses took part
            </p>
          ) : error || txStatsError ? (
            <p role="alert" className="text-text-dim text-xs">
              Could not load yesterday’s activity: {(error ?? txStatsError)!.message}
            </p>
          ) : data && txStats ? (
            <p className="text-text-dim text-xs">
              No complete activity statistics for {yesterday} (UTC+8).
            </p>
          ) : (
            <div className="bg-base-surface h-5 w-72 animate-pulse" />
          )}
          <ActivitiesStreamExplorer summary={summary} totalCount={count} />
        </DetailLayout>
      </main>
    </div>
  );
}
