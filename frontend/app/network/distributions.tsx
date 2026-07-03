'use client';

import { useQuery } from '@tanstack/react-query';
import { api, LabelCount, NetworkDistributions as NetworkDistributionsData } from '@/lib/api';
import { ProgressBar } from '@/components/ui/progress-bar';

const TOP_N = 8;

// A top-N horizontal bar chart for a single categorical dimension (versions, countries, ...).
// Reuses the shared ProgressBar so every bar renders its label as real DOM text — categorical
// distributions read far better as a labelled bar list than as a time-series bar chart.
function DistributionBars({ title, items }: { title: string; items: LabelCount[] }) {
  const top = [...items].sort((a, b) => b.count - a.count).slice(0, TOP_N);
  const max = top.reduce((m, it) => Math.max(m, it.count), 0);

  return (
    <div className="border-base-border bg-base-surface rounded border p-4">
      <h3 className="text-text-bright mb-3 font-mono text-sm font-bold">{title}</h3>
      {top.length === 0 ? (
        <p className="text-text-dim font-mono text-xs">No data yet</p>
      ) : (
        <div className="space-y-2">
          {top.map((it) => (
            <div key={it.label} className="flex items-center gap-3">
              <span className="text-text w-40 shrink-0 truncate font-mono text-xs" title={it.label}>
                {it.label}
              </span>
              <div className="min-w-0 flex-1">
                <ProgressBar value={it.count} max={max} showLabel={false} size="sm" color="blue" />
              </div>
              <span className="text-text-dim w-12 shrink-0 text-right font-mono text-xs tabular-nums">
                {it.count}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function ReachabilityStat({ data }: { data: NetworkDistributionsData }) {
  const { reachable, unreachable, totalKnown } = data;
  return (
    <div className="border-base-border bg-base-surface rounded border p-4">
      <h3 className="text-text-bright mb-3 font-mono text-sm font-bold">Reachability</h3>
      <ProgressBar value={reachable} max={totalKnown} showLabel={false} color="green" />
      <p className="text-text-dim mt-2 font-mono text-xs tabular-nums">
        {`${reachable} reachable · ${unreachable} unreachable`}
      </p>
    </div>
  );
}

export function NetworkDistributions() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['network', 'distributions'],
    queryFn: api.getNetworkDistributions,
    refetchInterval: 30000,
  });

  if (isLoading) {
    return <div className="bg-base-elevated h-64 w-full animate-pulse rounded" />;
  }
  if (error || !data) {
    return <div className="text-text-dim font-mono text-sm">Failed to load distributions.</div>;
  }

  return (
    <section className="space-y-4">
      <h2 className="text-text-bright font-mono text-lg font-bold">Distributions</h2>
      <ReachabilityStat data={data} />
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <DistributionBars title="Client Versions" items={data.versions} />
        <DistributionBars title="Countries" items={data.countries} />
        <DistributionBars title="Networks (ASN)" items={data.asns} />
        <DistributionBars title="Protocols" items={data.protocols} />
      </div>
    </section>
  );
}
