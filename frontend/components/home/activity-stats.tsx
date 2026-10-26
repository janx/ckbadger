'use client';

import { useMemo, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { BarChart, ChartFigure, type Bar } from '@/components/home/mini-chart';
import { RankedBars, type RankedRow } from '@/components/home/ranked-bars';
import { SECTION_HEADING_CLASS, bodyOf, derive, viewOf } from '@/components/home/section';
import {
  activityTypeShares,
  nonCellbaseCount,
  scriptShares,
  totalActivities,
} from '@/lib/activity-stats';
import {
  api,
  type ActivitySummary24h,
  type DailyActivityStats,
  type ChartResponse,
} from '@/lib/api';
import { dayLabel } from '@/lib/daily-series';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { getScriptDetailHref } from '@/lib/detail-routes';
import { formatCkbCompact } from '@/lib/utils';

const TYPE_BREAKDOWN = '/charts/activity-type-breakdown';

/** "Activity, last 24 h": three KPIs, ranked types and scripts, and the daily activity chart. */
export function ActivityStats() {
  const { data, error } = useQuery({
    queryKey: ['activity-summary-24h'],
    queryFn: () => api.getActivitySummary24h(),
    refetchInterval: 30000,
  });
  const d = useMemo(() => derive({ data, error }, summaryView, 'No activity yet'), [data, error]);

  return (
    <div data-testid="home-activity-stats" className="flex min-w-0 flex-col gap-[22px]">
      <div className="border-base-border-subtle flex h-11 items-center justify-between gap-4 border-b">
        <h2 className={SECTION_HEADING_CLASS}>Activity, last 24 h</h2>
        <Link href="/charts" className="text-interactive hover:text-jade text-xs transition-colors">
          Charts →
        </Link>
      </div>
      {d.state === 'loading' && (
        <div aria-hidden="true" className="bg-base-surface h-40 animate-pulse rounded" />
      )}
      {d.state === 'error' && (
        <div role="alert" className="text-rouge text-xs">
          {d.message}
        </div>
      )}
      {d.state === 'empty' && <div className="text-text-dim text-xs">{d.message}</div>}
      {d.state === 'ready' && <Summary view={d.view} />}
      <DailyActivityFigure />
    </div>
  );
}

interface SummaryView {
  activities: number;
  occurrences: number;
  addresses: number;
  ckbMoved: string;
  types: RankedRow[];
  scripts: RankedRow[];
  scriptCalls: number;
}

function summaryView(s: ActivitySummary24h): SummaryView {
  const scripts = scriptShares(s.scriptCounts, 5);
  const rows: RankedRow[] = scripts.top.map((r) => ({
    key: r.key,
    label: r.label,
    share: r.share,
    href: getScriptDetailHref({ name: r.name, codeHash: r.codeHash }),
  }));
  if (scripts.others) {
    rows.push({
      key: 'others',
      label: `${scripts.others.scripts} other${scripts.others.scripts === 1 ? '' : 's'}`,
      share: scripts.others.share,
    });
  }
  return {
    activities: s.activityCount,
    occurrences: totalActivities(s),
    addresses: s.uniqueAddressCount,
    ckbMoved: formatCkbCompact(s.totalCkbMoved).value,
    types: activityTypeShares(s).map((r) => ({
      key: r.key,
      label: r.label,
      share: r.share,
      href: TYPE_BREAKDOWN,
    })),
    scripts: rows,
    scriptCalls: scripts.total,
  };
}

function Kpi({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-text-dim text-xs">{label}</dt>
      <dd className="text-text-bright m-0 text-[22px] font-medium tabular-nums">{children}</dd>
    </div>
  );
}

function Summary({ view }: { view: SummaryView }) {
  const n = (x: number) => x.toLocaleString('en-US');
  return (
    <>
      <dl className="m-0 flex flex-wrap gap-x-11 gap-y-3">
        <Kpi label="Activities">{n(view.activities)}</Kpi>
        <Kpi label="Addresses">{n(view.addresses)}</Kpi>
        <Kpi label="CKB moved">{view.ckbMoved}</Kpi>
      </dl>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(240px,1fr))] gap-x-10 gap-y-[22px]">
        <RankedBars
          testId="home-activity-types"
          title={`By type, share of ${n(view.occurrences)} kind occurrences`}
          titleHref={TYPE_BREAKDOWN}
          rows={view.types}
        />
        <RankedBars
          testId="home-activity-scripts"
          title={`By script, share of ${n(view.scriptCalls)} calls`}
          titleHref="/charts/most-utilized-scripts"
          rows={view.scripts}
          wideLabels
        />
      </div>
    </>
  );
}

interface DailyView {
  bars: Bar[];
  first: string;
  last: { label: string; value: number };
  min: number;
  max: number;
}

/** Activities per day, oldest first; the last day is the headline. Days must be in order. */
export function dailyActivityView(
  days: DailyActivityStats[],
  transactions: ChartResponse
): DailyView | null {
  if (days.length === 0) return null;
  for (let i = 1; i < days.length; i++) {
    if (days[i].date <= days[i - 1].date) {
      throw new Error(`daily activity days out of order: ${days[i - 1].date} then ${days[i].date}`);
    }
  }
  const counts = new Map<string, string>();
  for (const point of transactions.data) {
    const date = point.date.replaceAll('-', '');
    if (counts.has(date)) throw new Error(`Duplicate daily transaction count for ${date}`);
    counts.set(date, point.value);
  }
  const totals = days.map((day) => {
    const value = counts.get(day.date.replaceAll('-', ''));
    if (value === undefined) throw new Error(`Missing daily transaction count for ${day.date}`);
    const count = Number(exactUnsignedInteger(value, `transaction count on ${day.date}`));
    return nonCellbaseCount(count, day.coinbaseCount, day.date);
  });
  const bars = days.map((day, i) => ({
    label: dayLabel(day.date),
    value: totals[i],
    state: i === days.length - 1 ? ('highlight' as const) : ('complete' as const),
  }));
  const last = bars[bars.length - 1];
  return {
    bars,
    first: bars[0].label,
    last: { label: last.label, value: last.value },
    min: Math.min(...totals),
    max: Math.max(...totals),
  };
}

function DailyActivityFigure() {
  const { data, error } = useQuery({
    queryKey: ['home-daily-activity', 30],
    queryFn: async () => {
      const [days, transactions] = await Promise.all([
        api.getDailyActivityStats(30),
        api.getTransactionCountChart(),
      ]);
      return { days, transactions };
    },
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const d = useMemo(
    () =>
      derive(
        { data, error },
        ({ days, transactions }) => dailyActivityView(days, transactions),
        'No daily activity yet'
      ),
    [data, error]
  );
  const v = viewOf(d);
  const n = (x: number) => x.toLocaleString('en-US');
  return (
    <ChartFigure
      testId="home-daily-activity"
      title="Daily activity"
      href="/charts/activity-volume"
      subtitle={v ? `activities per day, ${v.first} to ${v.last.label}` : 'activities per day'}
      headline={v ? n(v.last.value) : undefined}
      body={bodyOf(d, (view) => (
        <BarChart
          bars={view.bars}
          formatValue={n}
          ariaLabel={`Activities per day, ${view.first} to ${view.last.label}, between ${n(view.min)} and ${n(view.max)}, ${n(view.last.value)} on ${view.last.label}`}
        />
      ))}
    />
  );
}
