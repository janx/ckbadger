'use client';
import { TopicCharts } from '@/components/charts/topic-charts';
import { useQuery } from '@tanstack/react-query';
import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import { DetailLayout } from '@/components/ui/detail-layout';
import Link from '@/components/ui/link';
import { api } from '@/lib/api';
import { completedYesterdayActivities, previousChainDay } from '@/lib/activity-stats';
import { countNoun } from '@/lib/format-count';
import { NoBreak } from '@/components/ui/no-break';

export default function ActivitiesPage() {
  const { data, error } = useQuery({
    queryKey: ['daily-activity-stats', 2],
    queryFn: () => api.getDailyActivityStats(2),
    refetchInterval: 60000,
  });
  const yesterday = previousChainDay();
  const summary = data ? completedYesterdayActivities(data) : undefined;
  // yesterday's activities, the route's count: every transaction but the cellbases, each in one
  // kind (no page sums)
  const count = summary?.activityCount;
  return (
    <div className="min-h-screen">
      <main className="container mx-auto py-8">
        <DetailLayout
          title="Activities"
          kind="index"
          actions={
            <Link href="/transactions" look="onward" className="text-small">
              Transactions
            </Link>
          }
        >
          {/* What the page says first (lists lane P3): yesterday's activities, a count with its
              noun and its period; the list below is the latest, by day (its own line says so). */}
          {summary && count !== undefined ? (
            <p data-role="lede" className="text-text text-body max-w-measure">
              <NoBreak>
                <span className="text-text-bright">{count.toLocaleString('en-US')}</span>{' '}
                {countNoun(count, 'activity', 'activities')}
              </NoBreak>{' '}
              yesterday (UTC+8), by{' '}
              <NoBreak>
                <span className="text-text-bright">
                  {summary.uniqueAddressCount.toLocaleString('en-US')}
                </span>{' '}
                {countNoun(summary.uniqueAddressCount, 'address', 'addresses')}
              </NoBreak>
              .
            </p>
          ) : error ? (
            <p role="alert" className="text-text text-small max-w-measure">
              Could not load yesterday’s activity: {error.message}
            </p>
          ) : data ? (
            <p className="text-text-dim text-small max-w-measure">
              No complete activity statistics for {yesterday} (UTC+8).
            </p>
          ) : (
            <div className="bg-base-surface h-5 w-72 animate-pulse" />
          )}
          <ActivitiesStreamExplorer summary={summary} />
        </DetailLayout>
        <TopicCharts topic="/activities" />
      </main>
    </div>
  );
}
