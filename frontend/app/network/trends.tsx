'use client';

import { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type NetworkHistory } from '@/lib/api';
import { Chart, type ChartGap, type ChartPoint, type ChartSeries } from '@/components/ui/chart';
import { DATA_COLOR } from '@/lib/design-tokens';
import { formatExactDecimal } from '@/lib/exact-integer';
import { axisTicks } from '@/components/charts/chart-model';
import { formatUtc8Date } from '@/lib/utils';

// Seconds-precision "now" so the API drops the current (incomplete) daily bucket.
function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

/**
 * D2 (round 3, contracts R-C · D2): a daily `ts` is the unix second of a UTC+8 midnight; the day is
 * named on the site's one clock. Any other instant breaks the contract and is refused.
 */
function formatDay(ts: number): string {
  if (!Number.isSafeInteger(ts) || (ts + 8 * 3600) % 86_400 !== 0)
    throw new Error(`network history day ${ts} is not a UTC+8 midnight`);
  return formatUtc8Date(ts * 1000);
}

// Merge the two independent verified-peer histories into a same-axis dataset keyed by day.
export function mergePeerCounts(
  verified: NetworkHistory | undefined,
  reachable: NetworkHistory | undefined
): { data: ChartPoint[]; error: string | null } {
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
  const data: ChartPoint[] = [];
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
    // Verified is the whole retained set; reachable is the part answering now (CH-50).
    let label: string;
    try {
      label = formatDay(point.ts);
    } catch (cause) {
      return { data: [], error: `Invalid peer history: ${(cause as Error).message}` };
    }
    data.push({
      label,
      x: point.ts,
      values: { reachable: String(reachableCount), verified: String(point.scalar) },
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

/**
 * Each run of days without a retained observation becomes one shaded gap, from the last observed
 * day before it to the first observed day after it (or the coverage edge), so lines never bridge it.
 * Coverage and points are checked against each other within the one verified-peers response.
 */
export function peerGaps(history: NetworkHistory): ChartGap[] {
  const coverage = history.dailyCoverage;
  const observed = new Set(history.points.map((point) => point.ts));
  for (const day of coverage)
    if (day.hasRetainedObservation !== observed.has(day.ts))
      throw new Error(`peer coverage disagrees with its point at ${formatDay(day.ts)}`);
  const gaps: ChartGap[] = [];
  for (let i = 0; i < coverage.length; i++) {
    if (coverage[i].hasRetainedObservation) continue;
    let j = i;
    while (j + 1 < coverage.length && !coverage[j + 1].hasRetainedObservation) j++;
    const from = i > 0 ? coverage[i - 1].ts : coverage[i].ts;
    const to = j + 1 < coverage.length ? coverage[j + 1].ts : coverage[j].ts;
    const first = formatDay(coverage[i].ts).slice(5),
      last = formatDay(coverage[j].ts).slice(5);
    if (from < to)
      gaps.push({
        from,
        to,
        label: first === last ? `no rounds ${first}` : `no rounds ${first} – ${last}`,
      });
    i = j;
  }
  return gaps;
}

/** "Coverage: <day> through <day>." — or, for bounds off the UTC+8 clock, why not (D2). */
function coverageLine(from: number, to: number): string {
  try {
    return `Coverage: ${formatDay(from)} through ${formatDay(to)}.`;
  } catch (cause) {
    return `Coverage unreadable: ${(cause as Error).message}.`;
  }
}

function TrendPanel({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="border-base-border-subtle min-w-0 border-t pt-4">
      <h3 className="text-text-bright text-body mb-3 font-mono font-medium">{title}</h3>
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
        <p role="alert" className="text-negative text-small">
          Failed to load {title.toLowerCase()}.
        </p>
      ) : !history ? (
        <p className="text-text-dim text-small">Loading observations…</p>
      ) : history.points.length === 0 ? (
        <p className="text-text-dim text-small">No completed daily crawler observations yet.</p>
      ) : (
        <>
          <p className="text-text-dim text-meta mb-3">
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
                  className="border-base-border-subtle text-meta grid grid-cols-[6rem_minmax(0,1fr)] gap-3 border-t py-2"
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
  // The board: reachable a jade area, verified a white line over it (CH-58).
  const peerSeries: ChartSeries[] = [
    { key: 'reachable', label: 'Same-network reachable', color: DATA_COLOR },
    { key: 'verified', label: 'Verified (retained)', color: '#dee2ec', lineOnly: true },
  ];
  const peerCounts = mergePeerCounts(verifiedPeers.data, reachablePeers.data);
  const peerHistoryError =
    verifiedPeers.isError || reachablePeers.isError
      ? 'Failed to load verified peer history.'
      : peerCounts.error;
  const peerHistoryLoading = !peerHistoryError && (!verifiedPeers.data || !reachablePeers.data);
  // Gaps and the time axis exist only for a loaded, consistent pair of histories (#2).
  const peerHistory = !peerHistoryError && peerCounts.data.length ? verifiedPeers.data : undefined;
  const coverage = peerHistory?.dailyCoverage ?? [];
  const gaps = peerHistory ? peerGaps(peerHistory) : [];

  return (
    <section className="space-y-4">
      <h2 className="text-text-bright text-section font-mono font-medium">Over time</h2>
      <p className="text-text-dim font-mono text-xs">
        Daily history of retained verified peers, by UTC+8 day. The current (incomplete) day is
        excluded. Round counts were not recorded. Days older than the crawler&apos;s hourly
        retention show as gaps, never zero.
        {verifiedPeers.data?.coverageFrom != null && verifiedPeers.data.coverageTo != null && (
          <> {coverageLine(verifiedPeers.data.coverageFrom, verifiedPeers.data.coverageTo)}</>
        )}
      </p>

      <TrendPanel title="Retained verification state (daily)">
        {peerHistoryError ? (
          <p role="alert" className="text-negative font-mono text-xs">
            {peerHistoryError}
          </p>
        ) : peerHistoryLoading ? (
          <p role="status" className="text-text-dim font-mono text-xs">
            Loading verified peer history…
          </p>
        ) : peerCounts.data.length === 0 ? (
          <p className="text-text-dim font-mono text-xs">
            No completed daily crawler observations yet.
          </p>
        ) : (
          <>
            <Chart
              title="Verified and reachable peers"
              subtitle="Retained peers per completed UTC+8 day"
              data={peerCounts.data}
              series={peerSeries}
              unit="peers"
              kind="area"
              height={240}
              xDomain={
                coverage.length > 1 ? [coverage[0].ts, coverage[coverage.length - 1].ts] : undefined
              }
              gaps={gaps}
              formatValue={(value) => formatExactDecimal(value, 'peer count')}
              xTicks={axisTicks(peerCounts.data, 'date', 6)}
              pointDots
            />
            <p className="text-text-dim text-meta mt-2">
              {gaps.length > 0 && 'Shaded days have no retained round observation. '}
              Rounds per day are unavailable because the crawler did not record counts.
            </p>
          </>
        )}
      </TrendPanel>
    </section>
  );
}

/** The retained daily version and country counts: developer evidence, tier 2 (CH-54). */
export function NetworkShareHistory() {
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
  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
      <ShareTrend
        title="Client Version Share"
        history={versionShare.data}
        error={versionShare.isError}
      />
      <ShareTrend title="Country Share" history={countryShare.data} error={countryShare.isError} />
    </div>
  );
}
