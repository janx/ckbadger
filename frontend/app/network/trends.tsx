'use client';

import { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, NetworkHistory, StackedAreaDataPoint, StackedAreaSeries } from '@/lib/api';
import { StackedAreaChart } from '@/components/ui/stacked-area-chart';
import { getChartPaletteColor } from '@/lib/chart-colors';

// Seconds-precision "now" so the API drops the current (incomplete) daily bucket.
function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

function formatDay(ts: number): string {
  return new Date(ts * 1000).toISOString().slice(0, 10);
}

// Merge the two independent verified-peer histories into a same-axis dataset keyed by day.
export function mergePeerCounts(
  verified: NetworkHistory | undefined,
  reachable: NetworkHistory | undefined
): { data: StackedAreaDataPoint[]; error: string | null } {
  if (!verified || !reachable) return { data: [], error: null };

  const reachableByTs = new Map<number, number>();
  for (const point of reachable.points) {
    if (reachableByTs.has(point.ts)) {
      return {
        data: [],
        error: `Invalid peer history: duplicate reachablePeers point at ${point.ts}`,
      };
    }
    reachableByTs.set(point.ts, point.scalar);
  }

  const verifiedTimestamps = new Set<number>();
  const data: StackedAreaDataPoint[] = [];
  for (const point of [...verified.points].sort((left, right) => left.ts - right.ts)) {
    if (verifiedTimestamps.has(point.ts)) {
      return {
        data: [],
        error: `Invalid peer history: duplicate verifiedPeers point at ${point.ts}`,
      };
    }
    verifiedTimestamps.add(point.ts);
    const reachableCount = reachableByTs.get(point.ts);
    if (reachableCount == null) {
      return {
        data: [],
        error: `Invalid peer history: missing reachablePeers point at ${point.ts}`,
      };
    }
    if (reachableCount > point.scalar) {
      return {
        data: [],
        error: `Invalid peer history at ${point.ts}: reachablePeers ${reachableCount} exceeds verifiedPeers ${point.scalar}`,
      };
    }
    reachableByTs.delete(point.ts);
    data.push({
      date: formatDay(point.ts),
      values: {
        sameNetworkReachable: String(reachableCount),
        verifiedUnavailable: String(point.scalar - reachableCount),
      },
    });
  }
  const unmatchedReachableTimestamp = reachableByTs.keys().next().value;
  if (unmatchedReachableTimestamp != null) {
    return {
      data: [],
      error: `Invalid peer history: missing verifiedPeers point at ${unmatchedReachableTimestamp}`,
    };
  }
  return { data, error: null };
}

function ChartLegend({ series }: { series: StackedAreaSeries[] }) {
  return (
    <div className="mt-4 flex flex-wrap items-center justify-center gap-4">
      {series.map((s) => (
        <div key={s.key} className="flex items-center gap-2">
          <span className="h-3 w-3 rounded" style={{ backgroundColor: s.color }} />
          <span className="text-text-dim font-mono text-xs">{s.label}</span>
        </div>
      ))}
    </div>
  );
}

function TrendPanel({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="border-base-border-subtle min-w-0 border-t pt-4">
      <h3 className="text-text-bright mb-3 font-mono text-[13px] font-medium">{title}</h3>
      {children}
    </div>
  );
}

function ShareTrend({
  title,
  history,
  error,
}: {
  title: string;
  history: NetworkHistory | undefined;
  error: boolean;
}) {
  const byDay = new Map(history?.points.map((point) => [point.ts, point]) ?? []);
  return (
    <TrendPanel title={title}>
      {error ? (
        <p role="alert" className="text-negative text-[12px]">
          Failed to load {title.toLowerCase()}.
        </p>
      ) : !history ? (
        <p className="text-text-dim text-[12px]">Loading observations…</p>
      ) : history.points.length === 0 ? (
        <p className="text-text-dim text-[12px]">No completed daily crawler observations yet.</p>
      ) : (
        <>
          <p className="text-text-dim mb-3 text-[11px]">
            Retained top-label counts per observed day. The historical total and omitted labels were
            not recorded, so percentages are unavailable.
          </p>
          <div
            className="max-h-72 overflow-y-auto"
            role="list"
            aria-label={title + ' daily observations'}
          >
            {history.dailyCoverage.map((day) => {
              const point = byDay.get(day.ts);
              if (day.hasRetainedObservation !== Boolean(point))
                throw new Error(title + ' coverage disagrees with point at ' + formatDay(day.ts));
              return (
                <div
                  role="listitem"
                  key={day.ts}
                  className="border-base-border-subtle grid grid-cols-[6rem_minmax(0,1fr)] gap-3 border-t py-2 text-[11px]"
                >
                  <span className="text-text-dim">{formatDay(day.ts)}</span>
                  <span className="text-text min-w-0 break-words">
                    {point
                      ? point.buckets
                          .map((bucket) => bucket.label + ' ' + bucket.count.toLocaleString())
                          .join(' · ') || 'No retained labels'
                      : 'No retained round observation'}
                  </span>
                </div>
              );
            })}
          </div>
        </>
      )}
    </TrendPanel>
  );
}

export function NetworkTrends() {
  const verifiedPeers = useQuery({
    queryKey: ['network', 'history', 'verifiedPeers', 'day'],
    queryFn: () => api.getNetworkHistory('verifiedPeers', 'day', undefined, nowSeconds()),
    refetchInterval: 60000,
  });
  const reachablePeers = useQuery({
    queryKey: ['network', 'history', 'reachablePeers', 'day'],
    queryFn: () => api.getNetworkHistory('reachablePeers', 'day', undefined, nowSeconds()),
    refetchInterval: 60000,
  });
  const versionShare = useQuery({
    queryKey: ['network', 'history', 'versionShare', 'day'],
    queryFn: () => api.getNetworkHistory('versionShare', 'day', undefined, nowSeconds()),
    refetchInterval: 60000,
  });
  const countryShare = useQuery({
    queryKey: ['network', 'history', 'countryShare', 'day'],
    queryFn: () => api.getNetworkHistory('countryShare', 'day', undefined, nowSeconds()),
    refetchInterval: 60000,
  });

  const peerSeries: StackedAreaSeries[] = [
    {
      key: 'sameNetworkReachable',
      label: 'Same-network reachable',
      color: getChartPaletteColor(2),
    },
    {
      key: 'verifiedUnavailable',
      label: 'Verified unavailable',
      color: getChartPaletteColor(0),
    },
  ];
  const peerCounts = mergePeerCounts(verifiedPeers.data, reachablePeers.data);
  const coverage = verifiedPeers.data?.dailyCoverage ?? [];
  const hasGap = coverage.some((day) => !day.hasRetainedObservation);
  const countsByDay = new Map(peerCounts.data.map((point) => [point.date, point.values]));
  const maxVerified = Math.max(
    1,
    ...(verifiedPeers.data?.points ?? []).map((point) => point.scalar)
  );
  const peerHistoryError =
    verifiedPeers.isError || reachablePeers.isError
      ? 'Failed to load verified peer history.'
      : peerCounts.error;

  return (
    <section className="space-y-4">
      <h2 className="text-text-bright font-mono text-[15px] font-medium">Over time</h2>
      <p className="text-text-dim font-mono text-xs">
        Daily history of retained verified peers. The current (incomplete) day is excluded. Round
        counts were not recorded.
        {verifiedPeers.data?.coverageFrom != null && verifiedPeers.data.coverageTo != null && (
          <>
            {' '}
            Coverage: {formatDay(verifiedPeers.data.coverageFrom)} through{' '}
            {formatDay(verifiedPeers.data.coverageTo)}.
          </>
        )}
      </p>

      <TrendPanel title="Retained verification state (daily)">
        {peerHistoryError ? (
          <p className="text-negative font-mono text-xs">{peerHistoryError}</p>
        ) : peerCounts.data.length === 0 ? (
          <p className="text-text-dim font-mono text-xs">
            No completed daily crawler observations yet.
          </p>
        ) : (
          <>
            {hasGap ? (
              <div
                role="list"
                aria-label="Daily verified peer observations and missing days"
                className="border-base-border-subtle flex h-56 min-w-0 gap-px overflow-x-auto border-b"
              >
                {coverage.map((day) => {
                  const date = formatDay(day.ts);
                  const values = countsByDay.get(date);
                  if (day.hasRetainedObservation && !values)
                    throw new Error('coverage has no peer point at ' + date);
                  const reachable = values ? Number(values.sameNetworkReachable) : 0;
                  const unavailable = values ? Number(values.verifiedUnavailable) : 0;
                  return (
                    <div
                      role="listitem"
                      key={day.ts}
                      title={
                        values
                          ? date +
                            ': ' +
                            reachable +
                            ' reachable, ' +
                            unavailable +
                            ' verified unavailable'
                          : date + ': no retained round observation'
                      }
                      aria-label={
                        values
                          ? date +
                            ': ' +
                            reachable +
                            ' reachable, ' +
                            unavailable +
                            ' verified unavailable'
                          : date + ': no retained round observation'
                      }
                      className={
                        'flex min-w-[8px] flex-1 flex-col justify-end ' +
                        (values ? '' : 'bg-base-elevated')
                      }
                    >
                      {values && (
                        <>
                          <div
                            className="bg-aqua"
                            style={{ height: (unavailable / maxVerified) * 100 + '%' }}
                          />
                          <div
                            className="bg-jade"
                            style={{ height: (reachable / maxVerified) * 100 + '%' }}
                          />
                        </>
                      )}
                    </div>
                  );
                })}
              </div>
            ) : (
              <StackedAreaChart data={peerCounts.data} series={peerSeries} height={260} />
            )}
            <ChartLegend series={peerSeries} />
            <p className="text-text-dim mt-2 text-[11px]">
              Shaded days have no retained round observation. Rounds per day are unavailable because
              the crawler did not record counts.
            </p>
          </>
        )}
      </TrendPanel>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <ShareTrend
          title="Client Version Share"
          history={versionShare.data}
          error={versionShare.isError}
        />
        <ShareTrend
          title="Country Share"
          history={countryShare.data}
          error={countryShare.isError}
        />
      </div>
    </section>
  );
}
