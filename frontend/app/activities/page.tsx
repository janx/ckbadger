'use client';

import { Header } from '@/components/layout/header';
import { LatestActivities } from '@/components/latest-activities';
import { PageHeader } from '@/components/ui/page-header';

export default function ActivitiesPage() {
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Activities"
          subtitle="Browse the latest canonical activity stream across the CKB network"
        />

        <LatestActivities
          queryLimit={64}
          maxItems={64}
          showViewAllLink={false}
          scrollable
          panelClassName="h-auto min-h-[44rem]"
        />
      </main>
    </div>
  );
}
