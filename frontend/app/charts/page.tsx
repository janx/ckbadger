'use client';

import { useQuery } from '@tanstack/react-query';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { ChartSmallMultiple } from '@/components/charts/chart-small-multiple';
import { CHART_SECTIONS, INDEX_CHARTS, chartSectionId } from '@/components/charts/chart-catalogue';
import { api } from '@/lib/api';
import { TapOrHoverHint } from '@/components/ui/tap-or-hover-hint';
import { formatCount } from '@/lib/format-count';

export default function ChartsPage() {
  const network = useQuery({ queryKey: ['network-stats'], queryFn: api.getNetworkStats });
  return (
    <div className="min-h-screen">
      <main className="container mx-auto min-w-0 py-8">
        <DetailLayout
          title="Charts"
          kind="index"
          // One jump per section, the order every reader follows (CH-05, CH-10).
          actions={
            <nav
              aria-label="Chart sections"
              className="text-small flex flex-wrap gap-x-[18px] gap-y-1"
            >
              {CHART_SECTIONS.map((section) => (
                <a
                  key={section}
                  href={`#${chartSectionId(section)}`}
                  className="text-text-dim hover:text-jade"
                >
                  {section}
                </a>
              ))}
            </nav>
          }
        >
          {network.data?.syncStatus?.chartDataMayBeIncomplete && (
            <p
              role="status"
              className="text-warning border-warning/30 bg-warning/10 text-small rounded border px-3 py-2"
            >
              Chart data may be incomplete while the indexer synchronizes history.
            </p>
          )}
          {network.error && (
            <p role="alert" className="text-negative text-small">
              Network sync status unavailable: {network.error.message}
            </p>
          )}
          {CHART_SECTIONS.map((section) => {
            const charts = INDEX_CHARTS.filter((chart) => chart.section === section);
            return (
              <div key={section} id={chartSectionId(section)} className="scroll-mt-4">
                {/* The command runs on the section's chart count (rubbing × Song print, phase 5). */}
                <DetailSection title={section} args={formatCount(charts.length, 'chart')}>
                  <div className="grid min-w-0 grid-cols-1 gap-x-10 gap-y-9 sm:grid-cols-2 xl:grid-cols-4">
                    {charts.map((definition) => (
                      <ChartSmallMultiple key={definition.slug} definition={definition} />
                    ))}
                  </div>
                </DetailSection>
              </div>
            );
          })}
          <p className="text-text-dim text-small">
            Every chart stops at the last complete day. Open a chart for its full resolution and
            source; there, <TapOrHoverHint what="the chart" /> to read any point.
          </p>
        </DetailLayout>
      </main>
    </div>
  );
}
