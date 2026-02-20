'use client';

import Link from 'next/link';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import {
  TerminalPanel,
  TerminalPanelContent,
  TerminalPanelHeader,
  TerminalRow,
} from '@/components/ui/terminal-panel';
import { api, MostUtilizedScriptsChartItem } from '@/lib/api';
import { formatCkbCompact } from '@/lib/utils';

function MetricTable({
  title,
  items,
  metricKey,
}: {
  title: string;
  items: MostUtilizedScriptsChartItem[];
  metricKey: 'occupiedCapacity' | 'totalCellsCapacity';
}) {
  const metricLabel = metricKey === 'occupiedCapacity' ? 'Occupied CKB' : 'Total Cells Capacity';

  return (
    <div className="overflow-hidden rounded border border-slate-800">
      <div className="border-b border-slate-800 bg-slate-900/50 px-4 py-3 font-mono text-xs uppercase tracking-wider text-slate-300">
        {title}
      </div>
      <div className="flex border-b border-slate-800 bg-slate-900/30 px-4 py-2 font-mono text-xs uppercase tracking-wider text-slate-500">
        <div className="w-12">Rank</div>
        <div className="flex-1">Script</div>
        <div className="w-32 text-right">{metricLabel}</div>
      </div>
      {items.length === 0 && (
        <div className="px-4 py-6 text-center text-sm text-slate-500">No utilized scripts yet</div>
      )}
      {items.map((item, index) => {
        const linkHref =
          item.isKnownScript || !item.codeHash
            ? `/scripts/${encodeURIComponent(item.name)}`
            : `/script/${item.codeHash}`;
        const ckbValue = formatCkbCompact(item[metricKey]);

        return (
          <TerminalRow key={`${item.name}-${item.codeHash ?? index}-${metricKey}`}>
            <div className="flex items-center px-4">
              <div className="w-12 font-mono text-slate-500">{index + 1}</div>
              <div className="flex-1 overflow-hidden">
                <Link
                  href={linkHref}
                  className="text-terminal-green hover:text-terminal-green/80 block truncate font-mono text-sm transition-colors"
                  title={item.name}
                >
                  {item.name}
                </Link>
                <div className="font-mono text-xs text-slate-500">{item.scriptKind}</div>
              </div>
              <div className="w-32 text-right font-mono text-sm text-slate-200">
                <span title={`${ckbValue.full} CKB`}>{ckbValue.value}</span>
              </div>
            </div>
          </TerminalRow>
        );
      })}
    </div>
  );
}

export default function MostUtilizedScriptsPage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['chart-most-utilized-scripts'],
    queryFn: api.getMostUtilizedScriptsChart,
  });

  return (
    <div className="min-h-screen bg-slate-950">
      <Header />
      <main className="container mx-auto px-4 py-8">
        <div className="mb-6">
          <Link
            href="/charts"
            className="hover:text-terminal-green text-sm text-slate-500 transition-colors"
          >
            ← Back to Charts
          </Link>
        </div>

        <TerminalPanel>
          <TerminalPanelHeader indicator="active">Most Utilized Scripts</TerminalPanelHeader>
          <TerminalPanelContent className="space-y-6 p-6">
            {isLoading && (
              <div className="h-96 animate-pulse rounded border border-slate-800 bg-slate-900/50" />
            )}
            {error && (
              <div className="flex h-96 items-center justify-center text-slate-500">
                Failed to load chart data
              </div>
            )}
            {data && (
              <>
                <div className="grid gap-6 lg:grid-cols-2">
                  <MetricTable
                    title="Top 20 by Occupied CKB"
                    items={data.byOccupied}
                    metricKey="occupiedCapacity"
                  />
                  <MetricTable
                    title="Top 20 by Total Cells Capacity"
                    items={data.byTotalCellsCapacity}
                    metricKey="totalCellsCapacity"
                  />
                </div>
              </>
            )}
          </TerminalPanelContent>
        </TerminalPanel>
      </main>
    </div>
  );
}
