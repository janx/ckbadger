'use client';

import { ActivitiesStreamExplorer } from '@/components/activities-stream-explorer';
import { Header } from '@/components/layout/header';
import { PageHeader } from '@/components/ui/page-header';

export default function ActivitiesPage() {
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <PageHeader
          title="Activities"
          subtitle="Live canonical activity stream across CKB, with filters and infinite scroll"
        />

        <ActivitiesStreamExplorer />
      </main>
    </div>
  );
}
