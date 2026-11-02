'use client';

import { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type LabelCount, type NetworkHistory } from '@/lib/api';
import { Chart, type ChartGap, type ChartPoint, type ChartSeries } from '@/components/ui/chart';
import { AlignedBar, ScaleHeader } from '@/components/ui/aligned-bar';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { PhoneRow } from '@/components/ui/phone-row';
import { resourceScale } from '@/components/blocks/block-visual';
import { DATA_COLOR } from '@/lib/design-tokens';
import { formatExactDecimal } from '@/lib/exact-integer';
import { formatUtc8Date } from '@/lib/utils';
import { TopicSection } from '@/app/network/topic-section';

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

/** One day of a share history: its retained labels, or why it has none. */
interface ShareDay {
  ts: number;
  day: string;
  /** Null: no retained round that day. */
  buckets: LabelCount[] | null;
}

/** The day's labels, most common first (ties by label), and their exact sum. */
function ranked(buckets: readonly LabelCount[]): { labels: LabelCount[]; sum: number } {
  const labels = [...buckets].sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));
  let sum = 0;
  for (const bucket of labels) {
    if (!Number.isSafeInteger(bucket.count) || bucket.count < 0)
      throw new Error(`share history: invalid count ${bucket.count} for ${bucket.label}`);
    sum += bucket.count;
  }
  return { labels, sum };
}

/** What a day's row says in words: every label and count (the row's title, table-rows T8). */
function shareDayWords(row: ShareDay): string {
  if (row.buckets === null) return 'No retained round observation';
  if (row.buckets.length === 0) return 'No retained labels';
  return row.buckets
    .map((bucket) => `${bucket.label} ${bucket.count.toLocaleString()}`)
    .join(' · ');
}

const SHARE_TRACK = 80;

/**
 * Table-rows T8: a share history is one line a day — the day (the row's key), a count
 * bar of its retained labels with the most common in jade, how many labels, and the most common
 * with its count. Every label and count is the row's title; a day without a round says so.
 */
function ShareDays({ title, days }: { title: string; days: readonly ShareDay[] }) {
  const sums = days.flatMap((row) => (row.buckets ? [ranked(row.buckets).sum] : []));
  const scale = sums.some((sum) => sum > 0) ? resourceScale(sums) : null;
  const quiet = (words: string) => <span className="text-text-dim text-meta">{words}</span>;
  const bar = (row: ShareDay) => {
    if (row.buckets === null) return null;
    const { labels, sum } = ranked(row.buckets);
    if (sum === 0 || scale === null) return null;
    return (
      <AlignedBar
        label="Retained count"
        scale={scale}
        trackWidth={SHARE_TRACK}
        minFill={2}
        segments={[
          { key: 'leader', label: labels[0].label, value: labels[0].count, tone: 'data' },
          { key: 'rest', label: 'other labels', value: sum - labels[0].count, tone: 'overhead' },
        ]}
      />
    );
  };
  // The widest cell says why a day has no bar, on the day's one line.
  const leader = (row: ShareDay) => {
    if (!row.buckets?.length) return quiet(shareDayWords(row));
    const top = ranked(row.buckets).labels[0];
    return (
      <span className="flex min-w-0 items-baseline gap-2">
        <span className="text-text min-w-0 truncate">{top.label}</span>{' '}
        <span className="text-text shrink-0 tabular-nums">{top.count.toLocaleString()}</span>
      </span>
    );
  };
  const columns: ListColumn<ShareDay>[] = [
    {
      key: 'day',
      label: 'Day',
      // Each head holds its words in a span, so its text box is its words (R16), not the cell.
      header: <span>Day</span>,
      minWidth: 96,
      render: (row) => (
        <span className="text-body text-text-bright whitespace-nowrap leading-[18px]">
          {row.day}
        </span>
      ),
    },
    {
      key: 'count',
      label: 'Retained count',
      header:
        scale === null ? (
          <span>Retained count</span>
        ) : (
          <ScaleHeader label="Retained count" scale={scale} trackWidth={SHARE_TRACK} />
        ),
      minWidth: 136,
      render: bar,
    },
    {
      key: 'labels',
      label: 'Labels',
      header: <span>Labels</span>,
      minWidth: 64,
      numeric: true,
      render: (row) =>
        row.buckets?.length ? (
          <span className="text-text-dim">{row.buckets.length.toLocaleString()}</span>
        ) : null,
    },
    {
      key: 'leader',
      label: 'Most common',
      header: <span>Most common</span>,
      minWidth: 200,
      width: 'auto',
      render: leader,
    },
  ];
  return (
    <ListTable
      label={title + ' daily observations'}
      density="row"
      columns={columns}
      rows={days}
      rowKey={(row) => String(row.ts)}
      rowTitle={(row) => `${row.day} · ${shareDayWords(row)}`}
      mobileRow={(row) => (
        <PhoneRow
          testId="share-day-row-mobile"
          lines={[
            { key: 'day', tone: 'head', left: row.day, right: bar(row) },
            {
              key: 'leader',
              tone: 'body',
              left: leader(row),
              right: row.buckets?.length ? (
                <span className="text-text-dim">{row.buckets.length.toLocaleString()} labels</span>
              ) : undefined,
            },
            // A title is no hover on a touch screen: every label and count, quiet, last.
            Boolean(row.buckets?.length) && {
              key: 'labels',
              tone: 'quiet',
              left: shareDayWords(row),
            },
          ]}
        />
      )}
    />
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
  const days: ShareDay[] = (history?.dailyCoverage ?? []).map((day) => {
    const point = byDay.get(day.ts);
    if (day.hasRetainedObservation !== Boolean(point))
      throw new Error(title + ' coverage disagrees with point at ' + formatDay(day.ts));
    return { ts: day.ts, day: formatDay(day.ts), buckets: point ? point.buckets : null };
  });
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
          <p className="text-text-dim text-body mb-3">
            Retained top-label counts per observed day. The historical total and omitted labels were
            not recorded, so percentages are unavailable. Each bar is a day&apos;s retained count,
            its most common label in jade; every label is in the row&apos;s title.
          </p>
          {/* The head and six one-line days (61 + 6 × 41 px); a longer retention scrolls, raw data. */}
          <div className="max-h-80 overflow-y-auto">
            <ShareDays title={title} days={days} />
          </div>
        </>
      )}
    </TrendPanel>
  );
}

/** The daily peer histories, shared by the chart and its notes (one fetch each, react-query). */
function usePeerHistories() {
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
  return { verifiedPeers, reachablePeers };
}

/** The board: reachable a jade area, verified a white line over it (CH-58). */
const PEER_SERIES: ChartSeries[] = [
  { key: 'reachable', label: 'Same-network reachable', color: DATA_COLOR },
  { key: 'verified', label: 'Verified (retained)', color: '#dee2ec', lineOnly: true },
];
const PEER_UNIT = 'peers';
const formatPeers = (value: string) => formatExactDecimal(value, 'peer count');

/**
 * "Over time" (hierarchy v3): the chart named inside its plot with its values inside (no caption
 * block, no coloured key), the first and last day on the axis, one reading row that rests on the
 * latest day, and one caption. How to read it and its coverage are tier 2 (`NetworkTrendNotes`).
 */
export function NetworkTrends() {
  const { verifiedPeers, reachablePeers } = usePeerHistories();
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
  const days = peerCounts.data;
  const last = days[days.length - 1];
  // R14: the first and the last day name the axis.
  const xTicks = days.length
    ? [...new Set([0, days.length - 1])].map((index) => ({
        index,
        label: days[index].label.slice(5),
      }))
    : [];

  return (
    <TopicSection title="Over time">
      {peerHistoryError ? (
        <p role="alert" className="text-negative text-small font-mono">
          {peerHistoryError}
        </p>
      ) : peerHistoryLoading ? (
        <p role="status" className="text-text-dim text-small font-mono">
          Loading verified peer history…
        </p>
      ) : days.length === 0 ? (
        <p className="text-text-dim text-small font-mono">
          No completed daily crawler observations yet.
        </p>
      ) : (
        <>
          <Chart
            title="Verified and reachable peers"
            subtitle="Retained peers per completed UTC+8 day"
            data={days}
            series={PEER_SERIES}
            unit={PEER_UNIT}
            kind="area"
            height={200}
            xDomain={
              coverage.length > 1 ? [coverage[0].ts, coverage[coverage.length - 1].ts] : undefined
            }
            gaps={gaps}
            formatValue={formatPeers}
            xTicks={xTicks}
            pointDots
            plotName="verified and reachable peers per completed UTC+8 day"
            insideYTicks
            pinnedReadout={{
              label: 'Peer reading',
              idle: `${last.label}${PEER_SERIES.map(
                (s) => ` · ${s.label} ${formatPeers(last.values[s.key])} ${PEER_UNIT}`
              ).join('')}`,
            }}
          />
          <p data-role="caption" className="text-text text-body mt-3.5 max-w-[640px]">
            The white line is every verified node the crawler retained that day; the jade area is
            the part of them that was reachable.
          </p>
        </>
      )}
    </TopicSection>
  );
}

/**
 * Tier 2: how to read "Over time" — what each day is, what is left out, what a shaded day means —
 * and the days it covers. Shares the chart's verified-peer history (one fetch).
 */
export function NetworkTrendNotes() {
  const { verifiedPeers } = usePeerHistories();
  return (
    <p className="text-text-dim text-body">
      “Over time” draws the retained verification state (daily): verified and reachable peers per
      completed UTC+8 day. Daily history of retained verified peers, by UTC+8 day. The current
      (incomplete) day is excluded. Round counts were not recorded. Days older than the
      crawler&apos;s hourly retention show as gaps, never zero.
      {verifiedPeers.data?.coverageFrom != null && verifiedPeers.data.coverageTo != null && (
        <> {coverageLine(verifiedPeers.data.coverageFrom, verifiedPeers.data.coverageTo)}</>
      )}{' '}
      Shaded days have no retained round observation. Rounds per day are unavailable because the
      crawler did not record counts.
    </p>
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
    // Stacked: tier 2 gives the daily counts one column of its own from lg.
    <div className="grid grid-cols-1 gap-4">
      <ShareTrend
        title="Client Version Share"
        history={versionShare.data}
        error={versionShare.isError}
      />
      <ShareTrend title="Country Share" history={countryShare.data} error={countryShare.isError} />
    </div>
  );
}
