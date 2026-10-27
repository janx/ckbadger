'use client';

import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { CHART_SECTIONS, INDEX_CHARTS } from '@/components/charts/chart-catalogue';
import { api } from '@/lib/api';

export default function ChartsPage() {
  const network = useQuery({ queryKey: ['network-stats'], queryFn: api.getNetworkStats });
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto min-w-0 px-4 py-8">
        <DetailLayout
          title="Charts"
          kind="index"
          identity="Canonical chain aggregates and distributions"
        >
          {network.data?.syncStatus?.chartDataMayBeIncomplete && (
            <p
              role="status"
              className="text-warning border-warning/30 bg-warning/10 rounded border px-3 py-2 text-xs"
            >
              Chart data may be incomplete while the indexer synchronizes history.
            </p>
          )}
          {network.error && (
            <p role="alert" className="text-negative text-xs">
              Network sync status unavailable: {network.error.message}
            </p>
          )}
          {CHART_SECTIONS.map((section) => (
            <DetailSection key={section} title={section}>
              <div className="grid min-w-0 grid-cols-1 gap-x-10 gap-y-10 sm:grid-cols-2 xl:grid-cols-4">
                {INDEX_CHARTS.filter((chart) => chart.section === section).map((definition) => (
                  <ChartSmallMultiple key={definition.slug} definition={definition} />
                ))}
              </div>
            </DetailSection>
          ))}
        </DetailLayout>
      </main>
    </div>
  );
}
