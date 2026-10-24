'use client';

import { ActivityFeed } from '@/components/home/activity-feed';
import { ActivityStats } from '@/components/home/activity-stats';

/** Activity: the merged feed beside the last 24 hours' figures. */
export function ActivitySection() {
  return (
    <section
      aria-label="Activity"
      className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,460px),1fr))] gap-10"
    >
      <ActivityFeed />
      <ActivityStats />
    </section>
  );
}
