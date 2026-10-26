'use client';
import { useQuery } from '@tanstack/react-query';
import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import { Header } from '@/components/layout/header';
import { DetailLayout } from '@/components/ui/detail-layout';
import Link from '@/components/ui/link';
import { api } from '@/lib/api';
import { totalActivities } from '@/lib/activity-stats';

export default function ActivitiesPage() {
  const { data, error } = useQuery({
    queryKey: ['daily-activity-stats', 2],
    queryFn: () => api.getDailyActivityStats(2),
    refetchInterval: 60000,
  });
  const yesterday = new Date(Date.now() - 86400000).toISOString().slice(0, 10);
  const summary = data?.find(
    (day) => day.date.replace(/^(\d{4})(\d{2})(\d{2})$/, '$1-$2-$3') === yesterday
  );
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
          {summary ? (
            <p className="text-text text-[15px]">
              <span className="text-text-bright">
                {totalActivities(summary).toLocaleString('en-US')}
              </span>{' '}
              yesterday ·{' '}
              <span className="text-text-bright">
                {summary.uniqueAddressCount.toLocaleString('en-US')}
              </span>{' '}
              addresses took part
            </p>
          ) : error ? (
            <p role="alert" className="text-text-dim text-xs">
              Could not load yesterday’s activity: {error.message}
            </p>
          ) : data ? (
            <p className="text-text-dim text-xs">
              No complete activity statistics for {yesterday}.
            </p>
          ) : (
            <div className="bg-base-surface h-5 w-72 animate-pulse" />
          )}
          <ActivitiesStreamExplorer summary={summary} />
        </DetailLayout>
      </main>
    </div>
  );
}
