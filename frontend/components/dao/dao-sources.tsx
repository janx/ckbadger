'use client';

import { useQuery } from '@tanstack/react-query';
import { isAwaited } from '@/lib/query-state';
import { api } from '@/lib/api';
import Link from '@/components/ui/link';
import { ActivityRow, ActivityTitle, activityTitleText } from '@/components/activity/activity-row';
import { ActivityRuns, runSubject } from '@/components/activity/activity-runs';
import { PromptHeading } from '@/components/ui/prompt-heading';

/** Hierarchy R15 (v3): a topic page's activity feed shows three rows; the list pages page the rest. */
const RECENT_ROWS = 3;

/**
 * "Recent DAO activity" (hierarchy D7, rulings R7, R15): the three newest DAO activities in the
 * shared activity row, tight (10 px above and below) — every participant shown (ACTIVITY_DESIGN
 * "Show All Layers"), lock kinds in their own column as on the activities page — and links to
 * every DAO activity and every DAO transaction on their list pages. What the feed lists is said in
 * tier 2.
 */
export function DaoSources() {
  const activities = useQuery({
    queryKey: ['dao-recent-activities', RECENT_ROWS],
    queryFn: () => api.getGlobalActivities({ filter: 'dao', limit: RECENT_ROWS }),
  });
  const rows = activities.data?.data;
  return (
    <section className="min-w-0" aria-label="Recent DAO activity">
      {/* Phase 5 (B1): the feed as a command on its row count; the two list links are the note. */}
      <div className="mb-2.5">
        <PromptHeading
          command="dao activity"
          args={`--last ${RECENT_ROWS}`}
          note={
            <span className="text-small flex flex-wrap items-baseline gap-x-6 gap-y-1">
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
          }
        />
      </div>
      {activities.isError ? (
        <div role="alert" className="text-rouge text-small py-4">
          DAO activities could not be loaded: {String(activities.error)}
        </div>
      ) : isAwaited(activities) ? (
        <div className="text-text-dim text-small py-7">Loading DAO activities…</div>
      ) : !rows?.length ? (
        <div className="text-text-dim text-small py-7">No DAO activities at this indexed tip.</div>
      ) : (
        // B4: consecutive rows that say the same thing fold into one counted run
        <ActivityRuns
          age
          rows={rows.map((row) => ({
            key: row.txHash,
            subject: runSubject(row, activityTitleText(row)),
            activity: row,
            title: <ActivityTitle activity={row} />,
            row: <ActivityRow activity={row} lockColumn age className="py-2.5" />,
          }))}
        />
      )}
    </section>
  );
}
