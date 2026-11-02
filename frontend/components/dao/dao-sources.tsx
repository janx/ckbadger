'use client';

import { useQuery } from '@tanstack/react-query';
import { api } from '@/lib/api';
import Link from '@/components/ui/link';
import { ActivityRow } from '@/components/activity/activity-row';

/** Hierarchy R8: a topic page shows five rows; the list pages behind the links page the rest. */
const RECENT_ROWS = 5;

/**
 * "Recent DAO activity" (hierarchy D7, ruling R7): the five newest DAO activities in the shared
 * activity row — every participant shown (ACTIVITY_DESIGN "Show All Layers"), lock kinds in their
 * own column as on the activities page — and links to every DAO activity and every DAO
 * transaction on their list pages.
 */
export function DaoSources() {
  const activities = useQuery({
    queryKey: ['dao-recent-activities', RECENT_ROWS],
    queryFn: () => api.getGlobalActivities({ filter: 'dao', limit: RECENT_ROWS }),
  });
  const rows = activities.data?.data;
  return (
    <section className="min-w-0" aria-label="Recent DAO activity">
      <div className="mb-2.5 flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 className="text-text-bright text-section font-medium">Recent DAO activity</h2>
        <span className="text-text-dim text-small flex flex-wrap items-baseline gap-x-6 gap-y-1">
          <span>deposits, withdrawal requests and withdrawals · newest first</span>
          <Link
            href="/activities?filter=dao"
            className="text-text hover:text-text-bright hover:underline"
          >
            All DAO activity →
          </Link>
          <Link
            href="/transactions?kind=dao"
            className="text-text hover:text-text-bright hover:underline"
          >
            DAO transactions →
          </Link>
        </span>
      </div>
      {activities.isError ? (
        <div role="alert" className="text-rouge text-small py-4">
          DAO activities could not be loaded: {String(activities.error)}
        </div>
      ) : activities.isLoading ? (
        <div className="text-text-dim text-small py-7">Loading DAO activities…</div>
      ) : !rows?.length ? (
        <div className="text-text-dim text-small py-7">No DAO activities at this indexed tip.</div>
      ) : (
        rows.map((row) => (
          <ActivityRow key={`${row.txHash}:${row.txIndex}`} activity={row} lockColumn />
        ))
      )}
    </section>
  );
}
